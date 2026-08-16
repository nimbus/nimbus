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
async fn process_bound_observation_success_rechecks_to_exact_owner_absence() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let adapter = adapter(root.path());
    let command = command(
        "provider-owner-reopened-observation",
        WorkloadSagaPhase::Published,
    )
    .await;
    assert_eq!(command.step(), WorkloadProvisionStep::ObservePublication);
    assert_eq!(command.mode(), WorkloadProvisionCommandMode::Inspect);

    let first_owner =
        adapter.inspect_live(&command, || ProviderProvisionEffectObservation::Succeeded {
            evidence: b"first owner observed its process-bound publication".to_vec(),
        });
    assert!(matches!(
        first_owner,
        WorkloadProvisionInspectionResult::Succeeded { .. }
    ));

    let fresh_owner =
        adapter.inspect_live(&command, || ProviderProvisionEffectObservation::Absent {
            evidence: b"fresh owner proved the process-bound publication absent".to_vec(),
        });
    assert!(matches!(
        fresh_owner,
        WorkloadProvisionInspectionResult::Absent { .. }
    ));
    assert_eq!(
        adapter.inspect_live(&command, || {
            panic!("durable fresh-owner absence must replay without another provider read")
        }),
        fresh_owner
    );
}

#[tokio::test]
async fn restarted_process_bound_observation_rotates_only_for_owner_reopen() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let adapter = adapter(root.path());
    let label = "provider-owner-reopened-restarted-observation";
    let initial_attachment = command(label, WorkloadSagaPhase::WorkloadPrepared).await;
    assert_eq!(
        initial_attachment.step(),
        WorkloadProvisionStep::AttachNetwork
    );
    let initial_attachment_claim =
        claim_for_command(&initial_attachment).expect("initial attachment claim should derive");
    assert!(matches!(
        adapter.execute(&initial_attachment, || {
            ProviderProvisionEffectObservation::Succeeded {
                evidence: b"initial owner attached private network and PEP".to_vec(),
            }
        }),
        WorkloadProvisionInspectionResult::Succeeded { .. }
    ));
    let initial_publish = command(label, WorkloadSagaPhase::Ready).await;
    assert_eq!(initial_publish.step(), WorkloadProvisionStep::Publish);
    assert!(matches!(
        adapter.execute(&initial_publish, || {
            ProviderProvisionEffectObservation::Succeeded {
                evidence: b"initial owner published its process-bound listener".to_vec(),
            }
        }),
        WorkloadProvisionInspectionResult::Succeeded { .. }
    ));
    let initial = command(label, WorkloadSagaPhase::Published).await;
    let initial_claim = claim_for_command(&initial).expect("initial claim should derive");
    assert!(matches!(
        adapter.inspect_live(&initial, || ProviderProvisionEffectObservation::Succeeded {
            evidence: b"initial owner observed its process-bound publication".to_vec(),
        }),
        WorkloadProvisionInspectionResult::Succeeded { .. }
    ));

    let completed = crate::workload_saga::test_support::completed_published_restart_record(label);
    let reopened = completed
        .reopen_observed_publication_for_owner_recovery()
        .expect("completed restart publication should reopen for exact owner inspection");
    let coordinator = WorkloadSagaCoordinator::new(AppliedStore::new(Some(reopened.clone())));
    let confirmed = coordinator
        .inspect_confirmed_provision(reopened.key())
        .await
        .expect("owner-reopened inspection should confirm");
    let attachment_record = confirmed
        .confirmed_record()
        .expect("owner-reopened attachment inspection should remain durable")
        .clone();
    let attachment = confirmed
        .command()
        .expect("owner-reopened attachment inspection should issue one command");
    assert_eq!(attachment.step(), WorkloadProvisionStep::AttachNetwork);
    assert!(matches!(
        attachment.claim().authorization(),
        WorkloadProvisionDispatchAuthorization::OwnerReopenedAttachmentInspection
    ));
    let attachment_claim =
        claim_for_command(attachment).expect("owner-reopened attachment claim should derive");
    assert_ne!(
        attachment_claim.attempt_id(),
        initial_attachment_claim.attempt_id()
    );
    let attachment_absent =
        adapter.inspect(attachment, || ProviderProvisionEffectObservation::Absent {
            evidence: b"fresh owner proved the process-bound PEP absent".to_vec(),
        });
    assert!(matches!(
        attachment_absent,
        WorkloadProvisionInspectionResult::Absent { ref evidence }
            if evidence.origin()
                == WorkloadProvisionAbsenceOrigin::OwnerReopenedAttachmentInspection
    ));
    let attachment_absent =
        WorkloadProvisionCommandResult::for_command(attachment, attachment_absent)
            .expect("owner-reopened attachment absence should correlate");
    let WorkloadProvisionDecision::Proposed(reattach) =
        reduce_command_result(&attachment_record, attachment, attachment_absent)
            .expect("owner-reopened PEP absence should authorize one attachment repair")
    else {
        panic!("owner-reopened PEP absence should propose attachment repair");
    };
    let confirmed_reattach = coordinator
        .confirm_provision_transition(&attachment_record, &reattach)
        .await
        .expect("owner-reopened attachment repair should confirm");
    let reattach_record = confirmed_reattach
        .confirmed_record()
        .expect("owner-reopened attachment repair should remain durable")
        .clone();
    let reattach_command = confirmed_reattach
        .command()
        .expect("owner-reopened attachment repair should issue one command");
    assert_eq!(
        reattach_command.mode(),
        WorkloadProvisionCommandMode::Execute
    );
    assert_eq!(reattach_command.dispatch_epoch().as_u64(), 1);
    let attachment_effects = AtomicUsize::new(0);
    let reattached = adapter.execute(reattach_command, || {
        attachment_effects.fetch_add(1, Ordering::AcqRel);
        ProviderProvisionEffectObservation::Succeeded {
            evidence: b"fresh owner repaired the process-bound PEP".to_vec(),
        }
    });
    assert!(matches!(
        reattached,
        WorkloadProvisionInspectionResult::Succeeded { .. }
    ));
    assert_eq!(attachment_effects.load(Ordering::Acquire), 1);
    assert_eq!(
        adapter.execute(reattach_command, || {
            panic!("owner-reopened attachment repair replay must not bind twice")
        }),
        reattached
    );
    let reattached = WorkloadProvisionCommandResult::for_command(reattach_command, reattached)
        .expect("owner-reopened attachment repair should correlate");
    let WorkloadProvisionDecision::Proposed(inspect_publication) =
        reduce_command_result(&reattach_record, reattach_command, reattached)
            .expect("attachment repair should schedule publication inspection")
    else {
        panic!("attachment repair should propose publication inspection");
    };
    let confirmed_publication = coordinator
        .confirm_provision_transition(&reattach_record, &inspect_publication)
        .await
        .expect("owner-reopened publication inspection should confirm");
    let current_record = confirmed_publication
        .confirmed_record()
        .expect("owner-reopened publication inspection should remain durable")
        .clone();
    let current = confirmed_publication
        .command()
        .expect("owner-reopened publication inspection should issue one command");
    let WorkloadProvisionSubjects::Publication(current_publication) = current.subjects() else {
        panic!("owner-reopened publication inspection should retain one publication subject");
    };
    assert_eq!(
        current_publication.execution(),
        &completed.current_execution_reference(),
        "the durable publication subject should name the completed restart execution"
    );
    assert_eq!(
        current.execution(),
        current_publication.execution(),
        "provider validation and the journal subject must authenticate the same execution"
    );
    let current_claim = claim_for_command(current).expect("current claim should derive");
    assert_ne!(
        initial_claim.effect_subject(),
        current_claim.effect_subject()
    );
    assert_ne!(initial_claim.attempt_id(), current_claim.attempt_id());
    assert_eq!(current_claim.attempt_id(), current.attempt_id().as_str());
    assert!(matches!(
        current.claim().authorization(),
        WorkloadProvisionDispatchAuthorization::OwnerReopenedPublicationInspection
    ));

    let absent = adapter.inspect_live(current, || ProviderProvisionEffectObservation::Absent {
        evidence: b"fresh owner proved the restarted publication absent".to_vec(),
    });
    assert!(matches!(
        absent,
        WorkloadProvisionInspectionResult::Absent { ref evidence }
            if evidence.origin()
                == WorkloadProvisionAbsenceOrigin::OwnerReopenedPublicationInspection
    ));
    let absent = WorkloadProvisionCommandResult::for_command(current, absent)
        .expect("owner-reopened absence should correlate");
    let WorkloadProvisionDecision::Proposed(republish) =
        reduce_command_result(&current_record, current, absent)
            .expect("owner-reopened absence should authorize republication")
    else {
        panic!("owner-reopened absence should propose republication");
    };
    let confirmed_republish = coordinator
        .confirm_provision_transition(&current_record, &republish)
        .await
        .expect("owner-reopened republication should confirm");
    let republish_record = confirmed_republish
        .confirmed_record()
        .expect("owner-reopened republication should remain durable")
        .clone();
    let republish_command = confirmed_republish
        .command()
        .expect("owner-reopened republication should issue one command")
        .clone();
    let republish_claim =
        claim_for_command(&republish_command).expect("republication claim should derive");
    assert_eq!(republish_claim.attempt_id(), current_claim.attempt_id());
    assert_eq!(republish_claim.dispatch_epoch(), 1);
    let publication_effects = AtomicUsize::new(0);
    let republished = adapter.execute(&republish_command, || {
        publication_effects.fetch_add(1, Ordering::AcqRel);
        ProviderProvisionEffectObservation::Succeeded {
            evidence: b"fresh owner rebound the restarted publication".to_vec(),
        }
    });
    assert!(matches!(
        republished,
        WorkloadProvisionInspectionResult::Succeeded { .. }
    ));
    assert_eq!(publication_effects.load(Ordering::Acquire), 1);
    assert_eq!(
        adapter.execute(&republish_command, || {
            panic!("owner-reopened republication replay must not bind twice")
        }),
        republished
    );

    let republished = WorkloadProvisionCommandResult::for_command(&republish_command, republished)
        .expect("owner-reopened republication should correlate");
    let WorkloadProvisionDecision::Proposed(reobserve) =
        reduce_command_result(&republish_record, &republish_command, republished)
            .expect("owner-reopened republication should schedule re-observation")
    else {
        panic!("owner-reopened republication should propose re-observation");
    };
    let confirmed_reobserve = coordinator
        .confirm_provision_transition(&republish_record, &reobserve)
        .await
        .expect("owner-reopened re-observation should confirm");
    let reobserve_record = confirmed_reobserve
        .confirmed_record()
        .expect("owner-reopened re-observation should remain durable")
        .clone();
    let reobserve_command = confirmed_reobserve
        .command()
        .expect("owner-reopened re-observation should issue one command");
    let reobserve_claim =
        claim_for_command(reobserve_command).expect("re-observation claim should derive");
    assert_eq!(reobserve_claim.attempt_id(), current_claim.attempt_id());
    assert_eq!(reobserve_claim.dispatch_epoch(), 1);
    let reobserved = adapter.inspect_live(reobserve_command, || {
        ProviderProvisionEffectObservation::Succeeded {
            evidence: b"fresh owner observed the rebound publication".to_vec(),
        }
    });
    assert!(matches!(
        reobserved,
        WorkloadProvisionInspectionResult::Succeeded { .. }
    ));
    let reobserved = WorkloadProvisionCommandResult::for_command(reobserve_command, reobserved)
        .expect("owner-reopened re-observation should correlate");
    let WorkloadProvisionDecision::Proposed(observed) =
        reduce_command_result(&reobserve_record, reobserve_command, reobserved)
            .expect("owner-reopened re-observation should converge")
    else {
        panic!("owner-reopened re-observation should propose Observed truth");
    };
    let observed = coordinator
        .confirm_provision_transition(&reobserve_record, &observed)
        .await
        .expect("owner-reopened Observed truth should confirm")
        .confirmed_record()
        .expect("owner-reopened Observed truth should remain durable")
        .clone();
    assert_eq!(observed.phase(), WorkloadSagaPhase::Observed);

    let reopened_again = observed
        .reopen_observed_publication_for_owner_recovery()
        .expect("a later owner should reopen the same execution through a new claim");
    let next_coordinator =
        WorkloadSagaCoordinator::new(AppliedStore::new(Some(reopened_again.clone())));
    let next_confirmed = next_coordinator
        .inspect_confirmed_provision(reopened_again.key())
        .await
        .expect("later owner inspection should confirm");
    let next_command = next_confirmed
        .command()
        .expect("later owner attachment inspection should issue one command");
    assert_eq!(next_command.step(), WorkloadProvisionStep::AttachNetwork);
    let next_claim = claim_for_command(next_command).expect("later owner claim should derive");
    assert_ne!(next_claim.attempt_id(), current_claim.attempt_id());
    assert!(matches!(
        adapter.inspect(next_command, || {
            ProviderProvisionEffectObservation::Absent {
                evidence: b"later owner proved its process-bound PEP absent".to_vec(),
            }
        }),
        WorkloadProvisionInspectionResult::Absent { ref evidence }
            if evidence.origin()
                == WorkloadProvisionAbsenceOrigin::OwnerReopenedAttachmentInspection
    ));
}

