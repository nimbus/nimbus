use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use nimbus_core::TenantId;
use nimbus_network::{
    LocalPortLeaseAuthority, NetworkAttachmentReservationState, NetworkProviderHandle,
    NetworkProviderId, NetworkReservationClaim, NetworkResourceGeneration, NetworkSegmentAllocator,
    PortExposure, PortLeasePhase,
};
use tempfile::TempDir;

use super::*;
use crate::backends::container::ContainerSandboxBackend;
use crate::backends::krun::KrunSandboxBackend;
use crate::backends::oci::network::ipam::{
    NetavarkTeardownPlan, begin_netavark_setup, begin_netavark_teardown, complete_netavark_setup,
    complete_netavark_teardown, confirm_netavark_provider_detached,
    inspect_netavark_provider_operation,
};
use crate::backends::oci::network::{
    RecordingSegmentAllocator, SegmentAllocatorOperation, allocate_container_ips,
    bridge_gateway_addr, default_network_attachment_id, direct_test_ipam_authority,
    inspect_container_ips,
};
use crate::backends::oci::port_lease::{
    OciPortProvider, adopt_claimed_and_activate, claim_bind_attempts,
    prepare_rebind_after_confirmed_stop, provider_binding, target_for_ip, withdraw,
};
use crate::backends::oci::port_lifecycle::{
    InternalListenerReservation, LaunchPortBatchState, NetavarkPortLifetimeRegistry,
    OciPortLeaseCoordinator, ReservedLaunchPorts, SandboxLaunchPortPlan,
};
use crate::error::SandboxError;
use crate::instance::SandboxId;
use crate::spec::SandboxPortBinding;

mod authority;

use authority::stale_provenance_fails_before_effects;

#[derive(Debug, Clone, Copy)]
enum ContractBackend {
    Container,
    Krun,
}

impl ContractBackend {
    fn kind(self) -> AttachmentBackendKind {
        match self {
            Self::Container => AttachmentBackendKind::Container,
            Self::Krun => AttachmentBackendKind::Krun,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Container => "container",
            Self::Krun => "krun",
        }
    }

    fn adapter<'a>(self, input: OciAttachmentInput<'a>) -> OciAttachmentAdapter<'a> {
        match self {
            Self::Container => {
                <ContainerSandboxBackend as OciHostManagedAttachmentBackend>::host_managed_attachment_adapter(input)
            }
            Self::Krun => {
                <KrunSandboxBackend as OciHostManagedAttachmentBackend>::host_managed_attachment_adapter(input)
            }
        }
    }

    fn reserve_config(
        self,
        lifecycle: &OciAttachmentLifecycle<'_>,
        tenant_id: &TenantId,
        layout: &OciNetworkLayout,
        sandbox_id: &SandboxId,
        claim: &NetworkReservationClaim,
    ) -> Result<OciNetworkConfig> {
        let netavark = PathBuf::from("netavark-contract-not-executed");
        let aardvark = PathBuf::from("aardvark-contract-not-executed");
        match self {
            Self::Container => {
                <ContainerSandboxBackend as OciHostManagedAttachmentBackend>::reserve_attachment_config(
                    lifecycle,
                    tenant_id,
                    layout,
                    sandbox_id,
                    claim,
                    netavark,
                    aardvark,
                )
            }
            Self::Krun => {
                <KrunSandboxBackend as OciHostManagedAttachmentBackend>::reserve_attachment_config(
                    lifecycle,
                    tenant_id,
                    layout,
                    sandbox_id,
                    claim,
                    netavark,
                    aardvark,
                )
            }
        }
    }
}

struct ContractFixture {
    _temp_dir: TempDir,
    allocator: RecordingSegmentAllocator,
    tenant_id: TenantId,
    sandbox_id: SandboxId,
    layout: OciNetworkLayout,
    ipam: OciIpamAuthority,
    ports: OciPortLeaseCoordinator,
    lifetimes: NetavarkPortLifetimeRegistry,
    claim: NetworkReservationClaim,
    backend: ContractBackend,
}

