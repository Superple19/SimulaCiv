use rayon::ThreadPool;
use sim_core::{AgentId, DenseSlot, GroupId, SimulationDay};
use sim_model::features::{AgentFeatures, FeatureVector};
use sim_model::hashing::{canonical_event_hash, canonical_metrics_hash};
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, M0RunError, Phase4Backend, Phase4ExecutionPolicy,
    run_hybrid_authority_days, run_hybrid_authority_days_with_phase4_policy,
    run_hybrid_authority_days_with_rayon_phase4,
};
use sim_model::snapshot::restore_snapshot;
use sim_model::state::{AgentState, HybridWorldState};
use sim_model::storage::SegmentedAgentStorage;
use sim_model::{
    Action, DecisionError, Intent, IntentError, Phase3ScarcityScratch, Phase4CandidateIndexScratch,
    PrimaryActionChoice, SimConfig, generate_intents_storage_with_candidate_index,
    generate_intents_storage_with_candidate_index_rayon, initialize_world,
    phase3_observation_and_features_storage_with_scratch,
    phase4_primary_action_selection_storage_into,
    phase4_primary_action_selection_storage_into_rayon,
};
use std::num::NonZeroUsize;

const GATE_CONFIG: &str = r#"
[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 10
settlement_count = 2
initial_health = 1.0
initial_food = 25.0
initial_wealth = 10000
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

const EXPECTED_STATE: &str = "5b396f23a8195fd7155a7b9577b0eaca265e59768a81f0cafd8ab68c0d9d67b9";
const EXPECTED_METRICS: &str = "ffbadbfda9bba1f799d4e72eac222e4e58deca4905ee8447a44ece8cec3baa3b";
const EXPECTED_EVENTS: &str = "2a40e01a7cd0b981eba037a14cf2f40c748ae0ff9e0df290ed802ba8b0c51cac";

fn config() -> SimConfig {
    SimConfig::parse_and_validate(GATE_CONFIG).expect("gate config")
}

fn pools() -> Vec<(usize, ThreadPool)> {
    [1, 2, 4, 6, 12]
        .into_iter()
        .map(|threads| {
            (
                threads,
                rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .build()
                    .expect("thread pool"),
            )
        })
        .collect()
}

fn agents(count: usize) -> Vec<AgentState> {
    (0..count)
        .map(|i| AgentState {
            agent_id: AgentId(i as u32),
            dense_slot: DenseSlot(i as u32),
            alive: true,
            birth_day: SimulationDay(0),
            health: 1.0,
            food: if i % 3 == 0 { 2.0 } else { 10.0 },
            wealth: 1000,
            productivity: 0.8 + (i % 7) as f32 * 0.1,
            cooperation: (i % 5) as f32 * 0.2,
            aggression: (i % 4) as f32 * 0.2,
            risk_tolerance: (i % 6) as f32 * 0.15,
            group_id: GroupId(((i % 4) * 7) as u16),
        })
        .collect()
}

fn seeded_shuffle<T>(values: &mut [T], mut state: u64) {
    for index in (1..values.len()).rev() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        values.swap(index, (state as usize) % (index + 1));
    }
}

fn action_for(i: usize) -> Action {
    Action::ALL[i % Action::ALL.len()]
}

fn choices(count: usize, action: impl Fn(usize) -> Action) -> Vec<PrimaryActionChoice> {
    (0..count)
        .map(|i| PrimaryActionChoice {
            agent_id: AgentId(i as u32),
            action: action(i),
        })
        .collect()
}

fn features(count: usize, pattern: bool) -> Vec<AgentFeatures> {
    (0..count)
        .map(|i| {
            let values = if pattern {
                match i % 6 {
                    0 => [1.0, 0.0, 0.0, 0.0, 0.0],
                    1 => [0.0, 1.0, 0.0, 0.0, 0.0],
                    2 => [0.0, 0.0, 1.0, 0.0, 0.0],
                    3 => [0.0, 0.0, 0.0, 1.0, 0.0],
                    4 => [0.0, 0.0, 0.0, 0.0, 1.0],
                    _ => [0.0; 5],
                }
            } else {
                let mut state = (i as u64).wrapping_add(0x9e37_79b9);
                let mut next = || {
                    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                    ((state >> 40) as u32) as f32 / (1u32 << 24) as f32
                };
                [next(), next(), next(), next(), next()]
            };
            AgentFeatures {
                agent_id: AgentId(i as u32),
                features: FeatureVector::new(values),
            }
        })
        .collect()
}

fn selection_config() -> SimConfig {
    let mut config = config();
    for action in 0..5 {
        config.decision.base_weight_matrix[action][action] = 100.0;
    }
    config.decision.action_biases[5] = 100.0;
    config
}

