use super::*;
use nimbus_network::PortLeasePhase;

fn replace_anchor_with_nonempty_directory(path: &Path) {
    fs::remove_file(path).expect("published trust anchor should remove");
    fs::create_dir(path).expect("directory-shaped trust anchor should create");
    fs::write(path.join("blocker"), b"retain")
        .expect("nonempty anchor directory must force cleanup failure");
}

fn clear_anchor_blocker(path: &Path) {
    fs::remove_file(path.join("blocker")).expect("anchor blocker should remove");
    fs::remove_dir(path).expect("directory-shaped anchor should remove");
}

#[derive(Clone, Copy)]
enum PepCleanup {
    Final,
    Restart,
}

fn assert_assignmentless_cleanup_preserves_live_leased_pep(id: SandboxId, cleanup: PepCleanup) {
    let state_root = tempfile::TempDir::new().expect("egress state root should exist");
    let tenant = tenant();
    let port = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("port probe should bind")
        .local_addr()
        .expect("port probe address should resolve")
        .port();
    let manager = OciPortLeaseCoordinator::new(state_root.path(), port..=port);
    let (_, port_lease) = manager
        .reserve_internal_listener(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(Ipv4Addr::LOCALHOST.into()).expect("loopback target"),
            PortExposure::Private,
        )
        .expect("PEP lease should reserve before bind");
    let assignment = EgressProxyAssignment {
        host: Ipv4Addr::LOCALHOST.to_string(),
        port,
        port_lease: port_lease.clone(),
    };
    let registry = EgressProxyRegistry::with_roots_and_network_state(
        state_root.path().join("decision-logs"),
        state_root.path().join("trust-anchors"),
        state_root.path(),
    );
    ensure_egress_proxy_running(
        &registry,
        &tenant,
        &id,
        Some(&assignment),
        &EgressPolicy::deny_all(),
    )
    .expect("leased PEP should activate");

    let authority = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("authority should reopen");
    let durable_before = authority
        .inspect(port_lease.lease_id())
        .expect("lease should inspect")
        .expect("active lease should remain durable");
    assert_eq!(durable_before.phase(), PortLeasePhase::Active);
    let readiness_before = registry
        .readiness(&tenant, &id)
        .expect("readiness should inspect")
        .expect("live PEP should report readiness");
    let local_addr_before = registry
        .local_addr(&tenant, &id)
        .expect("local address should inspect")
        .expect("live PEP should retain its socket");
    let trust_anchor_path = registry.trust_anchor_path_for_test(&tenant, &id);
    let trust_anchor_before =
        fs::read(&trust_anchor_path).expect("published trust anchor should be readable");

    let error = match cleanup {
        PepCleanup::Final => registry
            .stop_with_assignment(&tenant, &id, None)
            .expect_err("final teardown without persisted lease authority must fail closed"),
        PepCleanup::Restart => registry
            .stop_for_restart(&tenant, &id, None)
            .expect_err("restart teardown without persisted lease authority must fail closed"),
    };
    assert!(
        error
            .to_string()
            .contains("persisted egress proxy assignment")
            && error.to_string().contains("not cleanup authority"),
        "assignment-less cleanup must identify the missing authority: {error}"
    );

    assert_eq!(
        authority
            .inspect(port_lease.lease_id())
            .expect("lease should inspect")
            .expect("active lease should remain durable"),
        durable_before,
        "assignment-less cleanup must not mutate durable listener authority"
    );
    assert_eq!(
        registry
            .readiness(&tenant, &id)
            .expect("readiness should inspect")
            .expect("live PEP should remain registered"),
        readiness_before,
        "assignment-less cleanup must not withdraw PEP readiness"
    );
    assert_eq!(
        registry
            .local_addr(&tenant, &id)
            .expect("local address should inspect"),
        Some(local_addr_before),
        "assignment-less cleanup must not stop or replace the provider socket"
    );
    assert_eq!(
        fs::read(&trust_anchor_path).expect("trust anchor should remain readable"),
        trust_anchor_before,
        "assignment-less cleanup must not remove or replace the trust anchor"
    );
    std::net::TcpStream::connect_timeout(&local_addr_before, std::time::Duration::from_secs(1))
        .expect("the exact provider socket must remain reachable");

    match cleanup {
        PepCleanup::Final => {
            registry
                .stop_with_assignment(&tenant, &id, Some(&assignment))
                .expect("exact final cleanup should converge");
            assert_eq!(
                authority
                    .inspect(port_lease.lease_id())
                    .expect("lease should inspect")
                    .expect("released receipt should remain durable")
                    .phase(),
                PortLeasePhase::Released
            );
        }
        PepCleanup::Restart => {
            registry
                .stop_for_restart(&tenant, &id, Some(&assignment))
                .expect("exact restart cleanup should converge");
            assert_eq!(
                authority
                    .inspect(port_lease.lease_id())
                    .expect("lease should inspect")
                    .expect("restart receipt should remain durable")
                    .phase(),
                PortLeasePhase::Reserved
            );
            registry
                .stop_with_assignment(&tenant, &id, Some(&assignment))
                .expect("exact final cleanup should consume the restart receipt");
        }
    }
}

