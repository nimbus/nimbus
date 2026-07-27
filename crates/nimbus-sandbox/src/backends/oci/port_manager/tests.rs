use std::fs::{self, OpenOptions};
use std::io::{BufRead as _, Read as _, Write as _};
use std::net::{IpAddr, Ipv4Addr};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Barrier, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::json;
use tempfile::TempDir;

use super::{
    InternalListenerReservation, LaunchPortBatchState, PortManager, ReservedLaunchPorts,
    SandboxLaunchPortPlan, new_launch_reservation_claim,
};
use crate::artifact_paths;
use crate::backends::oci::buildah::{OciExposedPort, OciExposedPortProtocol};
use crate::instance::{SandboxId, SandboxStatus};
use crate::spec::SandboxPortBinding;
use nimbus_core::TenantId;

#[path = "tests/batch_classification.rs"]
mod batch_classification;
#[path = "tests/machine_cleanup.rs"]
mod machine_cleanup;
#[path = "tests/teardown_progress.rs"]
mod teardown_progress;

const ALLOCATOR_CHILD_TEST: &str =
    "backends::oci::port_manager::tests::sandbox_and_pep_allocator_child";
const ALLOCATOR_KIND_ENV: &str = "NIMBUS_PORT_MANAGER_ALLOCATOR_KIND";
const ALLOCATOR_ROLE_ENV: &str = "NIMBUS_PORT_MANAGER_ALLOCATOR_ROLE";
const ALLOCATOR_STATE_ROOT_ENV: &str = "NIMBUS_PORT_MANAGER_ALLOCATOR_STATE_ROOT";
const ALLOCATOR_PROTOCOL_PREFIX: &str = "NIMBUS_PORT_MANAGER_ALLOCATOR/1\t";
const CHARACTERIZATION_PORT_MIN: u16 = 41_337;
const CHARACTERIZATION_PORT_MAX: u16 = 41_338;

fn reserve_complete_launch(
    manager: &PortManager,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    existing_bindings: &[SandboxPortBinding],
    exposed_ports: &[OciExposedPort],
) -> crate::error::Result<ReservedLaunchPorts> {
    let reservation_claim = new_launch_reservation_claim()?;
    manager.reserve_launch_ports_for_sandbox(
        SandboxLaunchPortPlan::new(tenant_id, sandbox_id, existing_bindings, exposed_ports),
        &reservation_claim,
    )
}

#[test]
fn two_real_allocator_processes_expose_sandbox_pep_port_collision() {
    let state_root = TempDir::new().expect("shared state root should exist");
    let mut sandbox =
        AllocatorProcess::spawn("sandbox", "sandbox", state_root.path()).expect("sandbox child");
    let mut pep = AllocatorProcess::spawn("pep", "pep", state_root.path()).expect("PEP child");
    assert_ne!(
        sandbox.process_id(),
        pep.process_id(),
        "the allocators must execute in distinct OS processes"
    );

    sandbox
        .wait_ready(Duration::from_secs(5))
        .expect("sandbox allocator should reach the release barrier");
    pep.wait_ready(Duration::from_secs(5))
        .expect("PEP allocator should reach the release barrier");
    sandbox.release().expect("sandbox child should release");
    pep.release().expect("PEP child should release");
    let sandbox_reported = sandbox
        .wait_selected(Duration::from_secs(5))
        .expect("sandbox allocator should report its selected port");
    let pep_reported = pep
        .wait_selected(Duration::from_secs(5))
        .expect("PEP allocator should report its selected port");

    let sandbox_port = read_characterized_port(state_root.path(), "sandbox");
    let pep_port = read_characterized_port(state_root.path(), "pep");
    assert_eq!(sandbox_port, sandbox_reported);
    assert_eq!(pep_port, pep_reported);
    assert!((CHARACTERIZATION_PORT_MIN..=CHARACTERIZATION_PORT_MAX).contains(&sandbox_port));
    assert!((CHARACTERIZATION_PORT_MIN..=CHARACTERIZATION_PORT_MAX).contains(&pep_port));
    assert_ne!(
        sandbox_port, pep_port,
        "sandbox and PEP allocations must hold distinct host-port leases"
    );
}

#[test]
#[ignore = "spawned only by the sandbox/PEP contention characterization"]
fn sandbox_and_pep_allocator_child() {
    let state_root = std::env::var_os(ALLOCATOR_STATE_ROOT_ENV)
        .map(std::path::PathBuf::from)
        .expect("allocator child state root should be set");
    let role = std::env::var(ALLOCATOR_ROLE_ENV).expect("allocator child role should be set");
    emit_allocator_checkpoint("ready");
    let mut command = String::new();
    std::io::stdin()
        .read_line(&mut command)
        .expect("allocator child should read its release command");
    assert_eq!(
        command.trim_end(),
        format!("{ALLOCATOR_PROTOCOL_PREFIX}release")
    );

    let manager = PortManager::new(
        &state_root,
        CHARACTERIZATION_PORT_MIN..=CHARACTERIZATION_PORT_MAX,
    );
    let tenant = tenant_id("contention-tenant");
    let sandbox_id = SandboxId::new(role.clone());
    let allocated_port = match std::env::var(ALLOCATOR_KIND_ENV).as_deref() {
        Ok("sandbox") => {
            reserve_complete_launch(
                &manager,
                &tenant,
                &sandbox_id,
                &[],
                &[tcp_exposed_port(8080)],
            )
            .expect("sandbox allocator should select its only configured port")
            .published_bindings
            .into_iter()
            .next()
            .expect("sandbox allocation should return one binding")
            .host_port
        }
        Ok("pep") => {
            manager
                .reserve_internal_listener(
                    &tenant,
                    &sandbox_id,
                    "egress-pep",
                    nimbus_network::PortBindTarget::ipv4_wildcard(),
                    nimbus_network::PortExposure::Private,
                )
                .expect("PEP allocator should select its only configured port")
                .0
        }
        Ok(other) => panic!("unknown allocator kind {other:?}"),
        Err(error) => {
            panic!("missing allocator kind in {ALLOCATOR_KIND_ENV}: {error}");
        }
    };
    persist_characterized_port(&state_root, &role, allocated_port)
        .expect("child should persist its selected port");
    emit_allocator_checkpoint(&format!("selected:{allocated_port}"));
}

#[test]
fn allocate_missing_bindings_uses_range_and_skips_existing_guest_ports() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = PortManager::new(temp_dir.path(), 15000..=15005);
    let existing = vec![SandboxPortBinding::tcp("http", 18080, 8080)];
    let exposed = vec![
        tcp_exposed_port(8080),
        tcp_exposed_port(5432),
        udp_exposed_port(5353),
    ];

    let allocated = manager
        .preview_missing_bindings(&existing, &exposed)
        .expect("port allocation should succeed");

    assert_eq!(
        allocated,
        vec![SandboxPortBinding::tcp("tcp-5432", 15000, 5432)]
    );
}

#[test]
fn netavark_preview_reuses_a_port_across_disjoint_specific_addresses() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = PortManager::new(temp_dir.path(), 15000..=15000);
    let existing = vec![
        SandboxPortBinding::tcp("other-loopback", 15000, 8080)
            .with_host_address("127.0.0.2".parse().expect("fixture address should parse")),
    ];

    let allocated = manager
        .preview_missing_bindings(&existing, &[tcp_exposed_port(5432)])
        .expect("disjoint specific Netavark targets may share one TCP port");

    assert_eq!(
        allocated,
        vec![SandboxPortBinding::tcp("tcp-5432", 15000, 5432)]
    );
}

