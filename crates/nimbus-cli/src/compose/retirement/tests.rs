use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nimbus::{
    Engine, SandboxBackendKind, SandboxHandle, SandboxId, SandboxOwnerSpec, SandboxPortBinding,
    SandboxProcessSpec, SandboxRootSpec, SandboxSpec, SandboxStatus, TenantId,
};
use nimbus_compute::workload_saga::{
    ConfirmedWorkloadProvisionCommand, ConfirmedWorkloadTeardownCommand,
    FinalIngressWithdrawalCapability, IngressPublicationCapability,
    IngressPublicationInspectionCapability, IngressTeardownCapabilities,
    NetworkAttachmentCapability, NetworkAttachmentTeardownCapabilities,
    NetworkDetachmentCapability, NetworkReleaseCapability, NetworkReservationCapability,
    WorkloadActivationCapability, WorkloadActivationPrerequisiteCapability,
    WorkloadExecutionDrainCapability, WorkloadExecutionStopCapability,
    WorkloadExecutionTeardownCapabilities, WorkloadPreparationCapability,
    WorkloadProvisionCapabilityFuture, WorkloadReadinessCapability,
    WorkloadTeardownCapabilityFuture, WorkloadTeardownCapabilityRegistry,
    WorkloadTeardownExecuteOutcome, WorkloadTeardownInspectOutcome,
    WorkloadTeardownProviderObservation, WorkloadTeardownProviderOutcome,
    sandbox_execution_provider_id,
};
use nimbus_compute::{
    WorkloadExecutionObservationCapability, WorkloadExecutionObservationFuture,
    WorkloadExecutionObservationRequest, WorkloadIngressObservationCapability,
    WorkloadIngressObservationFuture, WorkloadIngressObservationRequest,
    WorkloadProviderObservation,
};
use nimbus_network::{
    NetworkAddressFamily, NetworkAttachmentHandle, NetworkAttachmentProviderRegistration,
    NetworkBindRealmKind, NetworkCapabilityBundle, NetworkCapabilityRegistry,
    NetworkCapabilitySelection, NetworkControlPlaneLocality, NetworkEndpointCapabilitySet,
    NetworkExposure, NetworkForwardingCapabilitySet, NetworkForwardingFeature,
    NetworkIngressCapabilitySet, NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet,
    NetworkLifecycleFeature, NetworkPortAssignmentMode, NetworkProviderId,
    NetworkSovereigntyCapabilities, NetworkSovereigntyRequirements, NetworkTlsBehavior,
    PortProtocol, PublishedEndpoint, PublishedEndpointHandle,
};
use nimbus_sandbox::{SandboxExecutionAttemptId, SandboxInspection, SandboxNetworkStatus};
use nimbus_server::{EngineWorkloadSagaStore, ServerWorkloadComposition, ServerWorkloadProviders};
use nimbus_services::{
    ServiceBackend, ServiceDefinition, ServiceDefinitionCatalog, ServiceManager,
};
use nimbus_tenant::TenantIsolationContext;
use nimbus_workloads::{
    WorkloadFailureEvidence, WorkloadOwnerEvidenceDigest, WorkloadProvisionInspectionResult,
    WorkloadProvisionStep, WorkloadProvisionSubjects, WorkloadProvisionSuccessEvidence,
    WorkloadPublicationIntent, WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaFuture,
    WorkloadSagaKey, WorkloadSagaPage, WorkloadSagaPageRequest, WorkloadSagaPhase,
    WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaStoreError, WorkloadSagaTenantPage,
    WorkloadSagaTenantPageRequest, WorkloadTeardownCommandMode, WorkloadTeardownStep,
    WorkloadTeardownSubjects, WorkloadTeardownSuccessEvidence,
};
use tempfile::TempDir;
use tokio::sync::Semaphore;

use super::*;
use crate::compose::discovery::ResolvedComposeSelection;
use crate::compose::provision::PreparedComposeProvision;

const PROCESS_CHILD_ENV: &str = "NIMBUS_NNC_F2_PROCESS_CHILD";
const PROCESS_ROOT_ENV: &str = "NIMBUS_NNC_F2_PROCESS_ROOT";
const PROCESS_CRASH_ENV: &str = "NIMBUS_NNC_F2_PROCESS_CRASH";
const PROCESS_TEST_NAME: &str = "compose::retirement::tests::compose_down_process_reopen_resumes_same_attempt_without_duplicate_effect";

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderCall {
    service: String,
    step: WorkloadTeardownStep,
    mode: WorkloadTeardownCommandMode,
    attempt_id: String,
    dispatch_epoch: u64,
    durable_before_effect: bool,
}

#[derive(Clone)]
struct StepGate {
    step: WorkloadTeardownStep,
    entered: Arc<Semaphore>,
    release: Arc<Semaphore>,
}

struct RecordingComposeProvider {
    engine: Arc<Engine>,
    provider_root: PathBuf,
    calls: Mutex<Vec<ProviderCall>>,
    ambiguous_once: Mutex<Option<WorkloadTeardownStep>>,
    gate: Mutex<Option<StepGate>>,
    gate_entered: AtomicBool,
}

impl RecordingComposeProvider {
    fn new(engine: Arc<Engine>, provider_root: PathBuf) -> Arc<Self> {
        fs::create_dir_all(provider_root.join("effects"))
            .expect("provider effect root should exist");
        Arc::new(Self {
            engine,
            provider_root,
            calls: Mutex::new(Vec::new()),
            ambiguous_once: Mutex::new(None),
            gate: Mutex::new(None),
            gate_entered: AtomicBool::new(false),
        })
    }

    fn calls(&self) -> Vec<ProviderCall> {
        self.calls.lock().expect("provider call log").clone()
    }

    fn reset_calls(&self) {
        self.calls.lock().expect("provider call log").clear();
    }

    fn ambiguous_once_at(&self, step: WorkloadTeardownStep) {
        *self.ambiguous_once.lock().expect("ambiguous fault lock") = Some(step);
    }

    fn install_gate(&self, step: WorkloadTeardownStep) -> (Arc<Semaphore>, Arc<Semaphore>) {
        let entered = Arc::new(Semaphore::new(0));
        let release = Arc::new(Semaphore::new(0));
        *self.gate.lock().expect("provider gate lock") = Some(StepGate {
            step,
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
        });
        self.gate_entered.store(false, Ordering::Release);
        (entered, release)
    }