#[test]
fn selection_matches_serial_for_chunk_boundaries_and_shuffled_storage() {
    let config = selection_config();
    let pools = pools();
    for count in [0, 1, 63, 64, 65, 129] {
        for reversed in [false, true] {
            let mut agents = agents(count);
            if reversed {
                agents.reverse();
            }
            let storage = SegmentedAgentStorage::from_agents(&agents);
            let features = features(count, true);
            let mut serial = Vec::new();
            phase4_primary_action_selection_storage_into(
                &storage,
                SimulationDay(3),
                &config,
                &features,
                &mut serial,
            )
            .unwrap();
            if count >= 6 {
                let selected = serial
                    .iter()
                    .map(|choice| choice.action)
                    .collect::<std::collections::HashSet<_>>();
                assert_eq!(selected.len(), 6);
            }
            for (_, pool) in &pools {
                for chunk in [64, 128, 256, 512, 1024] {
                    let mut parallel = Vec::new();
                    phase4_primary_action_selection_storage_into_rayon(
                        &storage,
                        SimulationDay(3),
                        &config,
                        &features,
                        &mut parallel,
                        pool,
                        NonZeroUsize::new(chunk).unwrap(),
                    )
                    .unwrap();
                    assert_eq!(
                        parallel, serial,
                        "N={count}, reversed={reversed}, chunk={chunk}"
                    );
                }
            }
        }
    }

    let count = 257;
    let storage = SegmentedAgentStorage::from_agents(&agents(count));
    let random_features = features(count, false);
    let mut serial = Vec::new();
    phase4_primary_action_selection_storage_into(
        &storage,
        SimulationDay(17),
        &config,
        &random_features,
        &mut serial,
    )
    .unwrap();
    for (_, pool) in &pools {
        let mut parallel = Vec::new();
        phase4_primary_action_selection_storage_into_rayon(
            &storage,
            SimulationDay(17),
            &config,
            &random_features,
            &mut parallel,
            pool,
            NonZeroUsize::new(64).unwrap(),
        )
        .unwrap();
        assert_eq!(parallel, serial);
    }

    let uniform_config = SimConfig::parse_and_validate(GATE_CONFIG).expect("uniform config");
    let uniform_storage = SegmentedAgentStorage::from_agents(&agents(129));
    let uniform_features = features(129, true);
    let mut uniform_serial = Vec::new();
    phase4_primary_action_selection_storage_into(
        &uniform_storage,
        SimulationDay(0),
        &uniform_config,
        &uniform_features,
        &mut uniform_serial,
    )
    .unwrap();
    for (_, pool) in &pools {
        for chunk in [64, 128, 256, 512, 1024] {
            let mut uniform_parallel = Vec::new();
            phase4_primary_action_selection_storage_into_rayon(
                &uniform_storage,
                SimulationDay(0),
                &uniform_config,
                &uniform_features,
                &mut uniform_parallel,
                pool,
                NonZeroUsize::new(chunk).unwrap(),
            )
            .unwrap();
            assert_eq!(
                uniform_parallel, uniform_serial,
                "uniform utility boundary case"
            );
        }
    }
}

#[test]
fn selection_error_precedence_and_partial_output_match_serial() {
    let config = selection_config();
    let pools = pools();
    let mut agents = agents(2200);
    agents[17].health = 0.0;
    agents[70].alive = false;
    let storage = SegmentedAgentStorage::from_agents(&agents);
    let mut input = features(2200, true);
    input[1300].agent_id = AgentId(90_000);

    let mut serial_out = Vec::new();
    let serial_result = phase4_primary_action_selection_storage_into(
        &storage,
        SimulationDay(1),
        &config,
        &input,
        &mut serial_out,
    );
    assert_eq!(
        serial_result,
        Err(DecisionError::IneligibleAgent(AgentId(17)))
    );
    for (_, pool) in &pools {
        for chunk in [64, 256, 1024] {
            let mut parallel_out = Vec::new();
            let parallel_result = phase4_primary_action_selection_storage_into_rayon(
                &storage,
                SimulationDay(1),
                &config,
                &input,
                &mut parallel_out,
                pool,
                NonZeroUsize::new(chunk).unwrap(),
            );
            assert_eq!(parallel_result, serial_result);
            assert_eq!(parallel_out, serial_out);
        }
    }
}

