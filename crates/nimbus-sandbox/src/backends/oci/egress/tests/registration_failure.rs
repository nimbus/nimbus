use super::*;

#[test]
fn failed_anchor_removal_cannot_publish_clean_rebind_evidence() {
    let state_root = tempfile::TempDir::new().expect("egress state root should exist");
    let trust_anchor_root = state_root.path().join("trust-anchors");
    let tenant = tenant();
    let id = SandboxId::new("egress-anchor-removal-rebind-fence");
    let port = {
        let listener =
            std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("port probe should bind");
        listener
            .local_addr()
            .expect("port probe address should resolve")
            .port()
    };
    let address = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port);
    let manager = PortManager::new(state_root.path(), port..=port);
    let (_, port_lease) = manager
        .reserve_internal_listener(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(address.ip()).expect("loopback target"),
            PortExposure::Private,
        )
        .expect("PEP listener authority should reserve");
    let bind_claim = claim_bind_attempts(
        state_root.path(),
        std::slice::from_ref(&port_lease),
        OciPortProvider::EgressPep,
        None,
    )
    .expect("provider attempt should claim")
    .pop()
    .expect("one request produces one claim");
    let prepared = PreparedWorkloadPep::prepare(
        WorkloadPepConfig::without_active_policy().with_bind_addr(address),
    )
    .expect("test PEP should bind inertly");
    adopt_claimed_and_activate(
        state_root.path(),
        &port_lease,
        None,
        &bind_claim,
        prepared.local_addr(),
        OciPortProvider::EgressPep,
    )
    .expect("fixture should durably activate before acknowledgement loss");
    let registry = EgressProxyRegistry::with_roots_and_network_state(
        state_root.path().join("decision-logs"),
        &trust_anchor_root,
        state_root.path(),
    );
    let trust_anchor_path = registry.trust_anchor_path_for_test(&tenant, &id);
    fs::create_dir_all(&trust_anchor_path)
        .expect("directory-shaped trust anchor should be created");
    fs::write(trust_anchor_path.join("blocker"), b"retain")
        .expect("nonempty trust anchor must force removal failure");

    let error = registry.compensate_pep_pre_adoption_failure(
        PepPreAdoptionCompensation::bound(
            prepared,
            &trust_anchor_path,
            &port_lease,
            &bind_claim,
            PepPreAdoptionReleaseAuthority::Retain,
        ),
        SandboxError::OperationFailed {
            message: "injected activation acknowledgement loss".to_owned(),
        },
    );
    assert!(
        error
            .to_string()
            .contains("failed to remove egress trust anchor"),
        "cleanup must report the retained publication artifact: {error}"
    );
    let authority = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("authority should reopen");
    let retained = authority
        .inspect(port_lease.lease_id())
        .expect("lease should inspect")
        .expect("lease should remain durable");
    assert_eq!(
        retained.phase(),
        nimbus_network::PortLeasePhase::Active,
        "failed anchor withdrawal must not manufacture clean rebind evidence"
    );
    assert!(
        retained.binding().is_some() && trust_anchor_path.exists(),
        "provider binding and stale publication evidence must remain fenced together"
    );
}