    async fn provision_outcome(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionInspectionResult {
        WorkloadProvisionInspectionResult::Succeeded {
            attempt_id: command.attempt_id().clone(),
            dispatch_epoch: command.dispatch_epoch(),
            provider_target: command.provider_target().clone(),
            evidence: provision_success(command.step(), command.claim().attempt().subjects()),
        }
    }

    async fn teardown_outcome(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownProviderObservation {
        let store = EngineWorkloadSagaStore::new(Arc::clone(&self.engine));
        let durable = store
            .load(command.key())
            .await
            .expect("provider must read canonical Engine saga durability")
            .expect("confirmed provider command must already be durable");
        let durable_before_effect = durable.revision() == command.confirmed_revision()
            && durable.last_transition().transition_id() == command.confirmed_transition_id();
        assert!(
            durable_before_effect,
            "every provider command must follow its exact confirmed Engine transition"
        );

        let call = ProviderCall {
            service: command.key().workload_id().as_str().to_owned(),
            step: command.step(),
            mode: command.mode(),
            attempt_id: command.attempt_id().as_str().to_owned(),
            dispatch_epoch: command.dispatch_epoch().as_u64(),
            durable_before_effect,
        };
        self.record_call(&call);

        let marker = self.effect_marker(command);
        let success = || teardown_success(command.step(), command.subjects());
        let outcome = match command.mode() {
            WorkloadTeardownCommandMode::Execute => {
                let created = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&marker);
                match created {
                    Ok(mut file) => {
                        file.write_all(command.attempt_id().as_str().as_bytes())
                            .expect("provider effect marker should write");
                        file.sync_all()
                            .expect("provider effect marker should persist");
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        return WorkloadTeardownProviderObservation::for_command(
                            command,
                            WorkloadTeardownProviderOutcome::Execute(
                                WorkloadTeardownExecuteOutcome::DefiniteFailure(
                                    WorkloadFailureEvidence::new(
                                        "duplicate_provider_effect",
                                        WorkloadOwnerEvidenceDigest::sha256(
                                            "duplicate-provider-effect",
                                        ),
                                    )
                                    .expect("duplicate-effect evidence should validate"),
                                ),
                            ),
                        );
                    }
                    Err(error) => panic!("provider effect marker should create: {error}"),
                }

                if process_should_crash_at(command.step()) {
                    std::process::abort();
                }

                let gate = { self.gate.lock().expect("provider gate lock").clone() };
                if let Some(gate) = gate
                    && gate.step == command.step()
                    && !self.gate_entered.swap(true, Ordering::AcqRel)
                {
                    gate.entered.add_permits(1);
                    gate.release
                        .acquire()
                        .await
                        .expect("provider release gate should remain open")
                        .forget();
                }

                let ambiguous = {
                    let mut fault = self.ambiguous_once.lock().expect("ambiguous fault lock");
                    if fault.as_ref() == Some(&command.step()) {
                        fault.take();
                        true
                    } else {
                        false
                    }
                };
                WorkloadTeardownProviderOutcome::Execute(if ambiguous {
                    WorkloadTeardownExecuteOutcome::Ambiguous
                } else {
                    WorkloadTeardownExecuteOutcome::Succeeded(Box::new(success()))
                })
            }
            WorkloadTeardownCommandMode::Inspect => {
                WorkloadTeardownProviderOutcome::Inspect(if marker.is_file() {
                    WorkloadTeardownInspectOutcome::Satisfied(Box::new(success()))
                } else {
                    WorkloadTeardownInspectOutcome::NotCompleted(
                        WorkloadOwnerEvidenceDigest::sha256("provider-effect-absent"),
                    )
                })
            }
        };
        WorkloadTeardownProviderObservation::for_command(command, outcome)
    }

    fn effect_marker(&self, command: &ConfirmedWorkloadTeardownCommand) -> PathBuf {
        self.provider_root.join("effects").join(format!(
            "{}-{}-{}",
            command.key().tenant_id().as_str(),
            command.key().workload_id().as_str(),
            step_slug(command.step())
        ))
    }

    fn record_call(&self, call: &ProviderCall) {
        self.calls
            .lock()
            .expect("provider call log")
            .push(call.clone());
        let mut log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.provider_root.join("calls.log"))
            .expect("provider call log should open");
        writeln!(
            log,
            "{}|{}|{:?}|{}|{}",
            call.service,
            step_slug(call.step),
            call.mode,
            call.attempt_id,
            call.dispatch_epoch
        )
        .expect("provider call log should append");
        log.sync_all().expect("provider call log should persist");
    }
}

macro_rules! provision_effect_capability {
    ($trait_name:ident) => {
        impl $trait_name for RecordingComposeProvider {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(self.provision_outcome(command))
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(self.provision_outcome(command))
            }
        }
    };
}

macro_rules! provision_inspection_capability {
    ($trait_name:ident) => {
        impl $trait_name for RecordingComposeProvider {
            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(self.provision_outcome(command))
            }
        }
    };
}

provision_effect_capability!(NetworkReservationCapability);
provision_effect_capability!(WorkloadPreparationCapability);
provision_effect_capability!(NetworkAttachmentCapability);
provision_inspection_capability!(WorkloadActivationPrerequisiteCapability);
provision_effect_capability!(WorkloadActivationCapability);
provision_inspection_capability!(WorkloadReadinessCapability);
provision_effect_capability!(IngressPublicationCapability);
provision_inspection_capability!(IngressPublicationInspectionCapability);