#[test]
fn indexed_intents_match_serial_for_actions_groups_candidates_and_layouts() {
    let config = config();
    let pools = pools();
    for count in [1, 63, 64, 65, 129, 257] {
        for reversed in [false, true] {
            for action_mode in 0..6 {
                let mut values = agents(count);
                match action_mode {
                    0 => values.iter_mut().for_each(|agent| agent.food = 10.0),
                    1 => values.iter_mut().for_each(|agent| agent.food = 10.0),
                    2 => values.iter_mut().for_each(|agent| agent.food = 2.0),
                    3 => values.iter_mut().for_each(|agent| agent.food = 10.0),
                    _ => {}
                }
                if action_mode == 1 && count > 0 {
                    values[0].food = 2.0;
                }
                if action_mode == 3 {
                    for i in (0..count).step_by(2) {
                        values[i].food = 2.0;
                    }
                }
                if reversed {
                    values.reverse();
                }
                let storage = SegmentedAgentStorage::from_agents(&values);
                let action = match action_mode {
                    0 | 1 | 3 => Action::GiveFood,
                    2 => Action::StealFood,
                    5 => Action::Work,
                    _ => action_for(0),
                };
                let choices = choices(count, |i| {
                    if action_mode == 4 {
                        action_for(i)
                    } else {
                        action
                    }
                });
                let mut serial_index = Phase4CandidateIndexScratch::default();
                let mut serial = Vec::new();
                let serial_result = generate_intents_storage_with_candidate_index(
                    &storage,
                    SimulationDay(7),
                    &config,
                    &choices,
                    &mut serial_index,
                    &mut serial,
                );
                serial_result.unwrap();
                if action_mode == 0 {
                    assert!(serial.iter().all(|intent| match intent {
                        Intent::GiveFood {
                            target_agent_id, ..
                        } => target_agent_id.is_none(),
                        _ => false,
                    }));
                }
                if action_mode == 5 {
                    assert!(
                        serial
                            .iter()
                            .all(|intent| matches!(intent, Intent::Work { .. }))
                    );
                }
                if action_mode == 1 && count > 4 {
                    assert!(matches!(
                        serial[0],
                        Intent::GiveFood {
                            target_agent_id: None,
                            ..
                        }
                    ));
                    assert!(matches!(
                        serial[4],
                        Intent::GiveFood {
                            target_agent_id: Some(AgentId(0)),
                            ..
                        }
                    ));
                }
                if action_mode == 2 && count > 4 {
                    for intent in &serial {
                        if let Intent::StealFood {
                            agent_id,
                            target_agent_id: Some(target),
                            ..
                        } = intent
                        {
                            assert_ne!(*agent_id, *target, "initiator self-exclusion");
                        }
                    }
                }
                for (_, pool) in &pools {
                    for chunk in [64, 128, 256, 512, 1024] {
                        let mut index = Phase4CandidateIndexScratch::default();
                        let mut parallel = Vec::new();
                        generate_intents_storage_with_candidate_index_rayon(
                            &storage,
                            SimulationDay(7),
                            &config,
                            &choices,
                            &mut index,
                            &mut parallel,
                            pool,
                            NonZeroUsize::new(chunk).unwrap(),
                        )
                        .unwrap();
                        assert_eq!(
                            parallel, serial,
                            "N={count}, reversed={reversed}, mode={action_mode}, chunk={chunk}"
                        );
                    }
                }
            }
        }
    }

    let mut random_agents = agents(257);
    let mut state = 0x6a09_e667_f3bc_c909u64;
    for agent in &mut random_agents {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        agent.group_id = GroupId((((state >> 32) % 7) * 9) as u16);
        agent.food = ((state >> 40) % 1200) as f32 / 100.0;
    }
    seeded_shuffle(&mut random_agents, 0xbb67_ae85_84ca_a73bu64);
    let random_storage = SegmentedAgentStorage::from_agents(&random_agents);
    let mut state = 0x3c6e_f372_fe94_f82bu64;
    let random_choices = (0..257)
        .map(|i| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            PrimaryActionChoice {
                agent_id: AgentId(i as u32),
                action: Action::ALL[((state >> 32) % 6) as usize],
            }
        })
        .collect::<Vec<_>>();
    let mut serial_index = Phase4CandidateIndexScratch::default();
    let mut serial_intents = Vec::new();
    generate_intents_storage_with_candidate_index(
        &random_storage,
        SimulationDay(19),
        &config,
        &random_choices,
        &mut serial_index,
        &mut serial_intents,
    )
    .unwrap();
    for (_, pool) in &pools {
        for chunk in [64, 128, 256, 512, 1024] {
            let mut index = Phase4CandidateIndexScratch::default();
            let mut intents = Vec::new();
            generate_intents_storage_with_candidate_index_rayon(
                &random_storage,
                SimulationDay(19),
                &config,
                &random_choices,
                &mut index,
                &mut intents,
                pool,
                NonZeroUsize::new(chunk).unwrap(),
            )
            .unwrap();
            assert_eq!(intents, serial_intents, "seeded randomized Intent case");
        }
    }
}

