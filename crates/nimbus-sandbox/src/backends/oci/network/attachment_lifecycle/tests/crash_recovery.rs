//! NNC5.4 real-process crash cuts for the shared OCI attachment lifecycle.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use nimbus_network::{
    DurableNetworkAttachmentState, NetworkAttachmentSegmentAssociation, NetworkResourcePhase,
    NetworkResourceVersion, PortLeasePhase, PortLeaseRequest,
};
use nimbus_process_harness::{
    CrashCutChildContext, ProcessRoleSpec, SubprocessCrashCutHarness, SubprocessCrashCutResult,
    run_crash_cut_child, run_crash_recovery_child,
};
use serde::{Deserialize, Serialize};

use super::*;
use crate::backends::oci::network::SingleNodeSegmentAllocator;
use crate::backends::oci::network::ipam::{
    ContainerIpamAuthorityState, inspect_container_ipam_authority, load_released_container_ips,
};
use crate::backends::oci::network::netavark::authenticate_container_network_generation;

const CREATE_CRASH_CHILD: &str = concat!(
    "backends::oci::network::attachment_lifecycle::tests::crash_recovery::",
    "attachment_create_crash_child"
);
const CREATE_RECOVERY_CHILD: &str = concat!(
    "backends::oci::network::attachment_lifecycle::tests::crash_recovery::",
    "attachment_create_recovery_child"
);
const DELETE_CRASH_CHILD: &str = concat!(
    "backends::oci::network::attachment_lifecycle::tests::crash_recovery::",
    "attachment_delete_crash_child"
);
const DELETE_RECOVERY_CHILD: &str = concat!(
    "backends::oci::network::attachment_lifecycle::tests::crash_recovery::",
    "attachment_delete_recovery_child"
);
const DELETE_REPLAY_CHILD: &str = concat!(
    "backends::oci::network::attachment_lifecycle::tests::crash_recovery::",
    "attachment_delete_replay_child"
);
const REPLAY_GATE_CHILD: &str = concat!(
    "backends::oci::network::attachment_lifecycle::tests::crash_recovery::",
    "attachment_replay_gate_child"
);

const BACKEND_ENV: &str = "NIMBUS_NNC54_ATTACHMENT_BACKEND";
const CUT_ENV: &str = "NIMBUS_NNC54_ATTACHMENT_CUT";
const CREATE_RECOVERY_WITNESS: &str = "attachment.create.recovered.durable";
const DELETE_RECOVERY_WITNESS: &str = "attachment.delete.recovered.durable";
const PRE_CRASH_WITNESS: &str = "attachment.pre-crash-witness.json";
const CHILD_TIMEOUT: Duration = Duration::from_secs(15);
const REPLAY_BOUNDARY: &str = "attachment.replay.fresh-process";
const CREATE_ACTIVE_OBSERVATION: &str = "attachment.create.recovered-active";
const CREATE_CLEANUP_OBSERVATION: &str = "attachment.create.recovered-cleanup-pending";
const CREATE_REPLAY_OBSERVATION: &str = "attachment.create.replay-stable";
const DELETE_RECOVERY_OBSERVATION: &str = "attachment.delete.recovered-released";
const DELETE_REPLAY_OBSERVATION: &str = "attachment.delete.replay-stable";

#[derive(Clone, Copy, PartialEq, Eq)]
enum CreateRecoveryOutcome {
    Active,
    CleanupPending,
}

struct CreateCut {
    label: &'static str,
    phase: AttachmentAttachPhase,
    outcome: CreateRecoveryOutcome,
}

impl CreateCut {
    fn requires_stable_handle(&self) -> bool {
        matches!(
            self.phase,
            AttachmentAttachPhase::ProviderSetupComplete
                | AttachmentAttachPhase::Publishing
                | AttachmentAttachPhase::ListenerBindingsActive
                | AttachmentAttachPhase::BackendPublicationComplete
                | AttachmentAttachPhase::LifetimeRegistered
                | AttachmentAttachPhase::AttachmentConfirmed
                | AttachmentAttachPhase::Active
        )
    }
}

const CREATE_CUTS: [CreateCut; 10] = [
    CreateCut {
        label: "attachment.create.provider_attempt_prepared",
        phase: AttachmentAttachPhase::ProviderAttemptAuthenticated,
        outcome: CreateRecoveryOutcome::Active,
    },
    CreateCut {
        label: "attachment.create.namespace_created",
        phase: AttachmentAttachPhase::NamespaceCreated,
        outcome: CreateRecoveryOutcome::CleanupPending,
    },
    CreateCut {
        label: "attachment.create.listener_claims_held",
        phase: AttachmentAttachPhase::ListenerClaimsHeld,
        outcome: CreateRecoveryOutcome::CleanupPending,
    },
    CreateCut {
        label: "attachment.create.provider_ready",
        phase: AttachmentAttachPhase::ProviderSetupComplete,
        outcome: CreateRecoveryOutcome::CleanupPending,
    },
    CreateCut {
        label: "attachment.create.publishing",
        phase: AttachmentAttachPhase::Publishing,
        outcome: CreateRecoveryOutcome::CleanupPending,
    },
    CreateCut {
        label: "attachment.create.listeners_active",
        phase: AttachmentAttachPhase::ListenerBindingsActive,
        outcome: CreateRecoveryOutcome::Active,
    },
    CreateCut {
        label: "attachment.create.backend_publication_complete",
        phase: AttachmentAttachPhase::BackendPublicationComplete,
        outcome: CreateRecoveryOutcome::Active,
    },
    CreateCut {
        label: "attachment.create.lifetime_registered",
        phase: AttachmentAttachPhase::LifetimeRegistered,
        outcome: CreateRecoveryOutcome::Active,
    },
    CreateCut {
        label: "attachment.create.attachment_confirmed",
        phase: AttachmentAttachPhase::AttachmentConfirmed,
        outcome: CreateRecoveryOutcome::Active,
    },
    CreateCut {
        label: "attachment.create.active",
        phase: AttachmentAttachPhase::Active,
        outcome: CreateRecoveryOutcome::Active,
    },
];

