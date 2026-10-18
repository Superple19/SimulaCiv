use sim_core::{AgentId, DenseSlot, GroupId, SimulationDay};
use sim_model::features::{
    phase3_observation_and_features_into, phase3_observation_and_features_storage_with_scratch,
};
use sim_model::metrics::{phase10_observe, phase10_observe_storage};
use sim_model::resolution::{
    M3Phase6BStorageScratch, phase6a_work_resolution, phase6a_work_resolution_storage,
    phase6b_targeted_resolution, phase6b_targeted_resolution_storage_baseline,
    phase6b_targeted_resolution_storage_m3_with_scratch, phase7_market_clearance,
    phase7_market_clearance_storage,
};
use sim_model::state::{AgentState, SettlementState, WorldState};
use sim_model::{SegmentedAgentStorage, SettlementIntentPartition, SimConfig};

const CONFIG: &str = r#"
[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 1
settlement_count = 1
initial_health = 1.0
initial_food = 25.0
initial_wealth = 1000
initial_settlement_resource = 2000.0
initial_treasury = 5000

[traits]
prod_min = 0.8
prod_max = 1.5
coop_min = 0.2
coop_max = 0.8
aggr_min = 0.1
aggr_max = 0.5
risk_min = 0.1
risk_max = 0.5

[environment]
carrying_capacity = 10000.0
regrowth_rate = 0.1
base_metabolic_cost = 1.0
health_decay_rate = 0.05

[economy]
base_work_yield = 2.0
food_price = 100
target_food = 20.0
target_reserve = 5000
tax_rate = 0.1
welfare_payment = 50

[interaction]
gift_amount = 2.0
theft_amount = 3.0
theft_success_probability = 0.5
starvation_threshold = 5.0

[decision]
decision_temperature = 1.0
action_biases = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
base_weight_matrix = [
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0]
]
trait_weight_cooperation = 1.0
trait_weight_aggression = 1.0
trait_weight_risk_tolerance = 1.0
"#;

fn test_agent(group_id: GroupId) -> AgentState {
    AgentState {
        agent_id: AgentId(0),
        group_id,
        dense_slot: DenseSlot(0),
        alive: true,
        birth_day: SimulationDay(0),
        health: 1.0,
        food: 4.0,
        wealth: 1000,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.25,
        risk_tolerance: 0.5,
    }
}

fn test_world(group_id: GroupId, settlements: Vec<SettlementState>) -> WorldState {
    WorldState {
        current_day: SimulationDay(0),
        agents: vec![test_agent(group_id)],
        settlements,
        initial_money_supply: 1000,
    }
}

fn settlement_ids(case_index: usize, state: &mut u32) -> Vec<u16> {
    match case_index {
        0 => vec![8, 3, 8, 42, 3],
        1 => vec![0, 1, 2, 3],
        2 => vec![2, 0, 3, 1],
        _ => {
            *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let len = (*state as usize) % 24;
            (0..len)
                .map(|_| {
                    *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    ((*state >> 16) % 31) as u16
                })
                .collect()
        }
    }
}

