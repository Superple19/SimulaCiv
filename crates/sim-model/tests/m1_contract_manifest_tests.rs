//! Comprehensive consistency test suite for M1-02: Machine-Readable M1 Contract Manifest.
//!
//! Verifies that `contracts/m1_contract.toml` is a bit-exact, mechanically verifiable
//! projection of the frozen M1 contracts in `docs/contracts/M1_CONTRACT_FREEZE.md`
//! and the authoritative M0 reference oracle.

use std::path::Path;

use sim_core::{
    AgentId, DenseSlot, GroupId, Money, RngCoordinate, SimulationDay, coordinate_prng_f32,
    coordinate_prng_u64, u64_to_f32,
};
use sim_model::decision::Action;
use sim_model::events::{
    Event, EventKey, EventRecord, GLOBAL_PARTITION_KEY, ObservationEvent, StateTransitionEvent,
};
use sim_model::hashing::{
    canonical_event_bytes, canonical_event_hash, canonical_metrics_bytes, canonical_metrics_hash,
    canonical_state_bytes, canonical_state_hash,
};
use sim_model::metrics::DailyMetrics;
use sim_model::resolution::TargetedActionKind;
use sim_model::runner::{DayExecutionOptions, M0RunContext, run_m0_day};
use sim_model::snapshot::{SNAPSHOT_MAGIC, SnapshotMetadata, encode_snapshot};
use sim_model::state::{AgentState, SettlementState, WorldState};
use sim_model::subsystems::Subsystem;
use sim_model::{SimConfig, initialize_world};

const MANIFEST_STR: &str = include_str!("../../../contracts/m1_contract.toml");

// =========================================================================
// Hex Parsing Helpers
// =========================================================================

fn parse_hex_u64(s: &str) -> Result<u64, String> {
    let clean = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .ok_or_else(|| format!("expected '0x' prefix on u64 hex string: '{}'", s))?;
    if clean.chars().any(|c| c.is_ascii_uppercase()) {
        return Err(format!("uppercase hex not allowed: '{}'", s));
    }
    u64::from_str_radix(clean, 16).map_err(|e| format!("invalid hex u64 '{}': {}", s, e))
}

fn parse_hex_bytes(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) {
        return Err(format!("odd hex string length: {}", s.len()));
    }
    if s.chars().any(|c| c.is_ascii_uppercase()) {
        return Err(format!("uppercase hex not allowed: '{}'", s));
    }
    let mut bytes = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        let byte = u8::from_str_radix(&s[i..i + 2], 16)
            .map_err(|e| format!("invalid hex byte '{}': {}", &s[i..i + 2], e))?;
        bytes.push(byte);
    }
    Ok(bytes)
}

fn parse_manifest() -> toml::Value {
    toml::from_str(MANIFEST_STR).expect("contracts/m1_contract.toml must parse as valid TOML")
}

// =========================================================================
// Test Fixture Helpers
// =========================================================================

#[allow(clippy::too_many_arguments)]
fn make_test_agent(
    id: u32,
    slot: u32,
    alive: bool,
    birth_day: u32,
    health: f32,
    food: f32,
    wealth: Money,
    productivity: f32,
    cooperation: f32,
    aggression: f32,
    risk_tolerance: f32,
    group_id: u16,
) -> AgentState {
    AgentState {
        agent_id: AgentId(id),
        dense_slot: DenseSlot(slot),
        alive,
        birth_day: SimulationDay(birth_day),
        health,
        food,
        wealth,
        productivity,
        cooperation,
        aggression,
        risk_tolerance,
        group_id: GroupId(group_id),
    }
}

fn make_test_settlement(group_id: u16, resource: f32, treasury: Money) -> SettlementState {
    SettlementState {
        group_id: GroupId(group_id),
        resource,
        treasury,
    }
}