#[test]
fn machine_proxy_preview_keeps_wildcard_numeric_exclusion() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager =
        PortManager::new(temp_dir.path(), 15000..=15000).with_machine_port_proxy_bindings();
    let existing = vec![
        SandboxPortBinding::tcp("other-loopback", 15000, 8080)
            .with_host_address("127.0.0.2".parse().expect("fixture address should parse")),
    ];

    let error = manager
        .preview_missing_bindings(&existing, &[tcp_exposed_port(5432)])
        .expect_err("MachinePortProxy wildcard bindings must exclude the occupied number");

    assert!(
        error.to_string().contains("range 15000-15000 is exhausted"),
        "wildcard conflict should report exact preview exhaustion: {error}"
    );
}

#[test]
fn preview_rejects_zero_based_range_before_rendering_an_unexecutable_binding() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = PortManager::new(temp_dir.path(), 0..=15005);

    let error = manager
        .preview_missing_bindings(&[], &[tcp_exposed_port(8080)])
        .expect_err("plan rendering must reject the same zero-based range as execution");

    assert!(
        error
            .to_string()
            .contains("published port range must start above zero"),
        "the invalid range should fail before a port-zero preview is emitted: {error}"
    );
}

#[test]
fn conflicting_sandbox_binding_batch_leaks_no_partial_reservation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = PortManager::new(temp_dir.path(), 15000..=15005);
    let bindings = vec![
        SandboxPortBinding::tcp("first", 18080, 8080),
        SandboxPortBinding::tcp("second", 18080, 8081),
    ];

    let error = reserve_complete_launch(
        &manager,
        &tenant_id("tenant-a"),
        &SandboxId::new("batch-conflict"),
        &bindings,
        &[],
    )
    .expect_err("same host port in one sandbox batch must conflict");
    assert!(
        error.to_string().contains("conflicts with lease"),
        "the conflicting identities should be diagnosed: {error}"
    );

    let authority = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should remain readable");
    assert!(
        authority.list().expect("authority should list").is_empty(),
        "the first binding must roll back when a later request in the batch conflicts"
    );
}

#[test]
fn equal_tenant_local_sandbox_ids_have_distinct_host_global_leases() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = PortManager::new(temp_dir.path(), 15000..=15001);
    let sandbox_id = SandboxId::new("same-local-id");
    let exposed = [tcp_exposed_port(8080)];
    let tenant_a = tenant_id("tenant-a");
    let tenant_b = tenant_id("tenant-b");

    let first = reserve_complete_launch(&manager, &tenant_a, &sandbox_id, &[], &exposed)
        .expect("first tenant should reserve");
    let second = reserve_complete_launch(&manager, &tenant_b, &sandbox_id, &[], &exposed)
        .expect("second tenant should reserve a distinct authority identity");

    assert_ne!(
        first.published_leases[0].lease_id(),
        second.published_leases[0].lease_id(),
        "tenant-local sandbox identities must not alias host-global leases"
    );
    assert_eq!(first.published_bindings[0].host_port, 15000);
    assert_eq!(second.published_bindings[0].host_port, 15001);
    let substitution = manager
        .require_binding_leases(
            &tenant_b,
            &sandbox_id,
            &first.published_bindings,
            &first.published_leases,
        )
        .expect_err("tenant-b must not borrow tenant-a's durable listener authority");
    assert!(
        substitution
            .to_string()
            .contains("does not match the caller"),
        "cross-tenant substitution must fail at the logical-owner boundary: {substitution}"
    );
}

#[test]
fn published_lease_rejects_manifest_bind_scope_widening() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = PortManager::new(temp_dir.path(), 15000..=15000);
    let tenant = tenant_id("tenant-a");
    let sandbox_id = SandboxId::new("scope-authentication");
    let loopback = SandboxPortBinding::tcp("http", 15000, 8080);
    let reserved = reserve_complete_launch(
        &manager,
        &tenant,
        &sandbox_id,
        std::slice::from_ref(&loopback),
        &[],
    )
    .expect("loopback binding should reserve");
    let widened = loopback.with_host_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED));

    let error = manager
        .require_binding_leases(
            &tenant,
            &sandbox_id,
            std::slice::from_ref(&widened),
            &reserved.published_leases,
        )
        .expect_err("wildcard manifest substitution must fail");
    assert!(
        error.to_string().contains("does not match the caller"),
        "durable target and exposure must authenticate the original loopback scope: {error}"
    );
    assert_eq!(
        reserved.published_leases[0].binding().exposure(),
        nimbus_network::PortExposure::Loopback
    );
    assert_eq!(
        reserved.published_leases[0]
            .binding()
            .target()
            .specific_address(),
        Some(IpAddr::V4(Ipv4Addr::LOCALHOST))
    );
}

#[test]
fn machine_proxy_lease_reserves_guest_wildcard_and_exact_publication() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager =
        PortManager::new(temp_dir.path(), 15000..=15000).with_machine_port_proxy_bindings();
    let tenant = tenant_id("tenant-machine-scope");
    let sandbox_id = SandboxId::new("machine-scope");
    let external_loopback = SandboxPortBinding::tcp("http", 15000, 8080);
    let reserved = reserve_complete_launch(
        &manager,
        &tenant,
        &sandbox_id,
        std::slice::from_ref(&external_loopback),
        &[],
    )
    .expect("machine listener should reserve its guest and publication authority");

    assert_eq!(
        reserved.published_leases[0].binding().target(),
        &nimbus_network::PortBindTarget::ipv4_wildcard(),
        "durable conflict authority must describe the IPv4 wildcard socket the guest proxy binds"
    );
    assert_eq!(
        reserved.published_leases[0].binding().exposure(),
        nimbus_network::PortExposure::Loopback,
        "external provider exposure remains distinct desired metadata"
    );
    assert_eq!(
        reserved.published_leases[0].publication().host_address(),
        Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        "exact external publication intent must remain separate from the guest wildcard"
    );
    let substituted = external_loopback
        .clone()
        .with_host_address(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)));
    manager
        .require_binding_leases(
            &tenant,
            &sandbox_id,
            std::slice::from_ref(&substituted),
            &reserved.published_leases,
        )
        .expect_err("external publication substitution must fail before provider effects");
    let collision = manager
        .reserve_internal_listener(
            &tenant,
            &sandbox_id,
            "egress-pep",
            nimbus_network::PortBindTarget::ipv4_specific(Ipv4Addr::new(127, 0, 0, 2)),
            nimbus_network::PortExposure::Private,
        )
        .expect_err(
            "a specific-address PEP must conflict with the wildcard guest listener on the same port",
        );
    assert!(
        collision.to_string().contains("has no free slot"),
        "the portable authority must reject the real kernel overlap: {collision}"
    );
}

#[test]
fn machine_proxy_lease_authenticates_exact_external_publication_address() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager =
        PortManager::new(temp_dir.path(), 15000..=15000).with_machine_port_proxy_bindings();
    let tenant = tenant_id("tenant-machine-publication-address");
    let sandbox_id = SandboxId::new("machine-publication-address");
    let requested = SandboxPortBinding::tcp("http", 15000, 8080)
        .with_host_address(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)));
    let reserved = reserve_complete_launch(
        &manager,
        &tenant,
        &sandbox_id,
        std::slice::from_ref(&requested),
        &[],
    )
    .expect("machine listener should reserve its exact publication address");

    for substituted_address in [Ipv4Addr::new(198, 51, 100, 8), Ipv4Addr::UNSPECIFIED] {
        let substituted = requested
            .clone()
            .with_host_address(IpAddr::V4(substituted_address));
        let error = manager
            .require_binding_leases(
                &tenant,
                &sandbox_id,
                std::slice::from_ref(&substituted),
                &reserved.published_leases,
            )
            .expect_err(
                "a durable machine listener must reject an external publication-address \
                 substitution before provider effects",
            );
        assert!(
            error.to_string().contains("does not match the caller"),
            "the rejection must identify divergent exact publication intent: {error}"
        );
    }

    manager
        .release_never_bound_requests(&reserved.published_leases, &reserved.reservation_claim)
        .expect("test reservation should release after no effect");
}

