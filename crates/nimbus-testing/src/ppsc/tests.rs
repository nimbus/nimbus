use super::*;
use nimbus_core::{Error, StorageErrorKind, TenantId};
use nimbus_storage::{FaultInjector, FaultPoint};
use std::collections::BTreeMap;

fn observed(scenario: &PpscScenario) -> Vec<PpscObservedStep> {
    scenario
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| PpscObservedStep {
            index,
            outcome: step.expected,
            effects: Vec::new(),
        })
        .collect()
}

fn one_step_scenario(operation: PpscOperation, expected: PpscExpectedOutcome) -> PpscScenario {
    PpscScenario::new("one-step", 17, vec![PpscStep::new(operation, expected)])
        .expect("one-step scenario should be valid")
}

fn tenant_state(frontiers: PpscFrontiers, tenant: &str) -> PpscTenantState {
    PpscTenantState {
        journal: (1..=frontiers.durable_head)
            .map(|sequence| PpscJournalEntry {
                sequence,
                canonical_bytes: format!("record-{sequence}").into_bytes(),
            })
            .collect(),
        publications: (1..=frontiers.published_head)
            .map(|sequence| PpscPublication {
                tenant: tenant.to_string(),
                sequence,
                identity: format!("record-{sequence}"),
                step: 0,
            })
            .collect(),
        frontiers,
        ..PpscTenantState::default()
    }
}

fn history(
    backend: PpscBackend,
    scenario: PpscScenario,
    tenants: BTreeMap<String, PpscTenantState>,
    sequence_claims: Vec<PpscSequenceClaim>,
) -> PpscHistory {
    PpscHistory {
        backend,
        observed_steps: observed(&scenario),
        scenario,
        sequence_claims,
        terminal: PpscTerminalState { tenants },
    }
}

fn seed_farm_vars() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("NIMBUS_PPSC_BACKEND".to_string(), "redb".to_string()),
        ("NIMBUS_PPSC_SEED_START".to_string(), "0".to_string()),
        ("NIMBUS_PPSC_SEED_COUNT".to_string(), "1000".to_string()),
        ("NIMBUS_PPSC_SHARD_INDEX".to_string(), "0".to_string()),
        ("NIMBUS_PPSC_SHARD_COUNT".to_string(), "4".to_string()),
        (
            "NIMBUS_PPSC_FAILURE_DIR".to_string(),
            "target/ppsc-seed-farm/test".to_string(),
        ),
        (
            "NIMBUS_PPSC_REVISION".to_string(),
            "test-revision".to_string(),
        ),
    ])
}

fn seed_farm_config(
    vars: &BTreeMap<String, String>,
) -> Result<PpscSeedFarmConfig, PpscSeedFarmError> {
    PpscSeedFarmConfig::from_lookup(|name| vars.get(name).cloned())
}

#[test]
fn ppsc_scenario_replay_is_byte_deterministic() {
    let first = PpscScenario::seeded(83, 64).expect("seed should generate");
    let replay = PpscScenario::seeded(83, 64).expect("seed should replay");
    let different = PpscScenario::seeded(84, 64).expect("different seed should generate");

    assert_eq!(first, replay);
    assert_eq!(first.canonical_bytes(), replay.canonical_bytes());
    assert_ne!(first.canonical_bytes(), different.canonical_bytes());
    assert_eq!(first.steps.len(), 64);
    assert!(
        PpscScenario::seeded(83, PPSC_MAX_STEPS + 1)
            .expect_err("oversized scenarios must fail")
            .to_string()
            .contains("maximum")
    );
}

#[test]
fn ppsc_seed_farm_four_shards_cover_exact_range_without_overlap() {
    let mut selected = Vec::new();
    for shard_index in 0..4 {
        let seeds = select_shard(10_000, 1_000, shard_index, 4)
            .expect("valid seed-farm shard should select");
        assert_eq!(seeds.len(), 250);
        let shard_offset = u64::try_from(shard_index).expect("shard index should fit") * 250;
        assert_eq!(seeds.first(), Some(&(10_000 + shard_offset)));
        assert_eq!(seeds.last(), Some(&(10_249 + shard_offset)));
        selected.extend(seeds);
    }
    assert_eq!(selected, (10_000..11_000).collect::<Vec<_>>());
}

