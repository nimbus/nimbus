use std::path::Path;
use std::sync::Arc;

use nimbus_sandbox::backends::container::{ContainerSandboxBackend, ContainerSandboxBackendConfig};
use nimbus_sandbox::backends::krun::{KrunSandboxBackend, KrunSandboxBackendConfig, KrunStartMode};
use nimbus_sandbox::{
    ProviderCommandClaim, ProviderCommandClaimDecision, ProviderCommandClaimInput,
    ProviderCommandObservationKind, SandboxBackendKind,
};
use nimbus_workloads::{
    WorkloadExecutionProviderId, WorkloadOwnerEvidenceDigest, WorkloadSagaPhase,
    WorkloadSagaRecord, WorkloadTeardownProviderTarget,
};

use super::{
    ExactWorkloadTeardownCapability, WorkloadTeardownCapabilityRegistry,
    WorkloadTeardownExecuteOutcome, WorkloadTeardownInspectOutcome,
    WorkloadTeardownProviderOutcome, confirmed_teardown_commands, snapshot_files, stopped_intent,
    teardown_record_at,
};
use crate::workload_saga::{
    ContainerTeardownAdapter, KrunTeardownAdapter, WorkloadExecutionDrainCapability,
    WorkloadExecutionStopCapability, sandbox_execution_provider_id,
    validate_sandbox_teardown_command,
};

fn observed_krun_record(rootfs: &Path) -> WorkloadSagaRecord {
    let mut record = crate::workload_saga::provision_sandbox::tests::composed_record_with_rootfs(
        SandboxBackendKind::Krun,
        rootfs,
    );
    for _ in 0..8 {
        if record.phase() == WorkloadSagaPhase::Observed {
            return record;
        }
        record = crate::workload_saga::test_support::confirmed_provision(&record);
    }
    panic!("Krun fixture did not reach Observed");
}

fn krun_teardown_record_at(
    observed: &WorkloadSagaRecord,
    target: WorkloadSagaPhase,
) -> WorkloadSagaRecord {
    let successor_generation = observed.active_intent().generation().as_u64() + 1;
    let teardown = super::begin_teardown(observed, stopped_intent("krun", successor_generation));
    super::finish_teardown(teardown, target)
}

fn krun_config(root: &Path) -> KrunSandboxBackendConfig {
    let mut config = KrunSandboxBackendConfig::under_root(root);
    config.start_mode = KrunStartMode::Execute;
    config
}

fn assert_execute_failure_code(outcome: WorkloadTeardownProviderOutcome, expected: &str) {
    let WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::DefiniteFailure(
        failure,
    )) = outcome
    else {
        panic!("expected a definite Execute failure");
    };
    assert_eq!(failure.code(), expected);
}

fn mutate_claim(
    source: &ProviderCommandClaim,
    mutate: impl FnOnce(&mut ProviderCommandClaimInput),
) -> ProviderCommandClaim {
    let mut input = ProviderCommandClaimInput {
        authority_id: source.authority_id().to_owned(),
        effect_subject: source.effect_subject().to_owned(),
        source_attempt_id: source.source_attempt_id().map(str::to_owned),
        attempt_id: source.attempt_id().to_owned(),
        dispatch_epoch: source.dispatch_epoch(),
        workload_generation: source.workload_generation(),
        restart_ordinal: source.restart_ordinal(),
        desired_digest: source.desired_digest().to_owned(),
        source_digest: source.source_digest().to_owned(),
        network_plan_digest: source.network_plan_digest().to_owned(),
        provider_target_digest: source.provider_target_digest().to_owned(),
        operation: source.operation(),
    };
    mutate(&mut input);
    ProviderCommandClaim::new(input).expect("mutated claim should remain structurally valid")
}

#[test]
fn krun_execution_provider_identity_is_exact() {
    assert_eq!(
        sandbox_execution_provider_id(SandboxBackendKind::Krun),
        WorkloadExecutionProviderId::for_registration_key("nimbus-sandbox.krun-execution"),
    );
}