struct DeleteCut {
    label: &'static str,
    phase: AttachmentDetachPhase,
}

const DELETE_CUTS: [DeleteCut; 10] = [
    DeleteCut {
        label: "attachment.delete.attempt_prepared",
        phase: AttachmentDetachPhase::AttemptPrepared,
    },
    DeleteCut {
        label: "attachment.delete.backend_withdrawn",
        phase: AttachmentDetachPhase::BackendWithdrawn,
    },
    DeleteCut {
        label: "attachment.delete.segment_quarantined",
        phase: AttachmentDetachPhase::SegmentQuarantined,
    },
    DeleteCut {
        label: "attachment.delete.listener_cleanup_prepared",
        phase: AttachmentDetachPhase::ListenerCleanupPrepared,
    },
    DeleteCut {
        label: "attachment.delete.provider_detached",
        phase: AttachmentDetachPhase::ProviderDetached,
    },
    DeleteCut {
        label: "attachment.delete.namespace_removed",
        phase: AttachmentDetachPhase::NamespaceRemoved,
    },
    DeleteCut {
        label: "attachment.delete.listeners_settled",
        phase: AttachmentDetachPhase::ListenersSettled,
    },
    DeleteCut {
        label: "attachment.delete.ipam_released",
        phase: AttachmentDetachPhase::IpamReleased,
    },
    DeleteCut {
        label: "attachment.delete.segment_released",
        phase: AttachmentDetachPhase::SegmentReleased,
    },
    DeleteCut {
        label: "attachment.delete.attachment_terminal",
        phase: AttachmentDetachPhase::AttachmentTerminal,
    },
];

#[derive(Serialize, Deserialize)]
struct DurableCase {
    config: OciNetworkConfig,
    association: NetworkAttachmentSegmentAssociation,
    bindings: Vec<SandboxPortBinding>,
    leases: Vec<PortLeaseRequest>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct CreateRecoveryWitness {
    attachment: DurableNetworkAttachmentState,
    assigned_ips: Vec<std::net::Ipv4Addr>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PreCrashWitness {
    attachment: DurableNetworkAttachmentState,
    assigned_ips: Vec<std::net::Ipv4Addr>,
}

struct PersistentFixture {
    root: PathBuf,
    allocator: SingleNodeSegmentAllocator,
    tenant_id: TenantId,
    sandbox_id: SandboxId,
    layout: OciNetworkLayout,
    ipam: OciIpamAuthority,
    attachments: LocalNetworkAttachmentAuthority,
    ports: OciPortLeaseCoordinator,
    lifetimes: NetavarkPortLifetimeRegistry,
    case: DurableCase,
    backend: ContractBackend,
}

impl PersistentFixture {
    fn initialize(root: &Path, backend: ContractBackend) -> Self {
        std::fs::create_dir_all(root).expect("NNC5.4 case root should create");
        let tenant_id = tenant_id(backend);
        let sandbox_id = sandbox_id(backend);
        let layout = OciNetworkLayout::under_root(root, &tenant_id, &sandbox_id);
        layout
            .ensure_directories()
            .expect("NNC5.4 network layout should create");
        let allocator = SingleNodeSegmentAllocator::single_node_default(root);
        let ipam = direct_test_ipam_authority(&layout);
        let attachments = LocalNetworkAttachmentAuthority::open(root)
            .expect("NNC5.4 attachment authority should open");
        let ports = OciPortLeaseCoordinator::new(root, 32_000..=32_099);
        let lifetimes = NetavarkPortLifetimeRegistry::default();
        let claim = reservation_claim(&format!("nnc54-{}", backend.label()));
        let lifecycle =
            OciAttachmentLifecycle::new(&allocator, Some(&attachments), &ipam, &ports, &lifetimes);
        let config = backend
            .reserve_config(&lifecycle, &tenant_id, &layout, &sandbox_id, &claim)
            .expect("NNC5.4 attachment should reserve");
        allocator
            .adopt_reserved_attachment(
                &tenant_id,
                &default_network_attachment_id(&sandbox_id),
                &claim,
            )
            .expect("NNC5.4 attachment should adopt");
        let association = allocator
            .inspect_attachment_reservation(
                &tenant_id,
                &default_network_attachment_id(&sandbox_id),
                &claim,
            )
            .expect("NNC5.4 adopted association should inspect")
            .association()
            .expect("NNC5.4 adopted reservation should retain its association")
            .clone();
        let binding = SandboxPortBinding::tcp("http", 32_000, 8080);
        let mut reserved = ports
            .reserve_launch_ports_for_sandbox(
                SandboxLaunchPortPlan::new(
                    &tenant_id,
                    &sandbox_id,
                    std::slice::from_ref(&binding),
                    &[],
                ),
                &claim,
            )
            .expect("NNC5.4 listener should reserve");
        reserved
            .confirm_manifest_published()
            .expect("NNC5.4 listener identity should become durable");
        let case = DurableCase {
            config,
            association,
            bindings: reserved.published_bindings,
            leases: reserved.published_leases,
        };
        persist_json(&case_path(root), &case);
        Self {
            root: root.to_path_buf(),
            allocator,
            tenant_id,
            sandbox_id,
            layout,
            ipam,
            attachments,
            ports,
            lifetimes,
            case,
            backend,
        }
    }

    fn reopen(root: &Path, backend: ContractBackend) -> Self {
        let tenant_id = tenant_id(backend);
        let sandbox_id = sandbox_id(backend);
        let layout = OciNetworkLayout::under_root(root, &tenant_id, &sandbox_id);
        let case = serde_json::from_slice(
            &std::fs::read(case_path(root)).expect("NNC5.4 case metadata should read"),
        )
        .expect("NNC5.4 case metadata should decode");
        Self {
            root: root.to_path_buf(),
            allocator: SingleNodeSegmentAllocator::single_node_default(root),
            tenant_id,
            sandbox_id,
            ipam: direct_test_ipam_authority(&layout),
            attachments: LocalNetworkAttachmentAuthority::open(root)
                .expect("NNC5.4 attachment authority should reopen"),
            ports: OciPortLeaseCoordinator::new(root, 32_000..=32_099),
            lifetimes: NetavarkPortLifetimeRegistry::default(),
            layout,
            case,
            backend,
        }
    }

    fn lifecycle(&self) -> OciAttachmentLifecycle<'_> {
        OciAttachmentLifecycle::new(
            &self.allocator,
            Some(&self.attachments),
            &self.ipam,
            &self.ports,
            &self.lifetimes,
        )
    }

    fn adapter(&self) -> OciAttachmentAdapter<'_> {
        let input = OciAttachmentInput {
            workload_state_root: &self.layout.workload_state_root,
            tenant_id: &self.tenant_id,
            sandbox_id: &self.sandbox_id,
            display_name: "NNC5.4 crash-cut workload",
            hostname: "nnc54-crash-cut",
            bindings: &self.case.bindings,
            leases: &self.case.leases,
            auxiliary_listener: None,
            layout: &self.layout,
            config: &self.case.config,
            launch_claim: Some(&self.case.config.reservation_claim),
        };
        self.backend.adapter(input)
    }