#[test]
fn ppsc_seed_farm_single_seed_replay_is_deterministic() {
    let mut vars = seed_farm_vars();
    vars.retain(|name, _| {
        !matches!(
            name.as_str(),
            "NIMBUS_PPSC_SEED_START"
                | "NIMBUS_PPSC_SEED_COUNT"
                | "NIMBUS_PPSC_SHARD_INDEX"
                | "NIMBUS_PPSC_SHARD_COUNT"
        )
    });
    vars.insert("NIMBUS_PPSC_SEED".to_string(), "83".to_string());
    let config = seed_farm_config(&vars).expect("single seed should configure");
    assert_eq!(config.seeds, vec![83]);
    assert_eq!(config.selected_count(), 1);

    let first = PpscScenario::seeded(config.seeds[0], config.step_count)
        .expect("single seed should generate");
    let replay = PpscScenario::seeded(config.seeds[0], config.step_count)
        .expect("single seed should replay");
    assert_eq!(first.canonical_bytes(), replay.canonical_bytes());
}

#[test]
fn ppsc_seed_farm_rejects_zero_count_unknown_backend_and_invalid_shard() {
    let mut zero = seed_farm_vars();
    zero.insert("NIMBUS_PPSC_SEED_COUNT".to_string(), "0".to_string());
    assert!(
        seed_farm_config(&zero)
            .expect_err("zero-count farm must fail")
            .to_string()
            .contains("greater than zero")
    );

    let mut unknown = seed_farm_vars();
    unknown.insert("NIMBUS_PPSC_BACKEND".to_string(), "mysql".to_string());
    let error = seed_farm_config(&unknown).expect_err("bulk provider farm must fail");
    assert!(error.to_string().contains("unsupported"));
    assert!(error.to_string().contains("live-provider differential"));

    let mut invalid_shard = seed_farm_vars();
    invalid_shard.insert("NIMBUS_PPSC_SHARD_INDEX".to_string(), "4".to_string());
    assert!(
        seed_farm_config(&invalid_shard)
            .expect_err("out-of-range shard must fail")
            .to_string()
            .contains("must be less")
    );

    let mut overflowing_range = seed_farm_vars();
    overflowing_range.insert("NIMBUS_PPSC_SEED_START".to_string(), u64::MAX.to_string());
    overflowing_range.insert("NIMBUS_PPSC_SEED_COUNT".to_string(), "2".to_string());
    overflowing_range.insert("NIMBUS_PPSC_SHARD_COUNT".to_string(), "2".to_string());
    assert!(
        seed_farm_config(&overflowing_range)
            .expect_err("globally overflowing range must fail in every shard")
            .to_string()
            .contains("overflows")
    );
}

#[test]
fn ppsc_seed_farm_partial_or_failed_summary_requires_nonzero_exit() {
    let complete = PpscSeedFarmSummary {
        format_version: 1,
        revision: "test-revision".to_string(),
        backend: PpscBackend::Redb,
        seed_start: 0,
        seed_count: 4,
        shard_index: 0,
        shard_count: 1,
        selected: 4,
        executed: 4,
        passed: 4,
        failed: 0,
        retained: 0,
    };
    assert!(complete.is_complete_success());

    for incomplete in [
        PpscSeedFarmSummary {
            executed: 0,
            passed: 0,
            ..complete.clone()
        },
        PpscSeedFarmSummary {
            executed: 3,
            passed: 3,
            ..complete.clone()
        },
        PpscSeedFarmSummary {
            passed: 3,
            failed: 1,
            ..complete.clone()
        },
    ] {
        assert!(
            !incomplete.is_complete_success(),
            "zero, partial, and failed execution must propagate a nonzero result"
        );
    }
}

