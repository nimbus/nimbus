use super::*;

fn mutate_port_lease_request(
    request: &PortLeaseRequest,
    mutation: impl FnOnce(&mut serde_json::Value),
) -> PortLeaseRequest {
    let mut wire = serde_json::to_value(request).expect("port lease request should serialize");
    mutation(&mut wire);
    serde_json::from_value(wire).expect("substituted port lease request should remain valid")
}

fn assert_assignment_substitution_not_ready(
    registry: &EgressProxyRegistry,
    tenant: &TenantId,
    id: &SandboxId,
    assignment: &EgressProxyAssignment,
    label: &str,
    mutation: impl FnOnce(&mut serde_json::Value),
) {
    let mut substituted = assignment.clone();
    substituted.port_lease = mutate_port_lease_request(&assignment.port_lease, mutation);
    let observed = registry
        .authenticated_readiness(
            tenant,
            id,
            Some(&substituted),
            &EgressPolicy::deny_all(),
            None,
        )
        .unwrap_or_else(|error| panic!("{label} should produce an honest observation: {error}"));
    assert!(
        matches!(observed, EgressReadinessState::NotReady(_)),
        "{label} must emit no ready evidence: {observed:?}"
    );
}

fn assert_durable_record_substitution_not_ready(
    state_root: &Path,
    registry: &EgressProxyRegistry,
    tenant: &TenantId,
    id: &SandboxId,
    assignment: &EgressProxyAssignment,
    label: &str,
    mutation: impl FnOnce(&mut serde_json::Value),
) {
    use std::convert::Infallible;

    let store =
        nimbus_network::LocalNetworkStateStore::open(state_root).expect("state store should open");
    let lease_id = assignment.port_lease.lease_id().to_string();
    let original = store
        .transaction::<serde_json::Value, serde_json::Value, Infallible>(
            &nimbus_network::NetworkStatePartition::PortLeases,
            |partition| {
                let original = partition.clone();
                let record = partition
                    .get_mut("leases")
                    .and_then(serde_json::Value::as_object_mut)
                    .and_then(|leases| leases.get_mut(&lease_id))
                    .expect("the exact PEP lease should remain durable");
                mutation(record);
                Ok(original)
            },
        )
        .expect("test substitution should commit");
    let observed = registry.authenticated_readiness(
        tenant,
        id,
        Some(assignment),
        &EgressPolicy::deny_all(),
        None,
    );
    store
        .transaction::<serde_json::Value, (), Infallible>(
            &nimbus_network::NetworkStatePartition::PortLeases,
            |partition| {
                *partition = original;
                Ok(())
            },
        )
        .expect("exact durable authority should restore before assertion");

    let observed =
        observed.unwrap_or_else(|error| panic!("{label} should fail closed as evidence: {error}"));
    assert!(
        matches!(observed, EgressReadinessState::NotReady(_)),
        "{label} must emit no ready evidence: {observed:?}"
    );
}

fn start_leased_test_pep(
    name: &str,
    policy: &EgressPolicy,
) -> (
    tempfile::TempDir,
    EgressProxyRegistry,
    TenantId,
    SandboxId,
    EgressProxyAssignment,
) {
    let state_root = tempfile::TempDir::new().expect("egress state root should exist");
    let tenant = tenant();
    let id = SandboxId::new(name);
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
    let assignment = EgressProxyAssignment {
        host: Ipv4Addr::LOCALHOST.to_string(),
        port,
        port_lease,
    };
    let registry = EgressProxyRegistry::with_roots_and_network_state(
        state_root.path().join("decision-logs"),
        state_root.path().join("trust-anchors"),
        state_root.path(),
    );
    ensure_egress_proxy_running(&registry, &tenant, &id, Some(&assignment), policy)
        .expect("leased PEP should activate");
    (state_root, registry, tenant, id, assignment)
}