    fn assert_active(&self) {
        let attachment = self.attachment("active");
        self.assert_attachment_coordinates(&attachment, true);
        assert_eq!(attachment.resource().phase(), NetworkResourcePhase::Active);
        self.assert_allocator_state(NetworkAttachmentReservationState::Adopted);
        self.assert_live_ipam();
        let port = LocalPortLeaseAuthority::open(&self.root)
            .expect("NNC5.4 port authority should reopen")
            .inspect(self.case.leases[0].lease_id())
            .expect("NNC5.4 port lease should inspect")
            .expect("NNC5.4 port lease should remain durable");
        assert_eq!(port.phase(), PortLeasePhase::Active);
    }

    fn assert_released(&self, expected_ips: &[std::net::Ipv4Addr]) {
        let attachment = self.attachment("terminal");
        self.assert_attachment_coordinates(&attachment, true);
        assert_eq!(
            attachment.resource().phase(),
            NetworkResourcePhase::Released
        );
        self.assert_allocator_state(NetworkAttachmentReservationState::Absent);
        assert_eq!(
            inspect_container_ipam_authority(
                &self.ipam,
                &self.layout,
                &self.case.config,
                &self.sandbox_id,
            )
            .expect("NNC5.4 terminal IPAM authority should inspect"),
            ContainerIpamAuthorityState::Released,
            "terminal detach must retain the exact released IPAM generation witness"
        );
        assert_eq!(
            load_released_container_ips(
                &self.ipam,
                &self.layout,
                &self.case.config,
                &self.sandbox_id,
            )
            .expect("NNC5.4 terminal IPAM addresses should authenticate"),
            expected_ips,
            "terminal IPAM evidence must retain the exact pre-crash addresses"
        );
        let port = LocalPortLeaseAuthority::open(&self.root)
            .expect("NNC5.4 terminal port authority should reopen")
            .inspect(self.case.leases[0].lease_id())
            .expect("NNC5.4 terminal port lease should inspect")
            .expect("NNC5.4 terminal port lease evidence should remain durable");
        assert_eq!(port.phase(), PortLeasePhase::Released);
        assert!(
            std::fs::symlink_metadata(&self.layout.netns_path).is_err(),
            "NNC5.4 terminal detach must leave no persistent namespace"
        );
    }

    fn assert_cleanup_pending(&self, require_stable_handle: bool) {
        let attachment = self.attachment("fenced");
        self.assert_attachment_coordinates(&attachment, require_stable_handle);
        assert_eq!(
            attachment.resource().phase(),
            NetworkResourcePhase::CleanupPending
        );
        self.assert_allocator_state(NetworkAttachmentReservationState::Adopted);
        self.assert_live_ipam();
        let port = LocalPortLeaseAuthority::open(&self.root)
            .expect("NNC5.4 fenced port authority should reopen")
            .inspect(self.case.leases[0].lease_id())
            .expect("NNC5.4 fenced port lease should inspect")
            .expect("NNC5.4 fenced port lease should remain durable");
        assert_ne!(
            port.phase(),
            PortLeasePhase::Released,
            "cleanup-pending attachment must retain listener authority"
        );
    }