#[test]
fn intent_error_precedence_and_partial_outputs_match_serial() {
    let config = config();
    let pools = pools();
    let mut values = agents(2200);
    values[1300].alive = false;
    let storage = SegmentedAgentStorage::from_agents(&values);
    let mut input = choices(2200, |_| Action::Work);
    input[64].agent_id = AgentId(90_000);

    let mut serial_index = Phase4CandidateIndexScratch::default();
    let mut serial_out = Vec::new();
    let serial_result = generate_intents_storage_with_candidate_index(
        &storage,
        SimulationDay(2),
        &config,
        &input,
        &mut serial_index,
        &mut serial_out,
    );
    assert_eq!(
        serial_result,
        Err(IntentError::MissingAgent(AgentId(90_000)))
    );
    for (_, pool) in &pools {
        for chunk in [64, 256, 1024] {
            let mut index = Phase4CandidateIndexScratch::default();
            let mut parallel_out = Vec::new();
            let parallel_result = generate_intents_storage_with_candidate_index_rayon(
                &storage,
                SimulationDay(2),
                &config,
                &input,
                &mut index,
                &mut parallel_out,
                pool,
                NonZeroUsize::new(chunk).unwrap(),
            );
            assert_eq!(parallel_result, serial_result);
            assert_eq!(parallel_out, serial_out);
        }
    }

    let mut duplicate = vec![
        PrimaryActionChoice {
            agent_id: AgentId(0),
            action: Action::Work,
        },
        PrimaryActionChoice {
            agent_id: AgentId(1),
            action: Action::Work,
        },
        PrimaryActionChoice {
            agent_id: AgentId(1),
            action: Action::Work,
        },
    ];
    duplicate[0].action = Action::Work;
    let mut serial_index = Phase4CandidateIndexScratch::default();
    let mut serial_out = Vec::new();
    let serial_result = generate_intents_storage_with_candidate_index(
        &storage,
        SimulationDay(2),
        &config,
        &duplicate,
        &mut serial_index,
        &mut serial_out,
    );
    assert_eq!(serial_result, Err(IntentError::DuplicateChoice(AgentId(1))));
    for (_, pool) in &pools {
        let mut index = Phase4CandidateIndexScratch::default();
        let mut parallel_out = Vec::new();
        let parallel_result = generate_intents_storage_with_candidate_index_rayon(
            &storage,
            SimulationDay(2),
            &config,
            &duplicate,
            &mut index,
            &mut parallel_out,
            pool,
            NonZeroUsize::new(64).unwrap(),
        );
        assert_eq!(parallel_result, serial_result);
        assert_eq!(parallel_out, serial_out);
    }

    let missing_choice = choices(2199, |_| Action::Work);
    let mut serial_index = Phase4CandidateIndexScratch::default();
    let mut serial_out = Vec::new();
    let serial_result = generate_intents_storage_with_candidate_index(
        &storage,
        SimulationDay(2),
        &config,
        &missing_choice,
        &mut serial_index,
        &mut serial_out,
    );
    for (_, pool) in &pools {
        let mut index = Phase4CandidateIndexScratch::default();
        let mut parallel_out = Vec::new();
        let parallel_result = generate_intents_storage_with_candidate_index_rayon(
            &storage,
            SimulationDay(2),
            &config,
            &missing_choice,
            &mut index,
            &mut parallel_out,
            pool,
            NonZeroUsize::new(256).unwrap(),
        );
        assert_eq!(parallel_result, serial_result);
        assert_eq!(parallel_out, serial_out);
    }
}

#[test]
fn end_to_end_phase4_choices_and_indexed_intents_match_all_pool_settings() {
    let config = selection_config();
    let storage = SegmentedAgentStorage::from_agents(&agents(257));
    let input_features = features(257, false);
    let mut serial_choices = Vec::new();
    phase4_primary_action_selection_storage_into(
        &storage,
        SimulationDay(9),
        &config,
        &input_features,
        &mut serial_choices,
    )
    .unwrap();
    let mut serial_index = Phase4CandidateIndexScratch::default();
    let mut serial_intents = Vec::new();
    generate_intents_storage_with_candidate_index(
        &storage,
        SimulationDay(9),
        &config,
        &serial_choices,
        &mut serial_index,
        &mut serial_intents,
    )
    .unwrap();
    for (_, pool) in pools() {
        for chunk in [64, 128, 256, 512, 1024] {
            let mut rayon_choices = Vec::new();
            phase4_primary_action_selection_storage_into_rayon(
                &storage,
                SimulationDay(9),
                &config,
                &input_features,
                &mut rayon_choices,
                &pool,
                NonZeroUsize::new(chunk).unwrap(),
            )
            .unwrap();
            let mut index = Phase4CandidateIndexScratch::default();
            let mut rayon_intents = Vec::new();
            generate_intents_storage_with_candidate_index_rayon(
                &storage,
                SimulationDay(9),
                &config,
                &rayon_choices,
                &mut index,
                &mut rayon_intents,
                &pool,
                NonZeroUsize::new(chunk).unwrap(),
            )
            .unwrap();
            assert_eq!(rayon_choices, serial_choices);
            assert_eq!(rayon_intents, serial_intents);
        }
    }
}

fn gate_context() -> M0RunContext {
    M0RunContext::new(81985529216486895, 7)
}

fn options(snapshot_boundary: bool) -> DayExecutionOptions {
    DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary,
    }
}

fn collect(
    outcomes: Vec<sim_model::runner::DayOutcome>,
    metrics: &mut Vec<sim_model::DailyMetrics>,
    events: &mut Vec<sim_model::EventRecord>,
) {
    for outcome in outcomes {
        if let Some(day_metrics) = outcome.metrics {
            metrics.push(day_metrics);
        }
        events.extend(outcome.events);
    }
}

