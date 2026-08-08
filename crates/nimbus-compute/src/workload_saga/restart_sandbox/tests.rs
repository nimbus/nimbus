use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier, Mutex};

use nimbus_core::TenantId;
use nimbus_network::{
    NetworkAddressFamily, NetworkAttachmentProviderRegistration, NetworkBindRealmKind,
    NetworkCapabilityBundle, NetworkCapabilityRegistry, NetworkCapabilitySelection,
    NetworkControlPlaneLocality, NetworkEndpointCapabilitySet, NetworkExposure,
    NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet, NetworkLifecycleFeature,
    NetworkPortAssignmentMode, NetworkProviderId, NetworkSovereigntyCapabilities,
    NetworkSovereigntyRequirements, PortProtocol,
};
use nimbus_sandbox::backends::container::{ContainerSandboxBackendConfig, ContainerStartMode};
use nimbus_sandbox::backends::krun::{KrunSandboxBackendConfig, KrunStartMode};
use nimbus_sandbox::{
    SandboxBackendKind, SandboxOwnerSpec, SandboxProcessSpec, SandboxRootSpec,
    sandbox_network_plan_requirements,
};
use nimbus_tenant::{
    TenantIsolationContext, TenantIsolationPolicyInput, TenantServiceGrantPolicyDecision,
    WorkloadAttributes, WorkloadLocation,
};
use nimbus_workloads::{
    NodeIdentity, WorkloadActivationIntent, WorkloadPublicationIntent,
    WorkloadRestartCandidatePage, WorkloadRestartCandidatePageRequest,
    WorkloadRestartEvidenceDigest, WorkloadRestartNotBeforeUnixMillis, WorkloadRestartStep,
    WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaFuture, WorkloadSagaPage,
    WorkloadSagaPageRequest, WorkloadSagaPhase, WorkloadSagaRecord, WorkloadSagaStore,
    WorkloadSagaStoreError, WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest,
};
use serde_json::{Value, json};

use super::*;
use crate::workload_provision_composition::{
    WorkloadProvisionCompositionInput, WorkloadProvisionSourceSnapshot, compose_workload_provision,
};
use crate::workload_saga::{
    WorkloadRestartAdmissionDecision, WorkloadRestartAdmissionRequest, WorkloadRestartCommandMode,
    WorkloadRestartCommandOutcome, WorkloadRestartCommandResult, WorkloadRestartDecision,
    WorkloadRestartProviderObservation, WorkloadSagaCoordinator, apply_restart_result,
    decide_restart_admission, decide_restart_progress, test_support,
};

struct CommitScriptStore {
    commits: Mutex<VecDeque<WorkloadSagaCommit>>,
}

impl CommitScriptStore {
    fn new(commits: impl IntoIterator<Item = WorkloadSagaCommit>) -> Arc<Self> {
        Arc::new(Self {
            commits: Mutex::new(commits.into_iter().collect()),
        })
    }
}

impl WorkloadSagaStore for CommitScriptStore {
    fn load<'a>(
        &'a self,
        _key: &'a nimbus_workloads::WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async { Ok(None) })
    }

    fn compare_and_swap<'a>(
        &'a self,
        _expected: WorkloadSagaExpected,
        _next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            self.commits
                .lock()
                .expect("commit script lock should be healthy")
                .pop_front()
                .ok_or(WorkloadSagaStoreError::Corrupt)
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
        request: WorkloadRestartCandidatePageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadRestartCandidatePage> {
        Box::pin(async move { WorkloadRestartCandidatePage::new(&request, Vec::new(), false) })
    }

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        Box::pin(async move { WorkloadSagaTenantPage::new(tenant_id, &request, Vec::new(), false) })
    }
}

fn lifecycle() -> NetworkLifecycleCapabilitySet {
    NetworkLifecycleCapabilitySet::new([
        NetworkLifecycleFeature::DurableInspect,
        NetworkLifecycleFeature::Reconcile,
        NetworkLifecycleFeature::Delete,
    ])
}

