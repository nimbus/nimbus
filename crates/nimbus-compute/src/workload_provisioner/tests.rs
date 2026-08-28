use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::{
    NetworkAddressFamily, NetworkAttachmentProviderRegistration, NetworkBindRealmKind,
    NetworkCapabilityBundle, NetworkControlPlaneLocality, NetworkEndpointCapabilitySet,
    NetworkExposure, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet, NetworkLifecycleFeature,
    NetworkPortAssignmentMode, NetworkProviderId, NetworkSovereigntyCapabilities, PortProtocol,
};
use nimbus_sandbox::{
    SandboxBackendKind, SandboxOwnerSpec, SandboxProcessSpec, SandboxRootSpec,
    sandbox_network_plan_requirements,
};
use nimbus_tenant::{
    TenantIsolationContext, TenantIsolationPolicyInput, TenantServiceGrantPolicyDecision,
    WorkloadAttributes, WorkloadLocation,
};
use nimbus_workloads::{
    WorkloadOwnerEvidenceDigest, WorkloadProvisionCommandMode, WorkloadProvisionInspectionResult,
    WorkloadProvisionSourceEvidence, WorkloadProvisionSourceIdentity, WorkloadSagaCommit,
    WorkloadSagaExpected, WorkloadSagaFuture, WorkloadSagaPage, WorkloadSagaPageRequest,
    WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaStoreError, WorkloadSagaTenantPage,
    WorkloadSagaTenantPageRequest,
};
use tokio::sync::Semaphore;

use super::*;
use crate::workload_executable::encode_sandbox_spec;
use crate::workload_saga::{
    ConfirmedWorkloadProvisionCommand, IngressProvisionCapabilities, IngressPublicationCapability,
    IngressPublicationInspectionCapability, NetworkAttachmentCapability,
    NetworkAttachmentProvisionCapabilities, NetworkReservationCapability,
    WorkloadActivationCapability, WorkloadActivationPrerequisiteCapability,
    WorkloadExecutionProvisionCapabilities, WorkloadPreparationCapability,
    WorkloadProvisionCapabilityFuture, WorkloadProvisionRunDisposition,
    WorkloadProvisionSourceAuthorityError, WorkloadProvisionSourceFuture,
    WorkloadReadinessCapability,
};

const TENANT: &str = "tenant-provisioner";
const WORKLOAD: &str = "sandbox-a";
const PROFILE: &str = "python";
const GENERATION: u64 = 17;

#[derive(Default)]
struct RecordingSourceAuthority {
    evidence: Mutex<Option<WorkloadProvisionSourceEvidence>>,
    calls: AtomicUsize,
}

impl RecordingSourceAuthority {
    fn with_evidence(evidence: WorkloadProvisionSourceEvidence) -> Arc<Self> {
        Arc::new(Self {
            evidence: Mutex::new(Some(evidence)),
            calls: AtomicUsize::new(0),
        })
    }
}

impl WorkloadProvisionSourceAuthority for RecordingSourceAuthority {
    fn current_source<'a>(
        &'a self,
        _key: &'a WorkloadSagaKey,
        identity: &'a WorkloadProvisionSourceIdentity,
    ) -> WorkloadProvisionSourceFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::AcqRel);
            let evidence = self
                .evidence
                .lock()
                .expect("source authority lock should not be poisoned")
                .clone()
                .ok_or(WorkloadProvisionSourceAuthorityError::NotFound)?;
            if evidence.source_identity() != identity {
                return Err(WorkloadProvisionSourceAuthorityError::NotFound);
            }
            Ok(evidence)
        })
    }
}

struct DurableStore {
    record: Mutex<Option<WorkloadSagaRecord>>,
    loads: AtomicUsize,
    compare_and_swaps: AtomicUsize,
    ambiguous_before_apply_at: AtomicUsize,
    pause_after_first_submission: AtomicBool,
    first_submission_applied: Semaphore,
    release_first_submission: Semaphore,
}

impl Default for DurableStore {
    fn default() -> Self {
        Self {
            record: Mutex::new(None),
            loads: AtomicUsize::new(0),
            compare_and_swaps: AtomicUsize::new(0),
            ambiguous_before_apply_at: AtomicUsize::new(0),
            pause_after_first_submission: AtomicBool::new(false),
            first_submission_applied: Semaphore::new(0),
            release_first_submission: Semaphore::new(0),
        }
    }
}

impl DurableStore {
    fn pausing() -> Arc<Self> {
        Arc::new(Self {
            pause_after_first_submission: AtomicBool::new(true),
            ..Self::default()
        })
    }

    fn ambiguous_before_apply_at(call: usize) -> Arc<Self> {
        assert!(call > 0, "the injected CAS call must be one-based");
        Arc::new(Self {
            ambiguous_before_apply_at: AtomicUsize::new(call),
            ..Self::default()
        })
    }

    fn record(&self) -> Option<WorkloadSagaRecord> {
        self.record
            .lock()
            .expect("durable store lock should not be poisoned")
            .clone()
    }
}