fn assert_substituted_provider_port_preserves_live_leased_pep(id: SandboxId, cleanup: PepCleanup) {
    let state_root = tempfile::TempDir::new().expect("egress state root should exist");
    let tenant = tenant();
    let registry = EgressProxyRegistry::with_roots_and_network_state(
        state_root.path().join("decision-logs"),
        state_root.path().join("trust-anchors"),
        state_root.path(),
    );
    let request = port_lease_request(
        &tenant,
        &id,
        "egress-pep",
        OciPortLeaseIntent::host_internal(
            target_for_ip(Ipv4Addr::LOCALHOST.into()).expect("loopback target"),
            PortExposure::Private,
        ),
        nimbus_network::PortRequestMode::ProviderAssigned,
    );
    let request = reserve_provider_assigned(
        registry
            .port_authority()
            .expect("same-process PEP port authority should remain available"),
        request,
    )
    .expect("provider-assigned PEP identity should reserve");
    registry
        .ensure_running_with_lease(
            &tenant,
            &id,
            &EgressPolicy::deny_all(),
            loopback(),
            &request,
        )
        .expect("provider-assigned PEP should activate");

    let local_addr_before = registry
        .local_addr(&tenant, &id)
        .expect("local address should inspect")
        .expect("live PEP should retain its socket");
    let exact_assignment = EgressProxyAssignment {
        host: local_addr_before.ip().to_string(),
        port: local_addr_before.port(),
        port_lease: request.clone(),
    };
    let substituted_port = if local_addr_before.port() == u16::MAX {
        u16::MAX - 1
    } else {
        local_addr_before.port() + 1
    };
    let substituted_assignment = EgressProxyAssignment {
        host: local_addr_before.ip().to_string(),
        port: substituted_port,
        port_lease: request.clone(),
    };
    let authority = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("authority should reopen");
    let durable_before = authority
        .inspect(request.lease_id())
        .expect("lease should inspect")
        .expect("active lease should remain durable");
    assert_eq!(durable_before.phase(), PortLeasePhase::Active);
    let readiness_before = registry
        .readiness(&tenant, &id)
        .expect("readiness should inspect")
        .expect("live PEP should report readiness");
    let trust_anchor_path = registry.trust_anchor_path_for_test(&tenant, &id);
    let trust_anchor_before =
        fs::read(&trust_anchor_path).expect("published trust anchor should be readable");

    let error = match cleanup {
        PepCleanup::Final => registry
            .stop_with_assignment(&tenant, &id, Some(&substituted_assignment))
            .expect_err("final teardown with a substituted provider port must fail closed"),
        PepCleanup::Restart => registry
            .stop_for_restart(&tenant, &id, Some(&substituted_assignment))
            .expect_err("restart teardown with a substituted provider port must fail closed"),
    };
    assert!(
        error.to_string().contains("does not own expected port")
            && error.to_string().contains(&substituted_port.to_string()),
        "substituted cleanup must identify the unauthoritative concrete port: {error}"
    );

    assert_eq!(
        authority
            .inspect(request.lease_id())
            .expect("lease should inspect")
            .expect("active lease should remain durable"),
        durable_before,
        "substituted cleanup must not mutate durable listener authority"
    );
    assert_eq!(
        registry
            .readiness(&tenant, &id)
            .expect("readiness should inspect")
            .expect("live PEP should remain registered"),
        readiness_before,
        "substituted cleanup must not withdraw PEP readiness"
    );
    assert_eq!(
        registry
            .local_addr(&tenant, &id)
            .expect("local address should inspect"),
        Some(local_addr_before),
        "substituted cleanup must not stop or replace the provider socket"
    );
    assert_eq!(
        fs::read(&trust_anchor_path).expect("trust anchor should remain readable"),
        trust_anchor_before,
        "substituted cleanup must not remove or replace the trust anchor"
    );
    std::net::TcpStream::connect_timeout(&local_addr_before, std::time::Duration::from_secs(1))
        .expect("the exact provider socket must remain reachable");

    match cleanup {
        PepCleanup::Final => {
            registry
                .stop_with_assignment(&tenant, &id, Some(&exact_assignment))
                .expect("exact final cleanup should converge");
            assert_eq!(
                authority
                    .inspect(request.lease_id())
                    .expect("lease should inspect")
                    .expect("released receipt should remain durable")
                    .phase(),
                PortLeasePhase::Released
            );
        }
        PepCleanup::Restart => {
            registry
                .stop_for_restart(&tenant, &id, Some(&exact_assignment))
                .expect("exact restart cleanup should converge");
            assert_eq!(
                authority
                    .inspect(request.lease_id())
                    .expect("lease should inspect")
                    .expect("restart receipt should remain durable")
                    .phase(),
                PortLeasePhase::Reserved
            );
            registry
                .stop_with_assignment(&tenant, &id, Some(&exact_assignment))
                .expect("exact final cleanup should consume the restart receipt");
        }
    }
}