fn provider_realm(
    backend: SandboxBackendKind,
    label: &str,
) -> (NetworkCapabilityRegistry, NetworkCapabilitySelection) {
    let requirements = sandbox_network_plan_requirements(backend);
    let ingress_provider =
        NetworkProviderId::for_registration_key(&format!("restart-{label}-ingress"));
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
            .expect("restart provider realm should validate"),
        selection,
    )
}

fn observed_record(backend: SandboxBackendKind, label: &str, rootfs: &Path) -> WorkloadSagaRecord {
    let tenant_id =
        TenantId::new(format!("tenant-{label}")).expect("fixture tenant should validate");
    let local_node =
        NodeIdentity::new(format!("node-{label}")).expect("fixture node should validate");
    let decision =
        TenantIsolationContext::system(tenant_id.clone(), "sandbox-restart-adapter-substitution")
            .with_deployment_generation(7)
            .with_workload_location(WorkloadLocation::new().with_node_id(local_node.as_str()))
            .admit_decision(
                TenantIsolationPolicyInput::new(
                    WorkloadAttributes::sandbox(label)
                        .with_sandbox_id(format!("sandbox-{label}"))
                        .with_sandbox_backend(backend),
                )
                .with_services(TenantServiceGrantPolicyDecision::new(
                    std::iter::empty::<String>(),
                )),
            )
            .expect("fixture isolation decision should admit");
    let spec = SandboxSpec::new(
        tenant_id,
        SandboxOwnerSpec::standalone_named(label),
        backend,
        SandboxRootSpec::rootfs(rootfs),
        SandboxProcessSpec::new(["/bin/true"]),
    );
    let (registry, selection) = provider_realm(backend, label);
    let source_version =
        nimbus_workloads::WorkloadProvisionSourceResourceVersion::new("restart-source-v1")
            .expect("fixture source version should validate");
    let execution_provider_id = sandbox_execution_provider_id(backend);
    let stable_resource_id = format!("sandbox-{label}");
    let composed = compose_workload_provision(WorkloadProvisionCompositionInput {
        decision: &decision,
        local_node: &local_node,
        source: WorkloadProvisionSourceSnapshot::StandaloneSandbox {
            stable_resource_id: &stable_resource_id,
            profile: label,
            source_generation: nimbus_workloads::WorkloadProvisionSourceGeneration::new(1),
            resource_version: &source_version,
            sandbox_spec: &spec,
        },
        execution_provider_id: &execution_provider_id,
        capability_selection: &selection,
        capability_registry: &registry,
        sovereignty: NetworkSovereigntyRequirements::new(
            NetworkControlPlaneLocality::LocalOnly,
            [],
            true,
        ),
        endpoint_semantics: &[],
        activation: WorkloadActivationIntent::ActivateWhenAttached,
        publication: WorkloadPublicationIntent::Withheld,
    })
    .expect("real sandbox restart source should compose");
    let (key, intent) = composed.into_parts();
    let mut record = WorkloadSagaRecord::new(key, intent).expect("fixture saga should validate");
    for _ in 0..16 {
        if record.phase() == WorkloadSagaPhase::Observed {
            return record;
        }
        record = test_support::confirmed_provision(&record);
    }
    panic!("restart fixture must reach observed provision state")
}