impl WorkloadExecutionObservationCapability for RecordingComposeProvider {
    fn observe<'a>(
        &'a self,
        request: &'a WorkloadExecutionObservationRequest,
    ) -> WorkloadExecutionObservationFuture<'a> {
        Box::pin(async move {
            let spec =
                nimbus_compute::workload_executable::decode_sandbox_spec(request.executable())
                    .expect("fixture executable should decode");
            let content = request.compiled_network_plan().content();
            let endpoint_handles =
                if content.publication() == WorkloadPublicationIntent::PublishWhenReady {
                    content
                        .listeners()
                        .iter()
                        .enumerate()
                        .map(|(ordinal, listener)| {
                            let endpoint = PublishedEndpoint::new(
                                listener.name(),
                                listener.protocol(),
                                SocketAddr::from((Ipv4Addr::LOCALHOST, 49_152 + ordinal as u16)),
                            );
                            let endpoint = listener
                                .guest_port()
                                .map_or(endpoint.clone(), |port| endpoint.with_guest_port(port));
                            PublishedEndpointHandle::new(
                                listener.endpoint_id().clone(),
                                content.identity().generation(),
                                endpoint,
                            )
                        })
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
            let visible_endpoints = endpoint_handles
                .iter()
                .map(|endpoint| endpoint.endpoint().clone())
                .collect();
            let network_status = content.attachment().map(|attachment| {
                SandboxNetworkStatus::new(
                    Some(NetworkAttachmentHandle::new(
                        attachment.attachment_id().clone(),
                        content.identity().generation(),
                    )),
                    endpoint_handles,
                )
                .expect("fixture network status should validate")
            });
            WorkloadProviderObservation::Present(
                SandboxInspection::provider_authenticated_running_with_network_status(
                    SandboxHandle::new(
                        request.key().tenant_id().clone(),
                        SandboxId::new(request.execution().execution_id().as_str()),
                        spec.display_name(),
                        spec.backend,
                        SandboxStatus::Ready,
                        visible_endpoints,
                    ),
                    network_status,
                    SandboxExecutionAttemptId::new(request.execution().attempt_id().to_string())
                        .expect("fixture attempt should validate"),
                    b"compose-retirement-provider",
                ),
            )
        })
    }
}

impl WorkloadIngressObservationCapability for RecordingComposeProvider {
    fn observe<'a>(
        &'a self,
        _request: &'a WorkloadIngressObservationRequest,
    ) -> WorkloadIngressObservationFuture<'a> {
        Box::pin(async { WorkloadProviderObservation::Ambiguous })
    }
}

macro_rules! teardown_capability {
    ($trait_name:ident) => {
        impl $trait_name for RecordingComposeProvider {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadTeardownCommand,
            ) -> WorkloadTeardownCapabilityFuture<'a> {
                Box::pin(self.teardown_outcome(command))
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadTeardownCommand,
            ) -> WorkloadTeardownCapabilityFuture<'a> {
                Box::pin(self.teardown_outcome(command))
            }
        }
    };
}

teardown_capability!(FinalIngressWithdrawalCapability);
teardown_capability!(WorkloadExecutionDrainCapability);
teardown_capability!(WorkloadExecutionStopCapability);
teardown_capability!(NetworkDetachmentCapability);
teardown_capability!(NetworkReleaseCapability);

#[derive(Clone)]
struct StaticCatalog {
    definitions: Arc<BTreeMap<String, ServiceDefinition>>,
}

impl ServiceDefinitionCatalog for StaticCatalog {
    fn service_definition_for_tenant(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<ServiceDefinition> {
        self.definitions
            .get(service_name)
            .filter(|definition| &definition.tenant_id == tenant_id)
            .cloned()
    }

    fn service_definitions_for_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> BTreeMap<String, ServiceDefinition> {
        self.definitions
            .iter()
            .filter(|(_, definition)| &definition.tenant_id == tenant_id)
            .map(|(name, definition)| (name.clone(), definition.clone()))
            .collect()
    }
}

struct Fixture {
    root: PathBuf,
    compose_path: PathBuf,
    control_data_dir: PathBuf,
    selection: ResolvedComposeSelection,
    engine: Arc<Engine>,
    manager: Arc<nimbus_network::LocalNetworkManager>,
    services: Arc<ServiceManager>,
    provider: Arc<RecordingComposeProvider>,
    tenant_id: TenantId,
    backend: SandboxBackendKind,
    capability_selection: NetworkCapabilitySelection,
}

impl Fixture {
    fn open(
        root: &Path,
        backend: SandboxBackendKind,
        compose_services: &[&str],
        catalog_services: &[&str],
    ) -> Self {
        fs::create_dir_all(root).expect("fixture root should exist");
        let compose_path = root.join("compose.yaml");
        write_compose(&compose_path, backend, compose_services);
        let selection = ResolvedComposeSelection::explicit(compose_path.clone());
        let control_data_dir = root.join("control");
        let context = crate::compose::load_compose_project_context_for_selection(
            &selection,
            &control_data_dir,
        )
        .expect("fixture Compose context should load");
        let tenant_id = context.control_plane.local_tenant_id;
        let definitions = definitions(&tenant_id, backend, catalog_services, false);
        let services = Arc::new(ServiceManager::new(
            Arc::new(StaticCatalog {
                definitions: Arc::new(definitions),
            }),
            backend,
        ));
        let engine =
            Arc::new(Engine::new(root.join("engine")).expect("fixture Engine should open"));
        let (registry, capability_selection) = provider_realm(backend, true);
        let manager = nimbus_network::LocalNetworkManager::open(root.join("network"), registry)
            .expect("fixture network realm should open");
        let provider = RecordingComposeProvider::new(Arc::clone(&engine), root.join("provider"));
        Self {
            root: root.to_path_buf(),
            compose_path,
            control_data_dir,
            selection,
            engine,
            manager,
            services,
            provider,
            tenant_id,
            backend,
            capability_selection,
        }
    }

    fn composition(&self) -> ServerWorkloadComposition {
        self.composition_with_services_and_teardown(
            Arc::clone(&self.services),
            Arc::clone(&self.provider),
            true,
        )
    }

    fn composition_with_services(
        &self,
        services: Arc<ServiceManager>,
        provider: Arc<RecordingComposeProvider>,
    ) -> ServerWorkloadComposition {
        self.composition_with_services_and_teardown(services, provider, true)
    }

    fn composition_without_teardown_error(&self) -> nimbus_server::ServerWorkloadCompositionError {
        let attachment_provider = self.capability_selection.attachment_provider_id().clone();
        let ingress_provider = self.capability_selection.ingress_provider_id().clone();
        let execution_provider = sandbox_execution_provider_id(self.backend);
        let result = ServerWorkloadComposition::new(
            Arc::clone(&self.engine),
            Arc::clone(&self.manager),
            Arc::clone(&self.services),
            nimbus_workloads::NodeIdentity::new("compose-retirement-node")
                .expect("fixture node should validate"),
            self.capability_selection.clone(),
            NetworkSovereigntyRequirements::new(
                NetworkControlPlaneLocality::LocalOnly,
                BTreeSet::new(),
                true,
            ),
            ServerWorkloadProviders::new(
                attachment_provider,
                Arc::clone(&self.provider),
                execution_provider,
                Arc::clone(&self.provider),
                ingress_provider,
                Arc::clone(&self.provider),
            ),
        );
        match result {
            Ok(_) => panic!("incomplete teardown composition must fail closed"),
            Err(error) => error,
        }
    }