impl WorkloadSagaStore for DurableStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            self.loads.fetch_add(1, Ordering::AcqRel);
            let record = self
                .record
                .lock()
                .expect("durable store lock should not be poisoned")
                .clone();
            if record.as_ref().is_some_and(|record| record.key() != key) {
                return Err(WorkloadSagaStoreError::Corrupt);
            }
            Ok(record)
        })
    }

    fn compare_and_swap<'a>(
        &'a self,
        expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            let call = self.compare_and_swaps.fetch_add(1, Ordering::AcqRel) + 1;
            if self.ambiguous_before_apply_at.load(Ordering::Acquire) == call {
                return Err(WorkloadSagaStoreError::Ambiguous);
            }
            let first_submission = {
                let mut current = self
                    .record
                    .lock()
                    .expect("durable store lock should not be poisoned");
                if current.as_ref() == Some(&next) {
                    return Ok(WorkloadSagaCommit::Unchanged);
                }
                let matches = match (expected, current.as_ref()) {
                    (WorkloadSagaExpected::Missing, None) => true,
                    (WorkloadSagaExpected::Revision(expected), Some(record)) => {
                        expected == record.revision()
                    }
                    _ => false,
                };
                if !matches {
                    return Err(WorkloadSagaStoreError::Conflict {
                        expected,
                        observed: current.as_ref().map(WorkloadSagaRecord::revision),
                    });
                }
                let first_submission = current.is_none();
                *current = Some(next);
                first_submission
            };
            if first_submission && self.pause_after_first_submission.load(Ordering::Acquire) {
                self.first_submission_applied.add_permits(1);
                self.release_first_submission
                    .acquire()
                    .await
                    .expect("submission release semaphore should remain open")
                    .forget();
            }
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

#[derive(Default)]
struct RecordingProvider {
    calls: Mutex<
        Vec<(
            nimbus_workloads::WorkloadProvisionStep,
            WorkloadProvisionCommandMode,
        )>,
    >,
    execution_observations: AtomicUsize,
    ingress_observations: AtomicUsize,
    readiness_waits_remaining: AtomicUsize,
}

impl RecordingProvider {
    fn with_readiness_waits(count: usize) -> Arc<Self> {
        Arc::new(Self {
            readiness_waits_remaining: AtomicUsize::new(count),
            ..Self::default()
        })
    }

    fn outcome(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionInspectionResult {
        self.calls
            .lock()
            .expect("provider call lock should not be poisoned")
            .push((command.step(), command.mode()));
        if command.step() == nimbus_workloads::WorkloadProvisionStep::InspectWorkloadReadiness
            && self
                .readiness_waits_remaining
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
        {
            return WorkloadProvisionInspectionResult::InProgress {
                attempt_id: command.attempt_id().clone(),
                dispatch_epoch: command.dispatch_epoch(),
                provider_target: command.provider_target().clone(),
                evidence: WorkloadOwnerEvidenceDigest::sha256(
                    "fixture workload readiness remains in progress",
                ),
            };
        }
        WorkloadProvisionInspectionResult::Succeeded {
            attempt_id: command.attempt_id().clone(),
            dispatch_epoch: command.dispatch_epoch(),
            provider_target: command.provider_target().clone(),
            evidence: crate::workload_saga::test_support::success_for(command.claim().attempt()),
        }
    }

    fn calls(
        &self,
    ) -> Vec<(
        nimbus_workloads::WorkloadProvisionStep,
        WorkloadProvisionCommandMode,
    )> {
        self.calls
            .lock()
            .expect("provider call lock should not be poisoned")
            .clone()
    }
}

macro_rules! effect_capability {
    ($trait_name:ident) => {
        impl $trait_name for RecordingProvider {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.outcome(command) })
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.outcome(command) })
            }
        }
    };
}

macro_rules! inspection_capability {
    ($trait_name:ident) => {
        impl $trait_name for RecordingProvider {
            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.outcome(command) })
            }
        }
    };
}

effect_capability!(NetworkReservationCapability);
effect_capability!(WorkloadPreparationCapability);
effect_capability!(NetworkAttachmentCapability);
inspection_capability!(WorkloadActivationPrerequisiteCapability);
effect_capability!(WorkloadActivationCapability);
inspection_capability!(WorkloadReadinessCapability);
effect_capability!(IngressPublicationCapability);
inspection_capability!(IngressPublicationInspectionCapability);

impl crate::workload_projection::WorkloadExecutionObservationCapability for RecordingProvider {
    fn observe<'a>(
        &'a self,
        request: &'a crate::workload_projection::WorkloadExecutionObservationRequest,
    ) -> crate::workload_projection::WorkloadExecutionObservationFuture<'a> {
        Box::pin(async move {
            self.execution_observations.fetch_add(1, Ordering::AcqRel);
            crate::workload_projection::WorkloadProviderObservation::Present(
                crate::workload_projection::test_support::exact_execution_inspection(
                    request,
                    b"recording-provision-provider",
                ),
            )
        })
    }
}

impl crate::workload_projection::WorkloadIngressObservationCapability for RecordingProvider {
    fn observe<'a>(
        &'a self,
        _request: &'a crate::workload_projection::WorkloadIngressObservationRequest,
    ) -> crate::workload_projection::WorkloadIngressObservationFuture<'a> {
        Box::pin(async move {
            self.ingress_observations.fetch_add(1, Ordering::AcqRel);
            crate::workload_projection::WorkloadProviderObservation::Ambiguous
        })
    }
}

#[derive(Default)]
struct RecordingProjectionSink {
    calls: AtomicUsize,
}

impl crate::workload_projection::WorkloadProjectionSink for RecordingProjectionSink {
    fn project<'a>(
        &'a self,
        _projection: &'a crate::workload_projection::WorkloadObservedProjection,
    ) -> crate::workload_projection::WorkloadProjectionSinkFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::AcqRel);
            Ok(())
        })
    }
}