#[test]
fn registration_commit_failure_compensates_activated_provider_and_publication() {
    let state_root = tempfile::TempDir::new().expect("egress state root should exist");
    let trust_anchor_root = state_root.path().join("trust-anchors");
    let tenant = tenant();
    let id = SandboxId::new("egress-registration-commit-failure");
    let port = {
        let listener =
            std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("port probe should bind");
        listener
            .local_addr()
            .expect("port probe address should resolve")
            .port()
    };
    let address = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port);
    let manager = PortManager::new(state_root.path(), port..=port);
    let (_, port_lease, reservation_claim) = manager
        .reserve_internal_listener_for_coordinator(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(address.ip()).expect("loopback target"),
            PortExposure::Private,
        )
        .expect("PEP listener authority should reserve");
    let registry = EgressProxyRegistry::with_roots_and_network_state(
        state_root.path().join("decision-logs"),
        &trust_anchor_root,
        state_root.path(),
    );
    let trust_anchor_path = registry.trust_anchor_path_for_test(&tenant, &id);
    write_trust_anchor_file(
        &trust_anchor_root,
        &trust_anchor_path,
        "test public trust anchor",
    )
    .expect("published trust-anchor evidence should exist");
    let bind_claim = claim_bind_attempts(
        state_root.path(),
        std::slice::from_ref(&port_lease),
        OciPortProvider::EgressPep,
        Some(&reservation_claim),
    )
    .expect("provider attempt should claim")
    .pop()
    .expect("one request produces one claim");
    let prepared = PreparedWorkloadPep::prepare(
        WorkloadPepConfig::without_active_policy().with_bind_addr(address),
    )
    .expect("test PEP should bind inertly");
    adopt_claimed_and_activate(
        state_root.path(),
        &port_lease,
        Some(&reservation_claim),
        &bind_claim,
        prepared.local_addr(),
        OciPortProvider::EgressPep,
    )
    .expect("durable provider activation should commit");
    let artifacts = RegisteredArtifacts {
        trust_anchor_path: Some(trust_anchor_path.clone()),
        tenant_lease: registry.engine.fairness().checkout(&tenant),
        port_lease: Some(port_lease.clone()),
        cleanup: None,
    };
    let workload_id =
        EgressProxyRegistry::workload_id(&tenant, &id).expect("workload identity should derive");
    let slot = registry
        .engine
        .try_reserve(workload_id)
        .expect("registry should remain healthy")
        .expect("failure fixture should reserve exact registration authority");
    let proxy = prepared.start();
    let actual_addr = proxy.local_addr();
    let (primary_error, retained) = slot.retain_failed(
        EgressProxyError::OperationFailed {
            message: "injected registration commit failure".to_owned(),
        },
        proxy,
        artifacts,
    );

    let error = registry.compensate_pep_post_adoption_failure(FailedPepPostAdoption {
        tenant_id: &tenant,
        sandbox_id: &id,
        release_authority: PepPreAdoptionReleaseAuthority::FreshLaunch(&reservation_claim),
        failure_context: "egress PEP registration commit failed",
        primary_error,
        actual_addr,
        retained,
    });
    assert!(
        error
            .to_string()
            .contains("injected registration commit failure"),
        "the registry failure remains primary after successful cleanup: {error}"
    );
    assert!(
        !trust_anchor_path.exists(),
        "confirmed provider stop must precede publication withdrawal"
    );
    let retained = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("authority should reopen")
        .inspect(port_lease.lease_id())
        .expect("lease should inspect")
        .expect("listener authority should remain durable");
    assert_eq!(retained.phase(), nimbus_network::PortLeasePhase::Released);
    assert!(
        retained.bind_claim().is_none() && retained.reservation_claim().is_none(),
        "failed fresh launch must retire every mutable claim after confirmed stop"
    );
    std::net::TcpListener::bind(address)
        .expect("acknowledged provider cleanup must make the real port reusable");
}