impl ContractFixture {
    fn new(backend: ContractBackend, row: &str) -> Self {
        let temp_dir = TempDir::new().expect("contract temporary directory should exist");
        let tenant_id = TenantId::new(format!("nnc51-{}-{row}", backend.label()))
            .expect("contract tenant should validate");
        let sandbox_id = SandboxId::new(format!("nnc51-{}-{row}", backend.label()));
        let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant_id, &sandbox_id);
        layout
            .ensure_directories()
            .expect("contract network layout should exist");
        let ipam = direct_test_ipam_authority(&layout);
        let ports = OciPortLeaseCoordinator::new(temp_dir.path(), 32_000..=32_099);
        let allocator = RecordingSegmentAllocator::new(tenant_id.clone(), "127.92.0.0/24", 92);
        let claim = reservation_claim(&format!("{}-{row}", backend.label()));
        Self {
            _temp_dir: temp_dir,
            allocator,
            tenant_id,
            sandbox_id,
            layout,
            ipam,
            ports,
            lifetimes: NetavarkPortLifetimeRegistry::default(),
            claim,
            backend,
        }
    }

    fn lifecycle(&self) -> OciAttachmentLifecycle<'_> {
        OciAttachmentLifecycle::new(&self.allocator, &self.ipam, &self.ports, &self.lifetimes)
    }

    fn reserve_config(&self) -> OciNetworkConfig {
        self.backend
            .reserve_config(
                &self.lifecycle(),
                &self.tenant_id,
                &self.layout,
                &self.sandbox_id,
                &self.claim,
            )
            .expect("contract attachment should reserve")
    }

    fn reserve_and_adopt(&self) -> OciNetworkConfig {
        let config = self.reserve_config();
        self.allocator
            .adopt_reserved_attachment(
                &self.tenant_id,
                &default_network_attachment_id(&self.sandbox_id),
                &self.claim,
            )
            .expect("contract attachment should adopt");
        config
    }

    fn reserve_published_binding(&self) -> ReservedLaunchPorts {
        let binding = SandboxPortBinding::tcp("http", 32_000, 8080);
        let mut reserved = self
            .ports
            .reserve_launch_ports_for_sandbox(
                SandboxLaunchPortPlan::new(
                    &self.tenant_id,
                    &self.sandbox_id,
                    std::slice::from_ref(&binding),
                    &[],
                ),
                &self.claim,
            )
            .expect("contract listener should reserve under the attachment claim");
        reserved
            .confirm_manifest_published()
            .expect("contract listener identities should become durable");
        reserved
    }

    fn reserve_auxiliary_listener_for(
        &self,
        sandbox_id: &SandboxId,
        bind_ip: std::net::IpAddr,
    ) -> ReservedLaunchPorts {
        let mut reserved = self
            .ports
            .reserve_launch_ports_for_sandbox(
                SandboxLaunchPortPlan::new(&self.tenant_id, sandbox_id, &[], &[])
                    .with_internal_listener(InternalListenerReservation::new(
                        "egress-pep",
                        target_for_ip(bind_ip)
                            .expect("contract PEP address should produce a portable target"),
                        PortExposure::Private,
                    )),
                &self.claim,
            )
            .expect("contract auxiliary listener should reserve under the attachment claim");
        reserved
            .confirm_manifest_published()
            .expect("contract auxiliary listener identity should become durable");
        reserved
    }

    fn host_adapter<'a>(
        &'a self,
        backend: ContractBackend,
        config: &'a OciNetworkConfig,
        bindings: &'a [SandboxPortBinding],
        leases: &'a [nimbus_network::PortLeaseRequest],
    ) -> OciAttachmentAdapter<'a> {
        let input = OciAttachmentInput {
            workload_state_root: &self.layout.workload_state_root,
            tenant_id: &self.tenant_id,
            sandbox_id: &self.sandbox_id,
            display_name: "NNC5.1 contract workload",
            hostname: "nnc51-contract",
            bindings,
            leases,
            auxiliary_listener: None,
            layout: &self.layout,
            config,
            launch_claim: Some(&self.claim),
        };
        backend.adapter(input)
    }

    fn attachment_state(
        &self,
        claim: &NetworkReservationClaim,
    ) -> NetworkAttachmentReservationState {
        self.allocator
            .inspect_attachment_reservation(
                &self.tenant_id,
                &default_network_attachment_id(&self.sandbox_id),
                claim,
            )
            .expect("contract attachment state should inspect")
    }

    fn legacy_purge_marker(&self) -> PathBuf {
        self.layout
            .workload_state_root
            .join("networks")
            .join(".legacy-nimbus0-purged")
    }
}

fn reservation_claim(label: &str) -> NetworkReservationClaim {
    let provider =
        NetworkProviderId::for_registration_key("nimbus-sandbox.attachment-contract-test");
    NetworkReservationClaim::new(
        NetworkProviderHandle::new(provider, format!("attempt:{label}"))
            .expect("contract provider handle should validate"),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContractHostOperation {
    NamespaceCreated,
    ProviderSetup,
    ProviderTeardown,
    NamespaceRemoved,
}

#[derive(Default)]
struct ContractHostEffects {
    operations: Mutex<Vec<ContractHostOperation>>,
    fail_teardown_after_intent: bool,
}

impl ContractHostEffects {
    fn with_ambiguous_teardown() -> Self {
        Self {
            operations: Mutex::new(Vec::new()),
            fail_teardown_after_intent: true,
        }
    }

    fn record(&self, operation: ContractHostOperation) {
        self.operations
            .lock()
            .expect("contract host trace lock should not be poisoned")
            .push(operation);
    }

    fn operations(&self) -> Vec<ContractHostOperation> {
        self.operations
            .lock()
            .expect("contract host trace lock should not be poisoned")
            .clone()
    }
}

impl AttachmentHostEffects for ContractHostEffects {
    fn create_namespace(&self, context: &OciAttachmentContext<'_>) -> Result<()> {
        if let Some(parent) = context.layout.netns_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to create contract namespace parent {}: {error}",
                    parent.display()
                ),
            })?;
        }
        std::fs::write(&context.layout.netns_path, b"contract namespace").map_err(|error| {
            SandboxError::OperationFailed {
                message: format!(
                    "failed to write contract namespace {}: {error}",
                    context.layout.netns_path.display()
                ),
            }
        })?;
        self.record(ContractHostOperation::NamespaceCreated);
        Ok(())
    }

    fn setup_provider(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
    ) -> Result<Vec<std::net::Ipv4Addr>> {
        let (assigned_ips, claim) =
            begin_netavark_setup(ipam, context.layout, context.config, context.sandbox_id)?;
        self.record(ContractHostOperation::ProviderSetup);
        complete_netavark_setup(ipam, context.layout, &claim)?;
        Ok(assigned_ips)
    }

    fn teardown_provider(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
    ) -> Result<()> {
        let plan = begin_netavark_teardown(
            ipam,
            context.layout,
            context.config,
            context.sandbox_id,
            None,
        )?;
        self.record(ContractHostOperation::ProviderTeardown);
        if self.fail_teardown_after_intent {
            return Err(SandboxError::OperationFailed {
                message: "injected ambiguous provider teardown after durable intent".to_owned(),
            });
        }
        match plan {
            NetavarkTeardownPlan::AlreadyDetached => Ok(()),
            NetavarkTeardownPlan::RemoveProjection { claim } => {
                complete_netavark_teardown(ipam, context.layout, &claim)
            }
            NetavarkTeardownPlan::Run { claim, .. } => {
                confirm_netavark_provider_detached(ipam, context.layout, &claim)?;
                complete_netavark_teardown(ipam, context.layout, &claim)
            }
        }
    }

    fn remove_namespace(&self, context: &OciAttachmentContext<'_>) -> Result<()> {
        match std::fs::remove_file(&context.layout.netns_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "failed to remove contract namespace {}: {error}",
                        context.layout.netns_path.display()
                    ),
                });
            }
        }
        self.record(ContractHostOperation::NamespaceRemoved);
        Ok(())
    }
}