async fn commands_through(
    backend: SandboxBackendKind,
    label: &str,
    rootfs: &Path,
    target: WorkloadRestartStep,
    inspect_target: bool,
) -> Vec<ConfirmedWorkloadRestartCommand> {
    let observed = observed_record(backend, label, rootfs);
    let request = WorkloadRestartAdmissionRequest::for_explicit(
        &observed,
        &format!("restart-{label}"),
        WorkloadRestartNotBeforeUnixMillis::new(0),
    )
    .expect("explicit restart request should validate");
    let WorkloadRestartAdmissionDecision::Transition(admitted) =
        decide_restart_admission(&observed, &request).expect("restart should admit")
    else {
        panic!("new exact restart must transition");
    };
    let mut current = *admitted;
    let mut commands = Vec::new();

    for _ in 0..32 {
        let WorkloadRestartDecision::Proposed(proposed) =
            decide_restart_progress(&current, WorkloadRestartNotBeforeUnixMillis::new(0))
                .expect("restart state should reduce")
        else {
            panic!("restart fixture should propose its next durable transition");
        };
        if proposed.action_after_confirmation().is_none() {
            current = proposed.into_candidate();
            continue;
        }
        let claimed_step = proposed
            .candidate()
            .restart_state()
            .active()
            .and_then(|active| active.disposition().claim())
            .expect("effect proposal should retain an exact claim")
            .step();
        let store = if inspect_target && claimed_step == target {
            CommitScriptStore::new([WorkloadSagaCommit::Unchanged, WorkloadSagaCommit::Applied])
        } else {
            CommitScriptStore::new([WorkloadSagaCommit::Applied])
        };
        let confirmed = WorkloadSagaCoordinator::new(store)
            .claim_restart_command(&current, &proposed)
            .await
            .expect("fixture restart command should confirm");
        let durable = confirmed
            .confirmed_record()
            .expect("confirmed command should retain durable state")
            .clone();
        let command = confirmed
            .command()
            .expect("effect proposal should issue one command")
            .clone();
        assert_eq!(command.step(), claimed_step);
        commands.push(command.clone());
        if claimed_step == target {
            assert_eq!(
                command.mode(),
                if inspect_target {
                    WorkloadRestartCommandMode::Inspect
                } else {
                    WorkloadRestartCommandMode::Execute
                }
            );
            return commands;
        }

        let result = WorkloadRestartCommandResult::for_command(
            &command,
            WorkloadRestartCommandOutcome::Succeeded {
                evidence: WorkloadRestartEvidenceDigest::sha256(format!(
                    "fixture-success-{claimed_step:?}"
                )),
            },
        );
        let WorkloadRestartDecision::Proposed(completed) =
            apply_restart_result(&durable, &command, result)
                .expect("fixture command success should reduce")
        else {
            panic!("successful command should produce the next durable candidate");
        };
        current = completed.into_candidate();
    }
    panic!("restart command fixture exceeded its transition bound")
}

struct RestartCommands {
    quiescence: ConfirmedWorkloadRestartCommand,
    preparation: ConfirmedWorkloadRestartCommand,
}

async fn direct_restart_commands(
    backend: SandboxBackendKind,
    label: &str,
    rootfs: &Path,
) -> RestartCommands {
    let commands = commands_through(
        backend,
        label,
        rootfs,
        WorkloadRestartStep::PrepareExecution,
        false,
    )
    .await;
    let quiescence = commands
        .iter()
        .find(|command| command.step() == WorkloadRestartStep::QuiesceExecution)
        .expect("restart sequence should contain quiescence")
        .clone();
    let preparation = commands
        .into_iter()
        .find(|command| command.step() == WorkloadRestartStep::PrepareExecution)
        .expect("restart sequence should contain preparation");
    RestartCommands {
        quiescence,
        preparation,
    }
}

async fn inspection_quiescence_command(
    backend: SandboxBackendKind,
    label: &str,
    rootfs: &Path,
) -> ConfirmedWorkloadRestartCommand {
    commands_through(
        backend,
        label,
        rootfs,
        WorkloadRestartStep::QuiesceExecution,
        true,
    )
    .await
    .pop()
    .expect("inspection sequence should issue quiescence")
}