#[test]
fn machine_proxy_activation_records_wildcard_provider_endpoint() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager =
        PortManager::new(temp_dir.path(), 15000..=15000).with_machine_port_proxy_bindings();
    let tenant = tenant_id("tenant-machine-active-wildcard");
    let sandbox_id = SandboxId::new("machine-active-wildcard");
    let external_address = IpAddr::V6(std::net::Ipv6Addr::LOCALHOST);
    let binding = SandboxPortBinding::tcp("https", 15000, 8443).with_host_address(external_address);
    let reserved = reserve_complete_launch(
        &manager,
        &tenant,
        &sandbox_id,
        std::slice::from_ref(&binding),
        &[],
    )
    .expect("machine listener should reserve");
    let claims = manager
        .claim_machine_bindings(
            &tenant,
            &sandbox_id,
            std::slice::from_ref(&binding),
            &reserved.published_leases,
        )
        .expect("machine listener attempt should become durable");

    let active = manager
        .activate_machine_bindings(
            &tenant,
            &sandbox_id,
            std::slice::from_ref(&binding),
            &reserved.published_leases,
            &claims,
        )
        .expect("machine listener should adopt and activate");

    assert_eq!(active.len(), 1);
    assert_eq!(
        active[0].endpoint().target(),
        &nimbus_network::PortBindTarget::ipv4_wildcard(),
        "provider evidence must report the actual IPv4 wildcard guest listener"
    );
    assert_eq!(
        reserved.published_leases[0].publication().host_address(),
        Some(external_address),
        "activation must not collapse exact IPv6 publication intent into guest bind evidence"
    );
}

#[test]
fn failed_no_effect_launch_member_does_not_strand_reserved_sibling() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager =
        PortManager::new(temp_dir.path(), 15000..=15001).with_machine_port_proxy_bindings();
    let tenant = tenant_id("tenant-machine-failed-compensation");
    let sandbox_id = SandboxId::new("machine-failed-compensation");
    let bindings = [
        SandboxPortBinding::tcp("first", 15000, 8080),
        SandboxPortBinding::tcp("second", 15001, 8081),
    ];
    let reserved = reserve_complete_launch(&manager, &tenant, &sandbox_id, &bindings, &[])
        .expect("complete launch batch should reserve");
    let claim = manager
        .claim_machine_bindings(
            &tenant,
            &sandbox_id,
            std::slice::from_ref(&bindings[1]),
            std::slice::from_ref(&reserved.published_leases[1]),
        )
        .expect("second listener should claim its bind attempt")
        .pop()
        .expect("one listener should return one claim");
    manager
        .record_machine_proxy_bind_failure(
            &reserved.published_leases[1],
            &claim,
            std::net::SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 15001),
            std::io::ErrorKind::AddrInUse,
        )
        .expect("confirmed no-effect failure should remain launch-coordinator owned");

    assert_eq!(
        manager
            .classify_launch_port_batch(&reserved.published_leases, &reserved.reservation_claim,)
            .expect("failed no-effect evidence must remain compensatable with its sibling"),
        LaunchPortBatchState::NeverBound
    );
    manager
        .release_never_bound_requests(&reserved.published_leases, &reserved.reservation_claim)
        .expect("atomic compensation should release only the still-Reserved sibling");

    let authority = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen");
    let phases = reserved
        .published_leases
        .iter()
        .map(|request| {
            authority
                .inspect(request.lease_id())
                .expect("lease should inspect")
                .expect("terminal evidence should remain")
                .phase()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        phases,
        [
            nimbus_network::PortLeasePhase::Released,
            nimbus_network::PortLeasePhase::Failed,
        ],
        "compensation must preserve the failed member and release its reserved sibling"
    );
    let before_replay = authority
        .list()
        .expect("terminal compensation should inspect");
    assert_eq!(
        manager
            .classify_launch_port_batch(&reserved.published_leases, &reserved.reservation_claim,)
            .expect("the exact compensation replay must remain launch-coordinator owned"),
        LaunchPortBatchState::NeverBound,
        "Failed and Released no-effect records must not be relabeled as provider-owned"
    );
    assert_eq!(
        manager
            .classify_machine_cleanup_batch(
                &tenant,
                &sandbox_id,
                &bindings,
                &reserved.published_leases,
            )
            .expect("the exact terminal batch should classify for absent-provider cleanup"),
        LaunchPortBatchState::TerminalNoEffect,
        "Failed and Released no-effect records from one coordinator are a distinct terminal state"
    );
    assert_eq!(
        authority
            .list()
            .expect("terminal compensation should re-inspect"),
        before_replay,
        "classification must not mutate terminal lifecycle evidence"
    );
}

#[test]
fn published_and_internal_launch_reservations_fail_atomically() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = PortManager::new(temp_dir.path(), 15000..=15000);
    let reservation_claim =
        new_launch_reservation_claim().expect("launch reservation claim should mint");
    let error = manager
        .reserve_launch_ports_for_sandbox(
            SandboxLaunchPortPlan::new(
                &tenant_id("tenant-a"),
                &SandboxId::new("atomic-launch"),
                &[],
                &[tcp_exposed_port(8080)],
            )
            .with_internal_listener(InternalListenerReservation::new(
                "egress-pep",
                nimbus_network::PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
                nimbus_network::PortExposure::Private,
            )),
            &reservation_claim,
        )
        .expect_err("one port cannot satisfy both published and PEP listeners");
    assert!(
        error.to_string().contains("has no free slot"),
        "the complete launch batch should report capacity exhaustion: {error}"
    );
    assert!(
        nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
            .expect("authority should open")
            .list()
            .expect("authority should list")
            .is_empty(),
        "a failed combined batch must not strand its earlier published reservation"
    );
}