struct ContractPhaseObserver {
    phases: Vec<AttachmentAttachPhase>,
    fail_at: Option<AttachmentAttachPhase>,
}

impl ContractPhaseObserver {
    fn recording() -> Self {
        Self {
            phases: Vec::new(),
            fail_at: None,
        }
    }

    fn failing_at(phase: AttachmentAttachPhase) -> Self {
        Self {
            phases: Vec::new(),
            fail_at: Some(phase),
        }
    }
}

impl AttachmentPhaseObserver for ContractPhaseObserver {
    fn checkpoint(&mut self, phase: AttachmentAttachPhase) -> Result<()> {
        self.phases.push(phase);
        if self.fail_at == Some(phase) {
            return Err(SandboxError::OperationFailed {
                message: format!("injected attach phase failure at {phase:?}"),
            });
        }
        Ok(())
    }
}

struct MissingRegisteredLifetimeObserver<'a> {
    registry: &'a NetavarkPortLifetimeRegistry,
    tenant_id: &'a TenantId,
    sandbox_id: &'a SandboxId,
    fail_at: AttachmentAttachPhase,
    removed_batch: Option<OciPortBindLifetimeBatch>,
}

impl AttachmentPhaseObserver for MissingRegisteredLifetimeObserver<'_> {
    fn checkpoint(&mut self, phase: AttachmentAttachPhase) -> Result<()> {
        if phase == AttachmentAttachPhase::LifetimeRegistered {
            self.removed_batch = self
                .registry
                .take(self.tenant_id, self.sandbox_id)?
                .or_else(|| self.removed_batch.take());
        }
        if phase == self.fail_at {
            return Err(SandboxError::OperationFailed {
                message: format!("injected exact primary at {phase:?}"),
            });
        }
        Ok(())
    }
}

const ATTACH_PHASES: [AttachmentAttachPhase; 11] = [
    AttachmentAttachPhase::GenerationAuthenticated,
    AttachmentAttachPhase::LeasesAuthenticated,
    AttachmentAttachPhase::AuthorityAuthenticated,
    AttachmentAttachPhase::LegacyBridgePurged,
    AttachmentAttachPhase::NamespaceCreated,
    AttachmentAttachPhase::ListenerClaimsHeld,
    AttachmentAttachPhase::ProviderSetupComplete,
    AttachmentAttachPhase::ListenerBindingsActive,
    AttachmentAttachPhase::BackendPublicationComplete,
    AttachmentAttachPhase::LifetimeRegistered,
    AttachmentAttachPhase::AttachmentConfirmed,
];

// Row 1: PlanOnly owns rendering only and never enters the attachment seam.
fn plan_only_has_zero_attachment_effects(backend: ContractBackend) {
    let fixture = ContractFixture::new(backend, "plan-only");
    let config = OciNetworkConfig::default();
    let before = fixture.allocator.operations();
    let _adapter = fixture.host_adapter(backend, &config, &[], &[]);

    assert_eq!(fixture.allocator.operations(), before);
    assert!(
        fixture
            .ports
            .authority()
            .expect("port authority should open")
            .list()
            .expect("port authority should inspect")
            .is_empty(),
        "constructing the real adapter must not reserve ports"
    );
    assert!(!fixture.legacy_purge_marker().exists());
    assert!(!fixture.layout.netns_path.exists());
}

// Row 2: the exact coordinator claim is established before attachment/IPAM/ports.
fn claim_precedes_all_reservations(backend: ContractBackend) {
    let temp_dir = TempDir::new().expect("contract temporary directory should exist");
    let tenant_id = TenantId::new(format!("nnc51-{}-claim-order", backend.label()))
        .expect("contract tenant should validate");
    let sandbox_id = SandboxId::new(format!("nnc51-{}-claim-order", backend.label()));
    let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant_id, &sandbox_id);
    layout
        .ensure_directories()
        .expect("contract network layout should exist");
    let ipam = direct_test_ipam_authority(&layout);
    let ports = OciPortLeaseCoordinator::new(temp_dir.path(), 32_100..=32_199);
    let observed = Arc::new(AtomicBool::new(false));
    let observed_for_reservation = Arc::clone(&observed);
    let ipam_for_reservation = ipam.clone();
    let layout_for_reservation = layout.clone();
    let sandbox_for_reservation = sandbox_id.clone();
    let ports_for_reservation = ports.clone();
    let allocator = RecordingSegmentAllocator::new(tenant_id.clone(), "127.93.0.0/24", 93)
        .with_reserve_attachment_observer(move |_| {
            assert!(
                inspect_container_ips(
                    &ipam_for_reservation,
                    &layout_for_reservation,
                    &sandbox_for_reservation
                )
                .is_err(),
                "IPAM must be absent at the attachment reservation boundary"
            );
            assert!(
                ports_for_reservation
                    .authority()
                    .expect("port authority should open")
                    .list()
                    .expect("port authority should inspect")
                    .is_empty(),
                "port authority must be empty at the attachment reservation boundary"
            );
            observed_for_reservation.store(true, Ordering::SeqCst);
            Ok(())
        });
    let lifetimes = NetavarkPortLifetimeRegistry::default();
    let claim = reservation_claim(&format!("{}-claim-order", backend.label()));
    let lifecycle = OciAttachmentLifecycle::new(&allocator, &ipam, &ports, &lifetimes);

    backend
        .reserve_config(&lifecycle, &tenant_id, &layout, &sandbox_id, &claim)
        .expect("ordered reservation should succeed");

    assert!(
        observed.load(Ordering::SeqCst),
        "the first attachment reservation must be observed"
    );
    let operations = allocator.operations();
    assert!(
        matches!(
            operations.as_slice(),
            [
                SegmentAllocatorOperation::ReserveAttachment(..),
                SegmentAllocatorOperation::SegmentsFor(..),
                SegmentAllocatorOperation::BindAttachment(..)
            ]
        ),
        "reservation must follow one attachment -> IPAM -> segment-bind trace: {operations:?}"
    );
}