fn assert_exact_command(command: &ConfirmedWorkloadRestartCommand, backend: SandboxBackendKind) {
    assert_eq!(
        command.provider_selection(),
        &sandbox_execution_provider_id(backend)
    );
    assert_eq!(
        command.source().execution_provider_id(),
        command.provider_selection()
    );
    assert_eq!(
        command.source_execution().execution_id(),
        command.execution().execution_id()
    );
    assert_eq!(
        command.source_execution().workload_uid(),
        command.execution().workload_uid()
    );
    assert_eq!(
        command.source_execution().node_identity(),
        command.execution().node_identity()
    );
    assert_eq!(
        command.source_execution().generation(),
        command.execution().generation()
    );
    assert_eq!(
        command.source_execution().desired_digest(),
        command.execution().desired_digest()
    );
    assert_eq!(
        command.source_execution().restart_epoch().checked_next(),
        Some(command.execution().restart_epoch())
    );
    assert_eq!(command.restart_epoch(), command.execution().restart_epoch());
    assert_ne!(command.source_attempt_id(), command.attempt_id());
    assert_eq!(command.dispatch_epoch(), command.claim().dispatch_epoch());
    assert_eq!(command.inspection_version(), None);
    assert!(validate_sandbox_restart_command(command, backend).is_ok());
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RetainedAuthority {
    provision_network_plan: Value,
    network_config: Value,
    port_leases: Value,
    egress_proxy: Value,
}

impl RetainedAuthority {
    fn from_manifest(manifest: &Value) -> Self {
        Self {
            provision_network_plan: manifest["provision_network_plan"].clone(),
            network_config: manifest["network_config"].clone(),
            port_leases: manifest["port_leases"].clone(),
            egress_proxy: manifest["egress_proxy"].clone(),
        }
    }
}

struct ProviderFixture {
    manifest_path: PathBuf,
    runtime_marker: PathBuf,
    delete_log: PathBuf,
    retained: RetainedAuthority,
}

fn manifest_path_under(root: &Path) -> PathBuf {
    let mut pending = vec![root.to_path_buf()];
    let mut matches = Vec::new();
    while let Some(directory) = pending.pop() {
        let mut entries = fs::read_dir(&directory)
            .unwrap_or_else(|error| {
                panic!(
                    "fixture directory {} should read: {error}",
                    directory.display()
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .expect("fixture directory entries should read");
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if entry
                .file_type()
                .expect("fixture entry type should read")
                .is_dir()
            {
                pending.push(path);
            } else if entry.file_name() == "manifest.json" {
                matches.push(path);
            }
        }
    }
    assert_eq!(matches.len(), 1, "fixture should own one provider manifest");
    matches.pop().expect("one provider manifest should exist")
}

fn explicit_runtime_commands(
    sandbox_id: &str,
    runtime_marker: &Path,
    delete_log: &Path,
) -> (Value, Value) {
    let state = json!({
        "program": "/bin/sh",
        "args": [
            "-c",
            format!(
                "if [ -f \"$1\" ]; then printf '%s\\n' '{{\"id\":\"{sandbox_id}\",\"status\":\"running\"}}'; else printf '%s\\n' 'container `{sandbox_id}` does not exist: open `/run/crun/{sandbox_id}/status`: No such file or directory' >&2; exit 1; fi"
            ),
            "sh",
            runtime_marker.display().to_string(),
        ],
    });
    let delete = json!({
        "program": "/bin/sh",
        "args": [
            "-c",
            "rm -f \"$1\"; printf '%s\\n' delete >> \"$2\"",
            "sh",
            runtime_marker.display().to_string(),
            delete_log.display().to_string(),
        ],
    });
    (state, delete)
}

fn publish_manifest(path: &Path, manifest: &Value) {
    let mut bytes = serde_json::to_vec_pretty(manifest).expect("fixture manifest should encode");
    bytes.push(b'\n');
    fs::write(path, bytes).expect("fixture manifest should persist");
}

fn seed_container_provider(
    backend: &ContainerSandboxBackend,
    command: &ConfirmedWorkloadRestartCommand,
    root: &Path,
    runtime_present: bool,
) -> ProviderFixture {
    let Ok(validated) = validate_sandbox_restart_command(command, SandboxBackendKind::Container)
    else {
        panic!("container restart command should validate");
    };
    backend
        .reserve_provision_network(
            validated.spec().clone(),
            validated.sandbox_id().clone(),
            validated.attempt_fence().source_attempt_id().clone(),
            validated.network_plan().clone(),
        )
        .expect("container source network authority should reserve");
    let manifest_path = manifest_path_under(&root.join("state"));
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(&manifest_path).expect("container manifest should read"))
            .expect("container manifest should decode");
    assert_eq!(
        manifest["execution_attempt_id"],
        validated.attempt_fence().source_attempt_id().as_str()
    );
    let runtime_marker = root.join("container-runtime-present");
    let delete_log = root.join("container-runtime-delete.log");
    if runtime_present {
        fs::write(&runtime_marker, b"running\n").expect("runtime marker should persist");
    }
    let (state, delete) = explicit_runtime_commands(
        validated.sandbox_id().as_str(),
        &runtime_marker,
        &delete_log,
    );
    manifest["conmon_launch"]["state_command"] = state;
    manifest["conmon_launch"]["delete_command"] = delete;
    let retained = RetainedAuthority::from_manifest(&manifest);
    publish_manifest(&manifest_path, &manifest);
    ProviderFixture {
        manifest_path,
        runtime_marker,
        delete_log,
        retained,
    }
}

fn seed_krun_provider(
    backend: &KrunSandboxBackend,
    command: &ConfirmedWorkloadRestartCommand,
    root: &Path,
    runtime_present: bool,
) -> ProviderFixture {
    let Ok(validated) = validate_sandbox_restart_command(command, SandboxBackendKind::Krun) else {
        panic!("krun restart command should validate");
    };
    backend
        .reserve_provision_network(
            validated.spec().clone(),
            validated.sandbox_id().clone(),
            validated.attempt_fence().source_attempt_id().clone(),
            validated.network_plan().clone(),
        )
        .expect("krun source network authority should reserve");
    let manifest_path = manifest_path_under(&root.join("state"));
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(&manifest_path).expect("krun manifest should read"))
            .expect("krun manifest should decode");
    assert_eq!(
        manifest["execution_attempt_id"],
        validated.attempt_fence().source_attempt_id().as_str()
    );
    let runtime_marker = root.join("krun-runtime-present");
    let delete_log = root.join("krun-runtime-delete.log");
    if runtime_present {
        fs::write(&runtime_marker, b"running\n").expect("runtime marker should persist");
    }
    let (state, delete) = explicit_runtime_commands(
        validated.sandbox_id().as_str(),
        &runtime_marker,
        &delete_log,
    );
    manifest["conmon_launch"]["state_command"] = state;
    manifest["conmon_launch"]["delete_command"] = delete;
    manifest["launch_authority"] = json!({ "phase": "provider_owned" });
    manifest["creator_handoff"] = json!({
        "phase": "quiesced",
        "proof": {
            "kind": "never_spawned",
            "attempt_id": format!("quiesced-source:{}", command.source_attempt_id()),
        },
    });
    let retained = RetainedAuthority::from_manifest(&manifest);
    publish_manifest(&manifest_path, &manifest);
    ProviderFixture {
        manifest_path,
        runtime_marker,
        delete_log,
        retained,
    }
}

fn read_manifest(fixture: &ProviderFixture) -> Value {
    serde_json::from_slice(
        &fs::read(&fixture.manifest_path).expect("provider manifest should remain readable"),
    )
    .expect("provider manifest should remain valid")
}

fn assert_succeeded(observation: WorkloadRestartProviderObservation) {
    assert!(matches!(
        observation.into_outcome(),
        WorkloadRestartCommandOutcome::Succeeded { .. }
    ));
}

fn assert_definite_failure(label: &str, observation: WorkloadRestartProviderObservation) {
    let outcome = observation.into_outcome();
    assert!(
        matches!(
            outcome,
            WorkloadRestartCommandOutcome::DefiniteFailure { .. }
        ),
        "{label} should fail closed, observed {outcome:?}"
    );
}

fn delete_count(path: &Path) -> usize {
    fs::read_to_string(path)
        .map(|contents| contents.lines().count())
        .unwrap_or(0)
}

#[tokio::test]
async fn container_restart_quiescence_capability_authenticates_command() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let rootfs = root.path().join("rootfs");
    fs::create_dir(&rootfs).expect("fixture rootfs should exist");
    let commands = direct_restart_commands(
        SandboxBackendKind::Container,
        "container-quiescence",
        &rootfs,
    )
    .await;
    assert_exact_command(&commands.quiescence, SandboxBackendKind::Container);
    let mut config = ContainerSandboxBackendConfig::under_root(root.path());
    config.start_mode = ContainerStartMode::Execute;
    let backend = Arc::new(ContainerSandboxBackend::new(config));
    let fixture =
        seed_container_provider(backend.as_ref(), &commands.quiescence, root.path(), false);
    let source_manifest = read_manifest(&fixture);
    let adapter = ContainerProvisionAdapter::new(backend.clone())
        .expect("container restart journal should open");

    assert_succeeded(
        WorkloadExecutionQuiescenceCapability::execute(&adapter, &commands.quiescence).await,
    );
    assert_eq!(delete_count(&fixture.delete_log), 1);
    assert!(!fixture.runtime_marker.exists());
    let quiesced = read_manifest(&fixture);
    assert_eq!(
        quiesced["execution_attempt_id"],
        commands.quiescence.source_attempt_id().as_str()
    );
    assert_eq!(quiesced["restart_transition"]["phase"], "source_quiesced");
    assert_eq!(
        RetainedAuthority::from_manifest(&quiesced),
        fixture.retained
    );
    assert_eq!(
        source_manifest["network_config"], quiesced["network_config"],
        "quiescence must not release or replace retained network authority"
    );
}