#[test]
fn registration_commit_compensation_failure_retains_retryable_tombstone() {
    let state_root = tempfile::TempDir::new().expect("egress state root should exist");
    let trust_anchor_root = state_root.path().join("trust-anchors");
    let tenant = tenant();
    let id = SandboxId::new("egress-registration-compensation-retry");
    let port = {
        let listener =
            std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("port probe should bind");
        listener
            .local_addr()
            .expect("port probe address should resolve")
            .port()
    };
    let address = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port);
    let manager = PortManager::new(state_root.path(), port..=port);
    let (_, port_lease, reservation_claim) = manager
        .reserve_internal_listener_for_coordinator(
            &tenant,
            &id,
            "egress-pep",
            target_for_ip(address.ip()).expect("loopback target"),
            PortExposure::Private,
        )
        .expect("PEP listener authority should reserve");
    let assignment = EgressProxyAssignment {
        host: address.ip().to_string(),
        port,
        port_lease: port_lease.clone(),
    };
    let registry = EgressProxyRegistry::with_roots_and_network_state(
        state_root.path().join("decision-logs"),
        &trust_anchor_root,
        state_root.path(),
    );
    let trust_anchor_path = registry.trust_anchor_path_for_test(&tenant, &id);
    fs::create_dir_all(&trust_anchor_path)
        .expect("directory-shaped trust anchor should be created");
    fs::write(trust_anchor_path.join("blocker"), b"retain")
        .expect("nonempty trust-anchor directory should force removal failure");
    let bind_claim = claim_bind_attempts(
        state_root.path(),
        std::slice::from_ref(&port_lease),
        OciPortProvider::EgressPep,
        Some(&reservation_claim),
    )
    .expect("provider attempt should claim")
    .pop()
    .expect("one request produces one claim");
    let prepared = PreparedWorkloadPep::prepare(
        WorkloadPepConfig::without_active_policy().with_bind_addr(address),
    )
    .expect("test PEP should bind inertly");
    adopt_claimed_and_activate(
        state_root.path(),
        &port_lease,
        Some(&reservation_claim),
        &bind_claim,
        prepared.local_addr(),
        OciPortProvider::EgressPep,
    )
    .expect("durable provider activation should commit");
    let artifacts = RegisteredArtifacts {
        trust_anchor_path: Some(trust_anchor_path.clone()),
        tenant_lease: registry.engine.fairness().checkout(&tenant),
        port_lease: Some(port_lease),
        cleanup: None,
    };
    let workload_id =
        EgressProxyRegistry::workload_id(&tenant, &id).expect("workload identity should derive");
    let slot = registry
        .engine
        .try_reserve(workload_id)
        .expect("registry should remain healthy")
        .expect("failure fixture should reserve exact registration authority");
    let proxy = prepared.start();
    let actual_addr = proxy.local_addr();
    let (primary_error, retained) = slot.retain_failed(
        EgressProxyError::OperationFailed {
            message: "injected registration commit failure".to_owned(),
        },
        proxy,
        artifacts,
    );

    let first_error = registry.compensate_pep_post_adoption_failure(FailedPepPostAdoption {
        tenant_id: &tenant,
        sandbox_id: &id,
        release_authority: PepPreAdoptionReleaseAuthority::FreshLaunch(&reservation_claim),
        failure_context: "egress PEP registration commit failed",
        primary_error,
        actual_addr,
        retained,
    });
    assert!(
        first_error
            .to_string()
            .contains("failed to remove egress trust anchor"),
        "the first compensation must surface the concrete cleanup failure: {first_error}"
    );
    assert!(
        !registry
            .contains(&tenant, &id)
            .expect("conflicted registry should inspect"),
        "readiness must fail closed while a failed registration is quarantined"
    );

    fs::remove_file(trust_anchor_path.join("blocker")).expect("blocker should remove");
    fs::remove_dir(&trust_anchor_path).expect("directory-shaped anchor should remove");
    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("retry must resume the exact failed-registration cleanup evidence");
    let retained = nimbus_network::LocalPortLeaseAuthority::open(state_root.path())
        .expect("authority should reopen")
        .inspect(assignment.port_lease.lease_id())
        .expect("lease should inspect")
        .expect("listener authority should remain durable");
    assert_eq!(retained.phase(), nimbus_network::PortLeasePhase::Released);
    assert!(
        retained.bind_claim().is_none() && retained.reservation_claim().is_none(),
        "successful retry must retire every mutable claim after the fresh-launch release"
    );
    assert!(
        !registry
            .contains(&tenant, &id)
            .expect("cleaned registry should inspect"),
        "successful retry must retire the exact failed-registration tombstone"
    );
    std::net::TcpListener::bind(address)
        .expect("acknowledged provider cleanup must make the real port reusable");
}