#[test]
fn netavark_bind_attempt_is_durable_before_provider_adoption() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = PortManager::new(temp_dir.path(), 15000..=15000);
    let tenant = tenant_id("tenant-netavark-claim");
    let sandbox = SandboxId::new("netavark-claim");
    let bindings = [SandboxPortBinding::tcp("http", 15000, 8080)];
    let reserved = reserve_complete_launch(&manager, &tenant, &sandbox, &bindings, &[])
        .expect("launch should reserve the Netavark listener");

    let claims = manager
        .claim_netavark_bindings(&tenant, &sandbox, &bindings, &reserved.published_leases)
        .expect("provider attempt must become durable before Netavark runs");
    let authority = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen");
    let binding = authority
        .inspect(reserved.published_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("lease should remain durable");
    assert_eq!(
        binding.phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "a pre-effect claim remains Reserved but is no longer compensatable as never bound"
    );
    assert_eq!(binding.bind_claim(), claims.first());
    assert_eq!(
        binding.reservation_claim(),
        Some(&reserved.reservation_claim),
        "the launch claim remains available until exact provider adoption"
    );
    let release_error = manager
        .release_never_bound_requests(&reserved.published_leases, &reserved.reservation_claim)
        .expect_err("a claimed Netavark attempt must not be released as never bound");
    assert!(
        release_error
            .to_string()
            .contains("different in-flight provider bind attempt"),
        "ambiguous provider outcome must retain the port fence: {release_error}"
    );

    manager
        .activate_netavark_bindings(
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
            &claims,
        )
        .expect("the exact provider result should adopt and activate the claimed batch");
    let active = authority
        .inspect(reserved.published_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("lease should remain durable");
    assert_eq!(active.phase(), nimbus_network::PortLeasePhase::Active);
    assert!(active.bind_claim().is_none());
    assert!(active.reservation_claim().is_none());
    assert_eq!(
        active
            .binding()
            .expect("active Netavark lease must retain provider evidence")
            .actual_port()
            .get(),
        bindings[0].host_port
    );
}

#[test]
fn exact_netavark_claim_batch_is_recoverable_after_ambiguous_setup() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = PortManager::new(temp_dir.path(), 15000..=15000);
    let tenant = tenant_id("tenant-netavark-recovery");
    let sandbox = SandboxId::new("netavark-recovery");
    let bindings = [SandboxPortBinding::tcp("http", 15000, 8080)];
    let reserved = reserve_complete_launch(&manager, &tenant, &sandbox, &bindings, &[])
        .expect("launch should reserve the Netavark listener");
    manager
        .claim_netavark_bindings(&tenant, &sandbox, &bindings, &reserved.published_leases)
        .expect("provider attempt must become durable before Netavark runs");

    let state = manager
        .classify_launch_port_batch(&reserved.published_leases, &reserved.reservation_claim)
        .expect(
            "a uniform exact Netavark claim batch must remain recoverable after ambiguous setup",
        );
    let LaunchPortBatchState::NetavarkClaimed(claims) = state else {
        panic!("exact claimed batch must preserve Netavark cleanup authority: {state:?}");
    };
    manager
        .abandon_netavark_bind_claims_without_effect(
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
            &claims,
            Some(&reserved.reservation_claim),
        )
        .expect("confirmed provider absence should abandon the exact claim batch");
    manager
        .release_never_bound_requests(&reserved.published_leases, &reserved.reservation_claim)
        .expect("the launch coordinator should release the now-never-bound batch");
    let released = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen")
        .inspect(reserved.published_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("released evidence should remain durable");
    assert_eq!(released.phase(), nimbus_network::PortLeasePhase::Released);
    assert!(released.bind_claim().is_none());
}

#[test]
fn restart_retained_netavark_release_rejects_mixed_terminal_batch_without_mutation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = PortManager::new(temp_dir.path(), 15000..=15001);
    let tenant = tenant_id("tenant-netavark-restart-claim");
    let sandbox = SandboxId::new("netavark-restart-claim");
    let bindings = [
        SandboxPortBinding::tcp("http", 15000, 8080),
        SandboxPortBinding::tcp("metrics", 15001, 9090),
    ];
    let reserved = reserve_complete_launch(&manager, &tenant, &sandbox, &bindings, &[])
        .expect("launch should reserve the Netavark listener");
    let initial_claims = manager
        .claim_netavark_bindings(&tenant, &sandbox, &bindings, &reserved.published_leases)
        .expect("initial bind claim should become durable");
    manager
        .activate_netavark_bindings(
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
            &initial_claims,
        )
        .expect("initial binding should activate");
    let expected = manager
        .expected_netavark_bindings(&tenant, &sandbox, &bindings, &reserved.published_leases)
        .expect("expected binding should derive");
    manager
        .prepare_netavark_bindings_for_rebind(&reserved.published_leases, &expected)
        .expect("confirmed stop should retain the exact slot");
    let restart_claims = manager
        .claim_netavark_bindings(&tenant, &sandbox, &bindings, &reserved.published_leases)
        .expect("restart bind claim should become durable without a launch coordinator");

    let state = manager
        .classify_netavark_cleanup_batch(
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
            None,
        )
        .expect("restart claim batch should classify independently");
    assert_eq!(
        state,
        LaunchPortBatchState::NetavarkClaimed(restart_claims.clone())
    );
    manager
        .abandon_netavark_bind_claims_without_effect(
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
            &restart_claims,
            None,
        )
        .expect("confirmed absence should abandon exact restart claims");
    let authority = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen");
    authority
        .release_after_confirmed_stop(&reserved.published_leases[0])
        .expect("the first exact member should release");
    let before = authority.list().expect("mixed batch should inspect");
    let error = manager
        .release_restart_retained_bindings(&tenant, &sandbox, &bindings, &reserved.published_leases)
        .expect_err("mixed terminal and retained members must fail closed");
    assert!(
        error.to_string().contains("mixes restart-retained"),
        "mixed phase rejection must name the lifecycle conflict: {error}"
    );
    assert_eq!(
        authority.list().expect("mixed batch should re-inspect"),
        before,
        "classification must reject the complete batch before durable mutation"
    );
}

#[test]
fn launch_owned_netavark_cleanup_authenticates_tenant_and_sandbox_before_classification() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = PortManager::new(temp_dir.path(), 15_000..=15_000);
    let owner_tenant = tenant_id("tenant-netavark-launch-owner");
    let owner_sandbox = SandboxId::new("netavark-launch-owner");
    let binding = SandboxPortBinding::tcp("http", 15_000, 8080);
    let reserved = reserve_complete_launch(
        &manager,
        &owner_tenant,
        &owner_sandbox,
        std::slice::from_ref(&binding),
        &[],
    )
    .expect("launch owner should reserve the listener");
    let authority = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen");
    let before = authority
        .list()
        .expect("launch-owned listener should inspect");

    for (tenant, sandbox, mismatch) in [
        (
            tenant_id("tenant-netavark-foreign"),
            owner_sandbox.clone(),
            "tenant",
        ),
        (
            owner_tenant.clone(),
            SandboxId::new("netavark-launch-foreign"),
            "sandbox",
        ),
    ] {
        let error = manager
            .classify_netavark_cleanup_batch(
                &tenant,
                &sandbox,
                std::slice::from_ref(&binding),
                &reserved.published_leases,
                Some(&reserved.reservation_claim),
            )
            .expect_err("a copied launch manifest must not classify under foreign identity");
        assert!(
            error.to_string().contains("does not match"),
            "{mismatch} mismatch must be rejected by exact listener authority: {error}"
        );
    }

    assert_eq!(
        authority
            .list()
            .expect("launch-owned listener should re-inspect"),
        before,
        "foreign caller identity must fail before durable mutation"
    );
}

#[test]
fn machine_abandon_rejects_manager_provider_mismatch_without_mutation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let machine =
        PortManager::new(temp_dir.path(), 15000..=15000).with_machine_port_proxy_bindings();
    let netavark = PortManager::new(temp_dir.path(), 15000..=15000);
    let tenant = tenant_id("tenant-machine-abandon-provider");
    let sandbox = SandboxId::new("machine-abandon-provider");
    let bindings = [SandboxPortBinding::tcp("http", 15000, 8080)];
    let reserved = reserve_complete_launch(&machine, &tenant, &sandbox, &bindings, &[])
        .expect("machine listener should reserve");
    let claims = machine
        .claim_machine_bindings(&tenant, &sandbox, &bindings, &reserved.published_leases)
        .expect("machine provider attempt should become durable");
    let authority = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen");
    let before = authority
        .list()
        .expect("durable listener batch should inspect");

    let error = netavark
        .abandon_machine_bind_claims_without_effect(&reserved.published_leases, &claims)
        .expect_err("a Netavark manager must not interpret machine-provider absence");
    assert!(
        error
            .to_string()
            .contains("configured for Netavark, not MachinePortProxy"),
        "the provider ownership mismatch should be explicit: {error}"
    );
    assert_eq!(
        authority
            .list()
            .expect("durable listener batch should re-inspect"),
        before,
        "provider mismatch must fail before any durable claim mutation"
    );
}