#[test]
fn ppsc_seed_farm_failure_bundle_replaces_interruption_marker() {
    let directory = tempfile::tempdir().expect("artifact directory should build");
    let mut vars = seed_farm_vars();
    vars.insert(
        "NIMBUS_PPSC_FAILURE_DIR".to_string(),
        directory.path().display().to_string(),
    );
    let config = seed_farm_config(&vars).expect("farm should configure");
    let artifacts =
        PpscSeedFarmArtifacts::new(directory.path()).expect("artifact owner should build");
    let scenario =
        PpscScenario::seeded(config.seeds[0], config.step_count).expect("scenario should build");
    let pending = artifacts
        .begin_seed(&config, &scenario)
        .expect("interruption marker should write");
    assert!(pending.is_file());

    let failure = artifacts
        .mark_seed_failed(
            &config,
            &scenario,
            &pending,
            "durable sequence identity diverged",
        )
        .expect("failure bundle should replace marker");
    assert!(!pending.exists());
    let bundle: PpscSeedFarmFailureBundle =
        serde_json::from_slice(&std::fs::read(&failure).expect("failure bundle should read"))
            .expect("failure bundle should deserialize");
    assert_eq!(bundle.kind, PpscSeedFarmFailureKind::Failed);
    assert_eq!(bundle.seed, scenario.seed);
    assert_eq!(
        bundle.scenario.canonical_bytes(),
        scenario.canonical_bytes()
    );
    assert!(bundle.message.contains("sequence identity"));
    assert!(bundle.replay_command.contains("make verify-ppsc-seed-farm"));
}

#[test]
fn ppsc_seed_farm_interruption_retains_current_failure_bundle() {
    let directory = tempfile::tempdir().expect("artifact directory should build");
    let mut vars = seed_farm_vars();
    vars.insert(
        "NIMBUS_PPSC_FAILURE_DIR".to_string(),
        directory.path().display().to_string(),
    );
    let config = seed_farm_config(&vars).expect("farm should configure");
    let artifacts =
        PpscSeedFarmArtifacts::new(directory.path()).expect("artifact owner should build");
    let scenario =
        PpscScenario::seeded(config.seeds[0], config.step_count).expect("scenario should build");
    let pending = artifacts
        .begin_seed(&config, &scenario)
        .expect("interruption marker should write");

    let bundle: PpscSeedFarmFailureBundle = serde_json::from_slice(
        &std::fs::read(&pending).expect("interruption marker should survive"),
    )
    .expect("interruption marker should deserialize");
    assert_eq!(bundle.kind, PpscSeedFarmFailureKind::Interrupted);
    assert_eq!(bundle.seed, scenario.seed);
    assert_eq!(bundle.scenario, scenario);
}

#[test]
fn ppsc_seed_farm_success_removes_interruption_marker_and_writes_summary() {
    let directory = tempfile::tempdir().expect("artifact directory should build");
    let mut vars = seed_farm_vars();
    vars.insert(
        "NIMBUS_PPSC_FAILURE_DIR".to_string(),
        directory.path().display().to_string(),
    );
    let config = seed_farm_config(&vars).expect("farm should configure");
    let artifacts =
        PpscSeedFarmArtifacts::new(directory.path()).expect("artifact owner should build");
    let scenario =
        PpscScenario::seeded(config.seeds[0], config.step_count).expect("scenario should build");
    let pending = artifacts
        .begin_seed(&config, &scenario)
        .expect("interruption marker should write");
    artifacts
        .mark_seed_passed(&pending)
        .expect("successful seed should remove marker");
    assert!(!pending.exists());

    let summary = PpscSeedFarmSummary {
        format_version: 1,
        revision: config.revision.clone(),
        backend: config.backend,
        seed_start: config.seed_start,
        seed_count: config.seed_count,
        shard_index: config.shard_index,
        shard_count: config.shard_count,
        selected: config.selected_count(),
        executed: config.selected_count(),
        passed: config.selected_count(),
        failed: 0,
        retained: 4,
    };
    let path = artifacts
        .write_summary(&summary)
        .expect("summary should publish");
    let observed: PpscSeedFarmSummary =
        serde_json::from_slice(&std::fs::read(path).expect("summary should read"))
            .expect("summary should deserialize");
    assert_eq!(observed, summary);
    assert!(observed.is_complete_success());
}