fn tenant() -> TenantId {
    TenantId::new(TENANT).expect("fixture tenant should validate")
}

fn sandbox_spec() -> SandboxSpec {
    SandboxSpec::new(
        tenant(),
        SandboxOwnerSpec::standalone_named(PROFILE),
        SandboxBackendKind::Krun,
        SandboxRootSpec::rootfs("/fixture/rootfs"),
        SandboxProcessSpec::new(["/bin/true"]),
    )
}

fn request(node: &str, source_generation: u64) -> WorkloadProvisionRequest {
    let context = TenantIsolationContext::system(tenant(), "workload-provisioner-test")
        .with_deployment_generation(GENERATION)
        .with_workload_location(WorkloadLocation::new().with_node_id(node));
    let decision = context
        .admit_decision(
            TenantIsolationPolicyInput::new(
                WorkloadAttributes::sandbox(PROFILE)
                    .with_sandbox_id(WORKLOAD)
                    .with_sandbox_backend(SandboxBackendKind::Krun),
            )
            .with_services(TenantServiceGrantPolicyDecision::new(std::iter::empty::<
                String,
            >())),
        )
        .expect("fixture decision should admit");
    WorkloadProvisionRequest {
        decision,
        source: WorkloadProvisionSource::StandaloneSandbox {
            stable_resource_id: WORKLOAD.to_owned(),
            profile: PROFILE.to_owned(),
            source_generation: WorkloadProvisionSourceGeneration::new(source_generation),
            resource_version: WorkloadProvisionSourceResourceVersion::new(format!(
                "source-{source_generation}"
            ))
            .expect("source version should validate"),
            sandbox_spec: sandbox_spec(),
        },
        execution_provider_id: execution_provider_id(),
        endpoint_semantics: Vec::new(),
        activation: WorkloadActivationIntent::ActivateWhenAttached,
        publication: WorkloadPublicationIntent::Withheld,
    }
}

fn lifecycle() -> NetworkLifecycleCapabilitySet {
    NetworkLifecycleCapabilitySet::new([
        NetworkLifecycleFeature::DurableInspect,
        NetworkLifecycleFeature::Reconcile,
        NetworkLifecycleFeature::Delete,
    ])
}