#[test]
fn hybrid_trajectories_match_for_three_and_forty_days_at_all_thread_counts() {
    let config = config();
    let context = gate_context();
    for (threads, pool) in pools() {
        for chunk in [64, 128, 256, 512, 1024] {
            for days in [3, 40] {
                let mut serial = HybridWorldState::hybrid(initialize_world(&config).unwrap());
                let mut parallel = serial.clone();
                let opts = options(false);
                let serial_outcomes =
                    run_hybrid_authority_days(&mut serial, &config, &context, days, &opts).unwrap();
                let parallel_outcomes = run_hybrid_authority_days_with_rayon_phase4(
                    &mut parallel,
                    &config,
                    &context,
                    days,
                    &opts,
                    &pool,
                    NonZeroUsize::new(chunk).unwrap(),
                )
                .unwrap();
                assert_eq!(
                    parallel_outcomes, serial_outcomes,
                    "threads={threads}, chunk={chunk}, days={days}"
                );
                assert_eq!(
                    parallel, serial,
                    "threads={threads}, chunk={chunk}, days={days}"
                );
            }
        }
    }
}

#[test]
fn canonical_hashes_match_existing_gate_for_one_six_and_twelve_threads() {
    let config = config();
    let context = gate_context();
    for (threads, pool) in pools()
        .into_iter()
        .filter(|(threads, _)| [1, 6, 12].contains(threads))
    {
        let mut serial = HybridWorldState::hybrid(initialize_world(&config).unwrap());
        let mut parallel = serial.clone();
        let mut serial_metrics = Vec::with_capacity(500);
        let mut serial_events = Vec::new();
        let mut rayon_metrics = Vec::with_capacity(500);
        let mut rayon_events = Vec::new();
        for (days, boundary) in [(199, false), (1, true), (300, false)] {
            let opts = options(boundary);
            collect(
                run_hybrid_authority_days(&mut serial, &config, &context, days, &opts).unwrap(),
                &mut serial_metrics,
                &mut serial_events,
            );
            collect(
                run_hybrid_authority_days_with_rayon_phase4(
                    &mut parallel,
                    &config,
                    &context,
                    days,
                    &opts,
                    &pool,
                    NonZeroUsize::new(256).unwrap(),
                )
                .unwrap(),
                &mut rayon_metrics,
                &mut rayon_events,
            );
        }
        assert_eq!(parallel, serial, "threads={threads}");
        assert_eq!(rayon_metrics, serial_metrics, "threads={threads}");
        assert_eq!(rayon_events, serial_events, "threads={threads}");
        assert_eq!(
            parallel.canonical_state_hash().unwrap().to_hex(),
            EXPECTED_STATE
        );
        assert_eq!(
            canonical_metrics_hash(&rayon_metrics).unwrap().to_hex(),
            EXPECTED_METRICS
        );
        assert_eq!(
            canonical_event_hash(&rayon_events).unwrap().to_hex(),
            EXPECTED_EVENTS
        );
    }
}

#[test]
fn snapshots_match_on_days_100_250_and_500() {
    let config = config();
    let context = gate_context();
    for (threads, pool) in pools() {
        for chunk in [64, 128, 256, 512, 1024] {
            let mut serial = HybridWorldState::hybrid(initialize_world(&config).unwrap());
            let mut parallel = serial.clone();
            for (advance, boundary) in [
                (99, false),
                (1, true),
                (149, false),
                (1, true),
                (249, false),
                (1, true),
            ] {
                let opts = options(boundary);
                let serial_outcomes =
                    run_hybrid_authority_days(&mut serial, &config, &context, advance, &opts)
                        .unwrap();
                let rayon_outcomes = run_hybrid_authority_days_with_rayon_phase4(
                    &mut parallel,
                    &config,
                    &context,
                    advance,
                    &opts,
                    &pool,
                    NonZeroUsize::new(chunk).unwrap(),
                )
                .unwrap();
                assert_eq!(
                    rayon_outcomes, serial_outcomes,
                    "threads={threads}, chunk={chunk}"
                );
                assert_eq!(parallel, serial, "threads={threads}, chunk={chunk}");
                if boundary {
                    assert!(serial_outcomes[0].snapshot.is_some());
                    assert_eq!(rayon_outcomes[0].snapshot, serial_outcomes[0].snapshot);
                }
            }
        }
    }
}

#[test]
fn empty_indexed_intent_input_matches_serial() {
    let config = config();
    let storage = SegmentedAgentStorage::from_agents(&[]);
    let mut index = Phase4CandidateIndexScratch::default();
    let mut serial = Vec::<Intent>::new();
    let serial_result = generate_intents_storage_with_candidate_index(
        &storage,
        SimulationDay(0),
        &config,
        &[],
        &mut index,
        &mut serial,
    );
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let mut index = Phase4CandidateIndexScratch::default();
    let mut parallel = Vec::new();
    let parallel_result = generate_intents_storage_with_candidate_index_rayon(
        &storage,
        SimulationDay(0),
        &config,
        &[],
        &mut index,
        &mut parallel,
        &pool,
        NonZeroUsize::new(64).unwrap(),
    );
    assert_eq!(parallel_result, serial_result);
    assert_eq!(parallel, serial);
}