#[tokio::test]
async fn publication_absence_republishes_once_on_stable_provider_streams() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let adapter = adapter(root.path());
    let ready = provision_record(
        "provider-publication-retry",
        WorkloadSagaPhase::Ready,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let coordinator = WorkloadSagaCoordinator::new(AppliedStore::new(Some(ready.clone())));

    let WorkloadProvisionDecision::Proposed(publish) =
        WorkloadProvisionDecision::plan(&ready).expect("ready phase should propose publication")
    else {
        panic!("ready phase should propose one publication command");
    };
    let confirmed_publish = coordinator
        .confirm_provision_transition(&ready, &publish)
        .await
        .expect("publication claim should confirm");
    let publish_record = confirmed_publish
        .confirmed_record()
        .expect("publication claim should remain durable")
        .clone();
    let publish_command = confirmed_publish
        .command()
        .expect("publication claim should issue one command")
        .clone();
    let publish_claim = claim_for_command(&publish_command)
        .expect("publication command should produce a provider claim");
    assert_ne!(
        publish_claim.attempt_id(),
        publish_command.attempt_id().as_str(),
        "process-bound publication must use stable execution identity"
    );
    assert_eq!(
        publish_claim.attempt_id(),
        provider_attempt_id(&publish_command)
    );

    let publish_effects = AtomicUsize::new(0);
    let published = adapter.execute(&publish_command, || {
        publish_effects.fetch_add(1, Ordering::AcqRel);
        ProviderProvisionEffectObservation::Succeeded {
            evidence: b"initial publication bound".to_vec(),
        }
    });
    let published = WorkloadProvisionCommandResult::for_command(&publish_command, published)
        .expect("initial publication result should correlate");
    let WorkloadProvisionDecision::Proposed(published) =
        reduce_command_result(&publish_record, &publish_command, published)
            .expect("initial publication should reduce")
    else {
        panic!("initial publication should propose durable Published truth");
    };
    let published = coordinator
        .confirm_provision_transition(&publish_record, &published)
        .await
        .expect("Published truth should confirm")
        .confirmed_record()
        .expect("Published truth should remain durable")
        .clone();

    let WorkloadProvisionDecision::Proposed(observe) = WorkloadProvisionDecision::plan(&published)
        .expect("Published truth should propose observation")
    else {
        panic!("Published truth should propose one observation command");
    };
    let confirmed_observe = coordinator
        .confirm_provision_transition(&published, &observe)
        .await
        .expect("publication observation should confirm");
    let observe_record = confirmed_observe
        .confirmed_record()
        .expect("publication observation should remain durable")
        .clone();
    let observe_command = confirmed_observe
        .command()
        .expect("publication observation should issue one command")
        .clone();
    let observe_claim = claim_for_command(&observe_command)
        .expect("observation command should produce a provider claim");
    assert_eq!(
        publish_claim.attempt_id(),
        observe_claim.attempt_id(),
        "publication and observation streams must share stable execution identity"
    );

    let absent = adapter.inspect_live(&observe_command, || {
        ProviderProvisionEffectObservation::Absent {
            evidence: b"fresh owner proved publication absent".to_vec(),
        }
    });
    let absent = WorkloadProvisionCommandResult::for_command(&observe_command, absent)
        .expect("publication absence should correlate");
    let WorkloadProvisionDecision::Proposed(republish) =
        reduce_command_result(&observe_record, &observe_command, absent)
            .expect("publication absence should authorize exact republication")
    else {
        panic!("publication absence should propose one republication");
    };
    let confirmed_republish = coordinator
        .confirm_provision_transition(&observe_record, &republish)
        .await
        .expect("republication should confirm");
    let republish_record = confirmed_republish
        .confirmed_record()
        .expect("republication should remain durable")
        .clone();
    let republish_command = confirmed_republish
        .command()
        .expect("republication should issue one command")
        .clone();
    let republish_claim = claim_for_command(&republish_command)
        .expect("republication should produce a provider claim");
    assert_eq!(publish_claim.attempt_id(), republish_claim.attempt_id());
    assert_eq!(publish_claim.dispatch_epoch(), 0);
    assert_eq!(republish_claim.dispatch_epoch(), 1);

    let republished = adapter.execute(&republish_command, || {
        publish_effects.fetch_add(1, Ordering::AcqRel);
        ProviderProvisionEffectObservation::Succeeded {
            evidence: b"fresh owner rebound exact publication".to_vec(),
        }
    });
    assert!(matches!(
        republished,
        WorkloadProvisionInspectionResult::Succeeded { .. }
    ));
    assert_eq!(publish_effects.load(Ordering::Acquire), 2);
    assert_eq!(
        adapter.execute(&republish_command, || {
            panic!("exact republication replay must not bind again")
        }),
        republished
    );

    let republished = WorkloadProvisionCommandResult::for_command(&republish_command, republished)
        .expect("republication result should correlate");
    let WorkloadProvisionDecision::Proposed(reobserve) =
        reduce_command_result(&republish_record, &republish_command, republished)
            .expect("republication success should schedule exact re-observation")
    else {
        panic!("republication success should propose re-observation");
    };
    let confirmed_reobserve = coordinator
        .confirm_provision_transition(&republish_record, &reobserve)
        .await
        .expect("re-observation should confirm");
    let reobserve_record = confirmed_reobserve
        .confirmed_record()
        .expect("re-observation should remain durable")
        .clone();
    let reobserve_command = confirmed_reobserve
        .command()
        .expect("re-observation should issue one command")
        .clone();
    let reobserve_claim = claim_for_command(&reobserve_command)
        .expect("re-observation should produce a provider claim");
    assert_eq!(observe_claim.attempt_id(), reobserve_claim.attempt_id());
    assert_eq!(observe_claim.dispatch_epoch(), 0);
    assert_eq!(reobserve_claim.dispatch_epoch(), 1);

    let observations = AtomicUsize::new(0);
    let observed = adapter.inspect_live(&reobserve_command, || {
        observations.fetch_add(1, Ordering::AcqRel);
        ProviderProvisionEffectObservation::Succeeded {
            evidence: b"republication observed".to_vec(),
        }
    });
    assert!(matches!(
        observed,
        WorkloadProvisionInspectionResult::Succeeded { .. }
    ));
    assert_eq!(
        adapter.inspect_live(&reobserve_command, || {
            observations.fetch_add(1, Ordering::AcqRel);
            ProviderProvisionEffectObservation::Succeeded {
                evidence: b"republication observed".to_vec(),
            }
        }),
        observed
    );
    assert_eq!(
        observations.load(Ordering::Acquire),
        2,
        "process-bound observation replay must recheck live state without repeating publication"
    );

    let observed = WorkloadProvisionCommandResult::for_command(&reobserve_command, observed)
        .expect("re-observation result should correlate");
    let WorkloadProvisionDecision::Proposed(observed) =
        reduce_command_result(&reobserve_record, &reobserve_command, observed)
            .expect("re-observation success should converge")
    else {
        panic!("re-observation success should propose Observed truth");
    };
    let observed = coordinator
        .confirm_provision_transition(&reobserve_record, &observed)
        .await
        .expect("Observed truth should confirm")
        .confirmed_record()
        .expect("Observed truth should remain durable")
        .clone();
    assert_eq!(observed.phase(), WorkloadSagaPhase::Observed);
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