    fn attachment(&self, state: &str) -> DurableNetworkAttachmentState {
        self.attachments
            .get(
                &self.tenant_id,
                &default_network_attachment_id(&self.sandbox_id),
            )
            .unwrap_or_else(|error| panic!("NNC5.4 {state} attachment should inspect: {error}"))
            .unwrap_or_else(|| panic!("NNC5.4 {state} attachment should remain durable"))
    }

    fn pre_crash_witness(&self) -> PreCrashWitness {
        let attachment = self.attachment("pre-crash");
        let assigned_ips = self.assert_live_ipam();
        PreCrashWitness {
            attachment,
            assigned_ips,
        }
    }

    fn expected_version(&self) -> NetworkResourceVersion {
        NetworkResourceVersion::for_plan(
            &oci_attachment_plan(&self.tenant_id, &self.sandbox_id, self.backend.kind()),
            default_network_attachment_id(&self.sandbox_id).into(),
            self.case.association.lease_epoch(),
        )
    }

    fn assert_attachment_coordinates(
        &self,
        attachment: &DurableNetworkAttachmentState,
        require_stable_handle: bool,
    ) {
        let attachment_id = default_network_attachment_id(&self.sandbox_id);
        let expected_handle =
            oci_attachment_provider_handle(&self.tenant_id, &self.sandbox_id, self.backend.kind())
                .expect("NNC5.4 stable provider handle should compile");
        assert_eq!(
            attachment.tenant_id(),
            &self.tenant_id,
            "attachment tenant identity must survive process recovery"
        );
        assert_eq!(
            attachment
                .attachment_id()
                .expect("NNC5.4 attachment resource ID should authenticate"),
            &attachment_id,
            "stable attachment identity must never be inferred from an address"
        );
        assert_eq!(
            attachment.association(),
            &self.case.association,
            "claim, segment, and lease epoch must remain the initialized association"
        );
        assert_eq!(
            attachment.resource().version(),
            &self.expected_version(),
            "plan identity, resource identity, generation, digest, and lease epoch must match the \
             independently compiled desired attachment"
        );
        assert_eq!(
            attachment.selected_provider_id(),
            expected_handle.provider_id(),
            "selected provider identity must survive process recovery"
        );
        if require_stable_handle {
            assert_eq!(
                attachment.resource().provider_handle(),
                Some(&expected_handle),
                "provider-present or terminal evidence must retain the exact stable handle"
            );
        } else if let Some(handle) = attachment.resource().provider_handle() {
            assert_eq!(
                handle, &expected_handle,
                "an optional provider handle may be absent but may never be substituted"
            );
        }
    }

    fn assert_allocator_state(&self, expected: NetworkAttachmentReservationState) {
        let observation = self
            .allocator
            .inspect_attachment_reservation(
                &self.tenant_id,
                &default_network_attachment_id(&self.sandbox_id),
                &self.case.config.reservation_claim,
            )
            .expect("NNC5.4 allocator authority should inspect");
        assert_eq!(
            observation.state(),
            expected,
            "allocator retention/release must match the exact crash boundary"
        );
        if expected == NetworkAttachmentReservationState::Absent {
            assert_eq!(observation.association(), None);
        } else {
            assert_eq!(
                observation.association(),
                Some(&self.case.association),
                "retained allocator state must carry the exact original association"
            );
        }
    }

    fn assert_live_ipam(&self) -> Vec<std::net::Ipv4Addr> {
        assert_eq!(
            inspect_container_ipam_authority(
                &self.ipam,
                &self.layout,
                &self.case.config,
                &self.sandbox_id,
            )
            .expect("NNC5.4 live IPAM authority should inspect"),
            ContainerIpamAuthorityState::Live,
            "non-terminal attachment must retain exact live IPAM authority"
        );
        authenticate_container_network_generation(
            &self.ipam,
            &self.layout,
            &self.case.config,
            &self.sandbox_id,
        )
        .expect(
            "NNC5.4 live IPAM claim, segment, provider realm, and addresses should authenticate",
        )
    }