#[test]
fn machine_bind_failure_rejects_manager_provider_mismatch_without_mutation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let machine =
        PortManager::new(temp_dir.path(), 15000..=15000).with_machine_port_proxy_bindings();
    let netavark = PortManager::new(temp_dir.path(), 15000..=15000);
    let tenant = tenant_id("tenant-machine-failure-provider");
    let sandbox = SandboxId::new("machine-failure-provider");
    let bindings = [SandboxPortBinding::tcp("http", 15000, 8080)];
    let reserved = reserve_complete_launch(&machine, &tenant, &sandbox, &bindings, &[])
        .expect("machine listener should reserve");
    let claim = machine
        .claim_machine_bindings(&tenant, &sandbox, &bindings, &reserved.published_leases)
        .expect("machine provider attempt should become durable")
        .pop()
        .expect("one listener should return one claim");
    let authority = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen");
    let before = authority
        .list()
        .expect("claimed machine listener should inspect");

    let error = netavark
        .record_machine_proxy_bind_failure(
            &reserved.published_leases[0],
            &claim,
            std::net::SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 15000),
            std::io::ErrorKind::AddrInUse,
        )
        .expect_err("a Netavark manager must not interpret machine-provider bind absence");
    assert!(
        error
            .to_string()
            .contains("configured for Netavark, not MachinePortProxy"),
        "the provider ownership mismatch should be explicit: {error}"
    );
    assert_eq!(
        authority
            .list()
            .expect("claimed machine listener should re-inspect"),
        before,
        "provider mismatch must fail before terminalizing durable listener authority"
    );
}

#[test]
fn machine_abandon_rejects_mixed_provider_batch_without_mutation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let machine =
        PortManager::new(temp_dir.path(), 15000..=15001).with_machine_port_proxy_bindings();
    let tenant = tenant_id("tenant-machine-mixed-abandon");
    let sandbox = SandboxId::new("machine-mixed-abandon");
    let bindings = [
        SandboxPortBinding::tcp("http", 15000, 8080),
        SandboxPortBinding::tcp("admin", 15001, 8081),
    ];
    let reserved = reserve_complete_launch(&machine, &tenant, &sandbox, &bindings, &[])
        .expect("machine listener batch should reserve");
    let mut claims = machine
        .claim_machine_bindings(
            &tenant,
            &sandbox,
            &bindings[..1],
            &reserved.published_leases[..1],
        )
        .expect("first machine provider attempt should become durable");
    claims.extend(
        crate::backends::oci::port_lease::claim_bind_attempts(
            temp_dir.path(),
            &reserved.published_leases[1..],
            crate::backends::oci::port_lease::OciPortProvider::Netavark,
            Some(&reserved.reservation_claim),
        )
        .expect("second exact claim should model a foreign provider attempt"),
    );
    let authority = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen");
    let before = authority.list().expect("claimed batch should inspect");

    let error = machine
        .abandon_machine_bind_claims_without_effect(&reserved.published_leases, &claims)
        .expect_err("machine absence cannot authorize Netavark claim abandonment");
    assert!(
        error
            .to_string()
            .contains("cannot abandon MachinePortProxy claim from provider"),
        "the exact foreign provider should be diagnosed: {error}"
    );
    assert_eq!(
        authority.list().expect("claimed batch should re-inspect"),
        before,
        "mixed-provider rejection must precede every durable claim mutation"
    );
}

#[test]
fn machine_abandon_accepts_uniform_exact_machine_batch() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let machine =
        PortManager::new(temp_dir.path(), 15000..=15001).with_machine_port_proxy_bindings();
    let tenant = tenant_id("tenant-machine-uniform-abandon");
    let sandbox = SandboxId::new("machine-uniform-abandon");
    let bindings = [
        SandboxPortBinding::tcp("http", 15000, 8080),
        SandboxPortBinding::tcp("admin", 15001, 8081),
    ];
    let reserved = reserve_complete_launch(&machine, &tenant, &sandbox, &bindings, &[])
        .expect("machine listener batch should reserve");
    let claims = machine
        .claim_machine_bindings(&tenant, &sandbox, &bindings, &reserved.published_leases)
        .expect("uniform machine attempts should become durable");

    machine
        .abandon_machine_bind_claims_without_effect(&reserved.published_leases, &claims)
        .expect("confirmed machine-provider absence should abandon its exact batch");
    let records = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen")
        .list()
        .expect("released claim batch should inspect");
    assert_eq!(records.len(), 2);
    assert!(
        records.iter().all(|record| {
            record.phase() == nimbus_network::PortLeasePhase::Reserved
                && record.bind_claim().is_none()
                && record.reservation_claim() == Some(&reserved.reservation_claim)
        }),
        "uniform abandonment must clear only bind claims while retaining coordinator authority: \
         {records:?}"
    );
}

#[test]
fn teardown_rejects_cross_tenant_published_lease_substitution() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = PortManager::new(temp_dir.path(), 15000..=15000);
    let sandbox_id = SandboxId::new("same-local-id");
    let tenant_a = tenant_id("tenant-a");
    let tenant_b = tenant_id("tenant-b");
    let bindings = [SandboxPortBinding::tcp("http", 15000, 8080)];
    let reserved = reserve_complete_launch(&manager, &tenant_a, &sandbox_id, &bindings, &[])
        .expect("tenant-a should reserve its published listener");

    let error = manager
        .withdraw_bindings(
            &tenant_b,
            &sandbox_id,
            &bindings,
            &reserved.published_leases,
        )
        .expect_err("tenant-b manifest must not withdraw tenant-a's lease");
    assert!(
        error.to_string().contains("does not match the caller"),
        "teardown must reject the substituted tenant at logical ownership: {error}"
    );
    let durable = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen")
        .inspect(reserved.published_leases[0].lease_id())
        .expect("lease should inspect")
        .expect("lease should remain durable");
    assert_eq!(
        durable.phase(),
        nimbus_network::PortLeasePhase::Reserved,
        "rejected teardown must not transition the victim lease"
    );
}

#[test]
fn preview_bindings_never_treat_manifests_as_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let tenant_id = tenant_id("tenant-a");
    write_manifest(
        temp_dir.path(),
        &tenant_id,
        "active",
        SandboxStatus::Ready,
        &[(15000, 5432)],
    );
    write_manifest(
        temp_dir.path(),
        &tenant_id,
        "stopped",
        SandboxStatus::Stopped,
        &[(15001, 5432)],
    );

    let manager = PortManager::new(temp_dir.path(), 15000..=15002);
    let allocated = manager
        .preview_missing_bindings(&[], &[tcp_exposed_port(8080), tcp_exposed_port(8443)])
        .expect("port allocation should succeed");

    assert_eq!(
        allocated,
        vec![
            SandboxPortBinding::tcp("tcp-8080", 15000, 8080),
            SandboxPortBinding::tcp("tcp-8443", 15001, 8443),
        ]
    );
}

#[test]
fn not_ready_manifest_is_not_port_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let tenant_id = tenant_id("tenant-a");
    write_manifest(
        temp_dir.path(),
        &tenant_id,
        "not-ready",
        SandboxStatus::NotReady,
        &[(15000, 5432)],
    );

    let manager = PortManager::new(temp_dir.path(), 15000..=15001);
    let allocated = manager
        .preview_missing_bindings(&[], &[tcp_exposed_port(8080)])
        .expect("port allocation should succeed");

    assert_eq!(
        allocated,
        vec![SandboxPortBinding::tcp("tcp-8080", 15000, 8080)]
    );
}