// Row 3: generation and exact listener identity fence every filesystem/provider effect.
fn generation_and_leases_precede_effects(backend: ContractBackend) {
    let fixture = ContractFixture::new(backend, "pre-effect-auth");
    let config = fixture.reserve_and_adopt();
    let binding = SandboxPortBinding::tcp("http", 32_222, 8080);
    let bindings = vec![binding];
    let before = fixture.allocator.operations();
    let error = fixture
        .host_adapter(backend, &config, &bindings, &[])
        .attach(
            &fixture.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
            |_| Ok(()),
        )
        .expect_err("missing exact listener lease must fail before effects");

    assert!(
        error.to_string().contains("published bindings")
            && error.to_string().contains("durable port leases"),
        "lease-fence diagnostic should identify the mismatched request set: {error}"
    );
    assert_eq!(
        fixture.allocator.operations(),
        before,
        "lease authentication must not mutate attachment authority"
    );
    assert!(
        !fixture.legacy_purge_marker().exists() && !fixture.layout.netns_path.exists(),
        "lease authentication must precede legacy-purge and netns filesystem effects"
    );
}

// Row 4: both adapters execute one source-owned canonical attach trace.
fn happy_attach_has_one_canonical_trace(backend: ContractBackend) {
    let fixture = ContractFixture::new(backend, "happy-trace");
    let config = fixture.reserve_and_adopt();
    let host = ContractHostEffects::default();
    let mut observer = ContractPhaseObserver::recording();
    let callback_seen = AtomicBool::new(false);

    let assigned = fixture
        .host_adapter(backend, &config, &[], &[])
        .attach_with(
            &fixture.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
            &host,
            &mut observer,
            |assigned_ips| {
                assert!(!assigned_ips.is_empty());
                callback_seen.store(true, Ordering::SeqCst);
                Ok(())
            },
        )
        .expect("canonical attach should succeed");

    assert!(callback_seen.load(Ordering::SeqCst));
    assert_eq!(observer.phases, ATTACH_PHASES);
    assert_eq!(
        host.operations(),
        vec![
            ContractHostOperation::NamespaceCreated,
            ContractHostOperation::ProviderSetup,
        ]
    );
    assert_eq!(
        assigned,
        inspect_container_ips(&fixture.ipam, &fixture.layout, &fixture.sandbox_id)
            .expect("live attached IPAM should inspect")
    );
    assert_eq!(
        fixture.attachment_state(&fixture.claim),
        NetworkAttachmentReservationState::Adopted
    );
}

// Row 5: every represented phase failure re-enters the one compensation owner.
fn represented_attach_failures_reverse_compensate(backend: ContractBackend) {
    for phase in ATTACH_PHASES {
        let row = format!("reverse-{}", format!("{phase:?}").to_ascii_lowercase());
        let fixture = ContractFixture::new(backend, &row);
        let config = fixture.reserve_and_adopt();
        let host = ContractHostEffects::default();
        let mut observer = ContractPhaseObserver::failing_at(phase);

        let error = fixture
            .host_adapter(backend, &config, &[], &[])
            .attach_with(
                &fixture.lifecycle(),
                AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
                &host,
                &mut observer,
                |_| Ok(()),
            )
            .expect_err("injected attach checkpoint must fail");

        assert!(
            error
                .to_string()
                .contains(&format!("injected attach phase failure at {phase:?}")),
            "the injected primary must survive {phase:?} compensation: {error}"
        );
        assert!(
            !fixture.layout.netns_path.exists(),
            "{phase:?} compensation must remove only a completed namespace phase"
        );
        assert_eq!(
            fixture.attachment_state(&fixture.claim),
            NetworkAttachmentReservationState::Adopted,
            "{phase:?} compensation must retain the exact attachment generation"
        );
        assert!(
            inspect_container_ips(&fixture.ipam, &fixture.layout, &fixture.sandbox_id).is_ok(),
            "{phase:?} compensation must retain exact IPAM for retry"
        );
        let phase_index = ATTACH_PHASES
            .iter()
            .position(|candidate| candidate == &phase)
            .expect("phase belongs to the frozen matrix");
        if phase_index >= 4 {
            let operations = host.operations();
            assert_eq!(
                operations.first(),
                Some(&ContractHostOperation::NamespaceCreated),
                "{phase:?} must compensate only after namespace creation"
            );
            assert_eq!(
                operations.last(),
                Some(&ContractHostOperation::NamespaceRemoved),
                "{phase:?} must finish reverse compensation at namespace removal"
            );
        } else {
            assert!(
                host.operations().is_empty(),
                "{phase:?} precedes every host effect"
            );
        }
    }
}