#[test]
fn execution_policy_threshold_is_explicit_and_inclusive() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(6)
        .build()
        .unwrap();
    let auto = Phase4ExecutionPolicy::Auto {
        pool: &pool,
        threshold: NonZeroUsize::new(7500).unwrap(),
        chunk_size: NonZeroUsize::new(256).unwrap(),
    };
    for count in [100, 250, 500, 1000, 7499] {
        assert_eq!(auto.backend_for(count), Phase4Backend::Serial, "N={count}");
    }
    assert_eq!(auto.backend_for(7500), Phase4Backend::Rayon);
    assert_eq!(auto.backend_for(7501), Phase4Backend::Rayon);
    assert_eq!(
        Phase4ExecutionPolicy::Serial.backend_for(usize::MAX),
        Phase4Backend::Serial
    );
    assert_eq!(
        Phase4ExecutionPolicy::Rayon {
            pool: &pool,
            chunk_size: NonZeroUsize::new(256).unwrap(),
        }
        .backend_for(0),
        Phase4Backend::Rayon
    );
    let one_thread_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    assert_eq!(
        Phase4ExecutionPolicy::Auto {
            pool: &one_thread_pool,
            threshold: NonZeroUsize::new(1).unwrap(),
            chunk_size: NonZeroUsize::new(128).unwrap(),
        }
        .backend_for(usize::MAX),
        Phase4Backend::Serial
    );
    assert_eq!(pool.current_num_threads(), 6);
    assert!(NonZeroUsize::new(0).is_none());
}

#[test]
fn serial_explicit_rayon_and_auto_policies_match_short_trajectories() {
    let config = config();
    let context = gate_context();
    let options = options(false);
    for (threads, pool) in pools().into_iter().filter(|(threads, _)| *threads > 1) {
        for chunk in [128, 256] {
            let serial_initial = HybridWorldState::hybrid(initialize_world(&config).unwrap());
            let mut serial = serial_initial.clone();
            let expected =
                run_hybrid_authority_days(&mut serial, &config, &context, 40, &options).unwrap();

            for policy in [
                Phase4ExecutionPolicy::Rayon {
                    pool: &pool,
                    chunk_size: NonZeroUsize::new(chunk).unwrap(),
                },
                Phase4ExecutionPolicy::Auto {
                    pool: &pool,
                    threshold: NonZeroUsize::new(5).unwrap(),
                    chunk_size: NonZeroUsize::new(chunk).unwrap(),
                },
                Phase4ExecutionPolicy::Auto {
                    pool: &pool,
                    threshold: NonZeroUsize::new(100).unwrap(),
                    chunk_size: NonZeroUsize::new(chunk).unwrap(),
                },
            ] {
                let mut candidate = serial_initial.clone();
                let actual = run_hybrid_authority_days_with_phase4_policy(
                    &mut candidate,
                    &config,
                    &context,
                    40,
                    &options,
                    policy,
                )
                .unwrap();
                assert_eq!(actual, expected, "threads={threads}, chunk={chunk}");
                assert_eq!(candidate, serial, "threads={threads}, chunk={chunk}");
            }
        }
    }
}

#[test]
fn runner_level_errors_are_policy_independent() {
    let config = config();
    let context = gate_context();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    let options = options(false);
    let mut initial = HybridWorldState::hybrid(initialize_world(&config).unwrap());
    initial.world.current_day = SimulationDay(u32::MAX);
    let serial_result =
        run_hybrid_authority_days(&mut initial.clone(), &config, &context, 1, &options);
    assert_eq!(serial_result, Err(M0RunError::DayOverflow));
    for policy in [
        Phase4ExecutionPolicy::Rayon {
            pool: &pool,
            chunk_size: NonZeroUsize::new(256).unwrap(),
        },
        Phase4ExecutionPolicy::Auto {
            pool: &pool,
            threshold: NonZeroUsize::new(5).unwrap(),
            chunk_size: NonZeroUsize::new(256).unwrap(),
        },
    ] {
        let before = initial.clone();
        let mut candidate = before.clone();
        let result = run_hybrid_authority_days_with_phase4_policy(
            &mut candidate,
            &config,
            &context,
            1,
            &options,
            policy,
        );
        assert_eq!(result, serial_result);
        assert_eq!(candidate, before);
    }

    let mut legacy = HybridWorldState::legacy(initialize_world(&config).unwrap());
    assert!(matches!(
        run_hybrid_authority_days_with_phase4_policy(
            &mut legacy,
            &config,
            &context,
            1,
            &options,
            Phase4ExecutionPolicy::Auto {
                pool: &pool,
                threshold: NonZeroUsize::new(5).unwrap(),
                chunk_size: NonZeroUsize::new(256).unwrap(),
            },
        ),
        Err(M0RunError::InvariantViolation(_))
    ));
}