#[tokio::test]
async fn container_restart_preparation_retains_authority_and_binds_attempt() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let rootfs = root.path().join("rootfs");
    fs::create_dir(&rootfs).expect("fixture rootfs should exist");
    let commands = direct_restart_commands(
        SandboxBackendKind::Container,
        "container-preparation",
        &rootfs,
    )
    .await;
    assert_exact_command(&commands.preparation, SandboxBackendKind::Container);
    let mut config = ContainerSandboxBackendConfig::under_root(root.path());
    config.start_mode = ContainerStartMode::Execute;
    let backend = Arc::new(ContainerSandboxBackend::new(config));
    let fixture =
        seed_container_provider(backend.as_ref(), &commands.quiescence, root.path(), false);
    let adapter = ContainerProvisionAdapter::new(backend.clone())
        .expect("container restart journal should open");

    assert_succeeded(
        WorkloadExecutionQuiescenceCapability::execute(&adapter, &commands.quiescence).await,
    );
    assert_succeeded(
        WorkloadRestartPreparationCapability::execute(&adapter, &commands.preparation).await,
    );
    let prepared = read_manifest(&fixture);
    assert_eq!(
        prepared["execution_attempt_id"],
        commands.preparation.attempt_id().as_str()
    );
    assert_eq!(prepared["restart_transition"]["phase"], "target_prepared");
    assert_eq!(
        RetainedAuthority::from_manifest(&prepared),
        fixture.retained
    );
    assert_eq!(delete_count(&fixture.delete_log), 1);
}