    fn composition_with_services_and_teardown(
        &self,
        services: Arc<ServiceManager>,
        provider: Arc<RecordingComposeProvider>,
        include_teardown: bool,
    ) -> ServerWorkloadComposition {
        let attachment_provider = self.capability_selection.attachment_provider_id().clone();
        let ingress_provider = self.capability_selection.ingress_provider_id().clone();
        let execution_provider = sandbox_execution_provider_id(self.backend);
        let providers = ServerWorkloadProviders::new(
            attachment_provider.clone(),
            provider.clone(),
            execution_provider.clone(),
            provider.clone(),
            ingress_provider.clone(),
            provider.clone(),
        );
        let providers = if include_teardown {
            let teardown = WorkloadTeardownCapabilityRegistry::new(
                [NetworkAttachmentTeardownCapabilities::new(
                    attachment_provider,
                    provider.clone(),
                    provider.clone(),
                )],
                [WorkloadExecutionTeardownCapabilities::new(
                    execution_provider,
                    provider.clone(),
                    provider.clone(),
                )],
                [IngressTeardownCapabilities::new(
                    ingress_provider,
                    provider.clone(),
                )],
            )
            .expect("fixture teardown registry should validate");
            providers.with_teardown_capabilities(teardown)
        } else {
            providers
        };
        ServerWorkloadComposition::new(
            Arc::clone(&self.engine),
            Arc::clone(&self.manager),
            services,
            nimbus_workloads::NodeIdentity::new("compose-retirement-node")
                .expect("fixture node should validate"),
            self.capability_selection.clone(),
            NetworkSovereigntyRequirements::new(
                NetworkControlPlaneLocality::LocalOnly,
                BTreeSet::new(),
                true,
            ),
            providers,
        )
        .expect("fixture workload composition should validate")
    }

    async fn provision(&self, service_names: &[&str]) -> BTreeMap<String, WorkloadSagaRecord> {
        let store = Arc::new(EngineWorkloadSagaStore::new(Arc::clone(&self.engine)));
        let runtime = self
            .composition()
            .into_foreground_runtime(store)
            .await
            .expect("foreground startup recovery should complete");
        let context = TenantIsolationContext::system(self.tenant_id.clone(), "compose-test-up");
        for service_name in service_names {
            runtime
                .resource_provisioner()
                .provision_sandbox_service(
                    &context,
                    service_name,
                    &nimbus_compute::WorkloadProvisionCancellation::default(),
                )
                .await
                .unwrap_or_else(|error| panic!("{service_name} should provision: {error}"));
        }
        drop(runtime);
        self.provider.reset_calls();
        let store = EngineWorkloadSagaStore::new(Arc::clone(&self.engine));
        let mut records = BTreeMap::new();
        for service_name in service_names {
            let key = workload_key(&self.tenant_id, service_name);
            records.insert(
                (*service_name).to_owned(),
                store
                    .load(&key)
                    .await
                    .expect("fixture saga should load")
                    .expect("provisioned saga should exist"),
            );
        }
        records
    }

    async fn retire(
        &self,
        selected: Option<&str>,
    ) -> Result<ComposeRetirementReport, ComposeRetirementError> {
        retire_compose_services(
            &self.command(selected),
            &self.selection,
            &self.control_data_dir,
            PreparedComposeProvision::TestComposition(Box::new(self.composition())),
            Arc::clone(&self.engine),
        )
        .await
    }

    fn command(&self, selected: Option<&str>) -> ComposeDownCommand {
        ComposeDownCommand {
            service: selected.map(str::to_owned),
            file: vec![self.compose_path.clone()],
            tenant: None,
        }
    }

    async fn record(&self, service_name: &str) -> WorkloadSagaRecord {
        EngineWorkloadSagaStore::new(Arc::clone(&self.engine))
            .load(&workload_key(&self.tenant_id, service_name))
            .await
            .expect("fixture saga should load")
            .expect("fixture saga should exist")
    }

    fn stale_services(&self, service_names: &[&str]) -> Arc<ServiceManager> {
        Arc::new(ServiceManager::new(
            Arc::new(StaticCatalog {
                definitions: Arc::new(definitions(
                    &self.tenant_id,
                    self.backend,
                    service_names,
                    true,
                )),
            }),
            self.backend,
        ))
    }
}

struct AmbiguousOnceStore {
    inner: EngineWorkloadSagaStore,
    fail_next_cas: AtomicBool,
}

impl AmbiguousOnceStore {
    fn new(engine: Arc<Engine>) -> Arc<Self> {
        Arc::new(Self {
            inner: EngineWorkloadSagaStore::new(engine),
            fail_next_cas: AtomicBool::new(true),
        })
    }
}

impl WorkloadSagaStore for AmbiguousOnceStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        self.inner.load(key)
    }

    fn compare_and_swap<'a>(
        &'a self,
        expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            if self.fail_next_cas.swap(false, Ordering::AcqRel) {
                return Err(WorkloadSagaStoreError::Ambiguous);
            }
            self.inner.compare_and_swap(expected, next).await
        })
    }

    fn list_recoverable<'a>(
        &'a self,
        request: WorkloadSagaPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaPage> {
        self.inner.list_recoverable(request)
    }

    fn list_restart_candidates<'a>(
        &'a self,
        request: nimbus_workloads::WorkloadRestartCandidatePageRequest,
    ) -> nimbus_workloads::WorkloadSagaFuture<'a, nimbus_workloads::WorkloadRestartCandidatePage>
    {
        self.inner.list_restart_candidates(request)
    }

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        self.inner.list_for_tenant(tenant_id, request)
    }
}