#[tokio::test]
async fn krun_capabilities_substitute_drain_and_stop_without_registry_changes() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let observed = observed_krun_record(root.path());
    let adapter = Arc::new(
        KrunTeardownAdapter::new(Arc::new(KrunSandboxBackend::new(krun_config(root.path()))))
            .expect("Krun teardown adapter should open its provider journal"),
    );
    let container = Arc::new(
        ContainerTeardownAdapter::new(Arc::new(ContainerSandboxBackend::new(
            ContainerSandboxBackendConfig::under_root(root.path().join("container")),
        )))
        .expect("Container teardown adapter should open its provider journal"),
    );
    let registry = WorkloadTeardownCapabilityRegistry::new(
        [],
        [container.capabilities(), adapter.capabilities()],
        [],
    )
    .expect("real Container and Krun providers should register both execution roles");

    let (drain, _) = confirmed_teardown_commands(krun_teardown_record_at(
        &observed,
        WorkloadSagaPhase::Withdrawn,
    ))
    .await;
    assert!(matches!(
        drain.provider_target(),
        WorkloadTeardownProviderTarget::Execution { provider_id, .. }
            if provider_id == &sandbox_execution_provider_id(SandboxBackendKind::Krun)
    ));
    assert!(matches!(
        registry
            .select_exact(&drain)
            .expect("Krun drain should select exactly"),
        ExactWorkloadTeardownCapability::ExecutionDrain(_)
    ));

    let (stop, _) = confirmed_teardown_commands(krun_teardown_record_at(
        &observed,
        WorkloadSagaPhase::Drained,
    ))
    .await;
    assert!(matches!(
        registry
            .select_exact(&stop)
            .expect("Krun stop should select exactly"),
        ExactWorkloadTeardownCapability::ExecutionStop(_)
    ));

    let observed_container = super::observed_container_record(root.path());
    let (container_drain, _) = confirmed_teardown_commands(teardown_record_at(
        &observed_container,
        WorkloadSagaPhase::Withdrawn,
    ))
    .await;
    assert!(matches!(
        registry
            .select_exact(&container_drain)
            .expect("Container drain should select exactly"),
        ExactWorkloadTeardownCapability::ExecutionDrain(_)
    ));
    let (container_stop, _) = confirmed_teardown_commands(teardown_record_at(
        &observed_container,
        WorkloadSagaPhase::Drained,
    ))
    .await;
    assert!(matches!(
        registry
            .select_exact(&container_stop)
            .expect("Container stop should select exactly"),
        ExactWorkloadTeardownCapability::ExecutionStop(_)
    ));
}

#[tokio::test]
async fn krun_execute_and_inspect_reuse_exact_journal_across_backend_reopen() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let observed = observed_krun_record(root.path());
    let config = krun_config(root.path());

    for target in [WorkloadSagaPhase::Withdrawn, WorkloadSagaPhase::Drained] {
        let (execute, inspect) =
            confirmed_teardown_commands(krun_teardown_record_at(&observed, target)).await;
        let validated = validate_sandbox_teardown_command(&execute, SandboxBackendKind::Krun)
            .expect("exact Krun teardown command should lower");
        let backend = Arc::new(KrunSandboxBackend::new(config.clone()));
        let adapter = KrunTeardownAdapter::new(Arc::clone(&backend))
            .expect("Krun teardown adapter should open its provider journal");
        let first = match target {
            WorkloadSagaPhase::Withdrawn => {
                WorkloadExecutionDrainCapability::execute(&adapter, &execute).await
            }
            WorkloadSagaPhase::Drained => {
                WorkloadExecutionStopCapability::execute(&adapter, &execute).await
            }
            _ => unreachable!("fixture target is drain or stop"),
        };
        assert!(first.matches_command(&execute));
        assert!(matches!(
            first.clone().into_outcome(),
            WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Ambiguous)
        ));
        let journal = backend
            .attempt_idempotency_journal()
            .expect("Krun provider journal should reopen");
        let durable = journal
            .adopt_exact_attempt(validated.sandbox_command().provider_claim())
            .expect("exact Krun claim should read")
            .expect("the adapter should publish one durable provider result");
        assert_eq!(
            durable.claim(),
            validated.sandbox_command().provider_claim(),
            "the real backend callback must use the exact lowered execution claim"
        );
        let durable_before_reopen = snapshot_files(&config.workload_state_root);

        let reopened = KrunTeardownAdapter::new(Arc::new(KrunSandboxBackend::new(config.clone())))
            .expect("fresh Krun adapter should reopen the same journal");
        let replay = match target {
            WorkloadSagaPhase::Withdrawn => {
                WorkloadExecutionDrainCapability::execute(&reopened, &execute).await
            }
            WorkloadSagaPhase::Drained => {
                WorkloadExecutionStopCapability::execute(&reopened, &execute).await
            }
            _ => unreachable!("fixture target is drain or stop"),
        };
        assert_eq!(replay, first, "exact replay must adopt the durable result");
        let inspected = match target {
            WorkloadSagaPhase::Withdrawn => {
                WorkloadExecutionDrainCapability::inspect(&reopened, &inspect).await
            }
            WorkloadSagaPhase::Drained => {
                WorkloadExecutionStopCapability::inspect(&reopened, &inspect).await
            }
            _ => unreachable!("fixture target is drain or stop"),
        };
        assert!(inspected.matches_command(&inspect));
        assert!(matches!(
            inspected.into_outcome(),
            WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::Ambiguous)
        ));
        assert_eq!(
            snapshot_files(&config.workload_state_root),
            durable_before_reopen,
            "replay and missing-state inspection must keep provider bytes stable"
        );
    }
}