#[test]
fn active_egress_manifest_is_not_port_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let tenant_id = tenant_id("tenant-a");
    write_manifest_with_egress_proxy(
        temp_dir.path(),
        &tenant_id,
        "active",
        SandboxStatus::Ready,
        15000,
    );

    let manager = PortManager::new(temp_dir.path(), 15000..=15001);
    let allocated = manager
        .reserve_internal_listener(
            &tenant_id,
            &SandboxId::new("replacement"),
            "egress-pep",
            nimbus_network::PortBindTarget::ipv4_wildcard(),
            nimbus_network::PortExposure::Private,
        )
        .expect("durable authority should ignore manifest-only observation")
        .0;

    assert_eq!(allocated, 15000);
}

#[test]
fn stopped_egress_manifest_is_not_port_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let tenant_id = tenant_id("tenant-a");
    write_manifest_with_egress_proxy(
        temp_dir.path(),
        &tenant_id,
        "stopped",
        SandboxStatus::Stopped,
        15000,
    );

    let manager = PortManager::new(temp_dir.path(), 15000..=15001);
    let allocated = manager
        .reserve_internal_listener(
            &tenant_id,
            &SandboxId::new("replacement"),
            "egress-pep",
            nimbus_network::PortBindTarget::ipv4_wildcard(),
            nimbus_network::PortExposure::Private,
        )
        .expect("stopped manifest should not reserve a host port")
        .0;

    assert_eq!(allocated, 15000);
}

#[test]
fn active_manifest_is_observation_not_host_port_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let tenant_id = tenant_id("tenant-a");
    write_manifest(
        temp_dir.path(),
        &tenant_id,
        "active",
        SandboxStatus::Ready,
        &[(15000, 5432)],
    );

    let manager = PortManager::new(temp_dir.path(), 15000..=15001);
    let allocated = manager
        .reserve_internal_listener(
            &tenant_id,
            &SandboxId::new("new-owner"),
            "egress-pep",
            nimbus_network::PortBindTarget::ipv4_wildcard(),
            nimbus_network::PortExposure::Private,
        )
        .expect("the port lease authority, not a manifest scan, selects host ports")
        .0;

    assert_eq!(
        allocated, 15000,
        "observed manifest state must not reserve a host port without a durable PortLease"
    );
}

#[test]
fn tenant_port_quota_rejects_explicit_bindings_that_exceed_same_tenant_limit() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let tenant_id = tenant_id("tenant-a");

    let manager =
        PortManager::new(temp_dir.path(), 15000..=15002).with_max_ports_per_tenant(Some(0));
    let existing = vec![SandboxPortBinding::tcp("http", 18080, 8080)];
    let error = reserve_complete_launch(
        &manager,
        &tenant_id,
        &SandboxId::new("quota-candidate"),
        &existing,
        &[],
    )
    .expect_err("explicit bindings should still count against the tenant port quota");

    assert!(
        error.to_string().contains("published port quota exceeded")
            && error.to_string().contains("tenant-a")
            && error.to_string().contains("limit 0"),
        "expected tenant quota error, got: {error}"
    );
}

#[test]
fn tenant_quota_is_atomic_across_independent_managers() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let tenant = tenant_id("tenant-quota-race");
    let barrier = Arc::new(Barrier::new(3));
    let handles = [(15000, "quota-race-a"), (15001, "quota-race-b")]
        .into_iter()
        .map(|(port, sandbox)| {
            let state_root = temp_dir.path().to_path_buf();
            let tenant = tenant.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let manager =
                    PortManager::new(state_root, 15000..=15001).with_max_ports_per_tenant(Some(1));
                barrier.wait();
                reserve_complete_launch(
                    &manager,
                    &tenant,
                    &SandboxId::new(sandbox),
                    &[SandboxPortBinding::tcp("http", port, 8080)],
                    &[],
                )
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().expect("quota worker should join"))
        .collect::<Vec<_>>();

    assert_eq!(
        results.iter().filter(|result| result.is_ok()).count(),
        1,
        "the durable tenant limit must admit exactly one concurrent reservation: {results:?}"
    );
    assert_eq!(
        results.iter().filter(|result| result.is_err()).count(),
        1,
        "the other concurrent reservation must receive a quota rejection: {results:?}"
    );
    let authority = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should open");
    assert_eq!(
        authority
            .list()
            .expect("authority should list")
            .into_iter()
            .filter(|record| !record.phase().is_terminal())
            .count(),
        1,
        "the transaction must persist exactly one live published lease"
    );
}

#[test]
fn crash_retained_reservation_consumes_tenant_quota_without_manifest() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let tenant = tenant_id("tenant-quota-crash");
    let first = PortManager::new(temp_dir.path(), 15000..=15001).with_max_ports_per_tenant(Some(1));
    let first_reservation = reserve_complete_launch(
        &first,
        &tenant,
        &SandboxId::new("quota-crash-a"),
        &[SandboxPortBinding::tcp("http", 15000, 8080)],
        &[],
    )
    .expect("first reservation should persist before the simulated crash");
    drop(first);

    let reopened =
        PortManager::new(temp_dir.path(), 15000..=15001).with_max_ports_per_tenant(Some(1));
    let error = reserve_complete_launch(
        &reopened,
        &tenant,
        &SandboxId::new("quota-crash-b"),
        &[SandboxPortBinding::tcp("http", 15001, 8080)],
        &[],
    )
    .expect_err("a crash-retained durable reservation must consume tenant quota");
    assert!(
        error.to_string().contains("published port quota exceeded"),
        "the rejection must identify the caller-supplied tenant limit: {error}"
    );

    let authority = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen");
    let records = authority.list().expect("authority should list");
    assert_eq!(records.len(), 1, "the rejected request must not persist");
    assert_eq!(
        records[0].request(),
        &first_reservation.published_leases[0],
        "the crash-retained request remains the sole durable tenant usage"
    );
    assert_eq!(records[0].phase(), nimbus_network::PortLeasePhase::Reserved);
}

#[test]
fn foreign_coordinator_tenant_quota_replay_is_fenced() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let tenant = tenant_id("tenant-quota-replay");
    let manager =
        PortManager::new(temp_dir.path(), 15000..=15001).with_max_ports_per_tenant(Some(1));
    let binding = SandboxPortBinding::tcp("http", 15000, 8080);
    let sandbox = SandboxId::new("quota-replay");
    let first = reserve_complete_launch(
        &manager,
        &tenant,
        &sandbox,
        std::slice::from_ref(&binding),
        &[],
    )
    .expect("first request should consume one quota unit");
    let replay_error = reserve_complete_launch(
        &manager,
        &tenant,
        &sandbox,
        std::slice::from_ref(&binding),
        &[],
    )
    .expect_err("a different coordinator must not inherit compensation authority");
    assert!(
        replay_error
            .to_string()
            .contains("different launch reservation coordinator"),
        "the rejected replay must identify coordinator ownership: {replay_error}"
    );
    let records = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen")
        .list()
        .expect("authority should list");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].request(), &first.published_leases[0]);
    assert_eq!(
        records[0].reservation_claim(),
        Some(&first.reservation_claim),
        "a losing replay must not replace the first coordinator's claim"
    );

    let error = reserve_complete_launch(
        &manager,
        &tenant,
        &SandboxId::new("quota-replay-new"),
        &[SandboxPortBinding::tcp("other", 15001, 8081)],
        &[],
    )
    .expect_err("a distinct request must consume a second quota unit");
    assert!(error.to_string().contains("published port quota exceeded"));
}