impl nimbus_workloads::TenantRetirementStore for AmbiguousOnceStore {
    fn load_retirement<'a>(
        &'a self,
        tenant_id: &'a TenantId,
    ) -> nimbus_workloads::TenantRetirementFuture<
        'a,
        Option<nimbus_workloads::TenantRetirementRecord>,
    > {
        self.inner.load_retirement(tenant_id)
    }

    fn compare_and_swap_retirement<'a>(
        &'a self,
        expected: nimbus_workloads::TenantRetirementExpected,
        next: nimbus_workloads::TenantRetirementRecord,
    ) -> nimbus_workloads::TenantRetirementFuture<'a, nimbus_workloads::TenantRetirementCommit>
    {
        self.inner.compare_and_swap_retirement(expected, next)
    }

    fn delete_retirement<'a>(
        &'a self,
        expected: nimbus_workloads::TenantRetirementRecord,
    ) -> nimbus_workloads::TenantRetirementFuture<'a, nimbus_workloads::TenantRetirementCommit>
    {
        self.inner.delete_retirement(expected)
    }

    fn list_active_retirements<'a>(
        &'a self,
        request: nimbus_workloads::TenantRetirementPageRequest,
    ) -> nimbus_workloads::TenantRetirementFuture<'a, nimbus_workloads::TenantRetirementPage> {
        self.inner.list_active_retirements(request)
    }

    fn list_retirements<'a>(
        &'a self,
        request: nimbus_workloads::TenantRetirementPageRequest,
    ) -> nimbus_workloads::TenantRetirementFuture<'a, nimbus_workloads::TenantRetirementPage> {
        self.inner.list_retirements(request)
    }

    fn load_workload_mutation_epoch<'a>(
        &'a self,
        tenant_id: &'a TenantId,
    ) -> nimbus_workloads::TenantRetirementFuture<'a, nimbus_workloads::TenantWorkloadMutationEpoch>
    {
        self.inner.load_workload_mutation_epoch(tenant_id)
    }
}

#[test]
#[serial_test::serial]
fn compose_down_local_uses_engine_saga_and_compute_teardown() {
    run_async(async move {
        let temp = TempDir::new().expect("fixture root");
        let fixture = Fixture::open(temp.path(), SandboxBackendKind::Krun, &["db"], &["db"]);
        let running = fixture.provision(&["db"]).await.remove("db").unwrap();

        let report = fixture
            .retire(Some("db"))
            .await
            .expect("down should converge");
        let outcome = &report.outcomes()[0];
        assert_eq!(
            outcome.disposition(),
            ComposeServiceRetirementDisposition::Recorded
        );
        assert_eq!(outcome.service_name(), "db");
        assert_eq!(
            outcome.terminal_execution_reference(),
            Some(&running.current_execution_reference())
        );
        assert_complete_execute_order(&fixture.provider.calls(), "db");
        let recorded = fixture.record("db").await;
        assert_eq!(recorded.phase(), WorkloadSagaPhase::Recorded);
        assert!(
            recorded.revision().as_u64() >= running.revision().as_u64() + 10,
            "five effect claims and five confirmed results must be durable"
        );
        assert!(
            fixture
                .provider
                .calls()
                .iter()
                .all(|call| call.durable_before_effect)
        );

        let command_source = include_str!("../mod.rs");
        let engine_position = command_source
            .find("Engine::new_with_persistence_config")
            .expect("down must open the canonical Engine");
        let retirement_position = command_source
            .find("retirement::retire_compose_services")
            .expect("down must call the compute retirement composition");
        let quiesce_position = command_source
            .find("engine.quiesce().await")
            .expect("down must settle Engine ownership");
        assert!(engine_position < retirement_position && retirement_position < quiesce_position);
    });
}

#[test]
#[serial_test::serial]
fn compose_down_forwarded_uses_engine_saga_and_exact_machine_phases() {
    run_async(async move {
        let temp = TempDir::new().expect("fixture root");
        let fixture = Fixture::open(temp.path(), SandboxBackendKind::Container, &["db"], &["db"]);
        fixture.provision(&["db"]).await;

        fixture
            .retire(Some("db"))
            .await
            .expect("forwarded down should converge");
        assert_complete_execute_order(&fixture.provider.calls(), "db");
        let profile = include_str!("../../network_composition/forwarded/profile.rs");
        let guest_route = include_str!("../../machine/api/routes.rs");
        assert!(profile.contains(".teardown_capabilities()"));
        assert!(profile.contains("WorkloadTeardownCapabilityRegistry::new"));
        assert!(guest_route.contains("MACHINE_API_WORKLOAD_TEARDOWN_PHASE_PATH"));
        assert!(!guest_route.contains("service_sandbox_stop"));
    });
}

#[test]
#[serial_test::serial]
fn compose_down_unresolved_submission_makes_zero_provider_calls() {
    run_async(async move {
        let temp = TempDir::new().expect("fixture root");
        let fixture = Fixture::open(temp.path(), SandboxBackendKind::Krun, &["db"], &["db"]);
        fixture.provision(&["db"]).await;
        let missing_capability = fixture.composition_without_teardown_error();
        assert!(matches!(
            missing_capability,
            nimbus_server::ServerWorkloadCompositionError::MissingExactTeardownCapabilityRealm
        ));
        assert!(fixture.provider.calls().is_empty());

        let faulting_store = AmbiguousOnceStore::new(Arc::clone(&fixture.engine));
        let runtime = fixture
            .composition()
            .into_foreground_runtime(faulting_store)
            .await
            .expect("foreground startup recovery should complete");

        let error = retire_compose_services(
            &fixture.command(Some("db")),
            &fixture.selection,
            &fixture.control_data_dir,
            PreparedComposeProvision::TestRuntime(Box::new(runtime)),
            Arc::clone(&fixture.engine),
        )
        .await
        .expect_err("ambiguous stopped-successor submission must fail closed");
        assert_eq!(error.failed_service(), Some("db"));
        let error = error.into_nimbus_error();
        assert!(matches!(
            error,
            Error::Internal(message)
                if message.contains("failed at service db")
                    && message.contains("completed=[]")
                    && message.contains("unissued services retain their durable source authority")
        ));
        assert!(fixture.provider.calls().is_empty());

        fixture
            .retire(Some("db"))
            .await
            .expect("exact retry should converge");
        assert_complete_execute_order(&fixture.provider.calls(), "db");
    });
}