#[test]
fn ppsc_seed_farm_reuse_cleans_only_owned_stale_artifacts() {
    let directory = tempfile::tempdir().expect("artifact directory should build");
    let stale_summary = directory.path().join("summary.json");
    let stale_failure = directory
        .path()
        .join("seed-00000000000000000083-failure.json");
    let stale_summary_temporary = directory.path().join("summary.tmp-1234");
    let stale_seed_temporary = directory
        .path()
        .join("seed-00000000000000000083-failure.tmp-1234");
    let foreign = directory.path().join("operator-note.txt");
    let foreign_lookalike = directory.path().join("summary.tmp-operator");
    std::fs::write(&stale_summary, b"stale").expect("stale summary should write");
    std::fs::write(&stale_failure, b"stale").expect("stale failure should write");
    std::fs::write(&stale_summary_temporary, b"stale")
        .expect("stale summary temporary should write");
    std::fs::write(&stale_seed_temporary, b"stale").expect("stale seed temporary should write");
    std::fs::write(&foreign, b"preserve").expect("foreign note should write");
    std::fs::write(&foreign_lookalike, b"preserve").expect("foreign lookalike should write");

    PpscSeedFarmArtifacts::new(directory.path()).expect("artifact owner should clean its files");
    assert!(!stale_summary.exists());
    assert!(!stale_failure.exists());
    assert!(!stale_summary_temporary.exists());
    assert!(!stale_seed_temporary.exists());
    assert_eq!(
        std::fs::read(&foreign).expect("foreign note should remain"),
        b"preserve"
    );
    assert_eq!(
        std::fs::read(&foreign_lookalike).expect("foreign lookalike should remain"),
        b"preserve"
    );
}

#[test]
fn ppsc_scenario_rejects_cancellation_without_cancellable_admission() {
    let error = PpscScenario::new(
        "invalid-cancellation-route",
        91,
        vec![PpscStep::new(
            PpscOperation::CancelNext {
                tenant: "tenant-a".to_string(),
                route: PpscRoute::Direct,
            },
            PpscExpectedOutcome::Cancelled,
        )],
    )
    .expect_err("synchronous direct cancellation must fail before execution");
    assert!(error.to_string().contains("synchronous direct route"));
    assert!(
        error
            .to_string()
            .contains("queued-journal or execution-unit")
    );

    for route in [PpscRoute::QueuedJournal, PpscRoute::ExecutionUnit] {
        PpscScenario::new(
            "valid-cancellation-route",
            92,
            vec![PpscStep::new(
                PpscOperation::CancelNext {
                    tenant: "tenant-a".to_string(),
                    route,
                },
                PpscExpectedOutcome::Cancelled,
            )],
        )
        .expect("asynchronous cancellation route should remain valid");
    }

    let wrong_commit_phase = PpscScenario::new(
        "invalid-commit-phase-owner",
        93,
        vec![PpscStep::new(
            PpscOperation::CommitPhaseFault {
                tenant: "tenant-a".to_string(),
                route: PpscRoute::QueuedJournal,
                fault: PpscInjectedFault::AcknowledgementLoss,
            },
            PpscExpectedOutcome::AmbiguousRecovered,
        )],
    )
    .expect_err("storage fault must not route through the commit-phase operation");
    assert!(wrong_commit_phase.to_string().contains("non-commit-phase"));

    let wrong_storage_arm = PpscScenario::new(
        "invalid-storage-fault-owner",
        94,
        vec![PpscStep::new(
            PpscOperation::ArmFault {
                tenant: "tenant-a".to_string(),
                fault: PpscInjectedFault::PanicAfterDurable,
            },
            PpscExpectedOutcome::Observed,
        )],
    )
    .expect_err("commit-phase fault must not route through storage arm/release");
    assert!(wrong_storage_arm.to_string().contains("non-storage"));
}

#[test]
fn ppsc_backend_capability_table_rejects_unsupported_steps() {
    let crash = one_step_scenario(PpscOperation::Crash, PpscExpectedOutcome::Observed);
    assert!(crash.validate_for_backend(PpscBackend::Redb).is_ok());
    let memory_error = crash
        .validate_for_backend(PpscBackend::Memory)
        .expect_err("memory must not claim durable reopen");
    assert!(memory_error.to_string().contains("durable reopen"));
    assert!(memory_error.to_string().contains("NIMBUS_PPSC_SEED=17"));

    let provider = one_step_scenario(
        PpscOperation::ExpireProviderLease {
            tenant: "tenant-a".to_string(),
        },
        PpscExpectedOutcome::Observed,
    );
    for backend in [PpscBackend::Redb, PpscBackend::Sqlite] {
        assert!(
            provider
                .validate_for_backend(backend)
                .expect_err("embedded backend must reject provider authority")
                .to_string()
                .contains("provider sequence authority")
        );
    }
    for backend in PpscBackend::PROVIDERS {
        provider
            .validate_for_backend(backend)
            .expect("provider backend should accept provider authority");
    }
}