#[test]
fn assignmentless_final_cleanup_cannot_borrow_live_leased_pep_authority() {
    assert_assignmentless_cleanup_preserves_live_leased_pep(
        SandboxId::new("egress-assignmentless-final"),
        PepCleanup::Final,
    );
}

#[test]
fn assignmentless_restart_cleanup_cannot_borrow_live_leased_pep_authority() {
    assert_assignmentless_cleanup_preserves_live_leased_pep(
        SandboxId::new("egress-assignmentless-restart"),
        PepCleanup::Restart,
    );
}

#[test]
fn substituted_provider_port_cannot_authorize_final_pep_cleanup() {
    assert_substituted_provider_port_preserves_live_leased_pep(
        SandboxId::new("egress-substituted-provider-port-final"),
        PepCleanup::Final,
    );
}

#[test]
fn substituted_provider_port_cannot_authorize_restart_pep_cleanup() {
    assert_substituted_provider_port_preserves_live_leased_pep(
        SandboxId::new("egress-substituted-provider-port-restart"),
        PepCleanup::Restart,
    );
}

#[test]
fn assignmentless_cleanup_remains_idempotent_when_no_pep_is_registered() {
    let registry = EgressProxyRegistry::new();
    let tenant = tenant();
    let final_id = SandboxId::new("egress-assignmentless-absent-final");
    let restart_id = SandboxId::new("egress-assignmentless-absent-restart");

    registry
        .stop_with_assignment(&tenant, &final_id, None)
        .expect("absent final cleanup should remain idempotent");
    registry
        .stop_for_restart(&tenant, &restart_id, None)
        .expect("absent restart cleanup should remain idempotent");
}