fn make_test_world(
    day: u32,
    agents: Vec<AgentState>,
    settlements: Vec<SettlementState>,
) -> WorldState {
    let mut initial_money_supply: Money = 0;
    for a in &agents {
        initial_money_supply = initial_money_supply.saturating_add(a.wealth);
    }
    for s in &settlements {
        initial_money_supply = initial_money_supply.saturating_add(s.treasury);
    }

    WorldState {
        current_day: SimulationDay(day),
        agents,
        settlements,
        initial_money_supply,
    }
}

const GATE_CONFIG_TOML: &str = r#"
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

// =========================================================================
// Tests 1–25: Manifest Verification Suite
// =========================================================================

#[test]
fn test_01_manifest_toml_parses_successfully() {
    let manifest = parse_manifest();
    assert!(manifest.is_table(), "manifest must be a TOML table");
}

#[test]
fn test_02_manifest_schema_version_is_one() {
    let manifest = parse_manifest();
    let schema_ver = manifest["manifest"]["schema_version"]
        .as_integer()
        .expect("manifest.schema_version must be an integer");
    assert_eq!(schema_ver, 1, "manifest schema_version must be 1");
}

#[test]
fn test_03_contract_version_is_m1() {
    let manifest = parse_manifest();
    let contract_ver = manifest["manifest"]["contract_version"]
        .as_str()
        .expect("manifest.contract_version must be a string");
    assert_eq!(contract_ver, "M1", "manifest contract_version must be M1");
}

#[test]
fn test_04_semantic_authority_path_is_correct() {
    let manifest = parse_manifest();
    let authority_rel = manifest["manifest"]["semantic_authority"]
        .as_str()
        .expect("manifest.semantic_authority must be a string");
    assert_eq!(
        authority_rel, "docs/contracts/M1_CONTRACT_FREEZE.md",
        "semantic_authority must point to M1_CONTRACT_FREEZE.md"
    );

    let authority_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .join(authority_rel);
    assert!(
        authority_path.exists(),
        "semantic authority file must physically exist at {:?}",
        authority_path
    );
}

#[test]
fn test_05_phase_sequence_matches_frozen_m1_lifecycle() {
    let manifest = parse_manifest();
    let phases = manifest["phase"]
        .as_array()
        .expect("phase must be an array of tables");
    assert_eq!(phases.len(), 12, "there must be exactly 12 phase entries");

    let expected_phases = [
        ("1", "environment_regrowth"),
        ("2", "biological_degradation"),
        ("3", "feature_extraction"),
        ("4", "decision_intent_generation"),
        ("5", "locality_partitioning"),
        ("6A", "work_resolution"),
        ("6B", "targeted_resolution"),
        ("7", "market_clearance"),
        ("8", "welfare"),
        ("9", "mortality_commitment"),
        ("10", "metrics_observation"),
        ("11", "snapshot_event_flush"),
    ];

    for (idx, (exp_id, exp_name)) in expected_phases.iter().enumerate() {
        let entry = &phases[idx];
        let id = entry["id"].as_str().expect("phase id must be a string");
        let name = entry["name"].as_str().expect("phase name must be a string");
        assert_eq!(id, *exp_id, "phase[{}] id mismatch", idx);
        assert_eq!(name, *exp_name, "phase[{}] name mismatch", idx);
    }
}

#[test]
fn test_06_canonical_action_codes_match_production() {
    let manifest = parse_manifest();
    let codes = &manifest["action_codes"];

    assert_eq!(
        codes["work"].as_integer(),
        Some(Action::Work.index() as i64)
    );
    assert_eq!(
        codes["buy_food"].as_integer(),
        Some(Action::BuyFood.index() as i64)
    );
    assert_eq!(
        codes["sell_food"].as_integer(),
        Some(Action::SellFood.index() as i64)
    );
    assert_eq!(
        codes["give_food"].as_integer(),
        Some(Action::GiveFood.index() as i64)
    );
    assert_eq!(
        codes["steal_food"].as_integer(),
        Some(Action::StealFood.index() as i64)
    );
    assert_eq!(
        codes["idle"].as_integer(),
        Some(Action::Idle.index() as i64)
    );

    for i in 0..6 {
        assert!(Action::from_index(i).is_some());
    }
    assert!(Action::from_index(6).is_none());
}