    fn assert_delete_cut_retention(
        &self,
        cut: &'static DeleteCut,
        expected_ips: &[std::net::Ipv4Addr],
    ) {
        let expected_ipam = if matches!(
            cut.phase,
            AttachmentDetachPhase::IpamReleased
                | AttachmentDetachPhase::SegmentReleased
                | AttachmentDetachPhase::AttachmentTerminal
        ) {
            ContainerIpamAuthorityState::Released
        } else {
            ContainerIpamAuthorityState::Live
        };
        assert_eq!(
            inspect_container_ipam_authority(
                &self.ipam,
                &self.layout,
                &self.case.config,
                &self.sandbox_id,
            )
            .expect("NNC5.4 crash-cut IPAM authority should inspect"),
            expected_ipam,
            "IPAM generation must not release before its named cut"
        );
        let retained_ips = match expected_ipam {
            ContainerIpamAuthorityState::Live => self.assert_live_ipam(),
            ContainerIpamAuthorityState::Released => load_released_container_ips(
                &self.ipam,
                &self.layout,
                &self.case.config,
                &self.sandbox_id,
            )
            .expect("NNC5.4 released crash-cut IPAM addresses should authenticate"),
            ContainerIpamAuthorityState::Absent => {
                panic!("NNC5.4 delete cuts must retain live or released IPAM evidence")
            }
        };
        assert_eq!(
            retained_ips, expected_ips,
            "delete crash-cut IPAM evidence must retain the exact pre-crash addresses"
        );
        let expected_allocator = match cut.phase {
            AttachmentDetachPhase::AttemptPrepared | AttachmentDetachPhase::BackendWithdrawn => {
                NetworkAttachmentReservationState::Adopted
            }
            AttachmentDetachPhase::SegmentQuarantined
            | AttachmentDetachPhase::ListenerCleanupPrepared
            | AttachmentDetachPhase::ProviderDetached
            | AttachmentDetachPhase::NamespaceRemoved
            | AttachmentDetachPhase::ListenersSettled
            | AttachmentDetachPhase::IpamReleased => {
                NetworkAttachmentReservationState::ProviderCleanupPending
            }
            AttachmentDetachPhase::SegmentReleased | AttachmentDetachPhase::AttachmentTerminal => {
                NetworkAttachmentReservationState::Absent
            }
        };
        self.assert_allocator_state(expected_allocator);
        let port = LocalPortLeaseAuthority::open(&self.root)
            .expect("NNC5.4 crash-cut port authority should reopen")
            .inspect(self.case.leases[0].lease_id())
            .expect("NNC5.4 crash-cut port lease should inspect")
            .expect("NNC5.4 crash-cut port lease evidence should remain durable");
        if matches!(
            cut.phase,
            AttachmentDetachPhase::ListenersSettled
                | AttachmentDetachPhase::IpamReleased
                | AttachmentDetachPhase::SegmentReleased
                | AttachmentDetachPhase::AttachmentTerminal
        ) {
            assert_eq!(
                port.phase(),
                PortLeasePhase::Released,
                "Final detach listener lease must be released at the settled cut"
            );
        } else {
            assert_ne!(
                port.phase(),
                PortLeasePhase::Released,
                "listener authority must remain fenced before the settled cut"
            );
        }
    }
}

struct CreateCrashObserver<'a> {
    fixture: &'a PersistentFixture,
    context: &'a CrashCutChildContext,
    cut: &'static CreateCut,
}

impl AttachmentPhaseObserver for CreateCrashObserver<'_> {
    fn checkpoint(&mut self, phase: AttachmentAttachPhase) -> Result<()> {
        if phase == self.cut.phase {
            let witness = self.fixture.pre_crash_witness();
            self.fixture.assert_attachment_coordinates(
                &witness.attachment,
                self.cut.requires_stable_handle(),
            );
            persist_json(&self.fixture.root.join(PRE_CRASH_WITNESS), &witness);
            self.context
                .reach_boundary(self.cut.label)
                .unwrap_or_else(|error| panic!("failed to report {}: {error}", self.cut.label));
        }
        Ok(())
    }
}

struct DeleteCrashObserver<'a> {
    context: &'a CrashCutChildContext,
    cut: &'static DeleteCut,
}

impl AttachmentDetachPhaseObserver for DeleteCrashObserver<'_> {
    fn checkpoint(&mut self, phase: AttachmentDetachPhase) {
        if phase == self.cut.phase {
            self.context
                .reach_boundary(self.cut.label)
                .unwrap_or_else(|error| panic!("failed to report {}: {error}", self.cut.label));
        }
    }
}

#[test]
fn fresh_process_shared_attachment_crash_cuts_converge_without_duplicate_effects() {
    let parent_root = TempDir::new().expect("NNC5.4 parent root should create");
    let harness = SubprocessCrashCutHarness::new(CHILD_TIMEOUT);
    for backend in [ContractBackend::Container, ContractBackend::Krun] {
        for cut in &CREATE_CUTS {
            let create_root = parent_root.path().join(format!(
                "{}-{}",
                backend.label(),
                cut.label.replace('.', "-")
            ));
            let expected = match cut.outcome {
                CreateRecoveryOutcome::Active => CREATE_ACTIVE_OBSERVATION,
                CreateRecoveryOutcome::CleanupPending => CREATE_CLEANUP_OBSERVATION,
            };
            let recovered = harness
                .run(
                    &create_root,
                    cut.label,
                    expected,
                    child_spec(
                        "attachment-create-owner",
                        CREATE_CRASH_CHILD,
                        backend,
                        cut.label,
                    ),
                    child_spec(
                        "attachment-create-recovery",
                        CREATE_RECOVERY_CHILD,
                        backend,
                        cut.label,
                    ),
                )
                .unwrap_or_else(|error| {
                    panic!(
                        "{} {} create recovery failed: {error}",
                        backend.label(),
                        cut.label
                    )
                });
            assert_crash_result(&recovered, cut.label, expected);

            let replayed = harness
                .run(
                    &create_root,
                    REPLAY_BOUNDARY,
                    CREATE_REPLAY_OBSERVATION,
                    child_spec(
                        "attachment-create-replay-gate",
                        REPLAY_GATE_CHILD,
                        backend,
                        cut.label,
                    ),
                    child_spec(
                        "attachment-create-replay",
                        CREATE_RECOVERY_CHILD,
                        backend,
                        cut.label,
                    ),
                )
                .unwrap_or_else(|error| {
                    panic!(
                        "{} {} create replay failed: {error}",
                        backend.label(),
                        cut.label
                    )
                });
            assert_crash_result(&replayed, REPLAY_BOUNDARY, CREATE_REPLAY_OBSERVATION);
        }

        for cut in &DELETE_CUTS {
            let delete_root = parent_root.path().join(format!(
                "{}-{}",
                backend.label(),
                cut.label.replace('.', "-")
            ));
            let recovered = harness
                .run(
                    &delete_root,
                    cut.label,
                    DELETE_RECOVERY_OBSERVATION,
                    child_spec(
                        "attachment-delete-owner",
                        DELETE_CRASH_CHILD,
                        backend,
                        cut.label,
                    ),
                    child_spec(
                        "attachment-delete-recovery",
                        DELETE_RECOVERY_CHILD,
                        backend,
                        cut.label,
                    ),
                )
                .unwrap_or_else(|error| {
                    panic!(
                        "{} {} delete recovery failed: {error}",
                        backend.label(),
                        cut.label
                    )
                });
            assert_crash_result(&recovered, cut.label, DELETE_RECOVERY_OBSERVATION);

            let replayed = harness
                .run(
                    &delete_root,
                    REPLAY_BOUNDARY,
                    DELETE_REPLAY_OBSERVATION,
                    child_spec(
                        "attachment-delete-replay-gate",
                        REPLAY_GATE_CHILD,
                        backend,
                        cut.label,
                    ),
                    child_spec(
                        "attachment-delete-replay",
                        DELETE_REPLAY_CHILD,
                        backend,
                        cut.label,
                    ),
                )
                .unwrap_or_else(|error| {
                    panic!(
                        "{} {} delete replay failed: {error}",
                        backend.label(),
                        cut.label
                    )
                });
            assert_crash_result(&replayed, REPLAY_BOUNDARY, DELETE_REPLAY_OBSERVATION);
        }
    }
}