fn low_food_crossing_config() -> SimConfig {
    let mut config = config();
    config.world.initial_population = 5;
    config.world.initial_food = 0.0;
    config.environment.base_metabolic_cost = 1.0;
    config.environment.health_decay_rate = 0.1;
    config
}

fn next_phase4_item_count(
    world: &HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
) -> usize {
    let mut effective_config = config.clone();
    effective_config.world.master_seed = context.master_seed;
    effective_config.world.replicate_id = context.replicate_id;
    let mut storage = world.segmented_storage.as_ref().unwrap().clone();
    storage.phase2_degradation_with_config(&effective_config);
    let mut features = Vec::new();
    let mut scratch = Phase3ScarcityScratch::with_capacity(world.world.settlements.len());
    phase3_observation_and_features_storage_with_scratch(
        &storage,
        &world.world.settlements,
        &effective_config,
        &mut features,
        &mut scratch,
    )
    .unwrap();
    features.len()
}

#[test]
fn auto_policy_switches_on_current_eligible_count_and_survives_synthetic_crossings() {
    let config = low_food_crossing_config();
    let context = gate_context();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    let policy = Phase4ExecutionPolicy::Auto {
        pool: &pool,
        threshold: NonZeroUsize::new(5).unwrap(),
        chunk_size: NonZeroUsize::new(128).unwrap(),
    };
    let options = options(false);

    let mut initial_world = initialize_world(&config).unwrap();
    initial_world.agents[0].health = 0.2;
    for agent in &mut initial_world.agents {
        agent.food = 0.0;
    }
    let initial = HybridWorldState::hybrid(initial_world);
    assert_eq!(next_phase4_item_count(&initial, &config, &context), 5);
    assert_eq!(policy.backend_for(5), Phase4Backend::Rayon);

    let mut after_first = initial.clone();
    run_hybrid_authority_days(&mut after_first, &config, &context, 1, &options).unwrap();
    assert_eq!(next_phase4_item_count(&after_first, &config, &context), 4);
    assert_eq!(policy.backend_for(4), Phase4Backend::Serial);

    let mut auto_world = initial.clone();
    let mut serial_world = initial.clone();
    let auto_outcomes = run_hybrid_authority_days_with_phase4_policy(
        &mut auto_world,
        &config,
        &context,
        2,
        &options,
        policy,
    )
    .unwrap();
    let serial_outcomes =
        run_hybrid_authority_days(&mut serial_world, &config, &context, 2, &options).unwrap();
    assert_eq!(auto_outcomes, serial_outcomes);
    assert_eq!(auto_world, serial_world);

    // Natural eligibility only falls in this model. Synthesize an external population increase
    // between calls to prove Auto can also move from its serial to its Rayon zone.
    let mut below_initial = initialize_world(&config).unwrap();
    below_initial.agents[4].health = 0.0;
    below_initial.agents[4].food = 0.0;
    let below_world = HybridWorldState::hybrid(below_initial);
    let mut below_auto = below_world.clone();
    let mut below_serial = below_world;
    assert_eq!(next_phase4_item_count(&below_auto, &config, &context), 4);
    assert_eq!(policy.backend_for(4), Phase4Backend::Serial);
    run_hybrid_authority_days_with_phase4_policy(
        &mut below_auto,
        &config,
        &context,
        1,
        &options,
        policy,
    )
    .unwrap();
    run_hybrid_authority_days(&mut below_serial, &config, &context, 1, &options).unwrap();
    for world in [&mut below_auto, &mut below_serial] {
        let storage = world.segmented_storage.as_mut().unwrap();
        storage.alive_mut()[4] = true;
        storage.health_mut()[4] = 0.5;
        world.world.agents[4].alive = true;
        world.world.agents[4].health = 0.5;
    }
    assert_eq!(next_phase4_item_count(&below_auto, &config, &context), 5);
    assert_eq!(policy.backend_for(5), Phase4Backend::Rayon);
    let auto_outcome = run_hybrid_authority_days_with_phase4_policy(
        &mut below_auto,
        &config,
        &context,
        1,
        &options,
        policy,
    )
    .unwrap();
    let serial_outcome =
        run_hybrid_authority_days(&mut below_serial, &config, &context, 1, &options).unwrap();
    assert_eq!(auto_outcome, serial_outcome);
    assert_eq!(below_auto, below_serial);
}

