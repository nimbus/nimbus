use std::collections::BTreeMap;
use std::fs;
use std::net::Ipv4Addr;
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use nimbus_core::{Cidr, TenantId};
use nimbus_network::{
    ListenerId, LocalNetworkAuthority, LocalNetworkAuthorityRootMismatch, LocalNetworkManager,
    NetworkAttachmentId, NetworkLeaseEpoch, NetworkPlan, NetworkPlanContentDigest, NetworkPlanId,
    NetworkResourceGeneration, PortBindRealm, PortBindTarget, PortBindingSpec, PortExposure,
    PortIpv6Overlap, PortLeaseAccounting, PortLeaseError, PortLeaseFence, PortLeaseId,
    PortLeasePhase, PortLeaseRequest, PortProtocol, PortPublicationIntent, PortRequestMode,
};
use nimbus_sandbox::backends::container::{
    ContainerSandboxBackend, ContainerSandboxBackendConfig, ContainerStartMode,
};
use nimbus_sandbox::backends::krun::{KrunSandboxBackend, KrunSandboxBackendConfig, KrunStartMode};
use nimbus_sandbox::backends::{
    CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY, KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
    SandboxAttachmentRegistrationError,
};
use nimbus_sandbox::{
    OciNetworkProcess, OciNetworkProcessError, SandboxBackendKind, SandboxExecutionAttemptId,
    SandboxHandle, SandboxId, SandboxOwnerSpec, SandboxPortBinding, SandboxProcessSpec,
    SandboxProvisionDependencyListener, SandboxProvisionEndpointIdentity, SandboxProvisionListener,
    SandboxProvisionNetworkPlan, SandboxRootSpec, SandboxRootfsSpec, SandboxSpec,
    sandbox_network_plan_requirements,
};
use serde_json::Value;
use tempfile::tempdir;

static COMPOSITION_TEST_SERIALIZER: Mutex<()> = Mutex::new(());

const ACTIVE_SUPERNET: &str = "10.44.0.0/16";
const SUBSTITUTE_SUPERNET: &str = "10.45.0.0/16";
const ACTIVE_TENANT_PREFIX: u8 = 24;

fn composition_attempt_id(sandbox_id: &SandboxId) -> SandboxExecutionAttemptId {
    SandboxExecutionAttemptId::new(format!("production-composition:{sandbox_id}"))
        .expect("composition execution attempt should validate")
}

fn compiled_network_plan(
    spec: &SandboxSpec,
    sandbox_id: &SandboxId,
    label: &str,
) -> SandboxProvisionNetworkPlan {
    let incarnation = format!("composition-{label}:{}", sandbox_id.as_str());
    let generation = NetworkResourceGeneration::new(7);
    let requirements = sandbox_network_plan_requirements(spec.backend);
    let plan = NetworkPlan::new(
        NetworkPlanId::for_tenant_workload_plan(&spec.tenant_id, &incarnation),
        generation,
        NetworkPlanContentDigest::sha256(format!("production-composition:{label}")),
        requirements.capability_requirements().clone(),
    );
    let plan_id = plan.plan_id().clone();
    let endpoint_identities = spec.port_bindings.iter().map(|binding| {
        SandboxProvisionEndpointIdentity::new(
            ListenerId::for_tenant_workload_listener(&spec.tenant_id, &incarnation, &binding.name),
            nimbus_network::PublishedEndpointId::for_workload_endpoint(&incarnation, &binding.name),
        )
    });
    let listeners = spec.port_bindings.iter().map(|binding| {
        let listener_id =
            ListenerId::for_tenant_workload_listener(&spec.tenant_id, &incarnation, &binding.name);
        let (target, exposure) = match binding.host_address {
            std::net::IpAddr::V4(address) if address.is_unspecified() => {
                (PortBindTarget::ipv4_wildcard(), PortExposure::Public)
            }
            std::net::IpAddr::V4(address) => (
                PortBindTarget::ipv4_specific(address),
                if address.is_loopback() {
                    PortExposure::Loopback
                } else {
                    PortExposure::Private
                },
            ),
            std::net::IpAddr::V6(address) if address.is_unspecified() => (
                PortBindTarget::ipv6_wildcard(PortIpv6Overlap::Unknown),
                PortExposure::Public,
            ),
            std::net::IpAddr::V6(address) => (
                PortBindTarget::ipv6_specific(address, PortIpv6Overlap::Unknown)
                    .expect("fixture address should not be IPv4-mapped"),
                if address.is_loopback() {
                    PortExposure::Loopback
                } else {
                    PortExposure::Private
                },
            ),
        };
        let request = PortLeaseRequest::new(
            PortLeaseId::for_listener(&listener_id),
            listener_id.clone().into(),
            Some(spec.tenant_id.clone()),
            PortLeaseFence::new(generation, NetworkLeaseEpoch::new(1)),
            PortLeaseAccounting::TenantPublished,
            PortPublicationIntent::host(binding.host_address),
            PortBindingSpec::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                target,
                exposure,
                NonZeroU16::new(binding.host_port)
                    .map_or(PortRequestMode::ProviderAssigned, PortRequestMode::Exact),
            ),
        )
        .with_plan_id(plan_id.clone());
        SandboxProvisionListener::new(
            nimbus_network::PublishedEndpointId::for_workload_endpoint(&incarnation, &binding.name),
            listener_id,
            binding.clone(),
            request,
        )
    });
    SandboxProvisionNetworkPlan::new(
        plan,
        spec.tenant_id.clone(),
        generation,
        NetworkAttachmentId::for_workload_attachment(&incarnation, "primary"),
        endpoint_identities,
        listeners,
        [SandboxProvisionDependencyListener::new(
            ListenerId::for_tenant_workload_listener(&spec.tenant_id, &incarnation, "egress-pep"),
            "egress-pep",
            requirements.pep_provider_id().clone(),
        )],
    )
    .expect("composition network plan should validate")
}