#[test]
#[serial_test::serial]
fn compose_down_replay_is_idempotent_and_reports_durable_outcome() {
    run_async(async move {
        let temp = TempDir::new().expect("fixture root");
        let fixture = Fixture::open(
            temp.path(),
            SandboxBackendKind::Krun,
            &["cache", "db"],
            &["cache", "db"],
        );
        fixture.provision(&["db"]).await;
        let first = fixture.retire(Some("db")).await.expect("first down");
        let first_calls = fixture.provider.calls();
        let second = fixture.retire(Some("db")).await.expect("replayed down");

        assert_eq!(first.outcomes(), second.outcomes());
        assert_eq!(fixture.provider.calls(), first_calls);
        assert_eq!(
            second.outcomes()[0].disposition(),
            ComposeServiceRetirementDisposition::Recorded
        );
        assert!(
            second.outcomes()[0]
                .terminal_execution_reference()
                .is_some()
        );

        let recorded_calls = fixture.provider.calls();
        let source_only = fixture
            .retire(Some("cache"))
            .await
            .expect("source-only down should finalize without execution");
        assert_eq!(
            source_only.outcomes()[0].disposition(),
            ComposeServiceRetirementDisposition::SourceFinalized
        );
        assert!(
            source_only.outcomes()[0]
                .terminal_execution_reference()
                .is_none()
        );
        assert_eq!(fixture.provider.calls(), recorded_calls);
    });
}

#[test]
#[serial_test::serial]
fn compose_down_crossed_or_stale_identity_fails_before_provider_effects() {
    run_async(async move {
        let temp = TempDir::new().expect("fixture root");
        let fixture = Fixture::open(temp.path(), SandboxBackendKind::Krun, &["db"], &["db"]);
        fixture.provision(&["db"]).await;
        let stale_services = fixture.stale_services(&["db"]);
        let prepared = PreparedComposeProvision::TestComposition(Box::new(
            fixture.composition_with_services(stale_services, Arc::clone(&fixture.provider)),
        ));

        let error = retire_compose_services(
            &fixture.command(Some("db")),
            &fixture.selection,
            &fixture.control_data_dir,
            prepared,
            Arc::clone(&fixture.engine),
        )
        .await
        .expect_err("crossed source generation must fail closed");
        assert_eq!(error.failed_service(), Some("db"));
        assert!(fixture.provider.calls().is_empty());
        assert_eq!(
            fixture.record("db").await.phase(),
            WorkloadSagaPhase::Observed
        );
    });
}

#[test]
#[serial_test::serial]
fn compose_down_ambiguous_result_reopens_with_inspection_only() {
    run_async(async move {
        let temp = TempDir::new().expect("fixture root");
        let fixture = Fixture::open(temp.path(), SandboxBackendKind::Krun, &["db"], &["db"]);
        fixture.provision(&["db"]).await;
        fixture
            .provider
            .ambiguous_once_at(WorkloadTeardownStep::StopExecution);

        fixture
            .retire(Some("db"))
            .await
            .expect("ambiguous down should inspect and converge");
        let stop_calls = fixture
            .provider
            .calls()
            .into_iter()
            .filter(|call| call.step == WorkloadTeardownStep::StopExecution)
            .collect::<Vec<_>>();
        assert_eq!(stop_calls.len(), 2);
        assert_eq!(stop_calls[0].mode, WorkloadTeardownCommandMode::Execute);
        assert_eq!(stop_calls[1].mode, WorkloadTeardownCommandMode::Inspect);
        assert_eq!(stop_calls[0].attempt_id, stop_calls[1].attempt_id);
        assert_eq!(stop_calls[0].dispatch_epoch, stop_calls[1].dispatch_epoch);
        assert_eq!(effect_markers(&fixture.root).len(), 5);
    });
}

#[test]
#[serial_test::serial]
fn compose_down_process_reopen_resumes_same_attempt_without_duplicate_effect() {
    if std::env::var_os(PROCESS_CHILD_ENV).is_some() {
        run_process_child();
        return;
    }

    let temp = TempDir::new().expect("fixture root");
    run_async(async {
        let fixture = Fixture::open(temp.path(), SandboxBackendKind::Krun, &["db"], &["db"]);
        fixture.provision(&["db"]).await;
        fixture.engine.quiesce().await;
    });

    let crashed = spawn_process_child(temp.path(), true);
    assert!(
        !crashed.status.success(),
        "first child must crash after the exact effect"
    );
    let resumed = spawn_process_child(temp.path(), false);
    assert!(
        resumed.status.success(),
        "fresh child must resume from Engine and provider roots: stdout={} stderr={}",
        String::from_utf8_lossy(&resumed.stdout),
        String::from_utf8_lossy(&resumed.stderr)
    );

    let lines = fs::read_to_string(temp.path().join("provider/calls.log"))
        .expect("provider process log should exist");
    let stop = lines
        .lines()
        .filter(|line| line.contains("|stop_execution|"))
        .collect::<Vec<_>>();
    assert_eq!(stop.len(), 2, "{lines}");
    let first = stop[0].split('|').collect::<Vec<_>>();
    let second = stop[1].split('|').collect::<Vec<_>>();
    assert_eq!(first[2], "Execute");
    assert_eq!(second[2], "Inspect");
    assert_eq!(
        first[3], second[3],
        "fresh process must retain the same attempt"
    );
    assert_eq!(effect_markers(temp.path()).len(), 5);

    run_async(async {
        let engine =
            Arc::new(Engine::new(temp.path().join("engine")).expect("Engine should reopen"));
        let selection = ResolvedComposeSelection::explicit(temp.path().join("compose.yaml"));
        let context = crate::compose::load_compose_project_context_for_selection(
            &selection,
            &temp.path().join("control"),
        )
        .expect("Compose context should reopen");
        let record = EngineWorkloadSagaStore::new(engine)
            .load(&workload_key(&context.control_plane.local_tenant_id, "db"))
            .await
            .expect("record should load")
            .expect("record should remain durable");
        assert_eq!(record.phase(), WorkloadSagaPhase::Recorded);
    });
}