#[test]
#[ignore = "spawned only by the NNC5.4 real-process crash-cut parent"]
fn attachment_create_crash_child() {
    run_crash_cut_child(|context| {
        let backend = child_backend();
        let fixture = PersistentFixture::initialize(context.state_root(), backend);
        let cut = child_create_cut();
        let mut observer = CreateCrashObserver {
            fixture: &fixture,
            context,
            cut,
        };
        fixture
            .adapter()
            .attach_with(
                &fixture.lifecycle(),
                AttachmentAttachAuthority::FreshLaunch(&fixture.case.config.reservation_claim),
                &ContractHostEffects::default(),
                &mut observer,
                |_| Ok(()),
            )
            .map_err(|error| format!("create crash child failed before {}: {error}", cut.label))?;
        Err(format!(
            "create crash child returned without reaching {}",
            cut.label
        ))
    })
    .unwrap_or_else(|error| panic!("NNC5.4 create crash child failed: {error}"));
}

#[test]
#[ignore = "spawned only by the NNC5.4 real-process crash-cut parent"]
fn attachment_create_recovery_child() {
    run_crash_recovery_child(|context| {
        let root = context.state_root();
        let fixture = PersistentFixture::reopen(root, child_backend());
        let host = ContractHostEffects::default();
        let mut observer = ContractPhaseObserver::recording();
        let first_recovery = !root.join(CREATE_RECOVERY_WITNESS).is_file();
        let pre_crash: PreCrashWitness =
            read_json(&root.join(PRE_CRASH_WITNESS), "pre-crash witness");
        let before_attachment = fixture.attachment("pre-create-recovery");
        fixture.assert_attachment_coordinates(
            &before_attachment,
            child_create_cut().requires_stable_handle(),
        );
        assert_attachment_identity_preserved(&pre_crash.attachment, &before_attachment);
        if first_recovery {
            assert_eq!(
                before_attachment, pre_crash.attachment,
                "the first successor must observe the exact attachment record persisted before kill"
            );
        }
        fixture.assert_allocator_state(NetworkAttachmentReservationState::Adopted);
        let before_ips = fixture.assert_live_ipam();
        assert_eq!(
            before_ips, pre_crash.assigned_ips,
            "the first successor and every replay must retain the exact pre-crash addresses"
        );
        let result = fixture.adapter().attach_with(
            &fixture.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&fixture.case.config.reservation_claim),
            &host,
            &mut observer,
            |_| Ok(()),
        );
        let setup_count = host
            .operations()
            .iter()
            .filter(|operation| operation == &&ContractHostOperation::ProviderSetup)
            .count();
        let expected_setup_count = usize::from(
            first_recovery
                && child_create_cut().label == "attachment.create.provider_attempt_prepared",
        );
        assert_eq!(
            setup_count, expected_setup_count,
            "fresh recovery may execute the exact prepared setup once, but must never duplicate it"
        );
        let observation = match child_create_cut().outcome {
            CreateRecoveryOutcome::Active => {
                result.expect("fresh process should recover the exact desired attachment");
                fixture.assert_active();
                CREATE_ACTIVE_OBSERVATION
            }
            CreateRecoveryOutcome::CleanupPending => {
                result.expect_err("incomplete provider evidence must remain fenced");
                fixture.assert_cleanup_pending(child_create_cut().requires_stable_handle());
                CREATE_CLEANUP_OBSERVATION
            }
        };
        let after_attachment = fixture.attachment("post-create-recovery");
        assert_attachment_identity_preserved(&before_attachment, &after_attachment);
        let after_ips = fixture.assert_live_ipam();
        assert_eq!(
            after_ips, pre_crash.assigned_ips,
            "create recovery must retain the exact pre-crash IPAM generation and assigned addresses"
        );
        let witness = CreateRecoveryWitness {
            attachment: after_attachment,
            assigned_ips: after_ips,
        };
        if first_recovery {
            persist_json(&root.join(CREATE_RECOVERY_WITNESS), &witness);
            Ok(observation.to_owned())
        } else {
            let expected: CreateRecoveryWitness = serde_json::from_slice(
                &std::fs::read(root.join(CREATE_RECOVERY_WITNESS))
                    .expect("first create recovery witness should read"),
            )
            .expect("first create recovery witness should decode");
            assert_eq!(
                witness, expected,
                "fresh replay must retain exact version, phase, association, stable handle, and IPAM \
                 addresses"
            );
            Ok(CREATE_REPLAY_OBSERVATION.to_owned())
        }
    })
    .unwrap_or_else(|error| panic!("NNC5.4 create recovery child failed: {error}"));
}