#[test]
fn ppsc_embedded_backend_rejects_provider_authority_steps() {
    for (operation, expected) in [
        (
            PpscOperation::ExpireProviderLease {
                tenant: "tenant-a".to_string(),
            },
            PpscExpectedOutcome::Observed,
        ),
        (
            PpscOperation::ProviderTakeover {
                tenant: "tenant-a".to_string(),
            },
            PpscExpectedOutcome::Committed,
        ),
        (
            PpscOperation::AttemptStaleProviderWrite {
                tenant: "tenant-a".to_string(),
            },
            PpscExpectedOutcome::Fenced,
        ),
    ] {
        let scenario = one_step_scenario(operation, expected);
        for backend in [PpscBackend::Redb, PpscBackend::Sqlite] {
            let error = scenario
                .validate_for_backend(backend)
                .expect_err("embedded adapters must reject provider-authority operations");
            assert!(error.to_string().contains("provider sequence authority"));
            assert!(error.to_string().contains("NIMBUS_PPSC_SEED=17"));
        }
    }
}

#[test]
fn ppsc_legal_state_auditor_accepts_every_declared_terminal_state() {
    let scenario = one_step_scenario(
        PpscOperation::AdvanceWallClock { millis: 1 },
        PpscExpectedOutcome::Observed,
    );
    let legal_frontiers = [
        PpscFrontiers::default(),
        PpscFrontiers {
            assigned_high_water: 1,
            ..PpscFrontiers::default()
        },
        PpscFrontiers {
            assigned_high_water: 2,
            active_assigned_head: 1,
            durable_head: 1,
            ..PpscFrontiers::default()
        },
        PpscFrontiers {
            assigned_high_water: 3,
            active_assigned_head: 3,
            durable_head: 3,
            storage_applied_head: 2,
            published_head: 2,
            applied_head: 1,
        },
        PpscFrontiers {
            assigned_high_water: 4,
            active_assigned_head: 4,
            durable_head: 4,
            storage_applied_head: 4,
            published_head: 4,
            applied_head: 4,
        },
    ];
    for frontiers in legal_frontiers {
        let tenant = "tenant-a";
        let candidate = history(
            PpscBackend::Redb,
            scenario.clone(),
            BTreeMap::from([(tenant.to_string(), tenant_state(frontiers, tenant))]),
            Vec::new(),
        );
        audit_ppsc_history(&candidate).expect("declared legal frontier state should pass");
    }
}

#[test]
fn ppsc_legal_state_auditor_accepts_definitive_rollback_sequence_reuse() {
    let scenario = one_step_scenario(
        PpscOperation::AdvanceMonotonicClock { millis: 1 },
        PpscExpectedOutcome::Observed,
    );
    let claims = vec![
        PpscSequenceClaim {
            tenant: "tenant-a".to_string(),
            sequence: 1,
            identity: "first".to_string(),
            ownership: PpscSequenceOwnership::DefinitiveRollback,
            step: 0,
        },
        PpscSequenceClaim {
            tenant: "tenant-a".to_string(),
            sequence: 1,
            identity: "second".to_string(),
            ownership: PpscSequenceOwnership::Durable,
            step: 0,
        },
    ];
    let state = tenant_state(
        PpscFrontiers {
            assigned_high_water: 2,
            active_assigned_head: 1,
            durable_head: 1,
            storage_applied_head: 1,
            published_head: 1,
            applied_head: 1,
        },
        "tenant-a",
    );
    audit_ppsc_history(&history(
        PpscBackend::Redb,
        scenario,
        BTreeMap::from([("tenant-a".to_string(), state)]),
        claims,
    ))
    .expect("definitively rolled-back identity may release its active sequence claim");
}