#[test]
fn randomized_storage_phases_match_linear_public_lookup_and_error_semantics() {
    let config = SimConfig::parse_and_validate(CONFIG).expect("test config validates");
    let mut state = 0x7f4a_7c15_u32;
    let mut saw_duplicate = false;
    let mut saw_missing = false;
    let mut saw_unsorted = false;

    for case_index in 0..40 {
        let ids = settlement_ids(case_index, &mut state);
        let settlements: Vec<_> = ids
            .iter()
            .enumerate()
            .map(|(index, &id)| SettlementState {
                group_id: GroupId(id),
                resource: 10.0 + index as f32,
                treasury: 100 + index as i64,
            })
            .collect();
        saw_duplicate |= ids
            .iter()
            .enumerate()
            .any(|(index, id)| ids[..index].contains(id));
        saw_unsorted |= ids.windows(2).any(|pair| pair[0] > pair[1]);

        let group_id = match case_index {
            0 => GroupId(3),
            1 => GroupId(50),
            _ => {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                GroupId((state >> 16) as u16 % 40)
            }
        };
        let group_exists = ids.contains(&group_id.0);
        saw_missing |= !group_exists;

        let world = test_world(group_id, settlements.clone());
        let storage = SegmentedAgentStorage::from_agents(&world.agents);
        let partitions = vec![SettlementIntentPartition {
            group_id,
            intents: Vec::new(),
        }];

        let mut aos_features = Vec::new();
        let aos_features_result =
            phase3_observation_and_features_into(&world, &config, &mut aos_features);
        let mut storage_features = Vec::new();
        let mut phase3_scratch = sim_model::Phase3ScarcityScratch::default();
        let storage_features_result = phase3_observation_and_features_storage_with_scratch(
            &storage,
            &settlements,
            &config,
            &mut storage_features,
            &mut phase3_scratch,
        );
        assert_eq!(
            storage_features_result, aos_features_result,
            "case={case_index}"
        );
        if aos_features_result.is_ok() {
            assert_eq!(storage_features, aos_features, "case={case_index}");
        }

        let aos_metrics = phase10_observe(&world, 0);
        let storage_metrics = phase10_observe_storage(&storage, &settlements, 0);
        assert_eq!(storage_metrics, aos_metrics, "case={case_index}");

        let mut aos_work_world = world.clone();
        let aos_work = phase6a_work_resolution(&mut aos_work_world, &partitions);
        let mut work_storage = storage.clone();
        let mut work_settlements = settlements.clone();
        let storage_work =
            phase6a_work_resolution_storage(&mut work_storage, &mut work_settlements, &partitions);
        assert_eq!(storage_work, aos_work, "case={case_index}");
        assert_eq!(
            work_settlements, aos_work_world.settlements,
            "case={case_index}"
        );

        let mut aos_targeted_world = world.clone();
        let aos_targeted =
            phase6b_targeted_resolution(&mut aos_targeted_world, &config, &partitions);
        let mut baseline_storage = storage.clone();
        let storage_targeted = phase6b_targeted_resolution_storage_baseline(
            &mut baseline_storage,
            &settlements,
            SimulationDay(0),
            &config,
            &partitions,
        );
        let mut m3_storage = storage.clone();
        let m3_targeted = phase6b_targeted_resolution_storage_m3_with_scratch(
            &mut m3_storage,
            &settlements,
            SimulationDay(0),
            &config,
            &partitions,
            &mut M3Phase6BStorageScratch::default(),
        );
        assert_eq!(storage_targeted, aos_targeted, "case={case_index}");
        assert_eq!(m3_targeted, aos_targeted, "case={case_index}");

        let mut duplicate_partitions = partitions.clone();
        duplicate_partitions.push(partitions[0].clone());
        let mut aos_duplicate_world = world.clone();
        let aos_duplicate_targeted =
            phase6b_targeted_resolution(&mut aos_duplicate_world, &config, &duplicate_partitions);
        let mut storage_duplicate = storage.clone();
        let storage_duplicate_targeted = phase6b_targeted_resolution_storage_baseline(
            &mut storage_duplicate,
            &settlements,
            SimulationDay(0),
            &config,
            &duplicate_partitions,
        );
        assert_eq!(
            storage_duplicate_targeted, aos_duplicate_targeted,
            "duplicate partition case={case_index}"
        );

        let mut aos_market_world = world.clone();
        let aos_market = phase7_market_clearance(&mut aos_market_world, &partitions, 100, 0.1);
        let mut market_storage = storage.clone();
        let mut market_settlements = settlements.clone();
        let storage_market = phase7_market_clearance_storage(
            &mut market_storage,
            &mut market_settlements,
            &partitions,
            100,
            0.1,
        );
        assert_eq!(storage_market, aos_market, "case={case_index}");
        assert_eq!(
            market_settlements, aos_market_world.settlements,
            "case={case_index}"
        );

        let mut aos_duplicate_market = world.clone();
        let aos_duplicate_market_result =
            phase7_market_clearance(&mut aos_duplicate_market, &duplicate_partitions, 100, 0.1);
        let mut storage_duplicate_market = storage.clone();
        let mut duplicate_market_settlements = settlements.clone();
        let storage_duplicate_market_result = phase7_market_clearance_storage(
            &mut storage_duplicate_market,
            &mut duplicate_market_settlements,
            &duplicate_partitions,
            100,
            0.1,
        );
        assert_eq!(
            storage_duplicate_market_result, aos_duplicate_market_result,
            "duplicate market partition case={case_index}"
        );
    }

    assert!(saw_duplicate);
    assert!(saw_missing);
    assert!(saw_unsorted);
}