#[test]
#[ignore = "spawned only by the NNC5.4 real-process crash-cut parent"]
fn attachment_delete_crash_child() {
    run_crash_cut_child(|context| {
        let root = context.state_root();
        let backend = child_backend();
        let fixture = PersistentFixture::initialize(root, backend);
        let mut observer = ContractPhaseObserver::recording();
        fixture
            .adapter()
            .attach_with(
                &fixture.lifecycle(),
                AttachmentAttachAuthority::FreshLaunch(&fixture.case.config.reservation_claim),
                &ContractHostEffects::default(),
                &mut observer,
                |_| Ok(()),
            )
            .map_err(|error| format!("delete crash fixture failed to attach: {error}"))?;
        let pre_crash = fixture.pre_crash_witness();
        fixture.assert_attachment_coordinates(&pre_crash.attachment, true);
        persist_json(&root.join(PRE_CRASH_WITNESS), &pre_crash);
        fixture
            .adapter()
            .detach_host_managed_observed_with(
                &fixture.lifecycle(),
                AttachmentTeardownMode::Final,
                &ContractHostEffects::default(),
                &mut DeleteCrashObserver {
                    context,
                    cut: child_delete_cut(),
                },
                |_| Ok(()),
            )
            .map_err(|failure| format!("delete crash child failed: {}", failure.error))?;
        Err(format!(
            "delete crash child returned without reaching {}",
            child_delete_cut().label
        ))
    })
    .unwrap_or_else(|error| panic!("NNC5.4 delete crash child failed: {error}"));
}

#[test]
#[ignore = "spawned only by the NNC5.4 real-process crash-cut parent"]
fn attachment_delete_recovery_child() {
    run_crash_recovery_child(|context| {
        let root = context.state_root();
        let fixture = PersistentFixture::reopen(root, child_backend());
        let host = ContractHostEffects::default();
        let first_recovery = !root.join(DELETE_RECOVERY_WITNESS).is_file();
        let pre_crash: PreCrashWitness =
            read_json(&root.join(PRE_CRASH_WITNESS), "pre-crash witness");
        let before_attachment = fixture.attachment("pre-delete-recovery");
        fixture.assert_attachment_coordinates(&before_attachment, true);
        assert_attachment_identity_preserved(&pre_crash.attachment, &before_attachment);
        fixture.assert_delete_cut_retention(child_delete_cut(), &pre_crash.assigned_ips);
        fixture
            .adapter()
            .detach_host_managed_with(
                &fixture.lifecycle(),
                AttachmentTeardownMode::Final,
                &host,
                |_| Ok(()),
            )
            .map_err(|failure| {
                format!(
                    "fresh process failed exact provider-detached cleanup: {}",
                    failure.error
                )
            })?;
        let teardown_count = host
            .operations()
            .iter()
            .filter(|operation| operation == &&ContractHostOperation::ProviderTeardown)
            .count();
        let provider_was_already_detached = matches!(
            child_delete_cut().phase,
            AttachmentDetachPhase::ProviderDetached
                | AttachmentDetachPhase::NamespaceRemoved
                | AttachmentDetachPhase::ListenersSettled
                | AttachmentDetachPhase::IpamReleased
                | AttachmentDetachPhase::SegmentReleased
                | AttachmentDetachPhase::AttachmentTerminal
        );
        let expected_teardown_count = usize::from(first_recovery && !provider_was_already_detached);
        assert_eq!(
            teardown_count, expected_teardown_count,
            "fresh delete recovery may execute one not-yet-started teardown, but must never duplicate \
             an acknowledged provider detach"
        );
        fixture.assert_released(&pre_crash.assigned_ips);
        let after_attachment = fixture.attachment("post-delete-recovery");
        assert_attachment_identity_preserved(&before_attachment, &after_attachment);
        persist_bytes(
            &root.join(DELETE_RECOVERY_WITNESS),
            b"attachment.delete.recovered\n",
        );
        Ok(DELETE_RECOVERY_OBSERVATION.to_owned())
    })
    .unwrap_or_else(|error| panic!("NNC5.4 delete recovery child failed: {error}"));
}

#[test]
#[ignore = "spawned only by the NNC5.4 real-process crash-cut parent"]
fn attachment_delete_replay_child() {
    run_crash_recovery_child(|context| {
        let root = context.state_root();
        let fixture = PersistentFixture::reopen(root, child_backend());
        let pre_crash: PreCrashWitness =
            read_json(&root.join(PRE_CRASH_WITNESS), "pre-crash witness");
        let authority_path = nimbus_network::LocalNetworkStateStore::authority_path_for(root);
        let before = std::fs::read(&authority_path)
            .map_err(|error| format!("terminal authority bytes should read: {error}"))?;
        fixture
            .adapter()
            .detach_host_managed_with(
                &fixture.lifecycle(),
                AttachmentTeardownMode::Final,
                &ContractHostEffects::default(),
                |_| panic!("terminal replay must not execute backend withdrawal"),
            )
            .map_err(|failure| format!("terminal replay failed: {}", failure.error))?;
        assert_eq!(
            std::fs::read(&authority_path).expect("replayed authority bytes should read"),
            before,
            "terminal detach replay must be byte-stable"
        );
        fixture.assert_released(&pre_crash.assigned_ips);
        Ok(DELETE_REPLAY_OBSERVATION.to_owned())
    })
    .unwrap_or_else(|error| panic!("NNC5.4 delete replay child failed: {error}"));
}