#[test]
fn internal_pep_lease_does_not_consume_published_quota() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let tenant = tenant_id("tenant-quota-internal");
    let manager =
        PortManager::new(temp_dir.path(), 15000..=15002).with_max_ports_per_tenant(Some(1));
    let (pep_port, pep_lease, pep_reservation_claim) = manager
        .reserve_internal_listener_for_coordinator(
            &tenant,
            &SandboxId::new("quota-internal-pep"),
            "egress-pep",
            nimbus_network::PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            nimbus_network::PortExposure::Private,
        )
        .expect("host-internal PEP should reserve without consuming published quota");
    assert_eq!(
        pep_lease.accounting(),
        nimbus_network::PortLeaseAccounting::HostInternal
    );

    let first_published = reserve_complete_launch(
        &manager,
        &tenant,
        &SandboxId::new("quota-internal-published"),
        &[SandboxPortBinding::tcp("http", 15001, 8080)],
        &[],
    )
    .expect("one published endpoint should remain admissible");
    assert_eq!(
        first_published.published_leases[0].accounting(),
        nimbus_network::PortLeaseAccounting::TenantPublished
    );
    let error = reserve_complete_launch(
        &manager,
        &tenant,
        &SandboxId::new("quota-internal-over-limit"),
        &[SandboxPortBinding::tcp("other", 15002, 8081)],
        &[],
    )
    .expect_err("a second published endpoint must exceed the tenant limit");
    assert!(error.to_string().contains("published port quota exceeded"));

    let crossed_pep = manager
        .release_never_bound_requests(
            std::slice::from_ref(&pep_lease),
            &first_published.reservation_claim,
        )
        .expect_err("the published batch claim must not release the PEP batch");
    assert!(
        crossed_pep
            .to_string()
            .contains("different launch reservation coordinator")
    );
    let crossed_published = manager
        .release_never_bound_requests(
            std::slice::from_ref(&first_published.published_leases[0]),
            &pep_reservation_claim,
        )
        .expect_err("the PEP batch claim must not release the published batch");
    assert!(
        crossed_published
            .to_string()
            .contains("different launch reservation coordinator")
    );
    let still_reserved = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen")
        .list()
        .expect("authority should list");
    assert_eq!(
        still_reserved
            .iter()
            .filter(|record| record.phase() == nimbus_network::PortLeasePhase::Reserved)
            .count(),
        2,
        "crossed claims must not mutate either reservation"
    );

    manager
        .release_never_bound_requests(std::slice::from_ref(&pep_lease), &pep_reservation_claim)
        .expect("internal test reservation should release after no effect");
    manager
        .release_never_bound_requests(
            std::slice::from_ref(&first_published.published_leases[0]),
            &first_published.reservation_claim,
        )
        .expect("test reservations should release after no effects were created");
    assert!((15000..=15002).contains(&pep_port));
}

#[test]
fn tenant_quota_is_tenant_scoped_while_port_conflicts_remain_host_global() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let tenant_a = tenant_id("tenant-a");
    let tenant_b = tenant_id("tenant-b");
    let manager =
        PortManager::new(temp_dir.path(), 15000..=15002).with_max_ports_per_tenant(Some(2));
    reserve_complete_launch(
        &manager,
        &tenant_b,
        &SandboxId::new("durable-b"),
        &[SandboxPortBinding::tcp("redis", 15001, 6379)],
        &[],
    )
    .expect("tenant-b durable lease should reserve globally");
    let allocated = reserve_complete_launch(
        &manager,
        &tenant_a,
        &SandboxId::new("candidate-a"),
        &[],
        &[tcp_exposed_port(8080)],
    )
    .expect("other tenant leases should not consume tenant-a quota");

    assert_eq!(
        allocated.published_bindings,
        vec![SandboxPortBinding::tcp("tcp-8080", 15000, 8080)],
        "tenant-b usage must not consume tenant-a quota while its exact port remains globally fenced"
    );
}

#[test]
#[ignore = "NNC0.9 explicit allocation-scale characterization"]
fn manifest_count_does_not_affect_preview_port_selection() {
    const HOST_PORT_BASE: u16 = 20_000;
    const SAMPLE_COUNT: usize = 21;

    for manifest_count in [0usize, 64, 256, 1_024] {
        let temp_dir = TempDir::new().expect("temporary directory should exist");
        let tenant_id = tenant_id("nnc0-9-port-baseline");
        for index in 0..manifest_count {
            let offset = u16::try_from(index).expect("baseline manifest count fits u16");
            write_manifest(
                temp_dir.path(),
                &tenant_id,
                &format!("baseline-{index:04}"),
                SandboxStatus::Ready,
                &[(HOST_PORT_BASE + offset, 10_000 + offset)],
            );
        }

        let manager = PortManager::new(temp_dir.path(), HOST_PORT_BASE..=40_000);
        let expected = HOST_PORT_BASE;
        let mut samples_ns = Vec::with_capacity(SAMPLE_COUNT);
        for _ in 0..SAMPLE_COUNT {
            let started = std::time::Instant::now();
            let selected = manager
                .preview_missing_bindings(&[], &[tcp_exposed_port(8080)])
                .expect("preview should select its first inert port")[0]
                .host_port;
            samples_ns.push(started.elapsed().as_nanos());
            assert_eq!(
                selected, expected,
                "manifest count must not affect inert preview selection"
            );
        }
        samples_ns.sort_unstable();
        let p95_index = (SAMPLE_COUNT * 95).div_ceil(100) - 1;

        println!(
            "NNC0.9 port-allocation-baseline manifests={manifest_count} samples={SAMPLE_COUNT} median_ns={} p95_ns={} selected_port={expected}",
            samples_ns[SAMPLE_COUNT / 2],
            samples_ns[p95_index]
        );
    }
}

fn write_manifest(
    state_root: &std::path::Path,
    tenant_id: &TenantId,
    sandbox_id: &str,
    status: SandboxStatus,
    host_guest_ports: &[(u16, u16)],
) {
    let sandbox_id = SandboxId::new(sandbox_id);
    let manifest_path = artifact_paths::manifest_path(state_root, tenant_id, &sandbox_id);
    let container_dir = manifest_path
        .parent()
        .expect("manifest path should have a parent directory");
    fs::create_dir_all(container_dir).expect("container manifest directory should exist");
    let manifest = json!({
        "status": status,
        "spec": {
            "port_bindings": host_guest_ports
                .iter()
                .map(|(host_port, guest_port)| json!({
                    "name": format!("tcp-{guest_port}"),
                    "protocol": "tcp",
                    "host_address": "127.0.0.1",
                    "host_port": host_port,
                    "guest_port": guest_port,
                }))
                .collect::<Vec<_>>(),
        },
    });
    fs::write(
        manifest_path,
        serde_json::to_vec_pretty(&manifest).expect("manifest JSON should serialize"),
    )
    .expect("manifest JSON should be written");
}

fn write_manifest_with_egress_proxy(
    state_root: &std::path::Path,
    tenant_id: &TenantId,
    sandbox_id: &str,
    status: SandboxStatus,
    egress_proxy_port: u16,
) {
    let sandbox_id = SandboxId::new(sandbox_id);
    let manifest_path = artifact_paths::manifest_path(state_root, tenant_id, &sandbox_id);
    let container_dir = manifest_path
        .parent()
        .expect("manifest path should have a parent directory");
    fs::create_dir_all(container_dir).expect("container manifest directory should exist");
    let manifest = json!({
        "status": status,
        "egress_proxy": {
            "host": "10.89.0.1",
            "port": egress_proxy_port,
        },
        "spec": {
            "port_bindings": [],
        },
    });
    fs::write(
        manifest_path,
        serde_json::to_vec_pretty(&manifest).expect("manifest JSON should serialize"),
    )
    .expect("manifest JSON should be written");
}