#[test]
fn ppsc_legal_state_auditor_rejects_durable_sequence_reuse() {
    let scenario = one_step_scenario(
        PpscOperation::AdvanceMonotonicClock { millis: 1 },
        PpscExpectedOutcome::Observed,
    );
    let error = audit_ppsc_history(&history(
        PpscBackend::Redb,
        scenario,
        BTreeMap::new(),
        vec![
            PpscSequenceClaim {
                tenant: "tenant-a".to_string(),
                sequence: 7,
                identity: "durable-a".to_string(),
                ownership: PpscSequenceOwnership::Durable,
                step: 0,
            },
            PpscSequenceClaim {
                tenant: "tenant-a".to_string(),
                sequence: 7,
                identity: "different-b".to_string(),
                ownership: PpscSequenceOwnership::Durable,
                step: 0,
            },
        ],
    ))
    .expect_err("different content may not reuse durable sequence identity");
    assert_eq!(error.invariant, "durable-sequence-identity-reuse");
    assert!(error.to_string().contains("make verify-ppsc-seed-farm"));
}

#[test]
fn ppsc_legal_state_auditor_rejects_publication_leapfrog() {
    let scenario = one_step_scenario(
        PpscOperation::AdvanceWallClock { millis: 1 },
        PpscExpectedOutcome::Observed,
    );
    let tenant = "tenant-a";
    let mut state = tenant_state(
        PpscFrontiers {
            assigned_high_water: 2,
            active_assigned_head: 2,
            durable_head: 2,
            storage_applied_head: 2,
            published_head: 2,
            applied_head: 2,
        },
        tenant,
    );
    state.publications[0].sequence = 2;
    let error = audit_ppsc_history(&history(
        PpscBackend::Redb,
        scenario,
        BTreeMap::from([(tenant.to_string(), state)]),
        Vec::new(),
    ))
    .expect_err("a later record must not publish before its predecessor");
    assert_eq!(error.invariant, "publication-leapfrog");
}

#[test]
fn ppsc_retained_seed_covers_ambiguous_recovery_and_takeover() {
    let retained = retained_ppsc_scenarios();
    assert_eq!(retained.len(), 16);
    assert!(retained.iter().all(|scenario| {
        scenario.steps.iter().any(|step| {
            step.expected == PpscExpectedOutcome::AmbiguousRecovered
                && matches!(step.operation, PpscOperation::Mutation { .. })
        }) && scenario
            .steps
            .iter()
            .any(|step| matches!(step.operation, PpscOperation::Crash))
            && scenario
                .steps
                .iter()
                .any(|step| matches!(step.operation, PpscOperation::Reopen))
    }));

    let provider = retained_provider_authority_scenarios();
    assert_eq!(provider.len(), 3);
    assert!(provider.iter().all(|scenario| {
        scenario
            .steps
            .iter()
            .any(|step| matches!(step.operation, PpscOperation::ProviderTakeover { .. }))
            && scenario.steps.iter().any(|step| {
                matches!(
                    step.operation,
                    PpscOperation::AttemptStaleProviderWrite { .. }
                )
            })
    }));
}

#[test]
fn ppsc_retained_seed_covers_cancellation_overload_and_shutdown() {
    for scenario in retained_ppsc_scenarios() {
        assert!(scenario.steps.iter().any(|step| {
            step.expected == PpscExpectedOutcome::Cancelled
                && matches!(step.operation, PpscOperation::CancelNext { .. })
        }));
        assert!(scenario.steps.iter().any(|step| {
            step.expected == PpscExpectedOutcome::Overloaded
                && matches!(step.operation, PpscOperation::ForceOverload { .. })
        }));
        assert!(scenario.steps.iter().any(|step| {
            step.expected == PpscExpectedOutcome::Shutdown
                && matches!(step.operation, PpscOperation::Quiesce)
        }));
        assert!(matches!(
            scenario.steps.last().map(|step| &step.operation),
            Some(PpscOperation::Quiesce)
        ));
        assert_eq!(
            scenario
                .steps
                .iter()
                .filter(|step| matches!(step.operation, PpscOperation::Quiesce))
                .count(),
            1,
            "shutdown must be a unique terminal operation"
        );
        assert!(
            scenario
                .steps
                .iter()
                .any(|step| matches!(step.operation, PpscOperation::CommitPermutation { .. }))
        );
        assert!(
            scenario
                .steps
                .iter()
                .any(|step| matches!(step.operation, PpscOperation::ZeroWriteExecutionUnit { .. }))
        );
        assert!(
            scenario
                .steps
                .iter()
                .any(|step| matches!(step.operation, PpscOperation::ConflictRetry { .. }))
        );
        assert!(scenario.steps.iter().any(|step| {
            step.expected == PpscExpectedOutcome::ProviderError
                && matches!(step.operation, PpscOperation::Mutation { .. })
        }));
        assert!(scenario.steps.iter().any(|step| {
            matches!(
                step.operation,
                PpscOperation::PublicationPredecessorRace { .. }
            )
        }));
        for fault in [
            PpscInjectedFault::DurableBeforePublish,
            PpscInjectedFault::PanicAfterDurable,
        ] {
            assert!(scenario.steps.iter().any(|step| {
                matches!(
                    step.operation,
                    PpscOperation::CommitPhaseFault {
                        fault: candidate,
                        ..
                    } if candidate == fault
                )
            }));
        }
    }
}

