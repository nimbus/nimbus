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
        PpscOperation::Replay {
            tenant: "tenant-a".to_string(),
            sequence: 1,
            identity: "second".to_string(),
        },
        PpscExpectedOutcome::Committed,
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
fn ppsc_retained_seed_covers_ambiguous_replay_and_takeover() {
    let retained = retained_ppsc_scenarios();
    assert_eq!(retained.len(), 16);
    assert!(retained.iter().all(|scenario| {
        scenario.steps.iter().any(|step| {
            step.expected == PpscExpectedOutcome::AmbiguousRecovered
                && matches!(step.operation, PpscOperation::Mutation { .. })
        }) && scenario.steps.iter().any(|step| {
            step.expected == PpscExpectedOutcome::DefinitiveRollback
                && matches!(step.operation, PpscOperation::Replay { .. })
        })
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
        let saw_hold = candidate.steps.iter().any(|step| {
            matches!(
                step.operation,
                PpscOperation::ArmFault {
                    fault: PpscInjectedFault::PublicationPredecessorHeld,
                    ..
                }
            )
        });
        let saw_release = candidate.steps.iter().any(|step| {
            matches!(
                step.operation,
                PpscOperation::ReleaseFault {
                    fault: PpscInjectedFault::PublicationPredecessorHeld,
                    ..
                }
            )
        });
        saw_hold && saw_release
    });
    assert_eq!(shrunk.steps.len(), 2);
    assert!(matches!(
        shrunk.steps[0].operation,
        PpscOperation::ArmFault { .. }
    ));
    assert!(matches!(
        shrunk.steps[1].operation,
        PpscOperation::ReleaseFault { .. }
    ));
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
        .arm(
            tenant.clone(),
            PpscInjectedFault::PublicationPredecessorHeld,
        )
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