#[test]
fn test_07_rng_subsystem_ids_match_production() {
    let manifest = parse_manifest();
    let subs = &manifest["rng"]["subsystems"];

    assert_eq!(
        subs["initialization"].as_integer(),
        Some(Subsystem::Initialization.id() as i64)
    );
    assert_eq!(
        subs["decision"].as_integer(),
        Some(Subsystem::Decision.id() as i64)
    );
    assert_eq!(
        subs["theft_target"].as_integer(),
        Some(Subsystem::TheftTarget.id() as i64)
    );
    assert_eq!(
        subs["mutual_aid_target"].as_integer(),
        Some(Subsystem::MutualAidTarget.id() as i64)
    );
    assert_eq!(
        subs["theft_success"].as_integer(),
        Some(Subsystem::TheftSuccess.id() as i64)
    );
    assert_eq!(
        subs["resolution_priority"].as_integer(),
        Some(Subsystem::ResolutionPriority.id() as i64)
    );
}

#[test]
fn test_08_prng_golden_coordinates_match_u64_output() {
    let manifest = parse_manifest();
    let golden_entries = manifest["rng"]["golden"]
        .as_array()
        .expect("rng.golden must be an array of tables");

    for entry in golden_entries {
        let name = entry["name"].as_str().unwrap();
        let master_seed = parse_hex_u64(entry["master_seed"].as_str().unwrap()).unwrap();
        let replicate_id = entry["replicate_id"].as_integer().unwrap() as u32;
        let day = entry["day"].as_integer().unwrap() as u32;
        let phase = entry["phase"].as_integer().unwrap() as u8;
        let subsystem_id = entry["subsystem_id"].as_integer().unwrap() as u16;
        let agent_id = entry["agent_id"].as_integer().unwrap() as u32;
        let draw_index = entry["draw_index"].as_integer().unwrap() as u32;
        let expected_u64 = parse_hex_u64(entry["expected_u64"].as_str().unwrap()).unwrap();

        let coord = RngCoordinate::new(
            master_seed,
            replicate_id,
            day,
            phase,
            subsystem_id,
            agent_id,
            draw_index,
        );
        let actual_u64 = coordinate_prng_u64(&coord);
        assert_eq!(
            actual_u64, expected_u64,
            "PRNG output mismatch for fixture '{}'",
            name
        );
    }
}

#[test]
fn test_09_f32_conversion_produces_exact_bits() {
    let manifest = parse_manifest();
    let shift = manifest["rng"]["f32_conversion"]["upper_bit_shift"]
        .as_integer()
        .unwrap() as u32;
    let divisor = manifest["rng"]["f32_conversion"]["divisor"]
        .as_integer()
        .unwrap() as f32;

    let golden_entries = manifest["rng"]["golden"].as_array().unwrap();

    for entry in golden_entries {
        let master_seed = parse_hex_u64(entry["master_seed"].as_str().unwrap()).unwrap();
        let replicate_id = entry["replicate_id"].as_integer().unwrap() as u32;
        let day = entry["day"].as_integer().unwrap() as u32;
        let phase = entry["phase"].as_integer().unwrap() as u8;
        let subsystem_id = entry["subsystem_id"].as_integer().unwrap() as u16;
        let agent_id = entry["agent_id"].as_integer().unwrap() as u32;
        let draw_index = entry["draw_index"].as_integer().unwrap() as u32;
        let expected_u64 = parse_hex_u64(entry["expected_u64"].as_str().unwrap()).unwrap();

        let coord = RngCoordinate::new(
            master_seed,
            replicate_id,
            day,
            phase,
            subsystem_id,
            agent_id,
            draw_index,
        );

        let actual_f32 = coordinate_prng_f32(&coord);

        let upper = (expected_u64 >> shift) as u32;
        let derived_f32 = (upper as f32) / divisor;

        assert_eq!(actual_f32.to_bits(), derived_f32.to_bits());
        assert_eq!(u64_to_f32(expected_u64).to_bits(), derived_f32.to_bits());
        assert!((0.0..1.0).contains(&actual_f32));
    }
}

