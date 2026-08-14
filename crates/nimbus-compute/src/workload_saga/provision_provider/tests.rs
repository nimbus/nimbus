use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_core::TenantId;
use nimbus_workloads::{
    WorkloadActivationIntent, WorkloadPublicationIntent, WorkloadSagaCommit, WorkloadSagaExpected,
    WorkloadSagaFuture, WorkloadSagaPage, WorkloadSagaPageRequest, WorkloadSagaPhase,
    WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest,
};

use super::*;
use crate::workload_saga::recovery::tests::provision_record;
use crate::workload_saga::{
    WorkloadProvisionCommandMode, WorkloadProvisionCommandResult, WorkloadProvisionDecision,
    WorkloadSagaCoordinator, reduce_command_result,
};

struct AppliedStore {
    record: Mutex<Option<WorkloadSagaRecord>>,
}

impl AppliedStore {
    fn new(record: Option<WorkloadSagaRecord>) -> Arc<Self> {
        Arc::new(Self {
            record: Mutex::new(record),
        })
    }
}

impl WorkloadSagaStore for AppliedStore {
    fn load<'a>(
        &'a self,
        _key: &'a nimbus_workloads::WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            Ok(self
                .record
                .lock()
                .expect("fixture store lock should be healthy")
                .clone())
        })
    }

    fn compare_and_swap<'a>(
        &'a self,
        _expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            *self
                .record
                .lock()
                .expect("fixture store lock should be healthy") = Some(next);
            Ok(WorkloadSagaCommit::Applied)
        })
    }

    fn list_recoverable<'a>(
        &'a self,
        request: WorkloadSagaPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaPage> {
        Box::pin(async move { WorkloadSagaPage::new(&request, Vec::new(), false) })
    }

    fn list_restart_candidates<'a>(
        &'a self,
        request: nimbus_workloads::WorkloadRestartCandidatePageRequest,
    ) -> nimbus_workloads::WorkloadSagaFuture<'a, nimbus_workloads::WorkloadRestartCandidatePage>
    {
        Box::pin(async move {
            nimbus_workloads::WorkloadRestartCandidatePage::new(&request, Vec::new(), false)
        })
    }

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        Box::pin(async move { WorkloadSagaTenantPage::new(tenant_id, &request, Vec::new(), false) })
    }
}