#[test]
fn ppsc_hot_tenant_failure_preserves_other_tenant_progress() {
    let scenario = PpscScenario::new(
        "tenant-isolation",
        29,
        vec![
            PpscStep::new(
                PpscOperation::ForceOverload {
                    tenant: "hot".to_string(),
                },
                PpscExpectedOutcome::Overloaded,
            ),
            PpscStep::new(
                PpscOperation::Mutation {
                    tenant: "peer".to_string(),
                    route: PpscRoute::QueuedJournal,
                    key: "peer-key".to_string(),
                    value: 1,
                },
                PpscExpectedOutcome::Committed,
            ),
        ],
    )
    .expect("isolation scenario should build");
    let hot = tenant_state(PpscFrontiers::default(), "hot");
    let peer = tenant_state(
        PpscFrontiers {
            assigned_high_water: 1,
            active_assigned_head: 1,
            durable_head: 1,
            storage_applied_head: 1,
            published_head: 1,
            applied_head: 1,
        },
        "peer",
    );
    let passing = history(
        PpscBackend::Memory,
        scenario.clone(),
        BTreeMap::from([("hot".to_string(), hot.clone()), ("peer".to_string(), peer)]),
        Vec::new(),
    );
    audit_ppsc_history(&passing).expect("peer progress should contain hot-tenant failure");

    let failing = history(
        PpscBackend::Memory,
        scenario,
        BTreeMap::from([
            ("hot".to_string(), hot),
            (
                "peer".to_string(),
                tenant_state(PpscFrontiers::default(), "peer"),
            ),
        ]),
        Vec::new(),
    );
    assert_eq!(
        audit_ppsc_history(&failing)
            .expect_err("peer must make durable progress")
            .invariant,
        "tenant-isolation"
    );
}

#[test]
fn ppsc_shrinker_retains_minimal_ordered_failure() {
    let scenario = PpscScenario::seeded(37, 48).expect("seed should generate");
    let shrunk = shrink_failing_ppsc_scenario(&scenario, |candidate| {
        let saw_race = candidate.steps.iter().any(|step| {
            matches!(
                step.operation,
                PpscOperation::PublicationPredecessorRace { .. }
            )
        });
        let saw_crash = candidate
            .steps
            .iter()
            .any(|step| matches!(step.operation, PpscOperation::Crash));
        saw_race && saw_crash
    });
    assert_eq!(shrunk.steps.len(), 2);
    assert!(matches!(
        shrunk.steps[0].operation,
        PpscOperation::PublicationPredecessorRace { .. }
    ));
    assert!(matches!(shrunk.steps[1].operation, PpscOperation::Crash));
}