#[test]
#[serial_test::serial]
fn compose_down_partial_sibling_failure_preserves_completed_and_unissued_services() {
    run_async(async move {
        let temp = TempDir::new().expect("fixture root");
        let fixture = Fixture::open(
            temp.path(),
            SandboxBackendKind::Krun,
            &["cache", "db", "worker"],
            &["cache", "worker"],
        );
        fixture.provision(&["cache"]).await;

        let error = fixture
            .retire(None)
            .await
            .expect_err("missing db source must stop the batch");
        assert_eq!(error.failed_service(), Some("db"));
        assert_eq!(error.completed().len(), 1);
        assert_eq!(error.completed()[0].service_name(), "cache");
        assert_eq!(
            error.completed()[0].disposition(),
            ComposeServiceRetirementDisposition::Recorded
        );
        assert!(
            fixture
                .services
                .service_definition_for_tenant(&fixture.tenant_id, "worker")
                .is_some(),
            "unissued sibling source authority must remain present"
        );
        assert!(
            fixture
                .provider
                .calls()
                .iter()
                .all(|call| call.service == "cache")
        );
        let error = error.into_nimbus_error();
        assert!(matches!(
            error,
            Error::NotFound(message)
                if message.contains("failed at service db")
                    && message.contains("completed=[cache:recorded]")
                    && message.contains("unissued services retain their durable source authority")
        ));
        assert_eq!(
            fixture.record("cache").await.phase(),
            WorkloadSagaPhase::Recorded
        );
    });
}

#[test]
#[serial_test::serial]
fn compose_down_cancellation_after_submission_is_replayable() {
    run_async(async move {
        let temp = TempDir::new().expect("fixture root");
        let fixture = Fixture::open(temp.path(), SandboxBackendKind::Krun, &["db"], &["db"]);
        fixture.provision(&["db"]).await;
        let (entered, release) = fixture
            .provider
            .install_gate(WorkloadTeardownStep::StopExecution);
        let command = fixture.command(Some("db"));
        let selection = fixture.selection.clone();
        let control = fixture.control_data_dir.clone();
        let prepared = PreparedComposeProvision::TestComposition(Box::new(fixture.composition()));
        let engine = Arc::clone(&fixture.engine);
        let task = tokio::spawn(async move {
            retire_compose_services(&command, &selection, &control, prepared, engine).await
        });
        entered
            .acquire()
            .await
            .expect("stop gate should be entered")
            .forget();
        task.abort();
        assert!(
            task.await
                .expect_err("caller should be cancelled")
                .is_cancelled()
        );
        release.add_permits(1);

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if fixture.record("db").await.phase() == WorkloadSagaPhase::Recorded {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "retained teardown must finish after cancellation"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let calls = fixture.provider.calls();
        assert_complete_execute_order(&calls, "db");
        fixture
            .retire(Some("db"))
            .await
            .expect("replay should return durable truth");
        assert_eq!(
            fixture.provider.calls(),
            calls,
            "replay must not duplicate effects"
        );
    });
}

fn run_process_child() {
    let root = PathBuf::from(std::env::var_os(PROCESS_ROOT_ENV).expect("child root"));
    run_async(async {
        let fixture = Fixture::open(&root, SandboxBackendKind::Krun, &["db"], &["db"]);
        fixture
            .retire(Some("db"))
            .await
            .expect("process child down should converge");
        fixture.engine.quiesce().await;
    });
}

fn spawn_process_child(root: &Path, crash: bool) -> std::process::Output {
    let mut command =
        Command::new(std::env::current_exe().expect("test executable should resolve"));
    command
        .arg("--exact")
        .arg(PROCESS_TEST_NAME)
        .arg("--nocapture")
        .arg("--test-threads=1")
        .env(PROCESS_CHILD_ENV, "1")
        .env(PROCESS_ROOT_ENV, root);
    if crash {
        command.env(
            PROCESS_CRASH_ENV,
            step_slug(WorkloadTeardownStep::StopExecution),
        );
    }
    command.output().expect("fresh test process should launch")
}

fn process_should_crash_at(step: WorkloadTeardownStep) -> bool {
    std::env::var(PROCESS_CRASH_ENV)
        .ok()
        .is_some_and(|configured| configured == step_slug(step))
}

fn run_async(future: impl std::future::Future<Output = ()> + Send) {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("compose-retirement-fixture".to_owned())
            .stack_size(16 * 1024 * 1024)
            .spawn_scoped(scope, move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("fixture runtime should build")
                    .block_on(future);
            })
            .expect("fixture thread should spawn")
            .join()
            .expect("fixture thread should not panic");
    });
}

fn workload_key(tenant_id: &TenantId, service_name: &str) -> WorkloadSagaKey {
    WorkloadSagaKey::new(
        tenant_id.clone(),
        nimbus_core::WorkloadId::new(service_name).expect("service identity should validate"),
    )
}

fn definitions(
    tenant_id: &TenantId,
    backend: SandboxBackendKind,
    service_names: &[&str],
    stale: bool,
) -> BTreeMap<String, ServiceDefinition> {
    service_names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let spec = SandboxSpec::new(
                tenant_id.clone(),
                SandboxOwnerSpec::service(*name),
                backend,
                SandboxRootSpec::rootfs(format!(
                    "/fixture/{}{}",
                    name,
                    if stale { "-stale" } else { "" }
                )),
                SandboxProcessSpec::new(["/bin/service"]),
            )
            .with_port_bindings([SandboxPortBinding::tcp(
                "http",
                18_080 + u16::try_from(index).expect("fixture port offset"),
                8_080,
            )]);
            (
                (*name).to_owned(),
                ServiceDefinition::static_catalog(
                    tenant_id.clone(),
                    *name,
                    ServiceBackend::sandbox(spec),
                ),
            )
        })
        .collect()
}

fn write_compose(path: &Path, backend: SandboxBackendKind, service_names: &[&str]) {
    let backend = match backend {
        SandboxBackendKind::Krun => "krun",
        SandboxBackendKind::Container => "container",
    };
    let mut body = "name: Retirement Test\nservices:\n".to_owned();
    for service_name in service_names {
        body.push_str(&format!(
            "  {service_name}:\n    image: busybox:latest\n    x_nimbus:\n      backend: {backend}\n"
        ));
    }
    fs::write(path, body).expect("Compose fixture should write");
}