#[tokio::test]
async fn krun_adapter_fences_crossed_provider_digest_and_stale_generation_before_effect() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let adapter =
        KrunTeardownAdapter::new(Arc::new(KrunSandboxBackend::new(krun_config(root.path()))))
            .expect("Krun teardown adapter should open its provider journal");
    let observed_container = super::observed_container_record(root.path());
    let (container_command, _) = confirmed_teardown_commands(teardown_record_at(
        &observed_container,
        WorkloadSagaPhase::Withdrawn,
    ))
    .await;
    let before_provider_cross = snapshot_files(root.path());
    assert_execute_failure_code(
        WorkloadExecutionDrainCapability::execute(&adapter, &container_command)
            .await
            .into_outcome(),
        "sandbox_teardown_command_crossed",
    );
    assert_eq!(snapshot_files(root.path()), before_provider_cross);

    let observed_krun = observed_krun_record(root.path());
    let (command, _) = confirmed_teardown_commands(krun_teardown_record_at(
        &observed_krun,
        WorkloadSagaPhase::Withdrawn,
    ))
    .await;
    let validated = validate_sandbox_teardown_command(&command, SandboxBackendKind::Krun)
        .expect("exact Krun teardown command should lower");
    let sandbox_command = validated.sandbox_command();
    let claim = sandbox_command.provider_claim();
    assert_eq!(sandbox_command.tenant_id(), command.key().tenant_id());
    assert_eq!(
        sandbox_command.sandbox_id().as_str(),
        command.execution_locator().execution_id().as_str()
    );
    assert_eq!(
        sandbox_command.execution_attempt_id().as_str(),
        command.execution_locator().attempt_id().as_str()
    );
    assert_eq!(
        sandbox_command.provider_registration_key(),
        "nimbus-sandbox.krun-execution"
    );
    assert_eq!(claim.authority_id(), command.saga_id().as_str());
    assert_eq!(
        claim.effect_subject(),
        serde_json::to_string(&(command.execution_locator(), command.subjects()))
            .expect("exact execution subject should serialize")
    );
    assert!(
        claim
            .effect_subject()
            .contains(command.required_node().as_str()),
        "the exact node fence must be part of the provider effect subject"
    );
    assert_eq!(claim.attempt_id(), command.attempt_id().as_str());
    assert_eq!(claim.dispatch_epoch(), command.dispatch_epoch().as_u64());
    assert_eq!(claim.workload_generation(), command.generation().as_u64());
    assert_eq!(claim.desired_digest(), command.desired_digest().to_string());
    assert_eq!(claim.source_digest(), command.source_digest().to_string());
    assert_eq!(
        claim.network_plan_digest(),
        command.network_plan_digest().to_string()
    );
    assert_eq!(
        claim.provider_target_digest(),
        WorkloadOwnerEvidenceDigest::sha256(
            serde_json::to_vec(command.provider_target())
                .expect("exact provider target should serialize")
        )
        .to_string()
    );
    assert_eq!(
        claim.operation(),
        sandbox_command.operation().provider_operation()
    );

    for (label, current_claim, expected_code) in [
        (
            "crossed tenant sandbox execution or node subject",
            mutate_claim(claim, |input| {
                input.effect_subject = "{\"execution\":\"crossed\"}".to_owned();
            }),
            "sandbox_teardown_epoch_invalid",
        ),
        (
            "crossed provider attempt",
            mutate_claim(claim, |input| {
                input.attempt_id = "crossed-provider-attempt".to_owned();
            }),
            "sandbox_teardown_epoch_invalid",
        ),
        (
            "crossed desired digest",
            mutate_claim(claim, |input| input.desired_digest = "4".repeat(64)),
            "sandbox_teardown_epoch_invalid",
        ),
        (
            "crossed source digest",
            mutate_claim(claim, |input| input.source_digest = "5".repeat(64)),
            "sandbox_teardown_epoch_invalid",
        ),
        (
            "crossed network plan digest",
            mutate_claim(claim, |input| input.network_plan_digest = "6".repeat(64)),
            "sandbox_teardown_epoch_invalid",
        ),
        (
            "crossed provider target digest",
            mutate_claim(claim, |input| input.provider_target_digest = "7".repeat(64)),
            "sandbox_teardown_epoch_invalid",
        ),
        (
            "stale workload generation",
            mutate_claim(claim, |input| input.workload_generation += 1),
            "sandbox_teardown_command_stale",
        ),
        (
            "stale dispatch epoch",
            mutate_claim(claim, |input| input.dispatch_epoch += 1),
            "sandbox_teardown_command_stale",
        ),
    ] {
        let case_root = tempfile::tempdir().expect("case root should exist");
        let backend = Arc::new(KrunSandboxBackend::new(krun_config(case_root.path())));
        let journal = backend
            .attempt_idempotency_journal()
            .expect("Krun provider journal should open");
        assert!(matches!(
            journal
                .claim_dispatch_epoch(&current_claim)
                .expect("conflicting durable claim should publish"),
            ProviderCommandClaimDecision::ExecuteClaimed(_)
        ));
        let adapter = KrunTeardownAdapter::new(backend)
            .expect("Krun teardown adapter should reuse the provider journal");
        let before = snapshot_files(case_root.path());
        assert_execute_failure_code(
            WorkloadExecutionDrainCapability::execute(&adapter, &command)
                .await
                .into_outcome(),
            expected_code,
        );
        assert_eq!(
            snapshot_files(case_root.path()),
            before,
            "{label} must fail before provider mutation"
        );
    }
}