#[tokio::test]
async fn krun_restart_quiescence_capability_authenticates_command() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let rootfs = root.path().join("rootfs");
    fs::create_dir(&rootfs).expect("fixture rootfs should exist");
    let commands =
        direct_restart_commands(SandboxBackendKind::Krun, "krun-quiescence", &rootfs).await;
    assert_exact_command(&commands.quiescence, SandboxBackendKind::Krun);
    let mut config = KrunSandboxBackendConfig::under_root(root.path());
    config.start_mode = KrunStartMode::Execute;
    let backend = Arc::new(KrunSandboxBackend::new(config));
    let fixture = seed_krun_provider(backend.as_ref(), &commands.quiescence, root.path(), false);
    let adapter =
        KrunProvisionAdapter::new(backend.clone()).expect("krun restart journal should open");

    assert_succeeded(
        WorkloadExecutionQuiescenceCapability::execute(&adapter, &commands.quiescence).await,
    );
    assert_eq!(delete_count(&fixture.delete_log), 0);
    assert!(!fixture.runtime_marker.exists());
    let quiesced = read_manifest(&fixture);
    assert_eq!(
        quiesced["execution_attempt_id"],
        commands.quiescence.source_attempt_id().as_str()
    );
    assert_eq!(quiesced["creator_handoff"]["phase"], "quiesced");
    assert_eq!(
        RetainedAuthority::from_manifest(&quiesced),
        fixture.retained
    );
    assert!(
        fixture
            .manifest_path
            .parent()
            .expect("manifest should have a provider directory")
            .join(".nimbus-krun-restart.json")
            .is_file()
    );
}