fn provider_realm() -> (NetworkCapabilityRegistry, NetworkCapabilitySelection) {
    let requirements = sandbox_network_plan_requirements(SandboxBackendKind::Krun);
    let ingress_provider = NetworkProviderId::for_registration_key("fixture-ingress");
    let attachment = NetworkAttachmentProviderRegistration::new(
        requirements.required_attachment_provider_id().clone(),
        requirements.capability_requirements().attachment().clone(),
        [NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6],
        lifecycle(),
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let ingress = NetworkIngressProviderRegistration::new(
        ingress_provider.clone(),
        NetworkEndpointCapabilitySet::new(
            [NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6],
            [NetworkBindRealmKind::Host],
            [NetworkExposure::Loopback, NetworkExposure::Private],
            [PortProtocol::Tcp],
            [
                NetworkPortAssignmentMode::Exact,
                NetworkPortAssignmentMode::ProviderAssigned,
            ],
        ),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        lifecycle(),
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let selection = NetworkCapabilitySelection::new(
        requirements.required_attachment_provider_id().clone(),
        ingress_provider,
    );
    (
        NetworkCapabilityRegistry::new([NetworkCapabilityBundle::new(attachment, ingress)])
            .expect("fixture provider reports should validate"),
        selection,
    )
}

fn execution_provider_id() -> WorkloadExecutionProviderId {
    WorkloadExecutionProviderId::for_registration_key("fixture-execution")
}

fn source_evidence(source_generation: u64) -> WorkloadProvisionSourceEvidence {
    let spec = sandbox_spec();
    let executable = encode_sandbox_spec(&spec).expect("fixture executable should encode");
    let requirements = sandbox_network_plan_requirements(SandboxBackendKind::Krun);
    WorkloadProvisionSourceEvidence::standalone_sandbox(
        WorkloadProvisionSourceIdentity::standalone_sandbox(WORKLOAD, PROFILE)
            .expect("fixture source identity should validate"),
        WorkloadProvisionSourceGeneration::new(source_generation),
        WorkloadProvisionSourceResourceVersion::new(format!("source-{source_generation}"))
            .expect("source version should validate"),
        executable.content_digest(),
        requirements.required_attachment_provider_id().clone(),
        execution_provider_id(),
    )
    .expect("fixture source evidence should validate")
}

fn provisioner(
    store: Arc<DurableStore>,
    source: Arc<RecordingSourceAuthority>,
    provider: Arc<RecordingProvider>,
) -> Arc<WorkloadProvisioner> {
    provisioner_with_sink(
        store,
        source,
        provider,
        Arc::new(RecordingProjectionSink::default()),
    )
}

fn provisioner_with_sink(
    store: Arc<DurableStore>,
    source: Arc<RecordingSourceAuthority>,
    provider: Arc<RecordingProvider>,
    projection_sink: Arc<RecordingProjectionSink>,
) -> Arc<WorkloadProvisioner> {
    let (provider_reports, selection) = provider_realm();
    let attachment_provider = sandbox_network_plan_requirements(SandboxBackendKind::Krun)
        .required_attachment_provider_id()
        .clone();
    let capabilities = WorkloadProvisionCapabilityRegistry::new(
        [NetworkAttachmentProvisionCapabilities::new(
            attachment_provider,
            provider.clone(),
        )],
        [WorkloadExecutionProvisionCapabilities::new(
            execution_provider_id(),
            provider.clone(),
        )],
        [IngressProvisionCapabilities::new(
            selection.ingress_provider_id().clone(),
            provider,
        )],
    )
    .expect("fixture capabilities should validate");
    let store: Arc<dyn WorkloadSagaStore> = store;
    let source: Arc<dyn WorkloadProvisionSourceAuthority> = source;
    let coordinator = Arc::new(WorkloadSagaCoordinator::new(store));
    let teardown_runtime = Arc::new(WorkloadTeardownRuntime::new(
        Arc::clone(&coordinator),
        Arc::clone(&source),
        provider_reports.clone(),
        Arc::new(
            crate::workload_saga::WorkloadTeardownCapabilityRegistry::new([], [], [])
                .expect("empty teardown registry should validate"),
        ),
    ));
    Arc::new(
        WorkloadProvisioner::new(
            NodeIdentity::new("node-a").expect("fixture node should validate"),
            provider_reports,
            selection,
            NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
            coordinator,
            teardown_runtime,
            source,
            capabilities,
            projection_sink,
        )
        .expect("fixture provider realm should be coherent"),
    )
}

fn key() -> WorkloadSagaKey {
    WorkloadSagaKey::new(
        tenant(),
        WorkloadId::new(WORKLOAD).expect("fixture workload should validate"),
    )
}

#[test]
fn embedded_local_node_identity_is_canonical() {
    assert_eq!(
        embedded_local_node_identity().as_str(),
        "embedded-local-node"
    );
}

#[tokio::test]
async fn pre_cancel_has_zero_source_store_or_provider_calls() {
    let store = Arc::new(DurableStore::default());
    let source = RecordingSourceAuthority::with_evidence(source_evidence(1));
    let provider = Arc::new(RecordingProvider::default());
    let provisioner = provisioner(store.clone(), source.clone(), provider.clone());
    let cancellation = WorkloadProvisionCancellation::default();
    let reservations = AtomicUsize::new(0);
    cancellation.cancel();

    let result = provisioner
        .provision_with_source_reservation(request("node-a", 1), &cancellation, || {
            reservations.fetch_add(1, Ordering::AcqRel);
            Ok(())
        })
        .await;

    assert!(matches!(
        result.as_ref().map_err(Arc::as_ref),
        Err(WorkloadProvisionError::CancelledBeforeSubmission)
    ));
    assert_eq!(source.calls.load(Ordering::Acquire), 0);
    assert_eq!(store.loads.load(Ordering::Acquire), 0);
    assert_eq!(store.compare_and_swaps.load(Ordering::Acquire), 0);
    assert!(provider.calls().is_empty());
    assert_eq!(reservations.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn source_reservation_failure_precedes_store_and_provider_effects() {
    let store = Arc::new(DurableStore::default());
    let source = RecordingSourceAuthority::with_evidence(source_evidence(1));
    let provider = Arc::new(RecordingProvider::default());
    let provisioner = provisioner(store.clone(), source.clone(), provider.clone());

    let result = provisioner
        .provision_with_source_reservation(
            request("node-a", 1),
            &WorkloadProvisionCancellation::default(),
            || {
                Err(nimbus_core::Error::conflict(
                    "crossed desired source must not submit",
                ))
            },
        )
        .await;

    assert!(matches!(
        result.as_ref().map_err(Arc::as_ref),
        Err(WorkloadProvisionError::SourceReservation(
            nimbus_core::Error::Conflict { .. }
        ))
    ));
    assert_eq!(source.calls.load(Ordering::Acquire), 0);
    assert_eq!(store.loads.load(Ordering::Acquire), 0);
    assert_eq!(store.compare_and_swaps.load(Ordering::Acquire), 0);
    assert!(provider.calls().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_contending_with_keyed_insertion_is_waiter_only() {
    let store = DurableStore::pausing();
    let source = RecordingSourceAuthority::with_evidence(source_evidence(1));
    let provider = Arc::new(RecordingProvider::default());
    let provisioner = provisioner(store.clone(), source, provider.clone());
    let cancellation = WorkloadProvisionCancellation::default();
    let reservations = Arc::new(AtomicUsize::new(0));
    let entered = Arc::new(std::sync::Barrier::new(2));
    let release = Arc::new(std::sync::Barrier::new(2));
    provisioner.install_test_submission_boundary(entered.clone(), release.clone());

    let waiter_provisioner = provisioner.clone();
    let waiter_cancellation = cancellation.clone();
    let waiter_reservations = reservations.clone();
    let waiter = tokio::spawn(async move {
        waiter_provisioner
            .provision_with_source_reservation(
                request("node-a", 1),
                &waiter_cancellation,
                move || {
                    waiter_reservations.fetch_add(1, Ordering::AcqRel);
                    Ok(())
                },
            )
            .await
    });
    entered.wait();

    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (cancelled_tx, cancelled_rx) = std::sync::mpsc::channel();
    let contending_cancellation = cancellation.clone();
    let canceller = std::thread::spawn(move || {
        started_tx
            .send(())
            .expect("cancellation-start receiver should remain open");
        contending_cancellation.cancel();
        cancelled_tx
            .send(())
            .expect("cancellation completion receiver should remain open");
    });
    started_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("cancellation thread should reach the contended boundary");
    assert!(matches!(
        cancelled_rx.recv_timeout(std::time::Duration::from_millis(100)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));

    release.wait();
    cancelled_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("cancellation should complete after keyed insertion");
    canceller.join().expect("cancellation thread should join");
    let result = waiter.await.expect("waiter task should join");
    assert!(matches!(
        result.as_ref().map_err(Arc::as_ref),
        Err(WorkloadProvisionError::WaiterCancelled)
    ));

    store
        .first_submission_applied
        .acquire()
        .await
        .expect("durable submission signal should remain open")
        .forget();
    assert_eq!(store.compare_and_swaps.load(Ordering::Acquire), 1);
    assert_eq!(reservations.load(Ordering::Acquire), 1);
    assert!(store.record().is_some());
    assert!(provider.calls().is_empty());

    store.release_first_submission.add_permits(1);
    provisioner
        .resume(key(), &WorkloadProvisionCancellation::default())
        .await
        .expect("retained task or exact resume should converge");
    assert_eq!(provider.calls().len(), 6);
}

#[tokio::test]
async fn crossed_node_source_and_provider_realm_fail_before_cas_or_effect() {
    let store = Arc::new(DurableStore::default());
    let source = RecordingSourceAuthority::with_evidence(source_evidence(1));
    let provider = Arc::new(RecordingProvider::default());
    let provisioner = provisioner(store.clone(), source.clone(), provider.clone());
    let cancellation = WorkloadProvisionCancellation::default();

    let node = provisioner
        .provision(request("node-b", 1), &cancellation)
        .await;
    assert!(matches!(
        node.as_ref().map_err(Arc::as_ref),
        Err(WorkloadProvisionError::Composition(
            WorkloadProvisionCompositionError::NodeMismatch { .. }
        ))
    ));

    let mut crossed_source = request("node-a", 1);
    let WorkloadProvisionSource::StandaloneSandbox {
        stable_resource_id, ..
    } = &mut crossed_source.source
    else {
        unreachable!("fixture source is standalone")
    };
    *stable_resource_id = "crossed-source".to_owned();
    let source_result = provisioner.provision(crossed_source, &cancellation).await;
    assert!(matches!(
        source_result.as_ref().map_err(Arc::as_ref),
        Err(WorkloadProvisionError::Composition(_))
    ));

    let (reports, _) = provider_realm();
    let crossed_selection = NetworkCapabilitySelection::new(
        NetworkProviderId::for_registration_key("missing-attachment"),
        NetworkProviderId::for_registration_key("missing-ingress"),
    );
    let empty_capabilities = WorkloadProvisionCapabilityRegistry::new([], [], [])
        .expect("empty capability registry should fail closed");
    let coordinator = Arc::new(WorkloadSagaCoordinator::new(store.clone()));
    let teardown_runtime = Arc::new(WorkloadTeardownRuntime::new(
        Arc::clone(&coordinator),
        source.clone(),
        reports.clone(),
        Arc::new(
            crate::workload_saga::WorkloadTeardownCapabilityRegistry::new([], [], [])
                .expect("empty teardown registry should validate"),
        ),
    ));
    let crossed_provisioner = WorkloadProvisioner::new(
        NodeIdentity::new("node-a").expect("fixture node should validate"),
        reports,
        crossed_selection,
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        coordinator,
        teardown_runtime,
        source.clone(),
        empty_capabilities,
        Arc::new(RecordingProjectionSink::default()),
    );
    assert!(matches!(
        crossed_provisioner,
        Err(WorkloadProvisionConfigurationError::MissingExactSelection { .. })
    ));

    assert_eq!(source.calls.load(Ordering::Acquire), 0);
    assert_eq!(store.compare_and_swaps.load(Ordering::Acquire), 0);
    assert!(provider.calls().is_empty());
}

#[tokio::test]
async fn concurrent_exact_callers_share_one_tracked_run_and_provider_effect_per_step() {
    let store = Arc::new(DurableStore::default());
    let source = RecordingSourceAuthority::with_evidence(source_evidence(1));
    let provider = Arc::new(RecordingProvider::default());
    let projection_sink = Arc::new(RecordingProjectionSink::default());
    let provisioner =
        provisioner_with_sink(store, source, provider.clone(), projection_sink.clone());
    let left_cancellation = WorkloadProvisionCancellation::default();
    let right_cancellation = WorkloadProvisionCancellation::default();
    let reservations = Arc::new(AtomicUsize::new(0));
    let left_reservations = reservations.clone();
    let right_reservations = reservations.clone();

    let (left, right) = tokio::join!(
        provisioner.provision_with_source_reservation(
            request("node-a", 1),
            &left_cancellation,
            move || {
                left_reservations.fetch_add(1, Ordering::AcqRel);
                Ok(())
            }
        ),
        provisioner.provision_with_source_reservation(
            request("node-a", 1),
            &right_cancellation,
            move || {
                right_reservations.fetch_add(1, Ordering::AcqRel);
                Ok(())
            }
        ),
    );

    let left = left.expect("left exact caller should complete");
    let right = right.expect("right exact caller should complete");
    assert_eq!(left.record(), right.record());
    assert_eq!(
        left.disposition(),
        WorkloadProvisionRunDisposition::Observed
    );
    assert_eq!(left.projection(), WorkloadProjectionState::Projected);
    assert_eq!(right.projection(), WorkloadProjectionState::Projected);
    assert_eq!(
        provider.execution_observations.load(Ordering::Acquire),
        1,
        "one retained run must perform one exact read-only execution observation"
    );
    assert_eq!(provider.ingress_observations.load(Ordering::Acquire), 0);
    assert_eq!(projection_sink.calls.load(Ordering::Acquire), 1);
    assert_eq!(
        reservations.load(Ordering::Acquire),
        2,
        "each exact caller must revalidate source authority under the keyed lock"
    );
    let calls = provider.calls();
    assert_eq!(calls.len(), 6);
    for step in [
        nimbus_workloads::WorkloadProvisionStep::ReserveNetwork,
        nimbus_workloads::WorkloadProvisionStep::PrepareWorkload,
        nimbus_workloads::WorkloadProvisionStep::AttachNetwork,
        nimbus_workloads::WorkloadProvisionStep::InspectActivationPrerequisites,
        nimbus_workloads::WorkloadProvisionStep::ActivateWorkload,
        nimbus_workloads::WorkloadProvisionStep::InspectWorkloadReadiness,
    ] {
        assert_eq!(
            calls.iter().filter(|(called, _)| *called == step).count(),
            1
        );
    }
}

#[tokio::test]
async fn pending_readiness_retains_supervisor_and_converges_without_resubmission() {
    let store = Arc::new(DurableStore::default());
    let source = RecordingSourceAuthority::with_evidence(source_evidence(1));
    let provider = RecordingProvider::with_readiness_waits(2);
    let projection_sink = Arc::new(RecordingProjectionSink::default());
    let provisioner = provisioner_with_sink(
        store.clone(),
        source,
        provider.clone(),
        projection_sink.clone(),
    );

    let receipt = provisioner
        .provision(
            request("node-a", 1),
            &WorkloadProvisionCancellation::default(),
        )
        .await
        .expect("the first bounded provision receipt should remain truthful");
    assert_eq!(
        receipt.disposition(),
        WorkloadProvisionRunDisposition::Waiting
    );
    assert_eq!(
        receipt.projection(),
        WorkloadProjectionState::Pending(
            crate::workload_projection::WorkloadProjectionPendingReason::ProvisionWaiting,
        )
    );
    assert!(provisioner.has_tracked_submission(&key()));
    assert!(provisioner.has_running_tracked_task(&key()));

    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while provisioner.has_tracked_submission(&key()) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the retained supervisor should converge without another submission");

    let durable = store
        .record()
        .expect("retained supervision should preserve durable provision truth");
    assert_eq!(
        durable.phase(),
        nimbus_workloads::WorkloadSagaPhase::Observed
    );
    assert_eq!(projection_sink.calls.load(Ordering::Acquire), 1);
    let readiness_calls = provider
        .calls()
        .into_iter()
        .filter(|(step, _)| {
            *step == nimbus_workloads::WorkloadProvisionStep::InspectWorkloadReadiness
        })
        .collect::<Vec<_>>();
    assert_eq!(readiness_calls.len(), 3);
    assert_eq!(
        readiness_calls
            .iter()
            .filter(|(_, mode)| *mode == WorkloadProvisionCommandMode::Inspect)
            .count(),
        3,
        "readiness progress must converge through the read-only inspection seam"
    );
}

#[tokio::test]
async fn intermediate_cas_ambiguity_retains_supervisor_and_converges_without_resubmission() {
    // The sixth CAS claims network attachment after workload preparation. An
    // ambiguity before apply leaves durable truth at WorkloadPrepared, which
    // still requires retained supervision.
    let store = DurableStore::ambiguous_before_apply_at(6);
    let source = RecordingSourceAuthority::with_evidence(source_evidence(1));
    let provider = Arc::new(RecordingProvider::default());
    let provisioner = provisioner(store.clone(), source, provider.clone());

    let receipt = provisioner
        .provision(
            request("node-a", 1),
            &WorkloadProvisionCancellation::default(),
        )
        .await
        .expect("the ambiguous intermediate receipt should remain truthful");
    assert_eq!(
        receipt.disposition(),
        WorkloadProvisionRunDisposition::Waiting
    );
    assert_eq!(
        receipt.record().phase(),
        nimbus_workloads::WorkloadSagaPhase::WorkloadPrepared
    );
    assert!(provisioner.has_running_tracked_task(&key()));

    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while provisioner.has_tracked_submission(&key()) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("retained supervision should retry the exact durable phase");

    assert_eq!(
        store
            .record()
            .expect("retained supervision should preserve durable truth")
            .phase(),
        nimbus_workloads::WorkloadSagaPhase::Observed
    );
    assert_eq!(
        provider
            .calls()
            .iter()
            .filter(|(step, _)| { *step == nimbus_workloads::WorkloadProvisionStep::AttachNetwork })
            .count(),
        1,
        "the pre-effect CAS ambiguity must not duplicate attachment"
    );
}

#[tokio::test]
async fn prepare_only_quiescence_does_not_retain_a_polling_supervisor() {
    let store = Arc::new(DurableStore::default());
    let source = RecordingSourceAuthority::with_evidence(source_evidence(1));
    let provider = Arc::new(RecordingProvider::default());
    let provisioner = provisioner(store, source, provider.clone());
    let mut prepare_only = request("node-a", 1);
    prepare_only.activation = WorkloadActivationIntent::PrepareOnly;

    let receipt = provisioner
        .provision(prepare_only, &WorkloadProvisionCancellation::default())
        .await
        .expect("prepare-only provisioning should stop at its intended plateau");

    assert_eq!(
        receipt.disposition(),
        WorkloadProvisionRunDisposition::Waiting
    );
    assert_eq!(
        receipt.record().phase(),
        nimbus_workloads::WorkloadSagaPhase::NetworkAttached
    );
    assert!(!provisioner.has_tracked_submission(&key()));
    assert!(provider.calls().iter().all(|(step, _)| {
        !matches!(
            step,
            nimbus_workloads::WorkloadProvisionStep::InspectActivationPrerequisites
                | nimbus_workloads::WorkloadProvisionStep::ActivateWorkload
                | nimbus_workloads::WorkloadProvisionStep::InspectWorkloadReadiness
        )
    }));
}

#[tokio::test]
async fn retirement_joins_pending_supervisor_and_stops_later_inspection() {
    let store = Arc::new(DurableStore::default());
    let source = RecordingSourceAuthority::with_evidence(source_evidence(1));
    let provider = RecordingProvider::with_readiness_waits(100);
    let provisioner = provisioner(store, source, provider.clone());

    let receipt = provisioner
        .provision(
            request("node-a", 1),
            &WorkloadProvisionCancellation::default(),
        )
        .await
        .expect("the first bounded provision receipt should remain truthful");
    assert_eq!(
        receipt.disposition(),
        WorkloadProvisionRunDisposition::Waiting
    );
    assert!(provisioner.has_running_tracked_task(&key()));
    let calls_before_retirement = provider.calls().len();

    let (claim, joined) = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        provisioner
            .claim_retirement_and_join(&key(), || Ok::<_, nimbus_core::Error>("retirement-claim")),
    )
    .await
    .expect("retirement should join the retained supervisor within its retry delay")
    .expect("retirement should join the pending provision cleanly");
    assert_eq!(claim, "retirement-claim");
    assert_eq!(
        joined
            .expect("retirement should receive the last exact provision truth")
            .disposition(),
        WorkloadProvisionRunDisposition::Waiting
    );
    assert!(!provisioner.has_tracked_submission(&key()));

    tokio::time::sleep(std::time::Duration::from_millis(75)).await;
    assert_eq!(
        provider.calls().len(),
        calls_before_retirement,
        "retirement must stop retained readiness inspection before teardown"
    );
    provisioner.release_retirement_fence(&key());
}

#[tokio::test]
async fn crossed_same_key_generation_rejects_instead_of_joining() {
    let store = DurableStore::pausing();
    let source = RecordingSourceAuthority::with_evidence(source_evidence(1));
    let provider = Arc::new(RecordingProvider::default());
    let provisioner = provisioner(store.clone(), source, provider.clone());
    let first_cancellation = WorkloadProvisionCancellation::default();
    let first_provisioner = provisioner.clone();
    let first = tokio::spawn(async move {
        first_provisioner
            .provision(request("node-a", 1), &first_cancellation)
            .await
    });
    store
        .first_submission_applied
        .acquire()
        .await
        .expect("first submission signal should remain open")
        .forget();

    let crossed = provisioner
        .provision(
            request("node-a", 2),
            &WorkloadProvisionCancellation::default(),
        )
        .await;
    assert!(matches!(
        crossed.as_ref().map_err(Arc::as_ref),
        Err(WorkloadProvisionError::CrossedTrackedRequest)
    ));
    assert_eq!(store.compare_and_swaps.load(Ordering::Acquire), 1);
    assert!(provider.calls().is_empty());

    store.release_first_submission.add_permits(1);
    first
        .await
        .expect("first waiter task should join")
        .expect("first exact provision should complete");
}

#[tokio::test]
async fn post_submit_cancellation_keeps_one_record_and_fresh_provisioner_resume_converges() {
    let store = DurableStore::pausing();
    let source = RecordingSourceAuthority::with_evidence(source_evidence(1));
    let provider = Arc::new(RecordingProvider::default());
    let original = provisioner(store.clone(), source.clone(), provider.clone());
    let cancellation = WorkloadProvisionCancellation::default();
    let waiter_cancellation = cancellation.clone();
    let waiter_provisioner = original.clone();
    let waiter = tokio::spawn(async move {
        waiter_provisioner
            .provision(request("node-a", 1), &waiter_cancellation)
            .await
    });
    store
        .first_submission_applied
        .acquire()
        .await
        .expect("first submission signal should remain open")
        .forget();
    cancellation.cancel();
    let cancelled = waiter.await.expect("waiter task should join");
    assert!(matches!(
        cancelled.as_ref().map_err(Arc::as_ref),
        Err(WorkloadProvisionError::WaiterCancelled)
    ));
    let durable = store
        .record()
        .expect("durable desire must remain after waiter cancellation");
    assert_eq!(durable.key(), &key());
    assert_eq!(store.compare_and_swaps.load(Ordering::Acquire), 1);
    assert!(provider.calls().is_empty());

    let fresh = provisioner(store.clone(), source, provider.clone());
    let fresh_resume = tokio::spawn(async move {
        fresh
            .resume(key(), &WorkloadProvisionCancellation::default())
            .await
    });
    store.release_first_submission.add_permits(1);
    let resumed = fresh_resume
        .await
        .expect("fresh resume task should join")
        .expect("fresh provisioner should converge durable truth");
    assert_eq!(
        resumed.disposition(),
        WorkloadProvisionRunDisposition::Observed
    );
    assert_eq!(provider.calls().len(), 6);
}

#[tokio::test]
async fn retained_cancellation_wakes_multiple_waiters_at_the_wait_boundary() {
    let store = DurableStore::pausing();
    let source = RecordingSourceAuthority::with_evidence(source_evidence(1));
    let provider = Arc::new(RecordingProvider::default());
    let provisioner = provisioner(store.clone(), source, provider.clone());
    let cancellation = WorkloadProvisionCancellation::default();
    let waiters_entered = Arc::new(Semaphore::new(0));
    provisioner.install_test_wait_boundary(Arc::clone(&waiters_entered));

    let left_provisioner = provisioner.clone();
    let left_cancellation = cancellation.clone();
    let left = tokio::spawn(async move {
        left_provisioner
            .provision(request("node-a", 1), &left_cancellation)
            .await
    });
    store
        .first_submission_applied
        .acquire()
        .await
        .expect("first submission signal should remain open")
        .forget();

    let right_provisioner = provisioner.clone();
    let right_cancellation = cancellation.clone();
    let right = tokio::spawn(async move {
        right_provisioner
            .provision(request("node-a", 1), &right_cancellation)
            .await
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        waiters_entered.acquire_many(2),
    )
    .await
    .expect("both retained waiters should reach the tracked wait boundary before timeout")
    .expect("the retained-waiter boundary should remain open")
    .forget();
    cancellation.cancel();

    for result in [
        left.await.expect("left waiter should join"),
        right.await.expect("right waiter should join"),
    ] {
        assert!(matches!(
            result.as_ref().map_err(Arc::as_ref),
            Err(WorkloadProvisionError::WaiterCancelled)
        ));
    }
    assert_eq!(store.compare_and_swaps.load(Ordering::Acquire), 1);
    assert!(store.record().is_some());
    assert!(provider.calls().is_empty());

    store.release_first_submission.add_permits(1);
    provisioner
        .resume(key(), &WorkloadProvisionCancellation::default())
        .await
        .expect("retained task or exact resume should converge");
    assert_eq!(provider.calls().len(), 6);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_start_and_stop_linearize_at_the_source_fence() {
    // Schedule one: the retirement claim wins before keyed insertion. The
    // provisioner's keyed guard rejects both raw submission and public resume
    // before store or provider effects.
    let store = Arc::new(DurableStore::default());
    let source = RecordingSourceAuthority::with_evidence(source_evidence(1));
    let provider = Arc::new(RecordingProvider::default());
    let stop_wins_provisioner = provisioner(store.clone(), source, provider.clone());
    let entered = Arc::new(std::sync::Barrier::new(2));
    let release = Arc::new(std::sync::Barrier::new(2));
    stop_wins_provisioner.install_test_submission_boundary(entered.clone(), release.clone());
    let start_provisioner = stop_wins_provisioner.clone();
    let start = tokio::spawn(async move {
        start_provisioner
            .provision(
                request("node-a", 1),
                &WorkloadProvisionCancellation::default(),
            )
            .await
    });
    entered.wait();
    let (claimed, joined) = stop_wins_provisioner
        .claim_retirement_and_join(&key(), || Ok::<_, nimbus_core::Error>("retirement-claim"))
        .await
        .expect("retirement should acquire the source fence");
    assert_eq!(claimed, "retirement-claim");
    assert!(joined.is_none());
    let resume = stop_wins_provisioner
        .resume(key(), &WorkloadProvisionCancellation::default())
        .await;
    assert!(matches!(
        resume.as_ref().map_err(Arc::as_ref),
        Err(WorkloadProvisionError::RetirementInProgress)
    ));
    release.wait();
    let start = start.await.expect("start contender should join");
    assert!(matches!(
        start.as_ref().map_err(Arc::as_ref),
        Err(WorkloadProvisionError::RetirementInProgress)
    ));
    assert_eq!(store.compare_and_swaps.load(Ordering::Acquire), 0);
    assert!(provider.calls().is_empty());

    // Schedule two: keyed provision insertion wins. Retirement acquires its
    // claim and joins the exact retained completion before it can continue.
    let store = DurableStore::pausing();
    let source = RecordingSourceAuthority::with_evidence(source_evidence(1));
    let provider = Arc::new(RecordingProvider::default());
    let provisioner = provisioner(store.clone(), source, provider.clone());
    let start_provisioner = provisioner.clone();
    let start = tokio::spawn(async move {
        start_provisioner
            .provision(
                request("node-a", 1),
                &WorkloadProvisionCancellation::default(),
            )
            .await
    });
    store
        .first_submission_applied
        .acquire()
        .await
        .expect("durable submission signal should remain open")
        .forget();

    let claim_entered = Arc::new(Semaphore::new(0));
    let join_provisioner = provisioner.clone();
    let join_signal = claim_entered.clone();
    let stop = tokio::spawn(async move {
        join_provisioner
            .claim_retirement_and_join(&key(), move || {
                join_signal.add_permits(1);
                Ok::<_, nimbus_core::Error>("retirement-claim")
            })
            .await
    });
    claim_entered
        .acquire()
        .await
        .expect("retirement claim signal should remain open")
        .forget();
    assert!(provider.calls().is_empty());
    assert!(
        !stop.is_finished(),
        "retirement must join retained provision"
    );

    store.release_first_submission.add_permits(1);
    let start = start
        .await
        .expect("start task should join")
        .expect("winning start should complete");
    let (claim, joined) = stop
        .await
        .expect("stop task should join")
        .expect("stop should join the winning provision");
    assert_eq!(claim, "retirement-claim");
    assert_eq!(
        joined
            .expect("retirement should retain exact provision outcome")
            .record(),
        start.record()
    );
    assert_eq!(store.compare_and_swaps.load(Ordering::Acquire), 13);
    assert_eq!(provider.calls().len(), 6);
}