#[test]
fn test_10_snapshot_magic_and_schema_match_encoded_snapshot() {
    let manifest = parse_manifest();
    let magic_str = manifest["snapshot"]["magic_ascii"].as_str().unwrap();
    let schema_ver = manifest["snapshot"]["schema_version"].as_integer().unwrap() as u32;

    assert_eq!(SNAPSHOT_MAGIC, *b"SIMCIVM0");
    assert_eq!(magic_str.as_bytes(), &SNAPSHOT_MAGIC);

    let world = make_test_world(1, vec![], vec![]);
    let meta = SnapshotMetadata {
        day: 1,
        master_seed: 42,
        replicate_id: 1,
        model_version: "0.1.0".to_string(),
        config_version: "1".to_string(),
    };

    let bytes = encode_snapshot(&world, &meta).expect("snapshot encoding succeeds");
    assert!(bytes.len() >= 12);
    assert_eq!(&bytes[0..8], &SNAPSHOT_MAGIC);
    let encoded_schema = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    assert_eq!(encoded_schema, schema_ver);
}

#[test]
fn test_11_snapshot_resume_offset_is_d_plus_one() {
    let manifest = parse_manifest();
    let offset = manifest["snapshot"]["resume_day_offset"]
        .as_integer()
        .unwrap();
    assert_eq!(offset, 1, "snapshot resume_day_offset must be 1 (D + 1)");
}

#[test]
fn test_12_event_key_widths_and_lexical_order_contract() {
    let manifest = parse_manifest();
    assert_eq!(manifest["event_key"]["day_type"].as_str(), Some("u32"));
    assert_eq!(manifest["event_key"]["phase_type"].as_str(), Some("u8"));
    assert_eq!(
        manifest["event_key"]["partition_key_type"].as_str(),
        Some("u64")
    );
    assert_eq!(
        manifest["event_key"]["local_sequence_type"].as_str(),
        Some("u64")
    );
    assert_eq!(
        manifest["event_key"]["ordering"].as_str(),
        Some("lexicographic_ascending")
    );

    let k1 = EventKey::new(1, 2, 0, 0);
    let k2 = EventKey::new(1, 2, 0, 1);
    let k3 = EventKey::new(1, 2, 1, 0);
    let k4 = EventKey::new(1, 3, 0, 0);
    let k5 = EventKey::new(2, 1, 0, 0);

    assert!(k1 < k2);
    assert!(k2 < k3);
    assert!(k3 < k4);
    assert!(k4 < k5);
}

#[test]
fn test_13_global_partition_key_parses_to_u64_max() {
    let manifest = parse_manifest();
    let gpk_str = manifest["event_key"]["global_partition_key"]
        .as_str()
        .unwrap();
    let gpk = parse_hex_u64(gpk_str).unwrap();
    assert_eq!(gpk, u64::MAX);
    assert_eq!(gpk, GLOBAL_PARTITION_KEY);
}