// Row 6: a successful compensation returns the original primary diagnostic.
fn primary_failure_survives_cleanup(backend: ContractBackend) {
    let fixture = ContractFixture::new(backend, "primary");
    let config = fixture.reserve_and_adopt();
    let primary = SandboxError::OperationFailed {
        message: format!("{} exact primary", backend.label()),
    };

    let error = fixture
        .host_adapter(backend, &config, &[], &[])
        .complete_injected_setup(&fixture.lifecycle(), Err(primary))
        .expect_err("injected setup failure must be returned");

    assert_eq!(
        error.to_string(),
        format!(
            "sandbox operation failed: {} exact primary",
            backend.label()
        ),
        "successful compensation must not replace or wrap the primary failure"
    );

    for fail_at in [
        AttachmentAttachPhase::LifetimeRegistered,
        AttachmentAttachPhase::AttachmentConfirmed,
    ] {
        let row = format!(
            "missing-registered-lifetime-{}",
            format!("{fail_at:?}").to_ascii_lowercase()
        );
        let fixture = ContractFixture::new(backend, &row);
        let config = fixture.reserve_and_adopt();
        let reserved = fixture.reserve_published_binding();
        let host = ContractHostEffects::default();
        let mut observer = MissingRegisteredLifetimeObserver {
            registry: &fixture.lifetimes,
            tenant_id: &fixture.tenant_id,
            sandbox_id: &fixture.sandbox_id,
            fail_at,
            removed_batch: None,
        };

        let error = fixture
            .host_adapter(
                backend,
                &config,
                &reserved.published_bindings,
                &reserved.published_leases,
            )
            .attach_with(
                &fixture.lifecycle(),
                AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
                &host,
                &mut observer,
                |_| Ok(()),
            )
            .expect_err("lost registered lifetime must retain the primary failure");
        let rendered = error.to_string();
        assert!(
            rendered.contains(&format!("injected exact primary at {fail_at:?}"))
                && rendered.contains("lost its exact live Netavark lifetime batch")
                && rendered.contains("provider remains fenced"),
            "registry recovery must preserve both diagnostics and the live fence: {rendered}"
        );
        assert!(
            observer.removed_batch.is_some(),
            "the test must retain the exact live lifetime outside the missing registry entry"
        );
        assert_eq!(
            host.operations(),
            vec![
                ContractHostOperation::NamespaceCreated,
                ContractHostOperation::ProviderSetup,
            ],
            "missing exact compensation authority must not claim provider or namespace absence"
        );
        assert!(fixture.layout.netns_path.exists());
        let listener = LocalPortLeaseAuthority::open(fixture._temp_dir.path())
            .expect("authority should reopen")
            .inspect(reserved.published_leases[0].lease_id())
            .expect("listener should inspect")
            .expect("listener evidence should remain durable");
        assert_eq!(
            listener.phase(),
            PortLeasePhase::Active,
            "the exact live listener must remain fenced for reconciliation"
        );
    }
}

// Row 7: ambiguous compensation preserves every exact retry witness.
fn ambiguous_compensation_retains_fences(backend: ContractBackend) {
    let fixture = ContractFixture::new(backend, "ambiguous");
    let config = fixture.reserve_and_adopt();
    let host = ContractHostEffects::with_ambiguous_teardown();
    let mut observer =
        ContractPhaseObserver::failing_at(AttachmentAttachPhase::ProviderSetupComplete);

    let error = fixture
        .host_adapter(backend, &config, &[], &[])
        .attach_with(
            &fixture.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
            &host,
            &mut observer,
            |_| Ok(()),
        )
        .expect_err("ambiguous compensation must fail closed");

    let rendered = error.to_string();
    assert!(
        rendered.contains("injected attach phase failure at ProviderSetupComplete")
            && rendered.contains("namespace remains fenced")
            && rendered.contains("injected ambiguous provider teardown"),
        "ambiguity must preserve the primary and explain the retained fence: {rendered}"
    );
    assert!(fixture.layout.netns_path.exists());
    assert_eq!(
        host.operations(),
        vec![
            ContractHostOperation::NamespaceCreated,
            ContractHostOperation::ProviderSetup,
            ContractHostOperation::ProviderTeardown,
        ],
        "an ambiguous teardown must not claim namespace removal"
    );
    assert!(
        format!(
            "{:?}",
            inspect_netavark_provider_operation(
                &fixture.ipam,
                &fixture.layout,
                &config,
                &fixture.sandbox_id
            )
            .expect("ambiguous provider operation should inspect")
        )
        .contains("Deleting"),
        "ambiguous teardown must retain the exact provider attempt"
    );
    assert_eq!(
        inspect_container_ips(&fixture.ipam, &fixture.layout, &fixture.sandbox_id)
            .expect("exact IPAM generation must remain"),
        allocate_container_ips(&fixture.ipam, &fixture.layout, &config, &fixture.sandbox_id)
            .expect("idempotent allocation should return the retained generation")
    );
    assert_eq!(
        fixture.attachment_state(&fixture.claim),
        NetworkAttachmentReservationState::Adopted,
        "ambiguous provider compensation must retain the adopted segment hold"
    );
}

// Row 8: retry after exact compensation creates one live desired attachment.
fn retry_is_idempotent(backend: ContractBackend) {
    let fixture = ContractFixture::new(backend, "retry");
    let config = fixture.reserve_and_adopt();
    let original_ips = inspect_container_ips(&fixture.ipam, &fixture.layout, &fixture.sandbox_id)
        .expect("retry fixture should own IPAM");
    let adapter = fixture.host_adapter(backend, &config, &[], &[]);
    let host = ContractHostEffects::default();
    let mut failing =
        ContractPhaseObserver::failing_at(AttachmentAttachPhase::BackendPublicationComplete);
    adapter
        .attach_with(
            &fixture.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
            &host,
            &mut failing,
            |_| Ok(()),
        )
        .expect_err("the first attempt should enter exact compensation");

    let mut retry = ContractPhaseObserver::recording();
    let retry_ips = adapter
        .attach_with(
            &fixture.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
            &host,
            &mut retry,
            |_| Ok(()),
        )
        .expect("exact compensated retry should succeed");

    assert_eq!(retry.phases, ATTACH_PHASES);
    assert_eq!(retry_ips, original_ips);
    assert_eq!(
        inspect_container_ips(&fixture.ipam, &fixture.layout, &fixture.sandbox_id)
            .expect("retry must retain one IPAM generation"),
        original_ips
    );
    assert_eq!(
        fixture.attachment_state(&fixture.claim),
        NetworkAttachmentReservationState::Adopted
    );
    assert_eq!(
        fixture
            .allocator
            .operations()
            .iter()
            .filter(|operation| matches!(operation, SegmentAllocatorOperation::Acquire(..)))
            .count(),
        1,
        "only the successful retry may confirm the one live attachment"
    );
    assert_eq!(
        host.operations()
            .iter()
            .filter(|operation| **operation == ContractHostOperation::NamespaceCreated)
            .count(),
        2
    );
    assert_eq!(
        host.operations()
            .iter()
            .filter(|operation| **operation == ContractHostOperation::ProviderSetup)
            .count(),
        2
    );
    assert_eq!(
        host.operations()
            .iter()
            .filter(|operation| **operation == ContractHostOperation::ProviderTeardown)
            .count(),
        1
    );
}