fn reserve_and_prepare_container(
    backend: &ContainerSandboxBackend,
    spec: SandboxSpec,
    label: &str,
) -> nimbus_sandbox::Result<SandboxHandle> {
    let id = SandboxId::new(format!("composition-container-{label}"));
    let plan = compiled_network_plan(&spec, &id, label);
    let attempt = composition_attempt_id(&id);
    backend.reserve_provision_network(spec, id.clone(), attempt.clone(), plan)?;
    backend.prepare_provision_workload(&id, &attempt)
}

fn reserve_and_prepare_krun(
    backend: &KrunSandboxBackend,
    spec: SandboxSpec,
    label: &str,
) -> nimbus_sandbox::Result<SandboxHandle> {
    let id = SandboxId::new(format!("composition-krun-{label}"));
    let plan = compiled_network_plan(&spec, &id, label);
    let attempt = composition_attempt_id(&id);
    backend.reserve_provision_network(spec, id.clone(), attempt.clone(), plan)?;
    backend.prepare_provision_workload(&id, &attempt)
}

#[derive(Clone, Copy, Debug)]
enum BackendFixture {
    Container,
    Krun,
}

impl BackendFixture {
    const ALL: [Self; 2] = [Self::Container, Self::Krun];

    fn name(self) -> &'static str {
        match self {
            Self::Container => "container",
            Self::Krun => "krun",
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum MismatchFixture {
    AuthorityRoot,
    NodeSupernet,
    TenantPrefix,
}

impl MismatchFixture {
    const ALL: [Self; 3] = [Self::AuthorityRoot, Self::NodeSupernet, Self::TenantPrefix];
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PathSnapshot {
    exists: bool,
    entries: BTreeMap<PathBuf, SnapshotEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SnapshotEntry {
    Directory,
    File(Vec<u8>),
    Symlink(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AuthoritySnapshot {
    bytes: Option<Vec<u8>>,
    revision: u64,
}

#[test]
fn injected_backends_reject_divergent_authority_before_effects() {
    let _serial = composition_test_lock();

    for backend in BackendFixture::ALL {
        for mismatch in MismatchFixture::ALL {
            let fixture_root = tempdir().expect("composition fixture root should exist");
            let node_root = fixture_root.path().join("node-network");
            let workload_root = fixture_root
                .path()
                .join(format!("{}-workload", backend.name()));
            let foreign_root = fixture_root.path().join("foreign-network");
            let bootstrap = LocalNetworkManager::bootstrap(&node_root)
                .expect("fixture should claim one node authority");
            let authority = bootstrap.authority();
            let active_supernet = cidr(ACTIVE_SUPERNET);
            let process =
                OciNetworkProcess::new(authority.clone(), active_supernet, ACTIVE_TENANT_PREFIX)
                    .expect("the canonical OCI process composition should construct");

            let attempted_root = match mismatch {
                MismatchFixture::AuthorityRoot => foreign_root.clone(),
                MismatchFixture::NodeSupernet | MismatchFixture::TenantPrefix => node_root.clone(),
            };
            let attempted_supernet = match mismatch {
                MismatchFixture::NodeSupernet => cidr(SUBSTITUTE_SUPERNET),
                MismatchFixture::AuthorityRoot | MismatchFixture::TenantPrefix => active_supernet,
            };
            let attempted_prefix = match mismatch {
                MismatchFixture::TenantPrefix => ACTIVE_TENANT_PREFIX + 1,
                MismatchFixture::AuthorityRoot | MismatchFixture::NodeSupernet => {
                    ACTIVE_TENANT_PREFIX
                }
            };

            let workload_before = snapshot_path(&workload_root);
            let attempted_before = snapshot_path(&attempted_root);
            let authority_before = snapshot_authority(&authority);
            let result = inject_backend(
                backend,
                backend_config(
                    backend,
                    &workload_root,
                    &attempted_root,
                    attempted_supernet,
                    attempted_prefix,
                ),
                process.clone(),
            );
            let error = result.expect_err("divergent injected composition must fail closed");

            assert_typed_mismatch(
                mismatch,
                error,
                &authority,
                &attempted_root,
                active_supernet,
                attempted_supernet,
                attempted_prefix,
            );
            assert_eq!(
                snapshot_path(&workload_root),
                workload_before,
                "{backend:?} {mismatch:?} rejection must precede workload artifact creation"
            );
            assert_eq!(
                snapshot_path(&attempted_root),
                attempted_before,
                "{backend:?} {mismatch:?} rejection must not mutate its attempted root"
            );
            assert_eq!(
                snapshot_authority(&authority),
                authority_before,
                "{backend:?} {mismatch:?} rejection must not consume a durable revision"
            );

            drop(process);
            drop(authority);
            drop(bootstrap);
        }
    }
}

#[cfg(unix)]
#[test]
fn accepted_authority_alias_is_pinned_before_later_work() {
    let _serial = composition_test_lock();

    for backend in BackendFixture::ALL {
        let fixture_root = tempdir().expect("composition fixture root should exist");
        let node_root = fixture_root.path().join("node-network");
        let alias_root = fixture_root.path().join("accepted-network-alias");
        let foreign_root = fixture_root.path().join("retargeted-network");
        let workload_root = fixture_root
            .path()
            .join(format!("{}-alias-workload", backend.name()));
        fs::create_dir_all(&foreign_root).expect("foreign root should exist before retargeting");
        let bootstrap = LocalNetworkManager::bootstrap(&node_root)
            .expect("fixture should claim node authority");
        let authority = bootstrap.authority();
        std::os::unix::fs::symlink(&node_root, &alias_root)
            .expect("canonical network-root alias should create");
        let process = OciNetworkProcess::new(
            authority.clone(),
            cidr(ACTIVE_SUPERNET),
            ACTIVE_TENANT_PREFIX,
        )
        .expect("the canonical OCI process composition should construct");
        let foreign_before = snapshot_path(&foreign_root);

        let start_result = match backend {
            BackendFixture::Container => {
                let injected = expect_container_backend(
                    container_config(
                        &workload_root,
                        &alias_root,
                        cidr(ACTIVE_SUPERNET),
                        ACTIVE_TENANT_PREFIX,
                        ContainerStartMode::PlanOnly,
                    ),
                    process.clone(),
                );
                retarget_symlink(&alias_root, &foreign_root);
                reserve_and_prepare_container(
                    &injected,
                    sandbox_spec(
                        TenantId::new("alias-container").expect("fixture tenant should validate"),
                        "alias-container",
                        SandboxBackendKind::Container,
                        None,
                    ),
                    "alias-container",
                )
            }
            BackendFixture::Krun => {
                let injected = expect_krun_backend(
                    krun_config(
                        &workload_root,
                        &alias_root,
                        cidr(ACTIVE_SUPERNET),
                        ACTIVE_TENANT_PREFIX,
                        KrunStartMode::Execute,
                    ),
                    process.clone(),
                );
                retarget_symlink(&alias_root, &foreign_root);
                reserve_and_prepare_krun(
                    &injected,
                    sandbox_spec(
                        TenantId::new("alias-krun").expect("fixture tenant should validate"),
                        "alias-krun",
                        SandboxBackendKind::Krun,
                        None,
                    ),
                    "alias-krun",
                )
            }
        };
        start_result.unwrap_or_else(|error| {
            panic!(
                "{} must retain the authenticated authority after its accepted alias is \
                 retargeted: {error}",
                backend.name()
            )
        });

        assert_eq!(
            fs::canonicalize(&alias_root).expect("retargeted alias should resolve"),
            fs::canonicalize(&foreign_root).expect("foreign root should canonicalize"),
            "the test must leave the accepted spelling pointed at a foreign root"
        );
        assert_eq!(
            snapshot_path(&foreign_root),
            foreign_before,
            "{} must not follow or mutate the retargeted root",
            backend.name()
        );
        let manifests = manifest_files(&workload_root);
        assert_eq!(
            manifests.len(),
            1,
            "{} should publish exactly one PlanOnly manifest",
            backend.name()
        );
        let manifest: Value = serde_json::from_slice(
            &fs::read(&manifests[0]).expect("published manifest should remain readable"),
        )
        .expect("published manifest should remain valid JSON");
        assert_eq!(
            manifest
                .pointer("/network_layout/network_state_root")
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .as_deref(),
            Some(authority.state_root()),
            "{} must persist the immutable process root, not the accepted alias spelling",
            backend.name()
        );

        drop(process);
        drop(authority);
        drop(bootstrap);
    }
}

#[test]
fn distinct_workload_roots_share_only_portable_node_authority() {
    let _serial = composition_test_lock();
    let fixture_root = tempdir().expect("composition fixture root should exist");
    let node_root = fixture_root.path().join("node-network");
    let container_root = fixture_root.path().join("container-workload");
    let krun_root = fixture_root.path().join("krun-workload");
    let bootstrap =
        LocalNetworkManager::bootstrap(&node_root).expect("fixture should claim node authority");
    let authority = bootstrap.authority();
    let process = OciNetworkProcess::new(
        authority.clone(),
        cidr(ACTIVE_SUPERNET),
        ACTIVE_TENANT_PREFIX,
    )
    .expect("the canonical OCI process composition should construct");

    let container = expect_container_backend(
        container_config(
            &container_root,
            &node_root,
            cidr(ACTIVE_SUPERNET),
            ACTIVE_TENANT_PREFIX,
            ContainerStartMode::PlanOnly,
        ),
        process.clone(),
    );
    let krun = expect_krun_backend(
        krun_config(
            &krun_root,
            &node_root,
            cidr(ACTIVE_SUPERNET),
            ACTIVE_TENANT_PREFIX,
            KrunStartMode::Execute,
        ),
        process.clone(),
    );

    let container_tenant =
        TenantId::new("composition-container").expect("fixture tenant should validate");
    let krun_tenant = TenantId::new("composition-krun").expect("fixture tenant should validate");
    let container_handle = reserve_and_prepare_container(
        &container,
        sandbox_spec(
            container_tenant.clone(),
            "container-workload",
            SandboxBackendKind::Container,
            None,
        ),
        "distinct-workload-roots",
    )
    .expect("PlanOnly container should materialize only its workload artifacts");
    let krun_handle = reserve_and_prepare_krun(
        &krun,
        sandbox_spec(
            krun_tenant.clone(),
            "krun-workload",
            SandboxBackendKind::Krun,
            None,
        ),
        "distinct-workload-roots",
    )
    .expect("Krun reserve/prepare should materialize only workload-owned artifacts");
    assert_eq!(container_handle.tenant_id, container_tenant);
    assert_eq!(krun_handle.tenant_id, krun_tenant);

    let portable_request = exact_port_request("portable-node-state", "portable-node-state", 41_460);
    authority
        .port_leases()
        .reserve(portable_request.clone())
        .expect("manager-derived authority should persist portable port state");
    assert!(
        authority
            .port_leases()
            .inspect(portable_request.lease_id())
            .expect("portable state should remain readable")
            .is_some()
    );

    assert!(
        authority.authority_path().is_file(),
        "portable state must be written beneath the one node authority"
    );
    for workload_root in [&container_root, &krun_root] {
        assert!(
            workload_root.exists(),
            "each backend should retain its own workload artifacts"
        );
        assert!(
            !nimbus_network::LocalNetworkStateStore::authority_path_for(workload_root).exists(),
            "portable network authority must never be recreated under {}",
            workload_root.display()
        );
    }

    let container_snapshot = snapshot_path(&container_root);
    let krun_snapshot = snapshot_path(&krun_root);
    assert_snapshot_mentions(&container_snapshot, "composition-container");
    assert_snapshot_omits(&container_snapshot, "composition-krun");
    assert_snapshot_mentions(&krun_snapshot, "composition-krun");
    assert_snapshot_omits(&krun_snapshot, "composition-container");
}

#[test]
fn same_host_port_conflicts_before_provider_effect() {
    let _serial = composition_test_lock();
    let fixture_root = tempdir().expect("composition fixture root should exist");
    let node_root = fixture_root.path().join("node-network");
    let container_root = fixture_root.path().join("container-workload");
    let krun_root = fixture_root.path().join("krun-workload");
    let bootstrap =
        LocalNetworkManager::bootstrap(&node_root).expect("fixture should claim node authority");
    let authority = bootstrap.authority();
    let process = OciNetworkProcess::new(
        authority.clone(),
        cidr(ACTIVE_SUPERNET),
        ACTIVE_TENANT_PREFIX,
    )
    .expect("the canonical OCI process composition should construct");
    let container = expect_container_backend(
        container_config(
            &container_root,
            &node_root,
            cidr(ACTIVE_SUPERNET),
            ACTIVE_TENANT_PREFIX,
            ContainerStartMode::Execute,
        ),
        process.clone(),
    );
    let krun = expect_krun_backend(
        krun_config(
            &krun_root,
            &node_root,
            cidr(ACTIVE_SUPERNET),
            ACTIVE_TENANT_PREFIX,
            KrunStartMode::Execute,
        ),
        process,
    );

    let container_seed = exact_port_request("container-seed", "container-seed", 41_470);
    let krun_seed = exact_port_request("krun-seed", "krun-seed", 41_471);
    let port_authority = authority.port_leases();
    port_authority
        .reserve(container_seed.clone())
        .expect("the first distinct request should reserve");
    port_authority
        .reserve(krun_seed.clone())
        .expect("the second distinct request should reserve");

    let overlapping = exact_port_request("typed-overlap", "typed-overlap", 41_470);
    let authority_before_overlap = snapshot_authority(&authority);
    match port_authority.reserve(overlapping) {
        Err(PortLeaseError::PortConflict {
            conflicting_port,
            existing_lease_id,
            existing_phase,
            ..
        }) => {
            assert_eq!(conflicting_port.get(), 41_470);
            assert_eq!(existing_lease_id, container_seed.lease_id().clone());
            assert_eq!(existing_phase, PortLeasePhase::Reserved);
        }
        other => panic!("expected typed pre-bind port conflict, got {other:?}"),
    }
    assert_eq!(
        snapshot_authority(&authority),
        authority_before_overlap,
        "a rejected overlap must not mutate durable authority"
    );

    let container_error = reserve_and_prepare_container(
        &container,
        sandbox_spec(
            TenantId::new("container-port-conflict").expect("fixture tenant should validate"),
            "container-port-conflict",
            SandboxBackendKind::Container,
            Some(41_471),
        ),
        "port-conflict",
    )
    .expect_err("the injected container facade must observe the krun seed lease");
    assert_port_conflict(&container_error.to_string(), 41_471, &krun_seed);

    let krun_error = reserve_and_prepare_krun(
        &krun,
        sandbox_spec(
            TenantId::new("krun-port-conflict").expect("fixture tenant should validate"),
            "krun-port-conflict",
            SandboxBackendKind::Krun,
            Some(41_470),
        ),
        "port-conflict",
    )
    .expect_err("the injected krun facade must observe the container seed lease");
    assert_port_conflict(&krun_error.to_string(), 41_470, &container_seed);

    for request in [&container_seed, &krun_seed] {
        let record = port_authority
            .inspect(request.lease_id())
            .expect("seed lease should remain readable")
            .expect("seed lease should remain present");
        assert_eq!(record.phase(), PortLeasePhase::Reserved);
        assert!(
            record.binding().is_none()
                && record.bind_claim().is_none()
                && record.adoption_claim().is_none(),
            "conflict must precede every provider bind/adoption effect: {record:?}"
        );
    }
    assert_no_provider_binding_evidence(&authority);
}

#[test]
fn startup_failure_never_becomes_registered_capability() {
    let _serial = composition_test_lock();
    let fixture_root = tempdir().expect("composition fixture root should exist");
    let node_root = fixture_root.path().join("node-network");
    let container_root = fixture_root.path().join("container-workload");
    let krun_root = fixture_root.path().join("krun-workload");
    let bootstrap =
        LocalNetworkManager::bootstrap(&node_root).expect("fixture should claim node authority");
    let authority = bootstrap.authority();
    let process = OciNetworkProcess::new(
        authority.clone(),
        cidr(ACTIVE_SUPERNET),
        ACTIVE_TENANT_PREFIX,
    )
    .expect("the canonical OCI process composition should construct");

    install_corrupt_manifest(&container_root, "startup-container", "corrupt-container");
    install_corrupt_manifest(&krun_root, "startup-krun", "corrupt-krun");
    let container = expect_container_backend(
        container_config(
            &container_root,
            &node_root,
            cidr(ACTIVE_SUPERNET),
            ACTIVE_TENANT_PREFIX,
            ContainerStartMode::Execute,
        ),
        process.clone(),
    );
    let krun = expect_krun_backend(
        krun_config(
            &krun_root,
            &node_root,
            cidr(ACTIVE_SUPERNET),
            ACTIVE_TENANT_PREFIX,
            KrunStartMode::Execute,
        ),
        process,
    );

    let container_start = reserve_and_prepare_container(
        &container,
        sandbox_spec(
            TenantId::new("startup-container").expect("fixture tenant should validate"),
            "new-container-work",
            SandboxBackendKind::Container,
            None,
        ),
        "startup-failure",
    )
    .expect_err("cached container startup failure must fence new work");
    let corrupt_container_manifest =
        corrupt_manifest_path(&container_root, "startup-container", "corrupt-container");
    assert_cached_startup_failure(
        &container_start.to_string(),
        &corrupt_container_manifest,
        "unmatched artifact",
    );
    let krun_start = reserve_and_prepare_krun(
        &krun,
        sandbox_spec(
            TenantId::new("startup-krun").expect("fixture tenant should validate"),
            "new-krun-work",
            SandboxBackendKind::Krun,
            None,
        ),
        "startup-failure",
    )
    .expect_err("cached krun startup failure must fence new work");
    let corrupt_krun_manifest = corrupt_manifest_path(&krun_root, "startup-krun", "corrupt-krun");
    assert_cached_startup_failure(
        &krun_start.to_string(),
        &corrupt_krun_manifest,
        "unmatched artifact",
    );

    let container_registration = container.host_managed_attachment_registration();
    let krun_registration = krun.host_managed_attachment_registration();
    assert!(
        container_registration.is_err(),
        "a failed startup must never advertise a container capability"
    );
    assert!(
        krun_registration.is_err(),
        "a failed startup must never advertise a krun capability"
    );
    #[cfg(target_os = "linux")]
    {
        assert_startup_registration_failure(
            container_registration.expect_err("container registration must fail closed"),
            CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
            &corrupt_container_manifest,
            "unmatched artifact",
        );
        assert_startup_registration_failure(
            krun_registration.expect_err("krun registration must fail closed"),
            KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
            &corrupt_krun_manifest,
            "unmatched artifact",
        );
    }
    #[cfg(not(target_os = "linux"))]
    {
        assert_eq!(
            container_registration.expect_err("container registration must fail closed"),
            SandboxAttachmentRegistrationError::UnsupportedTarget {
                provider_key: CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
                target_os: std::env::consts::OS,
            },
            "target refusal may mask the cached diagnostic but must never register"
        );
        assert_eq!(
            krun_registration.expect_err("krun registration must fail closed"),
            SandboxAttachmentRegistrationError::UnsupportedTarget {
                provider_key: KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
                target_os: std::env::consts::OS,
            },
            "target refusal may mask the cached diagnostic but must never register"
        );
    }

    assert_eq!(
        manifest_files(&container_root),
        vec![corrupt_manifest_path(
            &container_root,
            "startup-container",
            "corrupt-container"
        )],
        "rejected container work must not publish another manifest"
    );
    assert_eq!(
        manifest_files(&krun_root),
        vec![corrupt_manifest_path(
            &krun_root,
            "startup-krun",
            "corrupt-krun"
        )],
        "rejected krun work must not publish another manifest"
    );
}

enum BackendConfig {
    Container(ContainerSandboxBackendConfig),
    Krun(KrunSandboxBackendConfig),
}

fn backend_config(
    backend: BackendFixture,
    workload_root: &Path,
    network_root: &Path,
    node_supernet: Cidr,
    tenant_prefix: u8,
) -> BackendConfig {
    match backend {
        BackendFixture::Container => BackendConfig::Container(container_config(
            workload_root,
            network_root,
            node_supernet,
            tenant_prefix,
            ContainerStartMode::PlanOnly,
        )),
        BackendFixture::Krun => BackendConfig::Krun(krun_config(
            workload_root,
            network_root,
            node_supernet,
            tenant_prefix,
            KrunStartMode::PlanOnly,
        )),
    }
}

fn inject_backend(
    backend: BackendFixture,
    config: BackendConfig,
    process: std::sync::Arc<OciNetworkProcess>,
) -> Result<(), OciNetworkProcessError> {
    match (backend, config) {
        (BackendFixture::Container, BackendConfig::Container(config)) => {
            ContainerSandboxBackend::with_network_process(config, process).map(|_| ())
        }
        (BackendFixture::Krun, BackendConfig::Krun(config)) => {
            KrunSandboxBackend::with_network_process(config, process).map(|_| ())
        }
        _ => unreachable!("backend fixture must retain its matching config"),
    }
}

fn container_config(
    workload_root: &Path,
    network_root: &Path,
    node_supernet: Cidr,
    tenant_prefix: u8,
    start_mode: ContainerStartMode,
) -> ContainerSandboxBackendConfig {
    let mut config =
        ContainerSandboxBackendConfig::plan_only(workload_root.join("bundles"), workload_root)
            .with_network_state_root(network_root);
    config.node_network_supernet = node_supernet.to_string();
    config.node_tenant_subnet_prefix = tenant_prefix;
    config.start_mode = start_mode;
    config
}

fn krun_config(
    workload_root: &Path,
    network_root: &Path,
    node_supernet: Cidr,
    tenant_prefix: u8,
    start_mode: KrunStartMode,
) -> KrunSandboxBackendConfig {
    let mut config =
        KrunSandboxBackendConfig::plan_only(workload_root.join("bundles"), workload_root)
            .with_network_state_root(network_root);
    config.node_network_supernet = node_supernet.to_string();
    config.node_tenant_subnet_prefix = tenant_prefix;
    config.start_mode = start_mode;
    config
}

fn expect_container_backend(
    config: ContainerSandboxBackendConfig,
    process: std::sync::Arc<OciNetworkProcess>,
) -> ContainerSandboxBackend {
    match ContainerSandboxBackend::with_network_process(config, process) {
        Ok(backend) => backend,
        Err(error) => panic!("matching container composition should construct: {error}"),
    }
}

fn expect_krun_backend(
    config: KrunSandboxBackendConfig,
    process: std::sync::Arc<OciNetworkProcess>,
) -> KrunSandboxBackend {
    match KrunSandboxBackend::with_network_process(config, process) {
        Ok(backend) => backend,
        Err(error) => panic!("matching krun composition should construct: {error}"),
    }
}

fn assert_typed_mismatch(
    expected: MismatchFixture,
    error: OciNetworkProcessError,
    authority: &LocalNetworkAuthority,
    attempted_root: &Path,
    active_supernet: Cidr,
    attempted_supernet: Cidr,
    attempted_prefix: u8,
) {
    match (expected, error) {
        (
            MismatchFixture::AuthorityRoot,
            OciNetworkProcessError::AuthorityRootMismatch(mismatch),
        ) => assert_authority_mismatch(&mismatch, authority, attempted_root),
        (
            MismatchFixture::NodeSupernet,
            OciNetworkProcessError::NodeSupernetMismatch { active, attempted },
        ) => {
            assert_eq!(active, active_supernet);
            assert_eq!(attempted, attempted_supernet);
        }
        (
            MismatchFixture::TenantPrefix,
            OciNetworkProcessError::TenantPrefixMismatch { active, attempted },
        ) => {
            assert_eq!(active, ACTIVE_TENANT_PREFIX);
            assert_eq!(attempted, attempted_prefix);
        }
        (expected, other) => panic!("expected typed {expected:?} mismatch, got {other:?}"),
    }
}

fn assert_authority_mismatch(
    mismatch: &LocalNetworkAuthorityRootMismatch,
    authority: &LocalNetworkAuthority,
    attempted_root: &Path,
) {
    assert_eq!(mismatch.active_authority_path(), authority.authority_path());
    assert_eq!(
        mismatch.attempted_authority_path(),
        nimbus_network::LocalNetworkStateStore::authority_path_for(attempted_root)
    );
}

fn exact_port_request(lease_key: &str, owner_key: &str, port: u16) -> PortLeaseRequest {
    let tenant = TenantId::new("production-composition").expect("fixture tenant should validate");
    let listener =
        ListenerId::for_tenant_workload_listener(&tenant, owner_key, "published-listener");
    let lease_id = PortLeaseId::for_listener(&ListenerId::for_tenant_workload_listener(
        &tenant,
        lease_key,
        "published-listener",
    ));
    PortLeaseRequest::new(
        lease_id,
        listener.into(),
        Some(tenant),
        PortLeaseFence::new(NetworkResourceGeneration::new(1), NetworkLeaseEpoch::new(1)),
        PortLeaseAccounting::TenantPublished,
        PortPublicationIntent::host(Ipv4Addr::LOCALHOST.into()),
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            PortExposure::Loopback,
            PortRequestMode::Exact(NonZeroU16::new(port).expect("fixture port should be non-zero")),
        ),
    )
}

fn sandbox_spec(
    tenant: TenantId,
    service: &str,
    backend: SandboxBackendKind,
    host_port: Option<u16>,
) -> SandboxSpec {
    let mut spec = SandboxSpec::new(
        tenant,
        SandboxOwnerSpec::service(service),
        backend,
        SandboxRootSpec::Rootfs(SandboxRootfsSpec::new("/fixture/rootfs")),
        SandboxProcessSpec::new(["/bin/sh", "-c", "exit 0"]),
    );
    if let Some(host_port) = host_port {
        spec = spec.with_port_binding(SandboxPortBinding::tcp("http", host_port, 8080));
    }
    spec
}

fn install_corrupt_manifest(workload_root: &Path, tenant: &str, sandbox: &str) {
    let path = corrupt_manifest_path(workload_root, tenant, sandbox);
    fs::create_dir_all(path.parent().expect("manifest should have a parent"))
        .expect("corrupt manifest parent should create");
    fs::write(path, b"{").expect("corrupt manifest fixture should write");
}

fn corrupt_manifest_path(workload_root: &Path, tenant: &str, sandbox: &str) -> PathBuf {
    workload_root
        .join("tenants")
        .join(tenant)
        .join("sandboxes")
        .join(sandbox)
        .join("state")
        .join("containers")
        .join(sandbox)
        .join("manifest.json")
}

fn manifest_files(workload_root: &Path) -> Vec<PathBuf> {
    snapshot_path(workload_root)
        .entries
        .into_iter()
        .filter_map(|(relative, entry)| {
            matches!(entry, SnapshotEntry::File(_))
                .then(|| workload_root.join(relative))
                .filter(|path| path.file_name().is_some_and(|name| name == "manifest.json"))
        })
        .collect()
}

fn assert_cached_startup_failure(error: &str, expected_manifest: &Path, expected_reason: &str) {
    assert!(
        error.contains("startup reconciliation did not complete"),
        "new work must preserve the cached startup failure: {error}"
    );
    assert_cached_startup_diagnostic(error, expected_manifest, expected_reason);
}

fn assert_cached_startup_diagnostic(error: &str, expected_manifest: &Path, expected_reason: &str) {
    assert!(
        error.contains(expected_reason) && error.contains(&expected_manifest.display().to_string()),
        "cached startup diagnostic must retain its exact artifact and reason: {error}"
    );
}

#[cfg(target_os = "linux")]
fn assert_startup_registration_failure(
    error: SandboxAttachmentRegistrationError,
    expected_provider: &'static str,
    expected_manifest: &Path,
    expected_reason: &str,
) {
    match error {
        SandboxAttachmentRegistrationError::StartupReconciliationFailed {
            provider_key,
            reason,
        } => {
            assert_eq!(provider_key, expected_provider);
            assert_cached_startup_diagnostic(&reason, expected_manifest, expected_reason);
        }
        other => panic!("expected cached startup registration refusal, got {other:?}"),
    }
}

fn assert_port_conflict(error: &str, port: u16, existing: &PortLeaseRequest) {
    assert!(
        error.contains(&format!("port {port} requested by lease"))
            && error.contains(&format!("conflicts with lease {}", existing.lease_id())),
        "backend must preserve the exact shared-authority port conflict: {error}"
    );
}

fn snapshot_authority(authority: &LocalNetworkAuthority) -> AuthoritySnapshot {
    let bytes = read_optional(authority.authority_path());
    let revision = bytes
        .as_ref()
        .map(|bytes| {
            serde_json::from_slice::<Value>(bytes)
                .expect("authority envelope should remain valid JSON")["body"]["revision"]
                .as_u64()
                .expect("authority envelope should carry a numeric revision")
        })
        .unwrap_or(0);
    AuthoritySnapshot { bytes, revision }
}

fn read_optional(path: &Path) -> Option<Vec<u8>> {
    match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => panic!("failed to snapshot {}: {error}", path.display()),
    }
}

#[cfg(unix)]
fn retarget_symlink(alias_root: &Path, foreign_root: &Path) {
    fs::remove_file(alias_root).expect("accepted alias should unlink before retarget");
    std::os::unix::fs::symlink(foreign_root, alias_root)
        .expect("accepted alias should retarget to the foreign root");
}

fn snapshot_path(root: &Path) -> PathSnapshot {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return PathSnapshot {
                exists: false,
                entries: BTreeMap::new(),
            };
        }
        Err(error) => panic!("failed to inspect {}: {error}", root.display()),
    };
    let mut entries = BTreeMap::new();
    snapshot_entry(root, root, &metadata, &mut entries);
    PathSnapshot {
        exists: true,
        entries,
    }
}

fn snapshot_entry(
    root: &Path,
    path: &Path,
    metadata: &fs::Metadata,
    entries: &mut BTreeMap<PathBuf, SnapshotEntry>,
) {
    let relative = path
        .strip_prefix(root)
        .expect("snapshot entry should remain beneath its root")
        .to_path_buf();
    if metadata.file_type().is_symlink() {
        entries.insert(
            relative,
            SnapshotEntry::Symlink(
                fs::read_link(path).expect("snapshot symlink target should remain readable"),
            ),
        );
        return;
    }
    if metadata.is_file() {
        entries.insert(
            relative,
            SnapshotEntry::File(fs::read(path).expect("snapshot file should remain readable")),
        );
        return;
    }
    entries.insert(relative, SnapshotEntry::Directory);
    let mut children = fs::read_dir(path)
        .unwrap_or_else(|error| panic!("failed to enumerate {}: {error}", path.display()))
        .map(|entry| entry.expect("snapshot directory entry should remain readable"))
        .collect::<Vec<_>>();
    children.sort_by_key(fs::DirEntry::file_name);
    for child in children {
        let metadata = fs::symlink_metadata(child.path())
            .expect("snapshot child metadata should remain readable");
        snapshot_entry(root, &child.path(), &metadata, entries);
    }
}

fn assert_snapshot_mentions(snapshot: &PathSnapshot, needle: &str) {
    assert!(
        snapshot
            .entries
            .keys()
            .any(|path| path.to_string_lossy().contains(needle)),
        "snapshot should contain tenant-owned artifacts for {needle}: {snapshot:?}"
    );
}

fn assert_snapshot_omits(snapshot: &PathSnapshot, needle: &str) {
    assert!(
        snapshot
            .entries
            .keys()
            .all(|path| !path.to_string_lossy().contains(needle)),
        "snapshot must not contain sibling workload artifacts for {needle}: {snapshot:?}"
    );
}

fn assert_no_provider_binding_evidence(authority: &LocalNetworkAuthority) {
    for record in authority
        .port_leases()
        .list()
        .expect("durable lease state should remain readable")
    {
        assert!(
            record.bind_claim().is_none()
                && record.adoption_claim().is_none()
                && record.binding().is_none()
                && record.confirmed_stopped_binding().is_none(),
            "pre-effect conflict must not publish provider evidence: {record:?}"
        );
    }
}

fn cidr(value: &str) -> Cidr {
    Cidr::parse(value).expect("fixture CIDR should validate")
}

fn composition_test_lock() -> std::sync::MutexGuard<'static, ()> {
    COMPOSITION_TEST_SERIALIZER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