pub(crate) async fn command(
    label: &str,
    phase: WorkloadSagaPhase,
) -> ConfirmedWorkloadProvisionCommand {
    let current = provision_record(
        label,
        phase,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    command_for_record(current).await
}

pub(crate) async fn command_for_record(
    current: WorkloadSagaRecord,
) -> ConfirmedWorkloadProvisionCommand {
    let WorkloadProvisionDecision::Proposed(proposed) =
        WorkloadProvisionDecision::plan(&current).expect("fixture phase should be reducible")
    else {
        panic!("fixture phase should propose a provider step");
    };
    let coordinator = WorkloadSagaCoordinator::new(AppliedStore::new(Some(current.clone())));
    coordinator
        .confirm_provision_transition(&current, &proposed)
        .await
        .expect("fixture transition should confirm")
        .command()
        .expect("direct confirmation should create a provider command")
        .clone()
}

pub(crate) async fn activation_command_for_record(
    current: WorkloadSagaRecord,
) -> ConfirmedWorkloadProvisionCommand {
    let WorkloadProvisionDecision::Proposed(prerequisite) =
        WorkloadProvisionDecision::plan(&current).expect("fixture phase should be reducible")
    else {
        panic!("network-attached fixture should propose prerequisite inspection");
    };
    let coordinator = WorkloadSagaCoordinator::new(AppliedStore::new(Some(current.clone())));
    let confirmed_prerequisite = coordinator
        .confirm_provision_transition(&current, &prerequisite)
        .await
        .expect("prerequisite transition should confirm");
    let prerequisite_command = confirmed_prerequisite
        .command()
        .expect("prerequisite confirmation should create a command");
    assert_eq!(
        prerequisite_command.step(),
        WorkloadProvisionStep::InspectActivationPrerequisites
    );
    let prerequisite_result = WorkloadProvisionCommandResult::for_command(
        prerequisite_command,
        WorkloadProvisionInspectionResult::Succeeded {
            attempt_id: prerequisite_command.attempt_id().clone(),
            dispatch_epoch: prerequisite_command.dispatch_epoch(),
            provider_target: prerequisite_command.provider_target().clone(),
            evidence: crate::workload_saga::test_support::success_for(
                prerequisite_command.claim().attempt(),
            ),
        },
    )
    .expect("prerequisite success should correlate");
    let WorkloadProvisionDecision::Proposed(activation) = reduce_command_result(
        confirmed_prerequisite
            .confirmed_record()
            .expect("prerequisite candidate should remain durable"),
        prerequisite_command,
        prerequisite_result,
    )
    .expect("prerequisite success should reduce") else {
        panic!("prerequisite success should propose activation");
    };
    coordinator
        .confirm_provision_transition(
            confirmed_prerequisite
                .confirmed_record()
                .expect("prerequisite candidate should remain durable"),
            &activation,
        )
        .await
        .expect("activation transition should confirm")
        .command()
        .expect("activation confirmation should create a command")
        .clone()
}

fn adapter(root: &Path) -> ProviderProvisionPhaseAdapter {
    ProviderProvisionPhaseAdapter::new(
        ProviderCommandAttemptJournal::open(root, "test-container")
            .expect("fixture journal should open"),
    )
}

#[tokio::test]
async fn exact_replay_adopts_success_and_invokes_one_effect() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let adapter = adapter(root.path());
    let command = command("provider-execute", WorkloadSagaPhase::NetworkReserved).await;
    assert_eq!(command.mode(), WorkloadProvisionCommandMode::Execute);
    let effects = AtomicUsize::new(0);

    let first = adapter.execute(&command, || {
        effects.fetch_add(1, Ordering::AcqRel);
        ProviderProvisionEffectObservation::Succeeded {
            evidence: b"prepared container manifest".to_vec(),
        }
    });
    assert!(matches!(
        first,
        WorkloadProvisionInspectionResult::Succeeded { .. }
    ));
    let replay = adapter.execute(&command, || {
        panic!("exact provider replay must not invoke the effect")
    });
    assert_eq!(replay, first);
    assert_eq!(effects.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn inspection_without_owner_claim_observes_and_durably_adopts_absence() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let adapter = adapter(root.path());
    let command = command("provider-inspection", WorkloadSagaPhase::NetworkAttached).await;
    assert_eq!(command.mode(), WorkloadProvisionCommandMode::Inspect);
    let inspections = AtomicUsize::new(0);

    let first = adapter.inspect(&command, || {
        inspections.fetch_add(1, Ordering::AcqRel);
        ProviderProvisionEffectObservation::Absent {
            evidence: b"manifest and runtime absent".to_vec(),
        }
    });
    assert!(matches!(
        first,
        WorkloadProvisionInspectionResult::Absent { .. }
    ));
    let replay = adapter.inspect(&command, || {
        panic!("durable exact absence must be adopted without another inspection")
    });
    assert_eq!(replay, first);
    assert_eq!(inspections.load(Ordering::Acquire), 1);
}

#[tokio::test]
async fn adopted_claimed_provision_inspects_and_publishes_exact_absence() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let adapter = adapter(root.path());
    let command = command("provider-claimed", WorkloadSagaPhase::NetworkAttached).await;
    let claim = claim_for_command(&command).expect("confirmed command should produce one claim");
    let execution = match adapter
        .attempt_idempotency_journal
        .claim_dispatch_epoch(&claim)
        .expect("initial claim should succeed")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("first claim should grant exact execution authority")
        }
    };

    let observed = adapter.inspect(&command, || ProviderProvisionEffectObservation::Absent {
        evidence: b"recovery proves the claimed provision effect absent".to_vec(),
    });
    assert!(matches!(
        observed,
        WorkloadProvisionInspectionResult::Absent { .. }
    ));
    let mut effects = 0_u64;
    assert!(
        adapter
            .attempt_idempotency_journal
            .execute_current_claim(execution, |_| {
                effects += 1;
                (
                    (),
                    ProviderCommandObservationKind::Succeeded,
                    None,
                    b"delayed provision effect must not run".to_vec(),
                )
            })
            .is_err()
    );
    assert_eq!(effects, 0);
}