#[tokio::test]
async fn krun_restart_preparation_retains_authority_and_binds_attempt() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let rootfs = root.path().join("rootfs");
    fs::create_dir(&rootfs).expect("fixture rootfs should exist");
    let commands =
        direct_restart_commands(SandboxBackendKind::Krun, "krun-preparation", &rootfs).await;
    assert_exact_command(&commands.preparation, SandboxBackendKind::Krun);
    let mut config = KrunSandboxBackendConfig::under_root(root.path());
    config.start_mode = KrunStartMode::Execute;
    let backend = Arc::new(KrunSandboxBackend::new(config));
    let fixture = seed_krun_provider(backend.as_ref(), &commands.quiescence, root.path(), false);
    let adapter =
        KrunProvisionAdapter::new(backend.clone()).expect("krun restart journal should open");

    assert_succeeded(
        WorkloadExecutionQuiescenceCapability::execute(&adapter, &commands.quiescence).await,
    );
    assert_succeeded(
        WorkloadRestartPreparationCapability::execute(&adapter, &commands.preparation).await,
    );
    let prepared = read_manifest(&fixture);
    assert_eq!(
        prepared["execution_attempt_id"],
        commands.preparation.attempt_id().as_str()
    );
    assert_eq!(
        RetainedAuthority::from_manifest(&prepared),
        fixture.retained
    );
    assert_eq!(prepared["launch_authority"]["phase"], "adopted");
    assert_eq!(
        prepared["launch_authority"]["reservation_claim"],
        fixture.retained.network_config["reservation_claim"]
    );
    assert_eq!(prepared["creator_handoff"]["phase"], "not_spawned");
}