#[test]
fn ppsc_storage_acknowledgement_loss_is_tenant_scoped_and_one_shot() {
    let injector = PpscStorageFaultInjector::new();
    let target = TenantId::new("ppsc-fault-target").expect("target tenant should build");
    let peer = TenantId::new("ppsc-fault-peer").expect("peer tenant should build");
    injector
        .arm(target.clone(), PpscInjectedFault::AcknowledgementLoss)
        .expect("acknowledgement loss should arm");

    injector
        .check(FaultPoint::StorageCommitAfterVisibilityBeforeReturn)
        .expect("an unscoped check must not consume a tenant arm");
    injector
        .check_for_tenant(FaultPoint::StorageCommitAfterVisibilityBeforeReturn, &peer)
        .expect("peer work must not consume the target tenant's arm");
    let error = injector
        .check_for_tenant(
            FaultPoint::StorageCommitAfterVisibilityBeforeReturn,
            &target,
        )
        .expect_err("the target tenant's acknowledgement should be lost once");
    assert_eq!(error.storage_kind(), Some(StorageErrorKind::Transient));
    injector
        .check_for_tenant(
            FaultPoint::StorageCommitAfterVisibilityBeforeReturn,
            &target,
        )
        .expect("the one-shot arm must disarm after its first failure");

    assert_eq!(
        injector
            .snapshot(&target, PpscInjectedFault::AcknowledgementLoss)
            .expect("target snapshot should load"),
        PpscStorageFaultSnapshot {
            active: false,
            visits: 1,
            fires: 1,
        }
    );
    assert_eq!(
        injector
            .snapshot(&peer, PpscInjectedFault::AcknowledgementLoss)
            .expect("peer snapshot should load"),
        PpscStorageFaultSnapshot {
            active: false,
            visits: 0,
            fires: 0,
        }
    );
}

#[test]
fn ppsc_storage_provider_transient_targets_journal_previsibility_until_release() {
    let injector = PpscStorageFaultInjector::new();
    let tenant = TenantId::new("ppsc-provider-transient").expect("tenant should build");
    injector
        .arm(tenant.clone(), PpscInjectedFault::ProviderTransient)
        .expect("provider transient should arm");

    injector
        .check_for_tenant(FaultPoint::StorageCommitBeforeVisibility, &tenant)
        .expect("a materialized-apply boundary must not consume the provider fault");

    for expected_fire in 1..=3 {
        let error = injector
            .check_for_tenant(FaultPoint::JournalAppendBeforeDurableFlush, &tenant)
            .unwrap_err();
        assert_eq!(error.storage_kind(), Some(StorageErrorKind::Transient));
        assert!(error.to_string().contains(&format!("fire {expected_fire}")));
    }
    assert_eq!(
        injector
            .snapshot(&tenant, PpscInjectedFault::ProviderTransient)
            .expect("armed snapshot should load"),
        PpscStorageFaultSnapshot {
            active: true,
            visits: 3,
            fires: 3,
        }
    );

    injector
        .release(&tenant, PpscInjectedFault::ProviderTransient)
        .expect("provider transient should release");
    injector
        .check_for_tenant(FaultPoint::JournalAppendBeforeDurableFlush, &tenant)
        .expect("released provider transient must stop failing");
    assert!(
        !injector
            .snapshot(&tenant, PpscInjectedFault::ProviderTransient)
            .expect("released snapshot should load")
            .active
    );
}

#[test]
fn ppsc_storage_fault_interface_rejects_wrong_owner_and_invalid_transitions() {
    let injector = PpscStorageFaultInjector::new();
    let tenant = TenantId::new("ppsc-storage-fault-contract").expect("tenant should build");

    let wrong_owner = injector
        .arm(tenant.clone(), PpscInjectedFault::PanicAfterDurable)
        .expect_err("publisher fault must not arm through storage");
    assert!(matches!(wrong_owner, Error::InvalidInput(_)));
    let never_armed = injector
        .release(&tenant, PpscInjectedFault::ProviderTransient)
        .expect_err("release before arm must fail");
    assert!(matches!(never_armed, Error::InvalidInput(_)));

    injector
        .arm(tenant.clone(), PpscInjectedFault::ProviderTransient)
        .expect("first arm should succeed");
    let duplicate = injector
        .arm(tenant.clone(), PpscInjectedFault::ProviderTransient)
        .expect_err("duplicate active arm must fail");
    assert!(matches!(duplicate, Error::InvalidInput(_)));
    injector
        .release(&tenant, PpscInjectedFault::ProviderTransient)
        .expect("release should succeed");
    injector
        .arm(tenant, PpscInjectedFault::ProviderTransient)
        .expect("a released fault may be armed again");
}