#[test]
fn ensure_running_rejects_reuse_with_stale_policy_bytes() {
    let registry = EgressProxyRegistry::new();
    let tenant = tenant();
    let id = SandboxId::new("egress-stale-policy-reuse");
    let assignment = start_test_pep(&registry, &tenant, &id, &EgressPolicy::deny_all());
    let changed = EgressPolicy::new([nimbus_egress::EgressRule::new(
        "changed-policy",
        nimbus_egress::EgressProtocol::Https,
        "example.com",
        443,
    )]);

    let error = ensure_egress_proxy_running(&registry, &tenant, &id, Some(&assignment), &changed)
        .expect_err("an occupied PEP must not validate against stale active policy bytes");

    assert!(
        error.to_string().contains("exact expected active policy"),
        "stale reuse should name the policy mismatch: {error}"
    );
    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("fixture PEP should stop");
}

#[test]
fn authenticated_readiness_rejects_missing_or_substituted_pep_evidence() {
    use crate::backends::oci::egress::readiness::EgressReadinessFailure;

    let (root, registry, tenant, id, assignment) =
        start_leased_test_pep("egress-authenticated-readiness", &EgressPolicy::deny_all());
    assert!(matches!(
        registry
            .authenticated_readiness(
                &tenant,
                &id,
                Some(&assignment),
                &EgressPolicy::deny_all(),
                None,
            )
            .expect("exact readiness should inspect"),
        EgressReadinessState::Ready(_)
    ));

    let changed_policy = EgressPolicy::new([nimbus_egress::EgressRule::new(
        "changed-policy",
        nimbus_egress::EgressProtocol::Https,
        "example.com",
        443,
    )]);
    assert!(matches!(
        registry
            .authenticated_readiness(&tenant, &id, Some(&assignment), &changed_policy, None)
            .expect("policy mismatch should be an honest not-ready observation"),
        EgressReadinessState::NotReady(EgressReadinessFailure::PolicyMismatch)
    ));

    let mut applying = EgressPolicyReloadState::initial();
    applying
        .begin()
        .expect("fixture should persist one applying attempt");
    assert!(matches!(
        registry
            .authenticated_readiness(
                &tenant,
                &id,
                Some(&assignment),
                &EgressPolicy::deny_all(),
                Some(&applying),
            )
            .expect("applying state should be an honest not-ready observation"),
        EgressReadinessState::NotReady(EgressReadinessFailure::ReloadApplying)
    ));

    let mut wrong_address = assignment.clone();
    wrong_address.host = Ipv4Addr::new(127, 0, 0, 2).to_string();
    assert!(matches!(
        registry
            .authenticated_readiness(
                &tenant,
                &id,
                Some(&wrong_address),
                &EgressPolicy::deny_all(),
                None,
            )
            .expect("address substitution should fail closed"),
        EgressReadinessState::NotReady(EgressReadinessFailure::ListenerAddressMismatch)
    ));

    let foreign_tenant = TenantId::new("tenant-egress-foreign").expect("foreign tenant id");
    assert!(matches!(
        registry
            .authenticated_readiness(
                &foreign_tenant,
                &id,
                Some(&assignment),
                &EgressPolicy::deny_all(),
                None,
            )
            .expect("foreign tenant lookup should fail closed"),
        EgressReadinessState::NotReady(EgressReadinessFailure::MissingRegistration)
    ));

    let foreign_listener =
        nimbus_network::ListenerId::for_tenant_workload_listener(&tenant, "foreign-sandbox", "pep");
    assert_assignment_substitution_not_ready(
        &registry,
        &tenant,
        &id,
        &assignment,
        "wrong lease request owner",
        |wire| {
            wire["owner_id"] = serde_json::to_value(nimbus_network::NetworkResourceId::from(
                foreign_listener.clone(),
            ))
            .expect("foreign listener owner should serialize");
        },
    );
    assert_assignment_substitution_not_ready(
        &registry,
        &tenant,
        &id,
        &assignment,
        "wrong lease generation",
        |wire| wire["generation"] = serde_json::json!(2),
    );
    assert_assignment_substitution_not_ready(
        &registry,
        &tenant,
        &id,
        &assignment,
        "wrong lease epoch",
        |wire| wire["lease_epoch"] = serde_json::json!(2),
    );
    assert_assignment_substitution_not_ready(
        &registry,
        &tenant,
        &id,
        &assignment,
        "wrong listener identity",
        |wire| {
            wire["lease_id"] =
                serde_json::to_value(nimbus_network::PortLeaseId::for_listener(&foreign_listener))
                    .expect("foreign listener lease should serialize");
        },
    );
    let foreign_sandbox = SandboxId::new("egress-authenticated-foreign-sandbox");
    let foreign_sandbox_observed = registry
        .authenticated_readiness(
            &tenant,
            &foreign_sandbox,
            Some(&assignment),
            &EgressPolicy::deny_all(),
            None,
        )
        .expect("foreign sandbox identity should produce an honest observation");
    assert!(
        matches!(
            foreign_sandbox_observed,
            EgressReadinessState::NotReady(EgressReadinessFailure::MissingRegistration)
        ),
        "wrong sandbox identity must emit no ready evidence: {foreign_sandbox_observed:?}"
    );

    let foreign_provider =
        nimbus_network::NetworkProviderId::for_registration_key("foreign-pep-provider");
    assert_durable_record_substitution_not_ready(
        root.path(),
        &registry,
        &tenant,
        &id,
        &assignment,
        "wrong durable provider",
        |record| {
            record["binding"]["provider_handle"]["provider_id"] =
                serde_json::to_value(&foreign_provider)
                    .expect("foreign provider identity should serialize");
            record["adoption_claim"]["provider_attempt"]["provider_id"] =
                serde_json::to_value(&foreign_provider)
                    .expect("foreign provider identity should serialize");
        },
    );
    assert_durable_record_substitution_not_ready(
        root.path(),
        &registry,
        &tenant,
        &id,
        &assignment,
        "wrong durable phase",
        |record| record["phase"] = serde_json::json!("withdrawing"),
    );
    assert_durable_record_substitution_not_ready(
        root.path(),
        &registry,
        &tenant,
        &id,
        &assignment,
        "wrong durable active lifetime",
        |record| {
            let generation = record["last_lifetime_generation"]
                .as_u64()
                .expect("active lifetime generation should serialize")
                + 1;
            record["last_lifetime_generation"] = serde_json::json!(generation);
            record["active_lifetime"]["generation"] = serde_json::json!(generation);
        },
    );

    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("fixture PEP should stop");
}