#[test]
#[ignore = "spawned only by the NNC8.1 exact-boundary replay parent"]
fn attachment_replay_gate_child() {
    run_crash_cut_child(|context| context.reach_boundary(REPLAY_BOUNDARY))
        .unwrap_or_else(|error| panic!("NNC8.1 replay gate failed: {error}"));
}

fn tenant_id(backend: ContractBackend) -> TenantId {
    TenantId::new(format!("tenant-nnc54-{}", backend.label()))
        .expect("NNC5.4 tenant should validate")
}

fn sandbox_id(backend: ContractBackend) -> SandboxId {
    SandboxId::new(format!("nnc54-{}", backend.label()))
}

fn case_path(root: &Path) -> PathBuf {
    root.join("attachment-crash-case.json")
}

fn child_spec(role: &str, test_name: &str, backend: ContractBackend, cut: &str) -> ProcessRoleSpec {
    ProcessRoleSpec::new(
        role,
        std::env::current_exe().expect("NNC8.1 test executable should resolve"),
    )
    .arg("--exact")
    .arg(test_name)
    .arg("--ignored")
    .arg("--nocapture")
    .env(BACKEND_ENV, backend.label())
    .env(CUT_ENV, cut)
}

fn child_backend() -> ContractBackend {
    match std::env::var(BACKEND_ENV).as_deref() {
        Ok("container") => ContractBackend::Container,
        Ok("krun") => ContractBackend::Krun,
        other => panic!("invalid NNC5.4 child backend {other:?}"),
    }
}

fn child_create_cut() -> &'static CreateCut {
    let label = std::env::var(CUT_ENV).expect("NNC5.4 child cut should be set");
    CREATE_CUTS
        .iter()
        .find(|cut| cut.label == label)
        .unwrap_or_else(|| panic!("invalid NNC5.4 create cut {label:?}"))
}

fn child_delete_cut() -> &'static DeleteCut {
    let label = std::env::var(CUT_ENV).expect("NNC5.4 child cut should be set");
    DELETE_CUTS
        .iter()
        .find(|cut| cut.label == label)
        .unwrap_or_else(|| panic!("invalid NNC5.4 delete cut {label:?}"))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, label: &str) -> T {
    serde_json::from_slice(
        &std::fs::read(path).unwrap_or_else(|error| panic!("NNC5.4 {label} should read: {error}")),
    )
    .unwrap_or_else(|error| panic!("NNC5.4 {label} should decode: {error}"))
}

fn persist_json(path: &Path, value: &impl Serialize) {
    persist_bytes(
        path,
        &serde_json::to_vec_pretty(value).expect("NNC5.4 metadata should encode"),
    );
}

fn persist_bytes(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("NNC5.4 durable parent should create");
    }
    let mut file = File::create(path).expect("NNC5.4 durable file should create");
    file.write_all(bytes)
        .expect("NNC5.4 durable bytes should write");
    file.sync_all().expect("NNC5.4 durable bytes should sync");
    File::open(
        path.parent()
            .expect("NNC5.4 durable file should have a parent"),
    )
    .and_then(|directory| directory.sync_all())
    .expect("NNC5.4 durable directory should sync");
}

fn assert_attachment_identity_preserved(
    before: &DurableNetworkAttachmentState,
    after: &DurableNetworkAttachmentState,
) {
    assert_eq!(
        after.tenant_id(),
        before.tenant_id(),
        "recovery must preserve exact tenant identity"
    );
    assert_eq!(
        after
            .attachment_id()
            .expect("post-recovery attachment identity should authenticate"),
        before
            .attachment_id()
            .expect("pre-recovery attachment identity should authenticate"),
        "recovery must preserve stable attachment identity"
    );
    assert_eq!(
        after.selected_provider_id(),
        before.selected_provider_id(),
        "recovery must preserve selected provider identity"
    );
    assert_eq!(
        after.association(),
        before.association(),
        "recovery must preserve claim, segment, and allocator epoch"
    );
    assert_eq!(
        after.resource().version(),
        before.resource().version(),
        "recovery must preserve plan generation, digest, resource ID, and lease epoch"
    );
    if let Some(handle) = before.resource().provider_handle() {
        assert_eq!(
            after.resource().provider_handle(),
            Some(handle),
            "an already-recorded stable provider handle must never change across recovery"
        );
    }
}

fn assert_crash_result(result: &SubprocessCrashCutResult, boundary: &str, observation: &str) {
    assert_eq!(result.boundary(), boundary);
    assert_eq!(result.observation(), observation);
    assert_eq!(
        result.crash_diagnostic().cleanup(),
        "killed-at-boundary-and-reaped"
    );
    assert_eq!(result.crash_diagnostic().successful(), Some(false));
    assert_eq!(result.recovery_diagnostic().successful(), Some(true));
}