#[test]
fn test_14_event_category_tags_match_canonical_bytes() {
    let manifest = parse_manifest();
    let exp_st_cat = manifest["event"]["category_tags"]["state_transition"]
        .as_integer()
        .unwrap() as u8;
    let exp_obs_cat = manifest["event"]["category_tags"]["observation"]
        .as_integer()
        .unwrap() as u8;

    // Construct a StateTransition event
    let st_event = EventRecord::new(
        EventKey::new(1, 9, GLOBAL_PARTITION_KEY, 0),
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(10),
        }),
    );
    let st_bytes = canonical_event_bytes(&[st_event]).unwrap();
    // Header: 16-byte domain + 4-byte count + 21-byte EventKey = 41 bytes offset
    assert_eq!(
        st_bytes[41], exp_st_cat,
        "state transition category tag mismatch"
    );

    // Construct an Observation event
    let obs_event = EventRecord::new(
        EventKey::new(1, 10, GLOBAL_PARTITION_KEY, 0),
        Event::Observation(ObservationEvent::DailyMetricsObserved(DailyMetrics {
            day: 1,
            population: 10,
            wealth_gini: 0.1,
            total_food_reserves: 100.0,
            total_treasury: 500,
        })),
    );
    let obs_bytes = canonical_event_bytes(&[obs_event]).unwrap();
    assert_eq!(
        obs_bytes[41], exp_obs_cat,
        "observation category tag mismatch"
    );
}

#[test]
fn test_15_state_transition_variant_tags_match_canonical_bytes() {
    let manifest = parse_manifest();
    let st_tags = &manifest["event"]["state_transition_tags"];

    let exp_work = st_tags["work_resolved"].as_integer().unwrap() as u8;
    let exp_food = st_tags["food_transferred"].as_integer().unwrap() as u8;
    let exp_market = st_tags["market_cleared"].as_integer().unwrap() as u8;
    let exp_welfare = st_tags["welfare_distributed"].as_integer().unwrap() as u8;
    let exp_mortality = st_tags["mortality_committed"].as_integer().unwrap() as u8;

    // 0: WorkResolved
    let e_work = EventRecord::new(
        EventKey::new(1, 6, 0, 0),
        Event::StateTransition(StateTransitionEvent::WorkResolved {
            group_id: GroupId(0),
            agent_id: AgentId(1),
            requested_harvest: 2.0,
            allocated_harvest: 2.0,
        }),
    );
    let b_work = canonical_event_bytes(&[e_work]).unwrap();
    assert_eq!(b_work[42], exp_work, "work_resolved tag mismatch");

    // 1: FoodTransferred
    let e_food = EventRecord::new(
        EventKey::new(1, 6, 0, 0),
        Event::StateTransition(StateTransitionEvent::FoodTransferred {
            group_id: GroupId(0),
            initiator_agent_id: AgentId(1),
            target_agent_id: Some(AgentId(2)),
            action_kind: TargetedActionKind::GiveFood,
            amount: 1.0,
            success: true,
        }),
    );
    let b_food = canonical_event_bytes(&[e_food]).unwrap();
    assert_eq!(b_food[42], exp_food, "food_transferred tag mismatch");

    // 2: MarketCleared
    let e_market = EventRecord::new(
        EventKey::new(1, 7, 0, 0),
        Event::StateTransition(StateTransitionEvent::MarketCleared {
            group_id: GroupId(0),
            food_price: 10,
            tax_rate: 0.1,
            total_effective_supply: 5.0,
            total_effective_demand: 5.0,
            total_sold: 5.0,
            total_revenue: 50,
            tax_withheld: 5,
            net_pool_proceeds: 45,
            proceeds_balance: 0,
            buyers_count: 1,
            sellers_count: 1,
        }),
    );
    let b_market = canonical_event_bytes(&[e_market]).unwrap();
    assert_eq!(b_market[42], exp_market, "market_cleared tag mismatch");

    // 3: WelfareDistributed
    let e_welfare = EventRecord::new(
        EventKey::new(1, 8, 0, 0),
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(0),
            agent_id: AgentId(1),
            payout: 25,
        }),
    );
    let b_welfare = canonical_event_bytes(&[e_welfare]).unwrap();
    assert_eq!(
        b_welfare[42], exp_welfare,
        "welfare_distributed tag mismatch"
    );

    // 4: MortalityCommitted
    let e_mortality = EventRecord::new(
        EventKey::new(1, 9, GLOBAL_PARTITION_KEY, 0),
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(1),
        }),
    );
    let b_mortality = canonical_event_bytes(&[e_mortality]).unwrap();
    assert_eq!(
        b_mortality[42], exp_mortality,
        "mortality_committed tag mismatch"
    );
}

