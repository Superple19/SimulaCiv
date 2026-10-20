use serde::Serialize;
use serde_json::json;
use sim_core::{AgentId, GroupId};
use sim_model::state::HybridWorldState;
use sim_model::{
    DayExecutionOptions, M0RunContext, M3pStorageRunContext, MigrationConfig, SimConfig,
    SimulationConfig, SimulationSystems, initialize_world, run_m3p_storage_days,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::error::Error;
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;

const HORIZON: u32 = 365;
const SEEDS: [u64; 5] = [0, 1, 3, 7, 81_985_529_216_486_895];
const REPLICATE_ID: u32 = 7;
const MIGRATION: MigrationConfig = MigrationConfig {
    migration_probability: 0.25,
    migration_threshold: 0.0,
    migration_cost: 10,
    migration_cooldown_days: 30,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct AgentRecord {
    alive: bool,
    birth_day: u32,
    health: u32,
    food: u32,
    wealth: i64,
    productivity: u32,
    cooperation: u32,
    aggression: u32,
    risk_tolerance: u32,
    group_id: u16,
}

impl From<&sim_model::AgentState> for AgentRecord {
    fn from(agent: &sim_model::AgentState) -> Self {
        Self {
            alive: agent.alive,
            birth_day: agent.birth_day.as_u32(),
            health: agent.health.to_bits(),
            food: agent.food.to_bits(),
            wealth: agent.wealth,
            productivity: agent.productivity.to_bits(),
            cooperation: agent.cooperation.to_bits(),
            aggression: agent.aggression.to_bits(),
            risk_tolerance: agent.risk_tolerance.to_bits(),
            group_id: agent.group_id.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct LineageKey {
    ancestor: u32,
    ordinal_path: Vec<u32>,
}

#[derive(Debug, Default)]
struct LineageTracker {
    keys: HashMap<AgentId, LineageKey>,
    successful_births: HashMap<AgentId, u32>,
}

impl LineageTracker {
    fn from_initial(world: &sim_model::WorldState) -> Self {
        Self {
            keys: world
                .agents
                .iter()
                .map(|agent| {
                    (
                        agent.agent_id,
                        LineageKey {
                            ancestor: agent.agent_id.0,
                            ordinal_path: Vec::new(),
                        },
                    )
                })
                .collect(),
            successful_births: HashMap::new(),
        }
    }

    fn add_births(&mut self, outcome: &sim_model::M3pDayOutcome) {
        for record in &outcome.events {
            if let sim_model::M3pEvent::Birth { parent, child, .. } = &record.event {
                let mut key = self.keys[parent].clone();
                let ordinal = self.successful_births.entry(*parent).or_default();
                key.ordinal_path.push(*ordinal);
                *ordinal += 1;
                self.keys.insert(*child, key);
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct DayMeasurement {
    day: u32,
    base_alive: u64,
    intervention_alive: u64,
    changed_records: u64,
    matched_records: u64,
    divergence_fraction: f64,
    unmatched_births: u64,
    daily_settlements_touched: u64,
    cumulative_settlements_touched: u64,
    first_other_settlement_day: Option<u32>,
    base_moves: u64,
    intervention_moves: u64,
    base_births: u64,
    intervention_births: u64,
    base_removed: u64,
    intervention_removed: u64,
    base_money: i128,
    intervention_money: i128,
    base_cumulative_removed_wealth: i128,
    intervention_cumulative_removed_wealth: i128,
    base_money_equals_initial: bool,
    intervention_money_equals_initial: bool,
    base_money_conserved_after_cleanup: bool,
    intervention_money_conserved_after_cleanup: bool,
    base_migration_transfers_balanced: bool,
    intervention_migration_transfers_balanced: bool,
    settlement_population_gini: Option<f64>,
    settlement_population_min: Option<u64>,
    settlement_population_max: Option<u64>,
    settlement_population: Option<BTreeMap<u16, u64>>,
}

#[derive(Debug, Clone, Serialize)]
struct CaseSummary {
    seed: u64,
    migration_enabled: bool,
    target_agent_id: u32,
    target_settlement_id: u16,
    first_divergent_day: Option<u32>,
    classification: String,
    cascade_onset_day: Option<u32>,
    first_other_settlement_day: Option<u32>,
    settlements_touched_at_day_365: u64,
    day_365_changed_records: u64,
    day_365_matched_records: u64,
    day_365_divergence_fraction: f64,
    total_base_moves: u64,
    total_intervention_moves: u64,
    base_money_equals_initial: bool,
    intervention_money_equals_initial: bool,
    base_money_conserved_after_cleanup: bool,
    intervention_money_conserved_after_cleanup: bool,
    base_migration_transfers_balanced: bool,
    intervention_migration_transfers_balanced: bool,
    days: Vec<DayMeasurement>,
}

#[derive(Debug)]
struct RunState {
    world: HybridWorldState,
    context: M3pStorageRunContext,
}

impl RunState {
    fn new(
        initial: &sim_model::WorldState,
        config: &SimConfig,
        run_context: &M0RunContext,
    ) -> Result<Self, Box<dyn Error>> {
        let world = HybridWorldState::hybrid(initial.clone());
        let context = M3pStorageRunContext::new(&world, config, run_context)?;
        Ok(Self { world, context })
    }

    fn step(&mut self, config: &SimConfig) -> Result<sim_model::M3pDayOutcome, Box<dyn Error>> {
        let outcome = run_m3p_storage_days(
            &mut self.world,
            config,
            &mut self.context,
            1,
            &DayExecutionOptions {
                metrics_enabled: true,
                events_enabled: true,
                snapshot_boundary: false,
            },
        )?
        .remove(0);
        Ok(black_box(outcome))
    }

    fn records(&self) -> Result<BTreeMap<AgentId, (AgentRecord, GroupId)>, Box<dyn Error>> {
        let storage = self.world.storage().ok_or("segmented storage missing")?;
        Ok(storage
            .to_agents()
            .iter()
            .map(|agent| (agent.agent_id, (AgentRecord::from(agent), agent.group_id)))
            .collect())
    }

    fn alive_by_group(&self) -> BTreeMap<u16, u64> {
        let storage = self.world.storage().expect("storage is initialized");
        let mut population = BTreeMap::new();
        for (slot, group_id) in storage.group_ids().iter().enumerate() {
            if storage.alive()[slot] {
                *population.entry(group_id.0).or_insert(0) += 1;
            }
        }
        population
    }

    fn money(&self) -> i128 {
        let storage = self.world.storage().expect("storage is initialized");
        storage
            .wealth()
            .iter()
            .map(|&wealth| i128::from(wealth))
            .sum::<i128>()
            + self
                .world
                .world
                .settlements
                .iter()
                .map(|settlement| i128::from(settlement.treasury))
                .sum::<i128>()
    }
}

fn config_for(seed: u64, migration: bool) -> Result<SimConfig, Box<dyn Error>> {
    let mut config = SimConfig::from_toml_str(include_str!(
        "../tests/fixtures/m3p_propagation_sustained.toml"
    ))?;
    config.profile = sim_model::SimulationProfile::M3p;
    config.propagation = None;
    config.simulation = Some(SimulationConfig {
        systems: SimulationSystems {
            relationship_graph: true,
            population_dynamics: true,
            propagation: false,
        },
    });
    config.world.initial_population = 10_000;
    config.world.settlement_count = 50;
    config.world.master_seed = seed;
    config.world.replicate_id = REPLICATE_ID;
    config.interaction.relationship_degree_k = Some(8);
    config.interaction.p_local = Some(0.5);
    if migration {
        config.enable_migration(MIGRATION);
    }
    config.validate()?;
    Ok(config)
}

fn target_agent(world: &sim_model::WorldState) -> (AgentId, GroupId) {
    let mut counts = BTreeMap::<GroupId, usize>::new();
    for agent in &world.agents {
        if agent.alive {
            *counts.entry(agent.group_id).or_default() += 1;
        }
    }
    let max_count = counts.values().copied().max().expect("world is populated");
    let group_id = counts
        .iter()
        .find_map(|(&group, &count)| (count == max_count).then_some(group))
        .expect("largest settlement exists");
    let agent_id = world
        .agents
        .iter()
        .filter(|agent| agent.alive && agent.group_id == group_id)
        .map(|agent| agent.agent_id)
        .min()
        .expect("target settlement has a living agent");
    (agent_id, group_id)
}

fn intervention_world(
    initial: &sim_model::WorldState,
    target_id: AgentId,
) -> sim_model::WorldState {
    let mut world = initial.clone();
    world
        .agents
        .iter_mut()
        .find(|agent| agent.agent_id == target_id)
        .expect("target AgentId exists")
        .food = 0.0;
    world
}

fn lineage_records(
    state: &RunState,
    lineage: &LineageTracker,
) -> Result<BTreeMap<LineageKey, (AgentRecord, GroupId)>, Box<dyn Error>> {
    Ok(state
        .records()?
        .into_iter()
        .map(|(agent_id, (record, group_id))| (lineage.keys[&agent_id].clone(), (record, group_id)))
        .collect())
}

fn settlement_gini(population: &BTreeMap<u16, u64>) -> f64 {
    let mut values = population.values().copied().collect::<Vec<_>>();
    values.sort_unstable();
    let total = values.iter().sum::<u64>() as f64;
    if values.is_empty() || total == 0.0 {
        return 0.0;
    }
    let n = values.len() as f64;
    values
        .iter()
        .enumerate()
        .map(|(index, value)| (2.0 * (index as f64 + 1.0) - n - 1.0) * *value as f64)
        .sum::<f64>()
        / (n * total)
}

fn divergence_counts(
    base: &BTreeMap<LineageKey, (AgentRecord, GroupId)>,
    intervention: &BTreeMap<LineageKey, (AgentRecord, GroupId)>,
    base_known: &BTreeSet<LineageKey>,
    intervention_known: &BTreeSet<LineageKey>,
) -> (u64, u64, u64, BTreeSet<u16>) {
    let common = base_known
        .intersection(intervention_known)
        .cloned()
        .collect::<BTreeSet<_>>();
    let all_current = base
        .keys()
        .chain(intervention.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let unmatched = all_current.difference(&common).count() as u64;
    let mut changed = 0_u64;
    let mut matched = 0_u64;
    let mut touched = BTreeSet::new();
    for key in all_current.difference(&common) {
        if let Some((_, group)) = base.get(key) {
            touched.insert(group.0);
        }
        if let Some((_, group)) = intervention.get(key) {
            touched.insert(group.0);
        }
    }
    for key in common {
        let left = base.get(&key);
        let right = intervention.get(&key);
        if left.is_none() && right.is_none() {
            continue;
        }
        matched += 1;
        if left != right {
            changed += 1;
            if let Some((_, group)) = left {
                touched.insert(group.0);
            }
            if let Some((_, group)) = right {
                touched.insert(group.0);
            }
        }
    }
    (changed, matched, unmatched, touched)
}

fn removed_wealth(
    previous: &BTreeMap<AgentId, (AgentRecord, GroupId)>,
    current: &BTreeMap<AgentId, (AgentRecord, GroupId)>,
) -> (u64, i128) {
    previous
        .iter()
        .filter(|(agent_id, _)| !current.contains_key(agent_id))
        .fold((0_u64, 0_i128), |(count, wealth), (_, (record, _))| {
            (count + 1, wealth + i128::from(record.wealth))
        })
}

fn migration_transfer_balanced(outcome: &sim_model::M3pDayOutcome) -> bool {
    let Some(migration) = &outcome.migration else {
        return true;
    };
    let Some(metrics) = migration.metrics else {
        return false;
    };
    let event_cost = migration
        .events
        .iter()
        .map(|event| i128::from(event.cost))
        .sum::<i128>();
    event_cost == i128::from(metrics.migration_cost)
        && i128::from(metrics.migrations) * i128::from(MIGRATION.migration_cost)
            == i128::from(metrics.migration_cost)
}

fn cascade_onset(days: &[DayMeasurement]) -> (Option<u32>, String) {
    let Some(first) = days.iter().find(|day| day.changed_records > 0) else {
        return (None, "no divergence".to_owned());
    };
    if first.divergence_fraction >= 0.5 {
        return (Some(first.day), "immediate global divergence".to_owned());
    }
    let mut maximum = first.changed_records;
    for day in days.iter().filter(|day| day.day > first.day) {
        if day.changed_records >= maximum.saturating_mul(2).max(maximum.saturating_add(5)) {
            return (Some(day.day), "cascade".to_owned());
        }
        maximum = maximum.max(day.changed_records);
    }
    (None, "no cascade".to_owned())
}

fn run_counterfactual_case(seed: u64, migration: bool) -> Result<CaseSummary, Box<dyn Error>> {
    let config = config_for(seed, migration)?;
    let run_context = M0RunContext::new(seed, REPLICATE_ID);
    let initial = initialize_world(&config)?;
    let (target_id, target_group) = target_agent(&initial);
    let branch_initial = intervention_world(&initial, target_id);
    let initial_money = initial
        .agents
        .iter()
        .map(|agent| i128::from(agent.wealth))
        .sum::<i128>()
        + initial
            .settlements
            .iter()
            .map(|settlement| i128::from(settlement.treasury))
            .sum::<i128>();
    let mut base = RunState::new(&initial, &config, &run_context)?;
    let mut intervention = RunState::new(&branch_initial, &config, &run_context)?;
    let mut base_lineage = LineageTracker::from_initial(&initial);
    let mut intervention_lineage = LineageTracker::from_initial(&branch_initial);
    let mut cumulative_touched = BTreeSet::new();
    let mut first_other_settlement_day = None;
    let mut first_divergent_day = None;
    let mut base_previous = base.records()?;
    let mut intervention_previous = intervention.records()?;
    let mut base_cumulative_removed_wealth = 0_i128;
    let mut intervention_cumulative_removed_wealth = 0_i128;
    let mut days = Vec::with_capacity(HORIZON as usize);

    for day_index in 0..HORIZON {
        let base_outcome = base.step(&config)?;
        let intervention_outcome = intervention.step(&config)?;
        base_lineage.add_births(&base_outcome);
        intervention_lineage.add_births(&intervention_outcome);
        let base_records = lineage_records(&base, &base_lineage)?;
        let intervention_records = lineage_records(&intervention, &intervention_lineage)?;
        let base_current = base.records()?;
        let intervention_current = intervention.records()?;
        let (base_removed, base_removed_today_wealth) =
            removed_wealth(&base_previous, &base_current);
        let (intervention_removed, intervention_removed_today_wealth) =
            removed_wealth(&intervention_previous, &intervention_current);
        base_cumulative_removed_wealth += base_removed_today_wealth;
        intervention_cumulative_removed_wealth += intervention_removed_today_wealth;
        base_previous = base_current;
        intervention_previous = intervention_current;
        let base_known = base_lineage.keys.values().cloned().collect::<BTreeSet<_>>();
        let intervention_known = intervention_lineage
            .keys
            .values()
            .cloned()
            .collect::<BTreeSet<_>>();
        let (changed, matched, unmatched, touched) = divergence_counts(
            &base_records,
            &intervention_records,
            &base_known,
            &intervention_known,
        );
        if changed > 0 && first_divergent_day.is_none() {
            first_divergent_day = Some(day_index + 1);
        }
        if touched.iter().any(|&group| group != target_group.0)
            && first_other_settlement_day.is_none()
        {
            first_other_settlement_day = Some(day_index + 1);
        }
        cumulative_touched.extend(touched.iter().copied());

        let base_alive = base_outcome.metrics.as_ref().unwrap().live_population;
        let intervention_alive = intervention_outcome
            .metrics
            .as_ref()
            .unwrap()
            .live_population;
        let base_births = base_outcome.metrics.as_ref().unwrap().births;
        let intervention_births = intervention_outcome.metrics.as_ref().unwrap().births;
        let base_money = base.money();
        let intervention_money = intervention.money();
        let migration_count = |outcome: &sim_model::M3pDayOutcome| {
            outcome
                .migration
                .as_ref()
                .and_then(|migration| migration.metrics)
                .map_or(0, |metrics| metrics.migrations)
        };
        let population = if migration {
            Some(base.alive_by_group())
        } else {
            None
        };
        let population_gini = population.as_ref().map(settlement_gini);
        let population_min = population
            .as_ref()
            .and_then(|population| population.values().copied().min());
        let population_max = population
            .as_ref()
            .and_then(|population| population.values().copied().max());
        let matched_for_fraction = matched;
        days.push(DayMeasurement {
            day: day_index + 1,
            base_alive,
            intervention_alive,
            changed_records: changed,
            matched_records: matched,
            divergence_fraction: if matched_for_fraction == 0 {
                0.0
            } else {
                changed as f64 / matched_for_fraction as f64
            },
            unmatched_births: unmatched,
            daily_settlements_touched: touched.len() as u64,
            cumulative_settlements_touched: cumulative_touched.len() as u64,
            first_other_settlement_day,
            base_moves: migration_count(&base_outcome),
            intervention_moves: migration_count(&intervention_outcome),
            base_births,
            intervention_births,
            base_removed,
            intervention_removed,
            base_money,
            intervention_money,
            base_cumulative_removed_wealth,
            intervention_cumulative_removed_wealth,
            base_money_equals_initial: base_money == initial_money,
            intervention_money_equals_initial: intervention_money == initial_money,
            base_money_conserved_after_cleanup: {
                base_money + base_cumulative_removed_wealth == initial_money
            },
            intervention_money_conserved_after_cleanup: {
                intervention_money + intervention_cumulative_removed_wealth == initial_money
            },
            base_migration_transfers_balanced: migration_transfer_balanced(&base_outcome),
            intervention_migration_transfers_balanced: {
                migration_transfer_balanced(&intervention_outcome)
            },
            settlement_population_gini: population_gini,
            settlement_population_min: population_min,
            settlement_population_max: population_max,
            settlement_population: population,
        });
    }
    let (cascade_onset_day, classification) = cascade_onset(&days);
    let final_day = days.last().unwrap();
    Ok(CaseSummary {
        seed,
        migration_enabled: migration,
        target_agent_id: target_id.0,
        target_settlement_id: target_group.0,
        first_divergent_day,
        classification,
        cascade_onset_day,
        first_other_settlement_day,
        settlements_touched_at_day_365: cumulative_touched.len() as u64,
        day_365_changed_records: final_day.changed_records,
        day_365_matched_records: final_day.matched_records,
        day_365_divergence_fraction: final_day.divergence_fraction,
        total_base_moves: days.iter().map(|day| day.base_moves).sum(),
        total_intervention_moves: days.iter().map(|day| day.intervention_moves).sum(),
        base_money_equals_initial: days.iter().all(|day| day.base_money_equals_initial),
        intervention_money_equals_initial: days
            .iter()
            .all(|day| day.intervention_money_equals_initial),
        base_money_conserved_after_cleanup: days
            .iter()
            .all(|day| day.base_money_conserved_after_cleanup),
        intervention_money_conserved_after_cleanup: days
            .iter()
            .all(|day| day.intervention_money_conserved_after_cleanup),
        base_migration_transfers_balanced: days
            .iter()
            .all(|day| day.base_migration_transfers_balanced),
        intervention_migration_transfers_balanced: days
            .iter()
            .all(|day| day.intervention_migration_transfers_balanced),
        days,
    })
}

fn main() -> Result<(), Box<dyn Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("docs/data/m4_06/experiment.json"));
    let population_runs = SEEDS
        .iter()
        .map(|&seed| run_counterfactual_case(seed, true))
        .collect::<Result<Vec<_>, _>>()?;
    let counterfactual_off = SEEDS
        .iter()
        .map(|&seed| run_counterfactual_case(seed, false))
        .collect::<Result<Vec<_>, _>>()?;
    let counterfactual_on = SEEDS
        .iter()
        .map(|&seed| run_counterfactual_case(seed, true))
        .collect::<Result<Vec<_>, _>>()?;
    let result = json!({
        "horizon_days": HORIZON,
        "population_config": {
            "preset": "M3P sustained from m3p_propagation_sustained.toml with propagation disabled",
            "initial_population": 10_000,
            "settlements": 50,
            "k": 8,
            "p_local": 0.5,
            "migration_probability": MIGRATION.migration_probability,
            "migration_threshold": MIGRATION.migration_threshold,
            "migration_cost": MIGRATION.migration_cost,
            "migration_cooldown_days": MIGRATION.migration_cooldown_days
        },
        "food_zero_intervention": {
            "target_policy": "lowest AgentId in the largest initial settlement; equal-size tie to lowest GroupId",
            "field": "food",
            "value": 0.0,
            "horizon_days": HORIZON
        },
        "lineage_and_divergence_method": "M4-03 lineage keys and D/L denominator; D counts differing matched records; unmatched births separate",
        "migration_on_population_distribution_and_conservation": population_runs,
        "food_zero_migration_off": counterfactual_off,
        "food_zero_migration_on": counterfactual_on
    });
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, serde_json::to_vec_pretty(&result)?)?;
    println!("wrote {}", output.display());
    Ok(())
}