#[test]
fn activation_ack_loss_anchor_failure_retains_restart_tombstone() {
    let state_root = tempfile::TempDir::new().expect("egress state root should exist");
    let tenant = tenant();
    let id = SandboxId::new("egress-activation-ack-restart-tombstone");
    let port = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("port probe should bind")
        .local_addr()
        .expect("port probe address should resolve")
        .port();
    let manager = OciPortLeaseCoordinator::new(state_root.path(), port..=port);
    let (_, port_lease) = manager
        .reserve_internal_listener(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(Ipv4Addr::LOCALHOST.into()).expect("loopback target"),
            PortExposure::Private,
        )
        .expect("PEP lease should reserve before bind");
    let mut registry = EgressProxyRegistry::with_roots_and_network_state(
        state_root.path().join("decision-logs"),
        state_root.path().join("trust-anchors"),
        state_root.path(),
    );
    let trust_anchor_path = registry.trust_anchor_path_for_test(&tenant, &id);
    let blocked_anchor = trust_anchor_path.clone();
    registry = registry.with_post_activation_observer(move || {
        replace_anchor_with_nonempty_directory(&blocked_anchor);
        Err(SandboxError::OperationFailed {
            message: "injected activation acknowledgement loss".to_owned(),
        })
    });

    let error = registry
        .ensure_running_with_lease(
            &tenant,
            &id,
            &EgressPolicy::deny_all(),
            (Ipv4Addr::LOCALHOST, port).into(),
            &port_lease,
        )
        .expect_err("acknowledgement loss plus anchor failure must retain cleanup");
    assert!(
        error
            .to_string()
            .contains("injected activation acknowledgement loss")
            && error
                .to_string()
                .contains("failed to remove egress trust anchor")
            && error.to_string().contains("stopping tombstone"),
        "the primary and exact retry state must remain visible: {error}"
    );
    assert!(
        !registry
            .contains(&tenant, &id)
            .expect("readiness should inspect"),
        "a retained stopping tombstone must deny readiness"
    );
    let assignment = EgressProxyAssignment {
        host: Ipv4Addr::LOCALHOST.to_string(),
        port,
        port_lease: port_lease.clone(),
    };
    let authority = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("authority should reopen");
    let pending = authority
        .inspect(port_lease.lease_id())
        .expect("lease should inspect")
        .expect("lease should remain durable");
    assert_eq!(
        pending.phase(),
        PortLeasePhase::Active,
        "restart cleanup must retain active authority until anchor withdrawal succeeds"
    );
    assert!(pending.binding().is_some());
    assert!(trust_anchor_path.exists());

    let fresh_registry = EgressProxyRegistry::with_roots_and_network_state(
        state_root.path().join("decision-logs"),
        state_root.path().join("trust-anchors"),
        state_root.path(),
    );
    let fresh_error = fresh_registry
        .stop_for_restart(&tenant, &id, Some(&assignment))
        .expect_err("fresh-process provider absence must remain NNC3.8-fenced");
    assert!(
        fresh_error.to_string().contains("provider evidence"),
        "fresh registry must report the missing provider proof: {fresh_error}"
    );
    assert_eq!(
        authority
            .inspect(port_lease.lease_id())
            .expect("lease should inspect")
            .expect("lease should remain durable"),
        pending,
        "fresh-registry rejection must not mutate durable authority"
    );
    assert!(trust_anchor_path.exists());

    clear_anchor_blocker(&trust_anchor_path);
    registry
        .stop_for_restart(&tenant, &id, Some(&assignment))
        .expect("the original tombstone must retry exact anchor cleanup");
    let retained = authority
        .inspect(port_lease.lease_id())
        .expect("lease should inspect")
        .expect("lease should remain durable");
    assert_eq!(retained.phase(), PortLeasePhase::Reserved);
    assert!(retained.binding().is_none());
    assert!(retained.confirmed_stopped_binding().is_some());
    assert!(!trust_anchor_path.exists());
    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("final stop may consume the durable restart receipt");
    assert_eq!(
        authority
            .inspect(port_lease.lease_id())
            .expect("lease should inspect")
            .expect("released receipt should remain durable")
            .phase(),
        PortLeasePhase::Released
    );
}