#[test]
fn test_16_observation_variant_tags_match_canonical_bytes() {
    let manifest = parse_manifest();
    let obs_tags = &manifest["event"]["observation_tags"];

    let exp_metrics = obs_tags["daily_metrics_observed"].as_integer().unwrap() as u8;
    let exp_snapshot = obs_tags["snapshot_emitted"].as_integer().unwrap() as u8;

    // 0: DailyMetricsObserved
    let e_metrics = EventRecord::new(
        EventKey::new(1, 10, GLOBAL_PARTITION_KEY, 0),
        Event::Observation(ObservationEvent::DailyMetricsObserved(DailyMetrics {
            day: 1,
            population: 10,
            wealth_gini: 0.1,
            total_food_reserves: 100.0,
            total_treasury: 500,
        })),
    );
    let b_metrics = canonical_event_bytes(&[e_metrics]).unwrap();
    assert_eq!(
        b_metrics[42], exp_metrics,
        "daily_metrics_observed tag mismatch"
    );

    // 1: SnapshotEmitted
    let e_snapshot = EventRecord::new(
        EventKey::new(1, 11, GLOBAL_PARTITION_KEY, 0),
        Event::Observation(ObservationEvent::SnapshotEmitted {
            day: 2,
            master_seed: 42,
            replicate_id: 1,
            schema_version: 1,
        }),
    );
    let b_snapshot = canonical_event_bytes(&[e_snapshot]).unwrap();
    assert_eq!(
        b_snapshot[42], exp_snapshot,
        "snapshot_emitted tag mismatch"
    );
}

#[test]
fn test_17_give_food_and_steal_food_action_codes_are_3_and_4() {
    let manifest = parse_manifest();
    let exp_give = manifest["event"]["food_transfer_action_codes"]["give_food"]
        .as_integer()
        .unwrap() as u8;
    let exp_steal = manifest["event"]["food_transfer_action_codes"]["steal_food"]
        .as_integer()
        .unwrap() as u8;

    assert_eq!(exp_give, 3);
    assert_eq!(exp_steal, 4);

    let e_give = EventRecord::new(
        EventKey::new(1, 6, 0, 0),
        Event::StateTransition(StateTransitionEvent::FoodTransferred {
            group_id: GroupId(0),
            initiator_agent_id: AgentId(1),
            target_agent_id: Some(AgentId(2)),
            action_kind: TargetedActionKind::GiveFood,
            amount: 1.0,
            success: true,
        }),
    );
    let b_give = canonical_event_bytes(&[e_give]).unwrap();
    // Offset for action_code in FoodTransferred with target Some:
    // 20 (header) + 21 (EventKey) = 41
    // + 1 (cat 41) + 1 (var 42) + 2 (group 43..44) + 4 (init 45..48)
    // + 1 (has_target 49) + 4 (target 50..53) = 54 (action_code)
    assert_eq!(
        b_give[54], exp_give,
        "GiveFood action code mismatch in bytes"
    );

    let e_steal = EventRecord::new(
        EventKey::new(1, 6, 0, 0),
        Event::StateTransition(StateTransitionEvent::FoodTransferred {
            group_id: GroupId(0),
            initiator_agent_id: AgentId(1),
            target_agent_id: Some(AgentId(2)),
            action_kind: TargetedActionKind::StealFood,
            amount: 1.0,
            success: true,
        }),
    );
    let b_steal = canonical_event_bytes(&[e_steal]).unwrap();
    assert_eq!(
        b_steal[54], exp_steal,
        "StealFood action code mismatch in bytes"
    );
}

