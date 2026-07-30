use std::collections::BTreeSet;
#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
use std::env;
#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
use std::sync::Arc;

#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
use nimbus_storage::provider_test_fixtures::ProviderLeaseTimeControl;
use nimbus_testing::ppsc::{
    PpscBackend, PpscScenario, PpscTerminalState, audit_ppsc_history, retained_ppsc_scenarios,
};
#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
use nimbus_testing::ppsc::{
    PpscExpectedOutcome, PpscOperation, PpscRoute, PpscStep, retained_provider_authority_scenarios,
};

#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
use super::{PpscEngineFactory, provider_takeover_value};
use super::{PpscEngineRunner, record_kind};
#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
use crate::EnginePersistenceConfig;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn redb_ppsc_seeded_journal_differential() {
    for scenario in retained_ppsc_scenarios() {
        let first = PpscEngineRunner::new_embedded(PpscBackend::Redb, &scenario)
            .await
            .run(scenario.clone())
            .await;
        let replay = PpscEngineRunner::new_embedded(PpscBackend::Redb, &scenario)
            .await
            .run(scenario.clone())
            .await;
        audit_ppsc_history(&first).unwrap_or_else(|error| panic!("redb first run: {error}"));
        audit_ppsc_history(&replay).unwrap_or_else(|error| panic!("redb replay: {error}"));
        assert_ppsc_terminal_matches(
            PpscBackend::Redb,
            &scenario,
            &first.terminal,
            &replay.terminal,
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sqlite_ppsc_seeded_journal_differential() {
    for scenario in retained_ppsc_scenarios() {
        let oracle = PpscEngineRunner::new_embedded(PpscBackend::Redb, &scenario)
            .await
            .run(scenario.clone())
            .await;
        let sqlite = PpscEngineRunner::new_embedded(PpscBackend::Sqlite, &scenario)
            .await
            .run(scenario.clone())
            .await;
        audit_ppsc_history(&oracle).unwrap_or_else(|error| panic!("redb oracle: {error}"));
        audit_ppsc_history(&sqlite).unwrap_or_else(|error| panic!("sqlite: {error}"));
        assert_ppsc_terminal_matches(
            PpscBackend::Sqlite,
            &scenario,
            &oracle.terminal,
            &sqlite.terminal,
        );
    }
}

#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
pub(crate) async fn exercise_ppsc_provider_retained_differential(
    backend: PpscBackend,
    config: EnginePersistenceConfig,
    lease_time_control: Arc<dyn ProviderLeaseTimeControl>,
) {
    for scenario in provider_replay_scenarios(retained_ppsc_scenarios(), false) {
        exercise_ppsc_provider_scenario_differential(
            backend,
            config.clone(),
            lease_time_control.clone(),
            scenario,
        )
        .await;
    }
}

#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
pub(crate) async fn exercise_ppsc_provider_scenario_differential(
    backend: PpscBackend,
    config: EnginePersistenceConfig,
    lease_time_control: Arc<dyn ProviderLeaseTimeControl>,
    scenario: PpscScenario,
) {
    let oracle = PpscEngineRunner::new_embedded(PpscBackend::Redb, &scenario)
        .await
        .run(scenario.clone())
        .await;
    let mut provider = PpscEngineRunner::new_configured_provider(backend, &scenario, config).await;
    provider.provider_lease_time_control = Some(lease_time_control);
    let provider = provider.run_with_provider_cleanup(scenario.clone()).await;

    audit_ppsc_history(&oracle).unwrap_or_else(|error| panic!("redb oracle: {error}"));
    audit_ppsc_history(&provider)
        .unwrap_or_else(|error| panic!("{} provider: {error}", backend.as_str()));
    assert_ppsc_terminal_matches(backend, &scenario, &oracle.terminal, &provider.terminal);
}

#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
pub(crate) async fn exercise_ppsc_provider_authority_extension(
    backend: PpscBackend,
    first_config: EnginePersistenceConfig,
    takeover_config: EnginePersistenceConfig,
    lease_time_control: Arc<dyn ProviderLeaseTimeControl>,
) {
    for scenario in provider_replay_scenarios(retained_provider_authority_scenarios(), true) {
        let oracle_scenario = provider_authority_terminal_oracle(&scenario);
        let oracle = PpscEngineRunner::new_embedded(PpscBackend::Redb, &oracle_scenario)
            .await
            .run(oracle_scenario)
            .await;
        let mut provider =
            PpscEngineRunner::new_configured_provider(backend, &scenario, first_config.clone())
                .await;
        provider.takeover_engine_factory = Some(PpscEngineFactory::Configured(Box::new(
            takeover_config.clone(),
        )));
        provider.provider_lease_time_control = Some(lease_time_control.clone());
        let provider = provider.run_with_provider_cleanup(scenario.clone()).await;

        audit_ppsc_history(&oracle)
            .unwrap_or_else(|error| panic!("provider terminal oracle: {error}"));
        audit_ppsc_history(&provider)
            .unwrap_or_else(|error| panic!("{} provider authority: {error}", backend.as_str()));
        assert_ppsc_terminal_matches(backend, &scenario, &oracle.terminal, &provider.terminal);
    }
}

#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
fn provider_replay_scenarios(
    defaults: Vec<PpscScenario>,
    requires_provider_authority: bool,
) -> Vec<PpscScenario> {
    const REPLAY_SCENARIO_ENV: &str = "NIMBUS_PPSC_REPLAY_SCENARIO_JSON";
    let Ok(json) = env::var(REPLAY_SCENARIO_ENV) else {
        return defaults;
    };
    let scenario = PpscScenario::from_canonical_json(&json)
        .unwrap_or_else(|error| panic!("{REPLAY_SCENARIO_ENV} is invalid: {error}"));
    assert_eq!(
        scenario.requires_provider_authority(),
        requires_provider_authority,
        "{REPLAY_SCENARIO_ENV} selected scenario '{}' for the wrong provider differential lane",
        scenario.name
    );
    vec![scenario]
}

#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
fn provider_authority_terminal_oracle(scenario: &PpscScenario) -> PpscScenario {
    let steps = scenario
        .steps
        .iter()
        .map(|step| match &step.operation {
            PpscOperation::ExpireProviderLease { .. }
            | PpscOperation::AttemptStaleProviderWrite { .. } => PpscStep::new(
                PpscOperation::AdvanceMonotonicClock { millis: 0 },
                PpscExpectedOutcome::Observed,
            ),
            PpscOperation::ProviderTakeover { tenant } => PpscStep::new(
                PpscOperation::Mutation {
                    tenant: tenant.clone(),
                    route: PpscRoute::QueuedJournal,
                    key: "provider-takeover".to_string(),
                    value: provider_takeover_value(scenario.seed),
                },
                PpscExpectedOutcome::Committed,
            ),
            _ => step.clone(),
        })
        .collect();
    PpscScenario::new(
        format!("{}-terminal-oracle", scenario.name),
        scenario.seed,
        steps,
    )
    .expect("provider-authority terminal oracle should remain a valid bounded scenario")
}

fn assert_ppsc_terminal_matches(
    backend: PpscBackend,
    scenario: &PpscScenario,
    oracle: &PpscTerminalState,
    candidate: &PpscTerminalState,
) {
    if candidate == oracle {
        return;
    }
    let tenant_names = oracle
        .tenants
        .keys()
        .chain(candidate.tenants.keys())
        .collect::<BTreeSet<_>>();
    for tenant in tenant_names {
        let Some(expected) = oracle.tenants.get(tenant) else {
            panic!(
                "PPSC {} seed {} has unexpected tenant {tenant}; replay: {}",
                backend.as_str(),
                scenario.seed,
                scenario.replay_command(backend)
            );
        };
        let Some(actual) = candidate.tenants.get(tenant) else {
            panic!(
                "PPSC {} seed {} is missing tenant {tenant}; replay: {}",
                backend.as_str(),
                scenario.seed,
                scenario.replay_command(backend)
            );
        };
        if actual.frontiers != expected.frontiers {
            panic!(
                "PPSC {} seed {} tenant {tenant} frontier divergence: expected {:?}, actual {:?}; replay: {}",
                backend.as_str(),
                scenario.seed,
                expected.frontiers,
                actual.frontiers,
                scenario.replay_command(backend)
            );
        }
        if actual.journal.len() != expected.journal.len() {
            panic!(
                "PPSC {} seed {} tenant {tenant} journal length divergence: expected {}, actual {}; frontiers {:?}; replay: {}",
                backend.as_str(),
                scenario.seed,
                expected.journal.len(),
                actual.journal.len(),
                actual.frontiers,
                scenario.replay_command(backend)
            );
        }
        for (index, (expected_record, actual_record)) in expected
            .journal
            .iter()
            .zip(actual.journal.iter())
            .enumerate()
        {
            if expected_record == actual_record {
                continue;
            }
            let expected_kind = ppsc_record_kind(&expected_record.canonical_bytes);
            let actual_kind = ppsc_record_kind(&actual_record.canonical_bytes);
            let byte_offset = expected_record
                .canonical_bytes
                .iter()
                .zip(actual_record.canonical_bytes.iter())
                .position(|(expected, actual)| expected != actual)
                .unwrap_or_else(|| {
                    expected_record
                        .canonical_bytes
                        .len()
                        .min(actual_record.canonical_bytes.len())
                });
            panic!(
                "PPSC {} seed {} tenant {tenant} first journal divergence at record {index}, sequence expected {} actual {}, byte {byte_offset}, lengths expected {} actual {}, event kinds expected {expected_kind} actual {actual_kind}; frontiers {:?}; replay: {}",
                backend.as_str(),
                scenario.seed,
                expected_record.sequence,
                actual_record.sequence,
                expected_record.canonical_bytes.len(),
                actual_record.canonical_bytes.len(),
                actual.frontiers,
                scenario.replay_command(backend)
            );
        }
        for (field, matches) in [
            ("publications", actual.publications == expected.publications),
            ("documents", actual.documents == expected.documents),
            ("schema", actual.schema == expected.schema),
            (
                "scheduled-jobs",
                actual.scheduled_jobs == expected.scheduled_jobs,
            ),
            (
                "trigger-cursor",
                actual.trigger_cursor == expected.trigger_cursor,
            ),
            (
                "projection-durable-sequence",
                actual.projection_durable_sequence == expected.projection_durable_sequence,
            ),
        ] {
            if !matches {
                let detail = match field {
                    "scheduled-jobs" => format!(
                        "; expected {:?}, actual {:?}",
                        expected.scheduled_jobs, actual.scheduled_jobs
                    ),
                    _ => String::new(),
                };
                panic!(
                    "PPSC {} seed {} tenant {tenant} terminal field {field} diverged after identical journal/frontiers{detail}; replay: {}",
                    backend.as_str(),
                    scenario.seed,
                    scenario.replay_command(backend)
                );
            }
        }
    }
    unreachable!("a terminal-state difference must identify a tenant field")
}

fn ppsc_record_kind(bytes: &[u8]) -> String {
    nimbus_storage::commit_log::deserialize_tenant_event_record(bytes)
        .map(|record| record_kind(record.events()))
        .unwrap_or_else(|error| format!("invalid-record:{error}"))
}