// Row 9: live/unknown runtime evidence cannot begin provider or authority cleanup.
fn runtime_absence_precedes_detach(backend: ContractBackend) {
    let fixture = ContractFixture::new(backend, "runtime-fence");
    let config = fixture.reserve_and_adopt();
    let reserved = fixture.reserve_published_binding();
    let adapter = fixture.host_adapter(
        backend,
        &config,
        &reserved.published_bindings,
        &reserved.published_leases,
    );
    let host = ContractHostEffects::default();
    let mut observer = ContractPhaseObserver::recording();
    adapter
        .attach_with(
            &fixture.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
            &host,
            &mut observer,
            |_| Ok(()),
        )
        .expect("runtime-fence fixture should own a live provider attachment");
    let before = fixture.allocator.operations();
    let before_host = host.operations();
    let authority =
        LocalPortLeaseAuthority::open(fixture._temp_dir.path()).expect("authority should reopen");
    let before_listener = authority
        .inspect(reserved.published_leases[0].lease_id())
        .expect("listener should inspect")
        .expect("listener should remain durable");
    assert_eq!(before_listener.phase(), PortLeasePhase::Active);

    let failure = adapter
        .detach_host_managed_with(
            &fixture.lifecycle(),
            AttachmentTeardownMode::Final,
            &host,
            |_| {
                Err(SandboxError::OperationFailed {
                    message: "runtime remains live or unknown".to_owned(),
                })
            },
        )
        .expect_err("unknown runtime evidence must reject detach");

    assert_eq!(
        failure.stage(),
        AttachmentDetachFailureStage::BeforeProviderDetach
    );
    assert!(
        failure
            .into_error()
            .to_string()
            .contains("runtime remains live or unknown")
    );
    assert_eq!(
        fixture.allocator.operations(),
        before,
        "runtime absence must be proven before quarantine or release authority mutates"
    );
    assert_eq!(
        host.operations(),
        before_host,
        "runtime absence must be proven before provider or namespace teardown"
    );
    assert_eq!(
        authority
            .inspect(reserved.published_leases[0].lease_id())
            .expect("listener should re-inspect")
            .expect("listener should remain durable"),
        before_listener,
        "runtime ambiguity must retain the exact active listener generation"
    );
    assert_eq!(
        fixture.attachment_state(&fixture.claim),
        NetworkAttachmentReservationState::Adopted
    );
    assert!(
        inspect_container_ips(&fixture.ipam, &fixture.layout, &fixture.sandbox_id).is_ok(),
        "unknown runtime evidence must retain IPAM"
    );
}

// Row 10: restart detach retains generation/IPAM/segment and prepares reuse.
fn restart_detach_retains_authority(backend: ContractBackend) {
    let fixture = ContractFixture::new(backend, "restart");
    let config = fixture.reserve_and_adopt();
    let reserved = fixture.reserve_published_binding();
    let adapter = fixture.host_adapter(
        backend,
        &config,
        &reserved.published_bindings,
        &reserved.published_leases,
    );
    let host = ContractHostEffects::default();
    let mut observer = ContractPhaseObserver::recording();
    adapter
        .attach_with(
            &fixture.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
            &host,
            &mut observer,
            |_| Ok(()),
        )
        .expect("restart fixture should own a live provider attachment");
    let before_ips = inspect_container_ips(&fixture.ipam, &fixture.layout, &fixture.sandbox_id)
        .expect("restart fixture should own IPAM");
    let callback_seen = AtomicBool::new(false);

    adapter
        .detach_host_managed_with(
            &fixture.lifecycle(),
            AttachmentTeardownMode::Restart,
            &host,
            |auxiliary| {
                assert_eq!(
                    auxiliary,
                    AttachmentAuxiliaryDisposition::ProviderOwned,
                    "restart retains backend-owned auxiliary publication authority"
                );
                callback_seen.store(true, Ordering::SeqCst);
                Ok(())
            },
        )
        .expect("restart detach should succeed");

    assert!(callback_seen.load(Ordering::SeqCst));
    assert_eq!(
        inspect_container_ips(&fixture.ipam, &fixture.layout, &fixture.sandbox_id)
            .expect("restart must retain IPAM"),
        before_ips
    );
    assert_eq!(
        fixture.attachment_state(&fixture.claim),
        NetworkAttachmentReservationState::Adopted,
        "restart must retain the exact segment generation"
    );
    assert!(
        !fixture
            .allocator
            .operations()
            .iter()
            .any(|operation| matches!(
                operation,
                SegmentAllocatorOperation::Quarantine(..)
                    | SegmentAllocatorOperation::Release(..)
                    | SegmentAllocatorOperation::FinalizeRelease(..)
            )),
        "restart must not enter terminal segment release"
    );
    assert_eq!(
        fixture
            .ports
            .classify_netavark_cleanup_batch(
                &fixture.tenant_id,
                &fixture.sandbox_id,
                &reserved.published_bindings,
                &reserved.published_leases,
                None,
            )
            .expect("restart-retained listener should classify"),
        LaunchPortBatchState::RestartRetained,
        "restart detach must retain the exact host-port generation for rebind"
    );
}