#[test]
fn test_18_canonical_domain_prefixes_match_encoded_preimages() {
    let manifest = parse_manifest();
    let pref_state = manifest["canonical"]["domain_prefixes"]["state"]
        .as_str()
        .unwrap();
    let pref_metrics = manifest["canonical"]["domain_prefixes"]["metrics"]
        .as_str()
        .unwrap();
    let pref_events = manifest["canonical"]["domain_prefixes"]["events"]
        .as_str()
        .unwrap();

    let world = make_test_world(0, vec![], vec![]);
    let state_bytes = canonical_state_bytes(&world).unwrap();
    assert_eq!(&state_bytes[0..pref_state.len()], pref_state.as_bytes());

    let metrics = vec![];
    let metrics_bytes = canonical_metrics_bytes(&metrics).unwrap();
    assert_eq!(
        &metrics_bytes[0..pref_metrics.len()],
        pref_metrics.as_bytes()
    );

    let events = vec![];
    let events_bytes = canonical_event_bytes(&events).unwrap();
    assert_eq!(&events_bytes[0..pref_events.len()], pref_events.as_bytes());
}

#[test]
fn test_19_state_independent_fixed_vector_matches_manifest() {
    let manifest = parse_manifest();
    let exp_len = manifest["fixed_vectors"]["state"]["byte_length"]
        .as_integer()
        .unwrap() as usize;
    let exp_preimage_hex = manifest["fixed_vectors"]["state"]["preimage_hex"]
        .as_str()
        .unwrap();
    let exp_sha256 = manifest["fixed_vectors"]["state"]["sha256"]
        .as_str()
        .unwrap();

    let world = make_test_world(
        1,
        vec![make_test_agent(
            42, 999, true, 0, 1.0, 10.0, 100, 1.0, 0.5, 0.1, 0.2, 7,
        )],
        vec![make_test_settlement(7, 500.0, 1000)],
    );

    let actual_bytes = canonical_state_bytes(&world).unwrap();
    assert_eq!(actual_bytes.len(), exp_len);

    let expected_bytes = parse_hex_bytes(exp_preimage_hex).unwrap();
    assert_eq!(actual_bytes.as_slice(), expected_bytes.as_slice());

    let actual_hash = canonical_state_hash(&world).unwrap();
    assert_eq!(actual_hash.to_hex(), exp_sha256);
}

#[test]
fn test_20_metrics_independent_fixed_vector_matches_manifest() {
    let manifest = parse_manifest();
    let exp_len = manifest["fixed_vectors"]["metrics"]["byte_length"]
        .as_integer()
        .unwrap() as usize;
    let exp_preimage_hex = manifest["fixed_vectors"]["metrics"]["preimage_hex"]
        .as_str()
        .unwrap();
    let exp_sha256 = manifest["fixed_vectors"]["metrics"]["sha256"]
        .as_str()
        .unwrap();

    let metrics = vec![DailyMetrics {
        day: 1,
        population: 100,
        wealth_gini: 0.25,
        total_food_reserves: 1500.0,
        total_treasury: 5000,
    }];

    let actual_bytes = canonical_metrics_bytes(&metrics).unwrap();
    assert_eq!(actual_bytes.len(), exp_len);

    let expected_bytes = parse_hex_bytes(exp_preimage_hex).unwrap();
    assert_eq!(actual_bytes.as_slice(), expected_bytes.as_slice());

    let actual_hash = canonical_metrics_hash(&metrics).unwrap();
    assert_eq!(actual_hash.to_hex(), exp_sha256);
}