fn provider_realm(
    backend: SandboxBackendKind,
    include_ipv6: bool,
) -> (NetworkCapabilityRegistry, NetworkCapabilitySelection) {
    let requirements = nimbus_sandbox::sandbox_network_plan_requirements(backend);
    let ingress_provider = NetworkProviderId::for_registration_key("compose-retirement-ingress");
    let lifecycle = NetworkLifecycleCapabilitySet::new([
        NetworkLifecycleFeature::DurableInspect,
        NetworkLifecycleFeature::Reconcile,
        NetworkLifecycleFeature::Delete,
    ]);
    let mut families = BTreeSet::from([NetworkAddressFamily::Ipv4]);
    if include_ipv6 {
        families.insert(NetworkAddressFamily::Ipv6);
    }
    let attachment = NetworkAttachmentProviderRegistration::new(
        requirements.required_attachment_provider_id().clone(),
        requirements.capability_requirements().attachment().clone(),
        families.clone(),
        lifecycle.clone(),
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let ingress = NetworkIngressProviderRegistration::new(
        ingress_provider.clone(),
        NetworkEndpointCapabilitySet::new(
            families,
            [NetworkBindRealmKind::Host],
            [NetworkExposure::Loopback, NetworkExposure::Private],
            [PortProtocol::Tcp],
            [
                NetworkPortAssignmentMode::Exact,
                NetworkPortAssignmentMode::ProviderAssigned,
            ],
        ),
        NetworkIngressCapabilitySet::new([]).with_tls_behaviors([NetworkTlsBehavior::Disabled]),
        NetworkForwardingCapabilitySet::new([NetworkForwardingFeature::PortForwarding]),
        lifecycle,
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

fn provision_success(
    step: WorkloadProvisionStep,
    subjects: &WorkloadProvisionSubjects,
) -> WorkloadProvisionSuccessEvidence {
    let evidence = WorkloadOwnerEvidenceDigest::sha256(format!("compose-provision-{step:?}"));
    match (step, subjects) {
        (WorkloadProvisionStep::ReserveNetwork, WorkloadProvisionSubjects::Network(reference)) => {
            WorkloadProvisionSuccessEvidence::NetworkReserved {
                reference: reference.clone(),
                evidence,
            }
        }
        (
            WorkloadProvisionStep::PrepareWorkload,
            WorkloadProvisionSubjects::Execution(reference),
        ) => WorkloadProvisionSuccessEvidence::WorkloadPrepared {
            reference: reference.clone(),
            evidence,
        },
        (WorkloadProvisionStep::AttachNetwork, WorkloadProvisionSubjects::Network(reference)) => {
            WorkloadProvisionSuccessEvidence::NetworkAttached {
                reference: reference.clone(),
                evidence,
            }
        }
        (
            WorkloadProvisionStep::InspectActivationPrerequisites,
            WorkloadProvisionSubjects::Readiness { network, execution },
        ) => WorkloadProvisionSuccessEvidence::ActivationPrerequisitesReady {
            network: network.clone(),
            execution: execution.clone(),
            evidence,
        },
        (
            WorkloadProvisionStep::ActivateWorkload,
            WorkloadProvisionSubjects::Execution(reference),
        ) => WorkloadProvisionSuccessEvidence::WorkloadActivated {
            reference: reference.clone(),
            evidence,
        },
        (
            WorkloadProvisionStep::InspectWorkloadReadiness,
            WorkloadProvisionSubjects::Readiness { network, execution },
        ) => WorkloadProvisionSuccessEvidence::WorkloadReady {
            network: network.clone(),
            execution: execution.clone(),
            evidence,
        },
        (WorkloadProvisionStep::Publish, WorkloadProvisionSubjects::Publication(reference)) => {
            WorkloadProvisionSuccessEvidence::Published {
                reference: reference.clone(),
                evidence,
            }
        }
        (
            WorkloadProvisionStep::ObservePublication,
            WorkloadProvisionSubjects::Publication(reference),
        ) => WorkloadProvisionSuccessEvidence::PublicationObserved {
            reference: reference.clone(),
            evidence,
        },
        _ => panic!("provision step and subjects must remain correlated"),
    }
}

fn teardown_success(
    step: WorkloadTeardownStep,
    subjects: &WorkloadTeardownSubjects,
) -> WorkloadTeardownSuccessEvidence {
    let evidence = WorkloadOwnerEvidenceDigest::sha256(format!("compose-teardown-{step:?}"));
    match (step, subjects) {
        (
            WorkloadTeardownStep::WithdrawPublication,
            WorkloadTeardownSubjects::Publication(reference),
        ) => WorkloadTeardownSuccessEvidence::PublicationAbsent {
            reference: reference.clone(),
            evidence,
        },
        (WorkloadTeardownStep::DrainExecution, WorkloadTeardownSubjects::Execution(reference)) => {
            WorkloadTeardownSuccessEvidence::ExecutionDrained {
                reference: reference.clone(),
                evidence,
            }
        }
        (WorkloadTeardownStep::StopExecution, WorkloadTeardownSubjects::Execution(reference)) => {
            WorkloadTeardownSuccessEvidence::ExecutionStopped {
                reference: reference.clone(),
                evidence,
            }
        }
        (WorkloadTeardownStep::DetachNetwork, WorkloadTeardownSubjects::Network(reference)) => {
            WorkloadTeardownSuccessEvidence::NetworkDetached {
                reference: reference.clone(),
                evidence,
            }
        }
        (WorkloadTeardownStep::ReleaseNetwork, WorkloadTeardownSubjects::Network(reference)) => {
            WorkloadTeardownSuccessEvidence::NetworkReleased {
                reference: reference.clone(),
                evidence,
            }
        }
        _ => panic!("teardown step and subjects must remain correlated"),
    }
}

fn assert_complete_execute_order(calls: &[ProviderCall], service: &str) {
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.service == service)
            .map(|call| (call.step, call.mode))
            .collect::<Vec<_>>(),
        [
            WorkloadTeardownStep::WithdrawPublication,
            WorkloadTeardownStep::DrainExecution,
            WorkloadTeardownStep::StopExecution,
            WorkloadTeardownStep::DetachNetwork,
            WorkloadTeardownStep::ReleaseNetwork,
        ]
        .map(|step| (step, WorkloadTeardownCommandMode::Execute))
    );
}

fn effect_markers(root: &Path) -> Vec<PathBuf> {
    let mut markers = fs::read_dir(root.join("provider/effects"))
        .expect("effect marker directory should exist")
        .map(|entry| entry.expect("effect marker should resolve").path())
        .collect::<Vec<_>>();
    markers.sort();
    markers
}

const fn step_slug(step: WorkloadTeardownStep) -> &'static str {
    match step {
        WorkloadTeardownStep::WithdrawPublication => "withdraw_publication",
        WorkloadTeardownStep::DrainExecution => "drain_execution",
        WorkloadTeardownStep::StopExecution => "stop_execution",
        WorkloadTeardownStep::DetachNetwork => "detach_network",
        WorkloadTeardownStep::ReleaseNetwork => "release_network",
    }
}