#[tokio::test]
async fn definite_failure_is_durable_and_invalid_provider_code_is_redacted() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let adapter = adapter(root.path());
    let command = command("provider-failure", WorkloadSagaPhase::NetworkReserved).await;

    let first = adapter.execute(&command, || {
        ProviderProvisionEffectObservation::DefiniteFailure {
            code: "NOT A PORTABLE CODE".to_owned(),
            evidence: b"redacted preparation failure".to_vec(),
        }
    });
    let WorkloadProvisionInspectionResult::DefiniteFailure { failure, .. } = &first else {
        panic!("definite provider failure should remain definite");
    };
    assert_eq!(failure.code(), "provider_definite_failure");
    let replay = adapter.execute(&command, || {
        panic!("failure replay must not invoke an effect")
    });
    assert_eq!(replay, first);
}

#[tokio::test]
async fn valid_provider_failure_code_is_normalized_identically_on_exact_replay() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let adapter = adapter(root.path());
    let command = command(
        "provider-stable-failure",
        WorkloadSagaPhase::NetworkReserved,
    )
    .await;

    let first = adapter.execute(&command, || {
        ProviderProvisionEffectObservation::DefiniteFailure {
            code: "provider_rejected_manifest".to_owned(),
            evidence: b"provider rejected the exact manifest".to_vec(),
        }
    });
    let WorkloadProvisionInspectionResult::DefiniteFailure { failure, .. } = &first else {
        panic!("definite provider failure should remain definite");
    };
    assert_eq!(failure.code(), "provider_definite_failure");

    let replay = adapter.execute(&command, || {
        panic!("exact failure replay must adopt the durable result")
    });
    assert_eq!(replay, first);
}

#[tokio::test]
async fn ambiguous_and_in_progress_inspection_never_gains_execute_authority() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let ambiguous_adapter = adapter(root.path());
    let ambiguous_command = command("provider-ambiguous", WorkloadSagaPhase::NetworkAttached).await;
    let ambiguous = ambiguous_adapter.inspect(&ambiguous_command, || {
        ProviderProvisionEffectObservation::Ambiguous {
            evidence: b"provider observation unavailable".to_vec(),
        }
    });
    assert!(matches!(
        ambiguous,
        WorkloadProvisionInspectionResult::Ambiguous { .. }
    ));
    let inspected = ambiguous_adapter.inspect(&ambiguous_command, || {
        ProviderProvisionEffectObservation::Absent {
            evidence: b"exact adopted ambiguity inspection proves absence".to_vec(),
        }
    });
    assert!(matches!(
        inspected,
        WorkloadProvisionInspectionResult::Absent { .. }
    ));
    let replay = ambiguous_adapter.inspect(&ambiguous_command, || {
        panic!("terminal inspected absence must replay without another provider read")
    });
    assert_eq!(replay, inspected);

    let second_root = tempfile::tempdir().expect("second temporary root should exist");
    let in_progress_adapter = adapter(second_root.path());
    let in_progress_command =
        command("provider-in-progress", WorkloadSagaPhase::NetworkAttached).await;
    let in_progress = in_progress_adapter.inspect(&in_progress_command, || {
        ProviderProvisionEffectObservation::InProgress {
            evidence: b"creator handoff pending".to_vec(),
        }
    });
    assert!(matches!(
        in_progress,
        WorkloadProvisionInspectionResult::InProgress { .. }
    ));
}