#[test]
fn fresh_launch_activation_ack_loss_anchor_failure_fences_release() {
    let state_root = tempfile::TempDir::new().expect("egress state root should exist");
    let tenant = tenant();
    let id = SandboxId::new("egress-activation-ack-final-tombstone");
    let port = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("port probe should bind")
        .local_addr()
        .expect("port probe address should resolve")
        .port();
    let manager = OciPortLeaseCoordinator::new(state_root.path(), port..=port);
    let (_, port_lease, reservation_claim) = manager
        .reserve_internal_listener_for_coordinator(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(Ipv4Addr::LOCALHOST.into()).expect("loopback target"),
            PortExposure::Private,
        )
        .expect("fresh launch should reserve its coordinator claim");
    let mut registry = EgressProxyRegistry::with_roots_and_network_state(
        state_root.path().join("decision-logs"),
        state_root.path().join("trust-anchors"),
        state_root.path(),
    );
    let trust_anchor_path = registry.trust_anchor_path_for_test(&tenant, &id);
    let blocked_anchor = trust_anchor_path.clone();
    registry = registry.with_post_activation_observer(move || {
        replace_anchor_with_nonempty_directory(&blocked_anchor);
        Err(SandboxError::OperationFailed {
            message: "injected fresh-launch activation acknowledgement loss".to_owned(),
        })
    });

    let error = registry
        .ensure_running_with_lease_and_release_authority(
            &tenant,
            &id,
            &EgressPolicy::deny_all(),
            (Ipv4Addr::LOCALHOST, port).into(),
            &port_lease,
            PepPreAdoptionReleaseAuthority::FreshLaunch(&reservation_claim),
        )
        .expect_err("failed post-activation cleanup must retain final-stop authority");
    assert!(
        error
            .to_string()
            .contains("fresh-launch activation acknowledgement loss")
            && error.to_string().contains("stopping tombstone"),
        "the exact post-adoption failure must remain visible: {error}"
    );
    let assignment = EgressProxyAssignment {
        host: Ipv4Addr::LOCALHOST.to_string(),
        port,
        port_lease: port_lease.clone(),
    };
    let authority = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("authority should reopen");
    let pending = authority
        .inspect(port_lease.lease_id())
        .expect("lease should inspect")
        .expect("lease should remain durable");
    assert_eq!(
        pending.phase(),
        PortLeasePhase::Withdrawing,
        "final cleanup must retain the listener fence until anchor withdrawal succeeds"
    );
    assert!(pending.binding().is_some());
    assert!(trust_anchor_path.exists());

    let fresh_registry = EgressProxyRegistry::with_roots_and_network_state(
        state_root.path().join("decision-logs"),
        state_root.path().join("trust-anchors"),
        state_root.path(),
    );
    fresh_registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect_err("fresh-registry cleanup must not manufacture provider absence");
    assert_eq!(
        authority
            .inspect(port_lease.lease_id())
            .expect("lease should inspect")
            .expect("lease should remain durable"),
        pending
    );

    clear_anchor_blocker(&trust_anchor_path);
    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("the retained final-stop tombstone must converge");
    assert!(!trust_anchor_path.exists());
    assert_eq!(
        authority
            .inspect(port_lease.lease_id())
            .expect("lease should inspect")
            .expect("released receipt should remain durable")
            .phase(),
        PortLeasePhase::Released
    );
    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("completed final-stop replay must be idempotent");
}

#[test]
fn released_lease_replay_does_not_repeat_withdraw_after_checkpoint_loss() {
    let state_root = tempfile::TempDir::new().expect("egress state root should exist");
    let tenant = tenant();
    let id = SandboxId::new("egress-released-checkpoint-loss");
    let port = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("port probe should bind")
        .local_addr()
        .expect("port probe address should resolve")
        .port();
    let manager = OciPortLeaseCoordinator::new(state_root.path(), port..=port);
    let (_, port_lease) = manager
        .reserve_internal_listener(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(Ipv4Addr::LOCALHOST.into()).expect("loopback target"),
            PortExposure::Private,
        )
        .expect("PEP lease should reserve before bind");
    let assignment = EgressProxyAssignment {
        host: Ipv4Addr::LOCALHOST.to_string(),
        port,
        port_lease: port_lease.clone(),
    };
    let registry = EgressProxyRegistry::with_roots_and_network_state(
        state_root.path().join("decision-logs"),
        state_root.path().join("trust-anchors"),
        state_root.path(),
    );
    let trust_anchor_path = registry.trust_anchor_path_for_test(&tenant, &id);
    ensure_egress_proxy_running(
        &registry,
        &tenant,
        &id,
        Some(&assignment),
        &EgressPolicy::deny_all(),
    )
    .expect("PEP should activate before cleanup");

    crate::backends::oci::egress::cleanup::set_post_durable_transition_fault(|| {
        Err(SandboxError::OperationFailed {
            message: "injected durable-transition acknowledgement loss".to_owned(),
        })
    });
    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect_err("post-transition acknowledgement loss must retain the cleanup checkpoint");
    let authority = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("authority should reopen");
    let released = authority
        .inspect(port_lease.lease_id())
        .expect("lease should inspect")
        .expect("lease should remain durable");
    assert_eq!(
        released.phase(),
        PortLeasePhase::Released,
        "the exact live-owner release must commit before the injected acknowledgement loss"
    );
    assert!(
        !trust_anchor_path.exists(),
        "provider and trust-anchor cleanup precede the durable lease transition"
    );

    let withdraw_attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed_attempts = std::sync::Arc::clone(&withdraw_attempts);
    crate::backends::oci::egress::cleanup::set_pre_withdraw_observer(move || {
        observed_attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    });
    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("released durable evidence must converge the retained tombstone");
    assert_eq!(
        withdraw_attempts.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "a durably Released exact lease must suppress duplicate withdrawal"
    );
    assert!(!trust_anchor_path.exists());
    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("completed replay should remain idempotent");
}