fn tenant_id(value: &str) -> TenantId {
    TenantId::new(value).expect("tenant id should parse")
}

fn tcp_exposed_port(port: u16) -> OciExposedPort {
    OciExposedPort {
        port,
        protocol: OciExposedPortProtocol::Tcp,
        raw: format!("{port}/tcp"),
    }
}

fn udp_exposed_port(port: u16) -> OciExposedPort {
    OciExposedPort {
        port,
        protocol: OciExposedPortProtocol::Udp,
        raw: format!("{port}/udp"),
    }
}

fn persist_characterized_port(
    state_root: &std::path::Path,
    role: &str,
    port: u16,
) -> Result<(), String> {
    let path = state_root.join(format!("{role}.selected-port"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
    writeln!(file, "{port}")
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("failed to persist {}: {error}", path.display()))
}

fn read_characterized_port(state_root: &std::path::Path, role: &str) -> u16 {
    let path = state_root.join(format!("{role}.selected-port"));
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
        .trim()
        .parse()
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
}

fn emit_allocator_checkpoint(checkpoint: &str) {
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{ALLOCATOR_PROTOCOL_PREFIX}{checkpoint}")
        .and_then(|()| stdout.flush())
        .expect("allocator child checkpoint should flush");
}

#[derive(Debug)]
enum AllocatorEvent {
    Ready,
    Selected(u16),
    ProtocolError(String),
    Eof,
}

struct AllocatorProcess {
    role: String,
    child: Child,
    stdin: Option<ChildStdin>,
    events: mpsc::Receiver<AllocatorEvent>,
    stdout: Arc<Mutex<String>>,
    stderr: Arc<Mutex<String>>,
    stdout_reader: Option<JoinHandle<()>>,
    stderr_reader: Option<JoinHandle<()>>,
}

impl AllocatorProcess {
    fn spawn(
        role: &str,
        allocator_kind: &str,
        state_root: &std::path::Path,
    ) -> Result<Self, String> {
        let mut child = Command::new(
            std::env::current_exe()
                .map_err(|error| format!("failed to resolve sandbox test binary: {error}"))?,
        )
        .arg("--exact")
        .arg(ALLOCATOR_CHILD_TEST)
        .arg("--ignored")
        .arg("--nocapture")
        .env(ALLOCATOR_KIND_ENV, allocator_kind)
        .env(ALLOCATOR_ROLE_ENV, role)
        .env(ALLOCATOR_STATE_ROOT_ENV, state_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to spawn allocator role {role:?}: {error}"))?;
        let stdin = child
            .stdin
            .take()
            .expect("piped allocator stdin should be present");
        let stdout = child
            .stdout
            .take()
            .expect("piped allocator stdout should be present");
        let stderr = child
            .stderr
            .take()
            .expect("piped allocator stderr should be present");

        let stdout_capture = Arc::new(Mutex::new(String::new()));
        let stdout_target = Arc::clone(&stdout_capture);
        let (event_tx, events) = mpsc::sync_channel(4);
        let stdout_reader = std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        let _ = event_tx.send(AllocatorEvent::Eof);
                        return;
                    }
                    Ok(_) => {
                        stdout_target
                            .lock()
                            .expect("allocator stdout lock should not be poisoned")
                            .push_str(&line);
                        let Some(value) = line.trim_end().strip_prefix(ALLOCATOR_PROTOCOL_PREFIX)
                        else {
                            continue;
                        };
                        let event = match value {
                            "ready" => AllocatorEvent::Ready,
                            selected if selected.starts_with("selected:") => selected
                                .trim_start_matches("selected:")
                                .parse::<u16>()
                                .map(AllocatorEvent::Selected)
                                .unwrap_or_else(|error| {
                                    AllocatorEvent::ProtocolError(format!(
                                        "invalid selected port {selected:?}: {error}"
                                    ))
                                }),
                            other => AllocatorEvent::ProtocolError(format!(
                                "unknown allocator checkpoint {other:?}"
                            )),
                        };
                        if event_tx.send(event).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        let _ = event_tx.send(AllocatorEvent::ProtocolError(format!(
                            "allocator stdout read failed: {error}"
                        )));
                        return;
                    }
                }
            }
        });

        let stderr_capture = Arc::new(Mutex::new(String::new()));
        let stderr_target = Arc::clone(&stderr_capture);
        let stderr_reader = std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stderr);
            let mut captured = String::new();
            if let Err(error) = reader.read_to_string(&mut captured) {
                captured.push_str(&format!("\n<stderr read failed: {error}>"));
            }
            *stderr_target
                .lock()
                .expect("allocator stderr lock should not be poisoned") = captured;
        });

        Ok(Self {
            role: role.to_owned(),
            child,
            stdin: Some(stdin),
            events,
            stdout: stdout_capture,
            stderr: stderr_capture,
            stdout_reader: Some(stdout_reader),
            stderr_reader: Some(stderr_reader),
        })
    }

    fn process_id(&self) -> u32 {
        self.child.id()
    }

    fn wait_ready(&mut self, timeout: Duration) -> Result<(), String> {
        match self.receive(timeout, "ready")? {
            AllocatorEvent::Ready => Ok(()),
            event => Err(self.unexpected("ready", &event)),
        }
    }

    fn release(&mut self) -> Result<(), String> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| format!("allocator role {:?} stdin is closed", self.role))?;
        writeln!(stdin, "{ALLOCATOR_PROTOCOL_PREFIX}release")
            .and_then(|()| stdin.flush())
            .map_err(|error| format!("failed to release allocator role {:?}: {error}", self.role))
    }

    fn wait_selected(&mut self, timeout: Duration) -> Result<u16, String> {
        match self.receive(timeout, "selected port")? {
            AllocatorEvent::Selected(port) => Ok(port),
            event => Err(self.unexpected("selected port", &event)),
        }
    }

    fn receive(&mut self, timeout: Duration, expected: &str) -> Result<AllocatorEvent, String> {
        match self.events.recv_timeout(timeout) {
            Ok(event) => Ok(event),
            Err(error) => {
                let role = self.role.clone();
                let diagnostic = self.diagnostic();
                Err(format!(
                    "allocator role {role:?} did not reach {expected:?} within {timeout:?}: {error}; {diagnostic}"
                ))
            }
        }
    }

    fn unexpected(&mut self, expected: &str, event: &AllocatorEvent) -> String {
        let event = match event {
            AllocatorEvent::ProtocolError(message) => message.clone(),
            other => format!("{other:?}"),
        };
        let role = self.role.clone();
        let diagnostic = self.diagnostic();
        format!("allocator role {role:?} reached {event}; expected {expected}; {diagnostic}")
    }

    fn diagnostic(&mut self) -> String {
        let status = self
            .child
            .try_wait()
            .map(|status| format!("{status:?}"))
            .unwrap_or_else(|error| format!("<status error: {error}>"));
        let stdout = self
            .stdout
            .lock()
            .expect("allocator stdout lock should not be poisoned");
        let stderr = self
            .stderr
            .lock()
            .expect("allocator stderr lock should not be poisoned");
        format!("status={status}; stdout={stdout:?}; stderr={stderr:?}")
    }
}

impl Drop for AllocatorProcess {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.stdout_reader.take() {
            let _ = reader.join();
        }
        if let Some(reader) = self.stderr_reader.take() {
            let _ = reader.join();
        }
    }
}