#[test]
fn authenticated_readiness_requires_exact_completed_reload_attempt() {
    use crate::backends::oci::egress::readiness::EgressReadinessFailure;

    let initial = EgressPolicy::deny_all();
    let (_root, registry, tenant, id, assignment) =
        start_leased_test_pep("egress-authenticated-reload", &initial);
    let desired = EgressPolicy::new([nimbus_egress::EgressRule::new(
        "reloaded-policy",
        nimbus_egress::EgressProtocol::Https,
        "example.com",
        443,
    )]);
    let compiled = desired
        .compile()
        .expect("desired reload policy should compile");
    let mut reload = EgressPolicyReloadState::initial();
    let attempt = reload.begin().expect("reload attempt should begin");
    let receipt = registry
        .reconcile_reload(&tenant, &id, compiled, attempt)
        .expect("exact reload should apply");
    reload
        .complete(receipt)
        .expect("exact receipt should complete durable state");

    assert!(matches!(
        registry
            .authenticated_readiness(&tenant, &id, Some(&assignment), &desired, Some(&reload))
            .expect("completed exact reload should inspect"),
        EgressReadinessState::Ready(_)
    ));

    let mut substituted_wire =
        serde_json::to_value(&reload).expect("reload state should serialize");
    substituted_wire["latest_attempt_generation"] = serde_json::json!(2);
    let substituted: EgressPolicyReloadState =
        serde_json::from_value(substituted_wire).expect("substituted fixture wire should parse");
    assert!(matches!(
        registry
            .authenticated_readiness(
                &tenant,
                &id,
                Some(&assignment),
                &desired,
                Some(&substituted),
            )
            .expect("attempt substitution should fail closed"),
        EgressReadinessState::NotReady(EgressReadinessFailure::ReloadAttemptMismatch { .. })
    ));

    registry
        .stop_with_assignment(&tenant, &id, Some(&assignment))
        .expect("fixture PEP should stop");
}