// Row 11: final detach releases only after provider/netns absence.
fn final_detach_releases_after_absence(backend: ContractBackend) {
    let fixture = ContractFixture::new(backend, "final");
    let config = fixture.reserve_and_adopt();
    let reserved = fixture.reserve_published_binding();
    let adapter = fixture.host_adapter(
        backend,
        &config,
        &reserved.published_bindings,
        &reserved.published_leases,
    );
    let host = ContractHostEffects::default();
    let mut observer = ContractPhaseObserver::recording();
    adapter
        .attach_with(
            &fixture.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
            &host,
            &mut observer,
            |_| Ok(()),
        )
        .expect("final fixture should own a live provider attachment");
    adapter
        .detach_host_managed_with(
            &fixture.lifecycle(),
            AttachmentTeardownMode::Final,
            &host,
            |_| Ok(()),
        )
        .expect("final detach should converge");

    assert!(!fixture.layout.status_path.exists());
    assert!(!fixture.layout.netns_path.exists());
    assert!(
        inspect_container_ips(&fixture.ipam, &fixture.layout, &fixture.sandbox_id).is_err(),
        "final detach must remove live IPAM after provider absence"
    );
    assert_eq!(
        fixture.attachment_state(&fixture.claim),
        NetworkAttachmentReservationState::Absent,
        "final detach must release exact attachment authority"
    );
    let listener = LocalPortLeaseAuthority::open(fixture._temp_dir.path())
        .expect("authority should reopen")
        .inspect(reserved.published_leases[0].lease_id())
        .expect("listener should inspect")
        .expect("released listener evidence should remain durable");
    assert_eq!(
        listener.phase(),
        PortLeasePhase::Released,
        "final detach must release the host-port lease only after provider absence"
    );
    let operations = fixture.allocator.operations();
    let quarantine = operations
        .iter()
        .position(|operation| matches!(operation, SegmentAllocatorOperation::Quarantine(..)))
        .expect("final detach should quarantine");
    let release = operations
        .iter()
        .position(|operation| matches!(operation, SegmentAllocatorOperation::Release(..)))
        .expect("final detach should release");
    assert!(
        quarantine < release,
        "quarantine must precede release: {operations:?}"
    );
}

// Row 13: machine forwarding is an explicit container-only capability.
fn machine_forwarding_capability_is_explicit(backend: ContractBackend) {
    let fixture = ContractFixture::new(backend, "machine-mode");
    let config = OciNetworkConfig::default();
    let forwarder = OciMachinePortForwarderConfig::gvproxy_for_provider_instance(
        format!("{}-contract-provider", backend.label()),
        NetworkResourceGeneration::new(1),
    )
    .expect("machine forwarder fixture should validate");
    let input = OciAttachmentInput {
        workload_state_root: &fixture.layout.workload_state_root,
        tenant_id: &fixture.tenant_id,
        sandbox_id: &fixture.sandbox_id,
        display_name: "machine mode",
        hostname: "machine-mode",
        bindings: &[],
        leases: &[],
        auxiliary_listener: None,
        layout: &fixture.layout,
        config: &config,
        launch_claim: None,
    };
    let adapter = match backend {
        ContractBackend::Container => {
            <ContainerSandboxBackend as OciMachineForwardedAttachmentBackend>::machine_forwarded_attachment_adapter(
                input, &forwarder,
            )
        }
        ContractBackend::Krun => {
            <KrunSandboxBackend as OciHostManagedAttachmentBackend>::host_managed_attachment_adapter(
                input,
            )
        }
    };

    let error = adapter
        .attach(
            &fixture.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
            |_| Ok(()),
        )
        .expect_err("fixture intentionally has no IPAM authority");
    match backend {
        ContractBackend::Container => {
            assert!(
                matches!(
                    adapter.context.publication,
                    AttachmentPublicationMode::MachineForwarded(_)
                ) && !matches!(error, SandboxError::BackendUnavailable { .. }),
                "container must route through its explicit machine-forwarded capability"
            );
        }
        ContractBackend::Krun => {
            assert!(
                matches!(
                    adapter.context.publication,
                    AttachmentPublicationMode::HostManaged
                ) && !matches!(error, SandboxError::BackendUnavailable { .. }),
                "krun has only the host-managed capability and cannot construct a \
                 machine-forwarded adapter"
            );
        }
    }
    assert!(!fixture.legacy_purge_marker().exists());
    assert!(!fixture.layout.netns_path.exists());
}

// Row 14: terminal cleanup is an idempotent convergence operation.
fn repeated_cleanup_is_idempotent(backend: ContractBackend) {
    let fixture = ContractFixture::new(backend, "cleanup-replay");
    let config = fixture.reserve_and_adopt();
    let reserved = fixture.reserve_published_binding();
    let adapter = fixture.host_adapter(
        backend,
        &config,
        &reserved.published_bindings,
        &reserved.published_leases,
    );
    let host = ContractHostEffects::default();
    let mut observer = ContractPhaseObserver::recording();
    adapter
        .attach_with(
            &fixture.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
            &host,
            &mut observer,
            |_| Ok(()),
        )
        .expect("cleanup replay fixture should own a live provider attachment");
    for attempt in 1..=2 {
        adapter
            .detach_host_managed_with(
                &fixture.lifecycle(),
                AttachmentTeardownMode::Final,
                &host,
                |_| Ok(()),
            )
            .unwrap_or_else(|failure| {
                panic!(
                    "final cleanup attempt {attempt} should converge: {}",
                    failure.into_error()
                )
            });
    }
    assert_eq!(
        fixture.attachment_state(&fixture.claim),
        NetworkAttachmentReservationState::Absent
    );
    assert!(inspect_container_ips(&fixture.ipam, &fixture.layout, &fixture.sandbox_id).is_err());
    assert_eq!(
        fixture
            .ports
            .classify_netavark_cleanup_batch(
                &fixture.tenant_id,
                &fixture.sandbox_id,
                &reserved.published_bindings,
                &reserved.published_leases,
                None,
            )
            .expect("replayed terminal listener should classify"),
        LaunchPortBatchState::TerminalNoEffect,
        "cleanup replay must not recreate or release a second listener generation"
    );
}

