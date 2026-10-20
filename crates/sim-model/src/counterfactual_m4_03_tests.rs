use super::*;

fn daily(day: u32, changed_agent_records: u64) -> DivergenceDayRecord {
    DivergenceDayRecord {
        day,
        changed_agent_records,
        newborn_crn_loss: 0,
        matched_alive_entities: 2_000,
        normalized_divergence: changed_agent_records as f64 / 2_000.0,
        unmatched_birth_records: 0,
        unmatched_birth_fraction: 0.0,
        base_alive_population: 2_000,
        branch_alive_population: 2_000,
        state_digests_differ: changed_agent_records > 0,
        graph_changed: false,
        allocator_changed: false,
        propagation_changed: false,
        settlement_global_changed: false,
    }
}

fn synthetic_branch() -> BranchResult {
    BranchResult {
        base_days: Vec::new(),
        branch_days: Vec::new(),
        first_divergence: Some(FirstDivergence {
            day: 0,
            slot: RecordedSlot::InterventionBoundary,
            first_differing_agent: Some(AgentId(0)),
            differing_components: vec!["agent_rows"],
            system_id: None,
            rng_subsystem_ids: Vec::new(),
        }),
        daily_differences: Vec::new(),
        forced_action: None,
    }
}

#[test]
fn flat_then_cascade_uses_frozen_threshold_and_log_growth() {
    let mut series = (0..12).map(|day| daily(day, 3)).collect::<Vec<_>>();
    series.extend([daily(12, 8), daily(13, 12), daily(14, 18)]);
    let result = synthetic_branch();
    let summary = summarize_divergence(
        &result,
        &series,
        &BranchIntervention::AgentFieldOverride {
            agent_id: AgentId(0),
            field: AgentField::Food,
            value: AgentFieldValue::Food(0.0),
        },
    );
    assert_eq!(summary.cascade_onset_day, Some(12));
    assert_eq!(summary.plateau_days, Some(12));
    assert!(summary.growth_slope.is_some_and(|slope| slope > 0.0));
    assert!(summary.doubling_time_days.is_some_and(|days| days > 0.0));
    assert_eq!(summary.peak_changed_agent_records, 18);
    assert_eq!(summary.days_to_peak, Some(14));
}

#[test]
fn flat_series_reports_no_cascade_and_no_growth() {
    let series = (0..30).map(|day| daily(day, 3)).collect::<Vec<_>>();
    let result = synthetic_branch();
    let summary = summarize_divergence(
        &result,
        &series,
        &BranchIntervention::AgentFieldOverride {
            agent_id: AgentId(0),
            field: AgentField::Food,
            value: AgentFieldValue::Food(0.0),
        },
    );
    assert_eq!(summary.cascade_onset_day, None);
    assert_eq!(summary.plateau_days, None);
    assert_eq!(summary.growth_slope, None);
    assert_eq!(summary.doubling_time_days, None);
    assert_eq!(
        summary.onset_vs_branch_target_death,
        TargetDeathRelation::NoCascade
    );
}