#[test]
fn auto_policy_snapshot_restore_and_500_day_hashes_match_serial() {
    let config = config();
    let context = gate_context();
    let normal_options = options(false);
    for target_day in [100u32, 250, 500] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(6)
            .build()
            .unwrap();
        let policy = Phase4ExecutionPolicy::Auto {
            pool: &pool,
            threshold: NonZeroUsize::new(5).unwrap(),
            chunk_size: NonZeroUsize::new(128).unwrap(),
        };
        let mut serial = HybridWorldState::hybrid(initialize_world(&config).unwrap());
        let mut automatic = serial.clone();
        let before_boundary = target_day - 1;
        if before_boundary > 0 {
            let serial_prior = run_hybrid_authority_days(
                &mut serial,
                &config,
                &context,
                before_boundary,
                &normal_options,
            )
            .unwrap();
            let auto_prior = run_hybrid_authority_days_with_phase4_policy(
                &mut automatic,
                &config,
                &context,
                before_boundary,
                &normal_options,
                policy,
            )
            .unwrap();
            assert_eq!(auto_prior, serial_prior);
            assert_eq!(automatic, serial);
        }
        let boundary_options = options(true);
        let serial_boundary =
            run_hybrid_authority_days(&mut serial, &config, &context, 1, &boundary_options)
                .unwrap();
        let auto_boundary = run_hybrid_authority_days_with_phase4_policy(
            &mut automatic,
            &config,
            &context,
            1,
            &boundary_options,
            policy,
        )
        .unwrap();
        assert_eq!(auto_boundary, serial_boundary);
        let serial_snapshot = serial_boundary[0].snapshot.as_ref().unwrap();
        let auto_snapshot = auto_boundary[0].snapshot.as_ref().unwrap();
        assert_eq!(auto_snapshot, serial_snapshot, "resume day {target_day}");

        let mut restored_serial =
            HybridWorldState::hybrid(restore_snapshot(serial_snapshot).unwrap().world);
        let mut restored_auto =
            HybridWorldState::hybrid(restore_snapshot(auto_snapshot).unwrap().world);
        let serial_continuous =
            run_hybrid_authority_days(&mut serial, &config, &context, 25, &normal_options).unwrap();
        let serial_resumed =
            run_hybrid_authority_days(&mut restored_serial, &config, &context, 25, &normal_options)
                .unwrap();
        let auto_continuous = run_hybrid_authority_days_with_phase4_policy(
            &mut automatic,
            &config,
            &context,
            25,
            &normal_options,
            policy,
        )
        .unwrap();
        let auto_resumed = run_hybrid_authority_days_with_phase4_policy(
            &mut restored_auto,
            &config,
            &context,
            25,
            &normal_options,
            policy,
        )
        .unwrap();
        assert_eq!(serial_resumed, serial_continuous);
        assert_eq!(auto_resumed, auto_continuous);
        assert_eq!(auto_continuous, serial_continuous);
        assert_eq!(restored_serial, serial);
        assert_eq!(restored_auto, automatic);
        assert_eq!(automatic, serial);
    }

    let pool6 = rayon::ThreadPoolBuilder::new()
        .num_threads(6)
        .build()
        .unwrap();
    let pool12 = rayon::ThreadPoolBuilder::new()
        .num_threads(12)
        .build()
        .unwrap();
    for pool in [&pool6, &pool12] {
        for auto in [false, true] {
            let policy = if auto {
                Phase4ExecutionPolicy::Auto {
                    pool,
                    threshold: NonZeroUsize::new(7500).unwrap(),
                    chunk_size: NonZeroUsize::new(128).unwrap(),
                }
            } else {
                Phase4ExecutionPolicy::Rayon {
                    pool,
                    chunk_size: NonZeroUsize::new(256).unwrap(),
                }
            };
            let mut serial_world = HybridWorldState::hybrid(initialize_world(&config).unwrap());
            let mut policy_world = serial_world.clone();
            let mut serial_outcomes = Vec::with_capacity(500);
            let mut policy_outcomes = Vec::with_capacity(500);
            for (days, boundary) in [(199, false), (1, true), (300, false)] {
                let execution_options = options(boundary);
                serial_outcomes.extend(
                    run_hybrid_authority_days(
                        &mut serial_world,
                        &config,
                        &context,
                        days,
                        &execution_options,
                    )
                    .unwrap(),
                );
                policy_outcomes.extend(
                    run_hybrid_authority_days_with_phase4_policy(
                        &mut policy_world,
                        &config,
                        &context,
                        days,
                        &execution_options,
                        policy,
                    )
                    .unwrap(),
                );
            }
            assert_eq!(policy_outcomes, serial_outcomes, "auto={auto}");
            assert_eq!(policy_world, serial_world, "auto={auto}");
            let metrics = policy_outcomes
                .iter()
                .filter_map(|outcome| outcome.metrics.clone())
                .collect::<Vec<_>>();
            let events = policy_outcomes
                .iter()
                .flat_map(|outcome| outcome.events.clone())
                .collect::<Vec<_>>();
            assert_eq!(
                policy_world.canonical_state_hash().unwrap().to_hex(),
                EXPECTED_STATE
            );
            assert_eq!(
                canonical_metrics_hash(&metrics).unwrap().to_hex(),
                EXPECTED_METRICS
            );
            assert_eq!(
                canonical_event_hash(&events).unwrap().to_hex(),
                EXPECTED_EVENTS
            );
        }
    }
}