// Row 15: the two real manifest adapters route through this same contract owner.
fn real_adapters_share_the_contract_owner(backend: ContractBackend) {
    let fixture = ContractFixture::new(backend, "real-adapter-compensation");
    let config = fixture.reserve_and_adopt();
    let adapter = fixture.host_adapter(backend, &config, &[], &[]);
    assert_eq!(adapter.context.backend, backend.kind());
    assert_eq!(adapter.context.provider_label, backend.label());
    assert!(matches!(
        adapter.context.publication,
        AttachmentPublicationMode::HostManaged
    ));
    let host = ContractHostEffects::default();
    let mut observer = ContractPhaseObserver::recording();
    let error = adapter
        .attach_with(
            &fixture.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
            &host,
            &mut observer,
            |_| {
                Err(SandboxError::OperationFailed {
                    message: "real adapter callback sentinel".to_owned(),
                })
            },
        )
        .expect_err("the real adapter must route callback failure through compensation");
    assert!(error.to_string().contains("real adapter callback sentinel"));
    assert_eq!(
        host.operations(),
        vec![
            ContractHostOperation::NamespaceCreated,
            ContractHostOperation::ProviderSetup,
            ContractHostOperation::ProviderTeardown,
            ContractHostOperation::NamespaceRemoved,
        ],
        "the concrete adapter must enter the shared reverse-compensation owner"
    );

    let fixture = ContractFixture::new(backend, "real-adapter-detach");
    let config = fixture.reserve_and_adopt();
    let adapter = fixture.host_adapter(backend, &config, &[], &[]);
    let host = ContractHostEffects::default();
    let mut observer = ContractPhaseObserver::recording();
    adapter
        .attach_with(
            &fixture.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
            &host,
            &mut observer,
            |_| Ok(()),
        )
        .expect("the real adapter should attach through the shared owner");
    let detach_callback_seen = AtomicBool::new(false);
    adapter
        .detach_host_managed_with(
            &fixture.lifecycle(),
            AttachmentTeardownMode::Final,
            &host,
            |_| {
                detach_callback_seen.store(true, Ordering::SeqCst);
                Ok(())
            },
        )
        .expect("the real adapter should detach through the shared owner");
    assert!(detach_callback_seen.load(Ordering::SeqCst));
    assert_eq!(
        fixture.attachment_state(&fixture.claim),
        NetworkAttachmentReservationState::Absent
    );
}

macro_rules! shared_contract_row {
    ($container:ident, $krun:ident, $case:ident) => {
        #[test]
        fn $container() {
            $case(ContractBackend::Container);
        }

        #[test]
        fn $krun() {
            $case(ContractBackend::Krun);
        }
    };
}

shared_contract_row!(
    container_01_plan_only_has_zero_attachment_effects,
    krun_01_plan_only_has_zero_attachment_effects,
    plan_only_has_zero_attachment_effects
);
shared_contract_row!(
    container_02_claim_precedes_all_reservations,
    krun_02_claim_precedes_all_reservations,
    claim_precedes_all_reservations
);
shared_contract_row!(
    container_03_generation_and_leases_precede_effects,
    krun_03_generation_and_leases_precede_effects,
    generation_and_leases_precede_effects
);
shared_contract_row!(
    container_04_happy_attach_has_one_canonical_trace,
    krun_04_happy_attach_has_one_canonical_trace,
    happy_attach_has_one_canonical_trace
);
shared_contract_row!(
    container_05_represented_attach_failures_reverse_compensate,
    krun_05_represented_attach_failures_reverse_compensate,
    represented_attach_failures_reverse_compensate
);
shared_contract_row!(
    container_06_primary_failure_survives_cleanup,
    krun_06_primary_failure_survives_cleanup,
    primary_failure_survives_cleanup
);
shared_contract_row!(
    container_07_ambiguous_compensation_retains_fences,
    krun_07_ambiguous_compensation_retains_fences,
    ambiguous_compensation_retains_fences
);
shared_contract_row!(
    container_08_retry_is_idempotent,
    krun_08_retry_is_idempotent,
    retry_is_idempotent
);
shared_contract_row!(
    container_09_runtime_absence_precedes_detach,
    krun_09_runtime_absence_precedes_detach,
    runtime_absence_precedes_detach
);
shared_contract_row!(
    container_10_restart_detach_retains_authority,
    krun_10_restart_detach_retains_authority,
    restart_detach_retains_authority
);
shared_contract_row!(
    container_11_final_detach_releases_after_absence,
    krun_11_final_detach_releases_after_absence,
    final_detach_releases_after_absence
);
shared_contract_row!(
    container_12_stale_provenance_fails_before_effects,
    krun_12_stale_provenance_fails_before_effects,
    stale_provenance_fails_before_effects
);
shared_contract_row!(
    container_13_machine_forwarding_capability_is_explicit,
    krun_13_machine_forwarding_capability_is_explicit,
    machine_forwarding_capability_is_explicit
);
shared_contract_row!(
    container_14_repeated_cleanup_is_idempotent,
    krun_14_repeated_cleanup_is_idempotent,
    repeated_cleanup_is_idempotent
);
shared_contract_row!(
    container_15_real_adapters_share_the_contract_owner,
    krun_15_real_adapters_share_the_contract_owner,
    real_adapters_share_the_contract_owner
);