#[test]
fn test_21_events_independent_fixed_vector_matches_manifest() {
    let manifest = parse_manifest();
    let exp_len = manifest["fixed_vectors"]["events"]["byte_length"]
        .as_integer()
        .unwrap() as usize;
    let exp_preimage_hex = manifest["fixed_vectors"]["events"]["preimage_hex"]
        .as_str()
        .unwrap();
    let exp_sha256 = manifest["fixed_vectors"]["events"]["sha256"]
        .as_str()
        .unwrap();

    let e1 = EventRecord::new(
        EventKey::new(1, 8, 0, 0),
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(0),
            agent_id: AgentId(42),
            payout: 25,
        }),
    );
    let e2 = EventRecord::new(
        EventKey::new(1, 9, GLOBAL_PARTITION_KEY, 0),
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(99),
        }),
    );

    // Provide in reverse order to ensure canonical sort before byte encoding
    let events = vec![e2, e1];

    let actual_bytes = canonical_event_bytes(&events).unwrap();
    assert_eq!(actual_bytes.len(), exp_len);

    let expected_bytes = parse_hex_bytes(exp_preimage_hex).unwrap();
    assert_eq!(actual_bytes.as_slice(), expected_bytes.as_slice());

    let actual_hash = canonical_event_hash(&events).unwrap();
    assert_eq!(actual_hash.to_hex(), exp_sha256);
}

fn run_graduation_trajectory() -> (WorldState, Vec<DailyMetrics>, Vec<EventRecord>) {
    let manifest = parse_manifest();
    let grad = &manifest["graduation"];
    let master_seed: u64 = grad["master_seed"].as_str().unwrap().parse().unwrap();
    let replicate_id = grad["replicate_id"].as_integer().unwrap() as u32;
    let total_days = grad["total_days"].as_integer().unwrap() as u32;
    let snapshot_day = grad["snapshot_occurrence_day"].as_integer().unwrap() as u32;

    let config = SimConfig::parse_and_validate(GATE_CONFIG_TOML).unwrap();
    let context = M0RunContext::new(master_seed, replicate_id);
    let mut world = initialize_world(&config).unwrap();

    let mut metrics = Vec::with_capacity(total_days as usize);
    let mut events = Vec::new();

    for d in 0..total_days {
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: d == snapshot_day,
        };
        let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
        if let Some(m) = outcome.metrics {
            metrics.push(m);
        }
        events.extend(outcome.events);
    }

    (world, metrics, events)
}

#[test]
fn test_22_500_day_graduation_state_hash_matches_manifest() {
    let manifest = parse_manifest();
    let exp_sha256 = manifest["graduation"]["state_sha256"].as_str().unwrap();

    let (world, _, _) = run_graduation_trajectory();
    let actual_hash = canonical_state_hash(&world).unwrap();
    assert_eq!(actual_hash.to_hex(), exp_sha256);
}

#[test]
fn test_23_500_day_graduation_metrics_hash_matches_manifest() {
    let manifest = parse_manifest();
    let exp_sha256 = manifest["graduation"]["metrics_sha256"].as_str().unwrap();

    let (_, metrics, _) = run_graduation_trajectory();
    let actual_hash = canonical_metrics_hash(&metrics).unwrap();
    assert_eq!(actual_hash.to_hex(), exp_sha256);
}

#[test]
fn test_24_500_day_graduation_event_hash_matches_manifest() {
    let manifest = parse_manifest();
    let exp_sha256 = manifest["graduation"]["events_sha256"].as_str().unwrap();

    let (_, _, events) = run_graduation_trajectory();
    let actual_hash = canonical_event_hash(&events).unwrap();
    assert_eq!(actual_hash.to_hex(), exp_sha256);
}

#[test]
fn test_25_repeated_manifest_driven_oracle_validation_is_deterministic() {
    let (w1, m1, e1) = run_graduation_trajectory();
    let (w2, m2, e2) = run_graduation_trajectory();

    let sh1 = canonical_state_hash(&w1).unwrap();
    let sh2 = canonical_state_hash(&w2).unwrap();
    assert_eq!(sh1, sh2);

    let mh1 = canonical_metrics_hash(&m1).unwrap();
    let mh2 = canonical_metrics_hash(&m2).unwrap();
    assert_eq!(mh1, mh2);

    let eh1 = canonical_event_hash(&e1).unwrap();
    let eh2 = canonical_event_hash(&e2).unwrap();
    assert_eq!(eh1, eh2);
}