#[test]
fn restart_rejects_released_lease_without_process_local_provider() {
    let state_root = tempfile::TempDir::new().expect("egress state root should exist");
    let tenant = tenant();
    let id = SandboxId::new("egress-restart-released");
    let port = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("port probe should bind")
        .local_addr()
        .expect("port probe address should resolve")
        .port();
    let manager = OciPortLeaseCoordinator::new(state_root.path(), port..=port);
    let (_, port_lease) = manager
        .reserve_internal_listener(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(Ipv4Addr::LOCALHOST.into()).expect("loopback target"),
            PortExposure::Private,
        )
        .expect("PEP lease should reserve");
    // Same-process fixture authority: retain one handle across terminalization
    // and the restart rejection assertions below.
    let authority = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("fixture authority should open");
    crate::backends::oci::port_lease::withdraw(&authority, &port_lease)
        .expect("effect-free lease should enter withdrawal");
    crate::backends::oci::port_lease::release(&authority, &port_lease)
        .expect("effect-free lease should release");
    let assignment = EgressProxyAssignment {
        host: Ipv4Addr::LOCALHOST.to_string(),
        port,
        port_lease: port_lease.clone(),
    };
    let registry = EgressProxyRegistry::with_roots_and_network_state(
        state_root.path().join("decision-logs"),
        state_root.path().join("trust-anchors"),
        state_root.path(),
    );
    assert_eq!(
        authority
            .inspect(port_lease.lease_id())
            .expect("lease should inspect")
            .expect("released lease should remain durable")
            .phase(),
        PortLeasePhase::Released
    );

    let error = registry
        .stop_for_restart(&tenant, &id, Some(&assignment))
        .expect_err("Released authority cannot be retained for rebind");
    assert!(
        error.to_string().contains("Released") && error.to_string().contains("not rebindable"),
        "restart rejection must identify the terminal durable phase: {error}"
    );
    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("final-release replay must remain idempotent");
}

#[test]
fn restart_rejects_failed_lease_without_process_local_provider() {
    let state_root = tempfile::TempDir::new().expect("egress state root should exist");
    let tenant = tenant();
    let id = SandboxId::new("egress-restart-failed");
    let port = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("port probe should bind")
        .local_addr()
        .expect("port probe address should resolve")
        .port();
    let manager = OciPortLeaseCoordinator::new(state_root.path(), port..=port);
    let (_, port_lease, reservation_claim) = manager
        .reserve_internal_listener_for_coordinator(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(Ipv4Addr::LOCALHOST.into()).expect("loopback target"),
            PortExposure::Private,
        )
        .expect("PEP lease should reserve for its launch coordinator");
    // Same-process fixture authority: retain one handle across terminalization
    // and the restart rejection assertions below.
    let authority = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("fixture authority should open");
    let bind_claim = crate::backends::oci::port_lease::claim_bind_attempts(
        &authority,
        std::slice::from_ref(&port_lease),
        crate::backends::oci::port_lease::OciPortProvider::EgressPep,
        Some(&reservation_claim),
    )
    .expect("PEP bind attempt should claim")
    .pop()
    .expect("one request should return one claim");
    crate::backends::oci::port_lease::record_bind_failure(
        &authority,
        &port_lease,
        &bind_claim,
        crate::backends::oci::port_lease::OciConfirmedBindFailure::new(
            (Ipv4Addr::LOCALHOST, port).into(),
            crate::backends::oci::port_lease::OciPortProvider::EgressPep,
            std::io::ErrorKind::AddrInUse,
        ),
        Some(&reservation_claim),
    )
    .expect("confirmed no-effect bind failure should persist");
    let assignment = EgressProxyAssignment {
        host: Ipv4Addr::LOCALHOST.to_string(),
        port,
        port_lease: port_lease.clone(),
    };
    let registry = EgressProxyRegistry::with_roots_and_network_state(
        state_root.path().join("decision-logs"),
        state_root.path().join("trust-anchors"),
        state_root.path(),
    );
    assert_eq!(
        authority
            .inspect(port_lease.lease_id())
            .expect("lease should inspect")
            .expect("failed lease should remain durable")
            .phase(),
        PortLeasePhase::Failed
    );

    let error = registry
        .stop_for_restart(&tenant, &id, Some(&assignment))
        .expect_err("Failed authority cannot be retained for rebind");
    assert!(
        error.to_string().contains("Failed") && error.to_string().contains("not rebindable"),
        "restart rejection must identify the terminal durable phase: {error}"
    );
    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("failed no-effect final-release replay must remain idempotent");
}