#[tokio::test]
async fn real_restart_adapters_reject_crossed_provider_attempt_and_inspection() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let container_root = root.path().join("container");
    let krun_root = root.path().join("krun");
    let container_rootfs = container_root.join("rootfs");
    let krun_rootfs = krun_root.join("rootfs");
    fs::create_dir_all(&container_rootfs).expect("container rootfs should exist");
    fs::create_dir_all(&krun_rootfs).expect("krun rootfs should exist");
    let container_commands = direct_restart_commands(
        SandboxBackendKind::Container,
        "crossed-container",
        &container_rootfs,
    )
    .await;
    let other_container_commands = direct_restart_commands(
        SandboxBackendKind::Container,
        "crossed-container-attempt",
        &container_rootfs,
    )
    .await;
    let krun_inspection = inspection_quiescence_command(
        SandboxBackendKind::Krun,
        "crossed-krun-inspection",
        &krun_rootfs,
    )
    .await;
    assert_eq!(krun_inspection.mode(), WorkloadRestartCommandMode::Inspect);

    let mut container_config = ContainerSandboxBackendConfig::under_root(&container_root);
    container_config.start_mode = ContainerStartMode::Execute;
    let container_backend = Arc::new(ContainerSandboxBackend::new(container_config));
    let fixture = seed_container_provider(
        container_backend.as_ref(),
        &container_commands.quiescence,
        &container_root,
        false,
    );
    let before = fs::read(&fixture.manifest_path).expect("source manifest should read");
    let container = ContainerProvisionAdapter::new(container_backend)
        .expect("container restart journal should open");
    let mut krun_config = KrunSandboxBackendConfig::under_root(&krun_root);
    krun_config.start_mode = KrunStartMode::Execute;
    let krun = KrunProvisionAdapter::new(Arc::new(KrunSandboxBackend::new(krun_config)))
        .expect("krun restart journal should open");

    assert_definite_failure(
        "crossed execution provider",
        WorkloadExecutionQuiescenceCapability::execute(&krun, &container_commands.quiescence).await,
    );
    let mut crossed_manifest: Value =
        serde_json::from_slice(&before).expect("source manifest should decode");
    crossed_manifest["execution_attempt_id"] = Value::String(
        other_container_commands
            .quiescence
            .source_attempt_id()
            .to_string(),
    );
    publish_manifest(&fixture.manifest_path, &crossed_manifest);
    let crossed_before = fs::read(&fixture.manifest_path).expect("crossed manifest should read");
    assert_definite_failure(
        "crossed execution attempt",
        WorkloadExecutionQuiescenceCapability::execute(&container, &container_commands.quiescence)
            .await,
    );
    assert_eq!(
        fs::read(&fixture.manifest_path).expect("crossed manifest should remain"),
        crossed_before,
        "a crossed execution-attempt fence must make zero manifest effects"
    );
    fs::write(&fixture.manifest_path, &before).expect("source manifest should restore");
    assert_definite_failure(
        "crossed inspection provider",
        WorkloadExecutionQuiescenceCapability::inspect(&container, &krun_inspection).await,
    );
    assert_eq!(
        fs::read(&fixture.manifest_path).expect("source manifest should remain"),
        before,
        "crossed provider, attempt, and inspection commands must have zero provider-manifest effects"
    );
    assert_eq!(delete_count(&fixture.delete_log), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_restart_dispatch_produces_one_provider_effect() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let rootfs = root.path().join("rootfs");
    fs::create_dir(&rootfs).expect("fixture rootfs should exist");
    let commands =
        direct_restart_commands(SandboxBackendKind::Krun, "krun-concurrent", &rootfs).await;
    let mut config = KrunSandboxBackendConfig::under_root(root.path());
    config.start_mode = KrunStartMode::Execute;
    let backend = Arc::new(KrunSandboxBackend::new(config));
    let fixture = seed_krun_provider(backend.as_ref(), &commands.quiescence, root.path(), true);
    let adapter =
        Arc::new(KrunProvisionAdapter::new(backend).expect("krun restart journal should open"));
    let command = Arc::new(commands.quiescence);
    let barrier = Arc::new(Barrier::new(2));
    let runtime = tokio::runtime::Handle::current();

    let outcomes = std::thread::scope(|scope| {
        let workers = (0..2)
            .map(|_| {
                let adapter = adapter.clone();
                let command = command.clone();
                let barrier = barrier.clone();
                let runtime = runtime.clone();
                scope.spawn(move || {
                    barrier.wait();
                    runtime.block_on(WorkloadExecutionQuiescenceCapability::execute(
                        adapter.as_ref(),
                        command.as_ref(),
                    ))
                })
            })
            .collect::<Vec<_>>();
        workers
            .into_iter()
            .map(|worker| {
                worker
                    .join()
                    .expect("concurrent restart dispatcher should not panic")
                    .into_outcome()
            })
            .collect::<Vec<_>>()
    });

    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, WorkloadRestartCommandOutcome::Succeeded { .. }))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, WorkloadRestartCommandOutcome::InProgress { .. }))
            .count(),
        1
    );
    assert_eq!(delete_count(&fixture.delete_log), 1);
    assert!(!fixture.runtime_marker.exists());
}