#[tokio::test]
async fn krun_inspect_maps_claim_progress_absence_and_exact_failure_code() {
    let observed = observed_krun_record(Path::new("/tmp/nnc65d2-krun-inspect-map"));
    let (_, inspect) = confirmed_teardown_commands(krun_teardown_record_at(
        &observed,
        WorkloadSagaPhase::Drained,
    ))
    .await;
    let validated = validate_sandbox_teardown_command(&inspect, SandboxBackendKind::Krun)
        .expect("exact Krun inspection command should lower");

    for (kind, failure_code) in [
        (ProviderCommandObservationKind::Claimed, None),
        (ProviderCommandObservationKind::InProgress, None),
        (ProviderCommandObservationKind::Absent, None),
        (
            ProviderCommandObservationKind::DefiniteFailure,
            Some("krun_teardown_process_identity_crossed"),
        ),
    ] {
        let root = tempfile::tempdir().expect("journal root should exist");
        let journal =
            nimbus_sandbox::ProviderCommandAttemptJournal::open(root.path(), "krun-runtime")
                .expect("Krun provider journal should open");
        assert!(matches!(
            journal
                .claim_dispatch_epoch(validated.sandbox_command().provider_claim())
                .expect("exact inspection attempt should claim"),
            ProviderCommandClaimDecision::ExecuteClaimed(_)
        ));
        if kind != ProviderCommandObservationKind::Claimed {
            journal
                .record_observation_with_failure_code(
                    validated.sandbox_command().provider_claim(),
                    kind,
                    failure_code,
                    b"exact Krun inspection evidence",
                )
                .expect("inspection observation should publish");
        }
        let durable = journal
            .adopt_exact_attempt(validated.sandbox_command().provider_claim())
            .expect("inspection observation should read")
            .expect("inspection observation should exist");
        let outcome = super::provider_outcome(&inspect, &durable);
        match kind {
            ProviderCommandObservationKind::Claimed
            | ProviderCommandObservationKind::InProgress => assert!(matches!(
                outcome,
                WorkloadTeardownProviderOutcome::Inspect(
                    WorkloadTeardownInspectOutcome::InProgress(_)
                )
            )),
            ProviderCommandObservationKind::Absent => assert!(matches!(
                outcome,
                WorkloadTeardownProviderOutcome::Inspect(
                    WorkloadTeardownInspectOutcome::NotCompleted(_)
                )
            )),
            ProviderCommandObservationKind::DefiniteFailure => {
                let WorkloadTeardownProviderOutcome::Inspect(
                    WorkloadTeardownInspectOutcome::DefiniteFailure(failure),
                ) = outcome
                else {
                    panic!("durable Krun failure must remain definite");
                };
                assert_eq!(failure.code(), failure_code.expect("case carries a code"));
            }
            _ => unreachable!("fixture covers exact inspection mapping cases"),
        }
    }
}
