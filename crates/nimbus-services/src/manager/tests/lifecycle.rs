use super::*;

#[tokio::test]
async fn start_service_for_decision_rejects_built_in_backend_before_launch() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([("browser".to_owned(), ServiceBackend::built_in("browser"))]),
        }),
        backend.clone(),
    );
    let isolation = TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
    let decision = manager
        .service_lifecycle_decision(&isolation, "browser")
        .expect("browser service activation decision should build");

    let error = manager
        .start_service_for_decision_async(&decision, "browser", HostCallCancellation::default())
        .await
        .expect_err("sandbox manager must reject built-in service backends");

    assert!(
        error.to_string().contains("built-in backend"),
        "error should name unsupported backend: {error}"
    );
    assert_eq!(
        backend.image_starts.load(Ordering::SeqCst),
        0,
        "built-in services must not reach image start"
    );
    assert_eq!(
        backend.build_starts.load(Ordering::SeqCst),
        0,
        "built-in services must not reach build start"
    );
}

#[tokio::test]
async fn start_service_for_decision_rejects_unadmitted_service_before_launch() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "cache".to_owned(),
                image_service_backend("cache", "redis:7"),
            )]),
        }),
        backend.clone(),
    );
    let isolation = TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
    let decision = manager
        .service_lifecycle_decision(&isolation, "db")
        .expect("db service activation decision should build");

    let error = manager
        .start_service_for_decision_async(&decision, "cache", HostCallCancellation::default())
        .await
        .expect_err("decision must reject a forged lower-seam service name");

    assert!(
        error.to_string().contains("permission denied"),
        "error should map to permission denial: {error}"
    );
    assert!(
        error
            .to_string()
            .contains("did not authorize service `cache`"),
        "error should name the rejected service: {error}"
    );
    assert_eq!(
        backend.image_starts.load(Ordering::SeqCst),
        0,
        "unadmitted service should fail before the sandbox backend is called"
    );
}

#[tokio::test]
async fn start_service_for_decision_rejects_unadmitted_sandbox_egress_before_launch() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let egress = EgressPolicy::new([EgressRule::new(
        "stripe",
        nimbus_egress::EgressProtocol::Https,
        "api.stripe.com",
        443,
    )]);
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "db".to_owned(),
                ServiceBackend::sandbox(sparse_image_spec("db").with_egress_policy(egress)),
            )]),
        }),
        backend.clone(),
    );
    let isolation = TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
    let decision = manager
        .service_lifecycle_decision(&isolation, "db")
        .expect("db service activation decision should build");

    let error = manager
        .start_service_for_decision_async(&decision, "db", HostCallCancellation::default())
        .await
        .expect_err("decision must reject unadmitted sandbox egress policy");

    assert!(
        error
            .to_string()
            .contains("did not authorize sandbox egress policy"),
        "error should name the egress-policy mismatch: {error}"
    );
    assert_eq!(
        backend.image_starts.load(Ordering::SeqCst),
        0,
        "unadmitted egress should fail before the sandbox backend is called"
    );
}

#[tokio::test]
async fn start_service_for_decision_accepts_matching_sandbox_egress_policy() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let egress = EgressPolicy::new([EgressRule::new(
        "stripe",
        nimbus_egress::EgressProtocol::Https,
        "api.stripe.com",
        443,
    )]);
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "db".to_owned(),
                ServiceBackend::sandbox(sparse_image_spec("db").with_egress_policy(egress.clone())),
            )]),
        }),
        backend.clone(),
    )
    .with_activation_poll_interval(Duration::from_millis(1))
    .with_activation_timeout(Duration::from_secs(1));
    let isolation = TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
    let decision = isolation
        .admit_decision(
            TenantIsolationPolicyInput::new(WorkloadAttributes::service("db"))
                .with_services(TenantServiceGrantPolicyDecision::new(["db"]))
                .with_network(
                    nimbus_tenant::TenantNetworkPolicyDecision::default()
                        .with_sandbox_egress(egress)
                        .expect("test egress policy should compile"),
                ),
        )
        .expect("decision with matching egress should admit");

    manager
        .start_service_for_decision_async(&decision, "db", HostCallCancellation::default())
        .await
        .expect("matching egress policy should start")
        .expect("handle should be returned");

    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn start_service_for_decision_rejects_unverified_image_before_materialization() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let image = "registry.example.com/nimbus/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([("api".to_owned(), image_service_backend("api", image))]),
        }),
        backend.clone(),
    );
    let isolation = TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
    let decision = isolation
        .admit_decision(
            TenantIsolationPolicyInput::new(WorkloadAttributes::service("api"))
                .with_services(TenantServiceGrantPolicyDecision::new(["api"]))
                .with_image(
                    nimbus_tenant::TenantImagePolicyDecision::digest_pinned(image)
                        .require_signature("https://issuer.example.com", "repo:nimbus/api"),
                ),
        )
        .expect("image policy decision should admit");

    let error = manager
        .start_service_for_decision_async(&decision, "api", HostCallCancellation::default())
        .await
        .expect_err("missing signature evidence should fail before image materialization");

    assert!(
        error.to_string().contains("requires a matching signature"),
        "image admission failure should be visible: {error}"
    );
    assert_eq!(
        backend.image_starts.load(Ordering::SeqCst),
        0,
        "unverified image must not reach sandbox materialization"
    );
}

#[tokio::test]
async fn start_service_for_decision_admits_verified_image_before_materialization() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let image = "registry.example.com/nimbus/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let backend = Arc::new(StubSandboxBackend::new(1));
    let verifier = Arc::new(RecordingImageVerifier::with_evidence(
        TenantImageVerificationEvidence::new()
            .with_signature("https://issuer.example.com", "repo:nimbus/api"),
    ));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([("api".to_owned(), image_service_backend("api", image))]),
        }),
        backend.clone(),
    )
    .with_image_verification_provider_arc(verifier.clone())
    .with_activation_poll_interval(Duration::from_millis(1))
    .with_activation_timeout(Duration::from_secs(1));
    let isolation = TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
    let decision = isolation
        .admit_decision(
            TenantIsolationPolicyInput::new(WorkloadAttributes::service("api"))
                .with_services(TenantServiceGrantPolicyDecision::new(["api"]))
                .with_image(
                    nimbus_tenant::TenantImagePolicyDecision::digest_pinned(image)
                        .require_signature("https://issuer.example.com", "repo:nimbus/api"),
                ),
        )
        .expect("image policy decision should admit");

    manager
        .start_service_for_decision_async(&decision, "api", HostCallCancellation::default())
        .await
        .expect("verified image should start")
        .expect("handle should be returned");

    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);
    assert_eq!(verifier.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        verifier
            .references
            .lock()
            .expect("image verifier references should not be poisoned")
            .as_slice(),
        [image]
    );
}

#[tokio::test]
async fn reload_service_egress_for_decision_updates_active_backend_policy() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "db".to_owned(),
                image_service_backend("db", "postgres:16"),
            )]),
        }),
        backend.clone(),
    )
    .with_activation_poll_interval(Duration::from_millis(1))
    .with_activation_timeout(Duration::from_secs(1));
    let isolation = TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
    let start_decision = manager
        .service_lifecycle_decision(&isolation, "db")
        .expect("db service activation decision should build");
    let handle = manager
        .start_service_for_decision_async(&start_decision, "db", HostCallCancellation::default())
        .await
        .expect("service should start")
        .expect("handle should exist");
    let egress = EgressPolicy::new([EgressRule::new(
        "stripe",
        nimbus_egress::EgressProtocol::Https,
        "api.stripe.com",
        443,
    )]);
    let reload_decision = isolation
        .admit_decision(
            TenantIsolationPolicyInput::new(WorkloadAttributes::service("db"))
                .with_services(TenantServiceGrantPolicyDecision::new(["db"]))
                .with_network(
                    nimbus_tenant::TenantNetworkPolicyDecision::default()
                        .with_sandbox_egress(egress.clone())
                        .expect("test egress policy should compile"),
                ),
        )
        .expect("reload decision with egress should admit");

    let reloaded = manager
        .reload_service_egress_for_decision_async(&tenant_id, &reload_decision, "db")
        .await
        .expect("egress reload should apply")
        .expect("active handle should remain");

    assert_eq!(reloaded.id, handle.id);
    let reloads = backend
        .egress_reloads
        .lock()
        .expect("backend lock should not be poisoned");
    assert_eq!(reloads.len(), 1);
    assert_eq!(reloads[0].0, handle.id.as_str());
    assert_eq!(reloads[0].1, egress);
}

#[tokio::test]
async fn ensure_service_binding_async_starts_declared_image_service_once() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(2));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "db".to_owned(),
                image_service_backend("db", "postgres:16"),
            )]),
        }),
        backend.clone(),
    )
    .with_activation_poll_interval(Duration::from_millis(1))
    .with_activation_timeout(Duration::from_secs(1));

    let binding = manager
        .ensure_service_binding_async(&tenant_id, "db", HostCallCancellation::default())
        .await
        .expect("image-backed service activation should succeed")
        .expect("db binding should exist");

    assert_eq!(binding.host, "127.0.0.1");
    assert_eq!(binding.port, 15432);
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);
    assert_eq!(backend.build_starts.load(Ordering::SeqCst), 0);

    let second = manager
        .ensure_service_binding_async(&tenant_id, "db", HostCallCancellation::default())
        .await
        .expect("cached service activation should succeed")
        .expect("db binding should still exist");
    assert_eq!(second.port, 15432);
    assert_eq!(
        backend.image_starts.load(Ordering::SeqCst),
        1,
        "existing active handle should prevent duplicate starts"
    );

    let snapshot = manager.snapshot_for_tenant(&tenant_id);
    assert_eq!(
        snapshot
            .get("db")
            .expect("db binding should be in snapshot")
            .port,
        15432
    );
}

#[tokio::test]
async fn resolve_service_binding_uses_cached_snapshot_without_backend_inspect() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "db".to_owned(),
                image_service_backend("db", "postgres:16"),
            )]),
        }),
        backend.clone(),
    )
    .with_activation_poll_interval(Duration::from_millis(1))
    .with_activation_timeout(Duration::from_secs(1));

    manager
        .ensure_service_binding_async(&tenant_id, "db", HostCallCancellation::default())
        .await
        .expect("service activation should succeed")
        .expect("db binding should exist");
    assert!(manager.snapshot_for_tenant(&tenant_id).contains_key("db"));

    let sandbox_id = backend
        .handles
        .lock()
        .expect("backend lock should not be poisoned")
        .keys()
        .next()
        .expect("backend should have a started sandbox")
        .clone();
    backend
        .handles
        .lock()
        .expect("backend lock should not be poisoned")
        .remove(&sandbox_id);
    let inspect_calls_before = backend.inspect_calls.load(Ordering::SeqCst);

    let binding = manager
        .resolve_service_binding(&tenant_id, "db")
        .expect("service binding snapshot should not fail")
        .expect("cached service binding should still project");

    assert_eq!(binding.port, 15432);
    assert_eq!(
        backend.inspect_calls.load(Ordering::SeqCst),
        inspect_calls_before,
        "sync resolve_service_binding must not inspect the backend"
    );
    assert!(
        manager.snapshot_for_tenant(&tenant_id).contains_key("db"),
        "snapshot-only lookups should not mutate cached handles"
    );
}

#[tokio::test]
async fn stop_service_for_context_async_stops_active_handle_and_clears_snapshot() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "db".to_owned(),
                image_service_backend("db", "postgres:16"),
            )]),
        }),
        backend.clone(),
    )
    .with_activation_poll_interval(Duration::from_millis(1))
    .with_activation_timeout(Duration::from_secs(1));
    let isolation = TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");

    manager
        .start_service_for_context_async(&isolation, "db", HostCallCancellation::default())
        .await
        .expect("service should start")
        .expect("active handle should exist");
    let stopped = manager
        .stop_service_for_context_async(&isolation, "db")
        .await
        .expect("service should stop")
        .expect("stopped handle should be returned");

    assert_eq!(stopped.status, SandboxStatus::Stopped);
    assert!(stopped.published_endpoints.is_empty());
    assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 1);
    assert!(
        manager.snapshot_for_tenant(&tenant_id).is_empty(),
        "stopped service should not remain in runtime service snapshots"
    );
}

/// NNC0.6 fail-before baseline for NNCF11. The semantic barrier parks the
/// backend stop after it has begun, without a timing sleep, so a concurrent
/// cache-only lookup can prove whether withdrawal happened before the await.
#[tokio::test]
#[ignore = "NNC0.6 expected red until NNC6.6 fences service resolution before backend stop"]
async fn nnc0_6_service_binding_is_withdrawn_before_backend_stop_waits() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let stop_barrier = Arc::new(StopBarrier::default());
    let backend = Arc::new(StubSandboxBackend::new(1).with_stop_barrier(Arc::clone(&stop_barrier)));
    let manager = Arc::new(
        ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    image_service_backend("db", "postgres:16"),
                )]),
            }),
            backend,
        )
        .with_activation_poll_interval(Duration::from_millis(1))
        .with_activation_timeout(Duration::from_secs(1)),
    );
    let isolation = TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");

    manager
        .start_service_for_context_async(&isolation, "db", HostCallCancellation::default())
        .await
        .expect("service should start")
        .expect("active handle should exist");
    assert!(
        manager
            .resolve_service_binding(&tenant_id, "db")
            .expect("pre-stop lookup should resolve")
            .is_some(),
        "precondition: the ready service must be routable before stop begins"
    );

    let stop_manager = Arc::clone(&manager);
    let stop_task = tokio::spawn(async move {
        stop_manager
            .stop_service_for_context_async(&isolation, "db")
            .await
    });
    tokio::time::timeout(Duration::from_secs(1), stop_barrier.entered.acquire())
        .await
        .expect("backend stop should reach the semantic barrier")
        .expect("stop barrier should remain open")
        .forget();

    let binding_during_stop = manager
        .resolve_service_binding(&tenant_id, "db")
        .expect("concurrent cache lookup should not error");

    // Always release and join before asserting the expected-red invariant so a
    // failure cannot strand a task in the runtime during test teardown.
    stop_barrier.release.add_permits(1);
    let stopped = tokio::time::timeout(Duration::from_secs(1), stop_task)
        .await
        .expect("backend stop should complete after release")
        .expect("stop task should join")
        .expect("service stop should succeed")
        .expect("stopped handle should be returned");
    assert_eq!(stopped.status, SandboxStatus::Stopped);
    assert!(
        manager.snapshot_for_tenant(&tenant_id).is_empty(),
        "completed stop should clear the cached service snapshot"
    );

    assert!(
        binding_during_stop.is_none(),
        "NNCF11: service resolution must be withdrawn before awaiting backend stop; \
         the cache still returned a routable binding after stop had begun"
    );
}

#[tokio::test]
async fn stop_service_for_decision_async_requires_exact_service_grant() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "db".to_owned(),
                image_service_backend("db", "postgres:16"),
            )]),
        }),
        backend.clone(),
    )
    .with_activation_poll_interval(Duration::from_millis(1))
    .with_activation_timeout(Duration::from_secs(1));
    let isolation = TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
    manager
        .start_service_for_context_async(&isolation, "db", HostCallCancellation::default())
        .await
        .expect("service should start")
        .expect("active handle should exist");
    let denied_decision = isolation
        .admit_decision(
            TenantIsolationPolicyInput::new(WorkloadAttributes::service("db")).with_image(
                nimbus_tenant::TenantImagePolicyDecision::default().allow_local_build(),
            ),
        )
        .expect("decision without service grant should still build");

    let error = manager
        .stop_service_for_decision_async(&denied_decision, "db")
        .await
        .expect_err("stop must require an exact service grant");

    assert!(
        error.to_string().contains("db"),
        "service grant error should name the denied service: {error}"
    );
    assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 0);
    assert!(
        !manager.snapshot_for_tenant(&tenant_id).is_empty(),
        "denied stop must leave the active service snapshot intact"
    );
}

#[tokio::test]
async fn retained_stopping_service_blocks_replacement_and_explicit_stop_converges_once() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(usize::MAX));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "db".to_owned(),
                image_service_backend("db", "postgres:16"),
            )]),
        }),
        backend.clone(),
    )
    .with_activation_poll_interval(Duration::from_millis(1))
    .with_activation_timeout(Duration::from_millis(10));
    let isolation = TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
    let key = super::super::types::TenantServiceKey::new(&tenant_id, "db");
    let handle = backend.sandbox_handle(&tenant_id, "db", SandboxStatus::Stopping);
    backend
        .handles
        .lock()
        .expect("backend handle map should not be poisoned")
        .insert(handle.id.as_str().to_owned(), handle.clone());
    manager
        .state
        .lock()
        .expect("manager state should not be poisoned")
        .handles
        .insert(key, handle.clone());
    let retained = retained_stopping_inspection(handle);
    backend.report_inspection(retained.clone());

    let observed = manager
        .inspect_service_lifecycle_for_context_async(&isolation, "db")
        .await
        .expect("typed service inspection should succeed")
        .expect("retained service should remain visible");
    assert_eq!(observed, retained);

    let start_error = manager
        .start_service_for_context_async(&isolation, "db", HostCallCancellation::default())
        .await
        .expect_err("retained cleanup must block a replacement rather than activate it");
    assert!(
        start_error.to_string().contains("did not become ready"),
        "the retained service should remain fenced on its existing lifecycle: {start_error}"
    );
    assert_eq!(
        backend.image_starts.load(Ordering::SeqCst),
        0,
        "lazy activation must not create a replacement while cleanup is retained"
    );

    let stopped = manager
        .stop_service_for_context_async(&isolation, "db")
        .await
        .expect("explicit stop should converge retained cleanup")
        .expect("stopped handle should remain observable");
    assert_eq!(stopped.status, SandboxStatus::Stopped);
    assert!(stopped.published_endpoints.is_empty());
    assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 1);

    assert!(
        manager
            .stop_service_for_context_async(&isolation, "db")
            .await
            .expect("stop replay should be idempotent")
            .is_none()
    );
    assert_eq!(
        backend.stop_calls.load(Ordering::SeqCst),
        1,
        "cleanup convergence must execute at most once after final removal"
    );
}

#[tokio::test]
async fn service_inspection_rejects_crossed_identity_before_cache_or_lifecycle_effects() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let isolation = TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");

    for case in ["sandbox-id", "tenant", "service-name", "backend"] {
        let backend = Arc::new(StubSandboxBackend::new(usize::MAX));
        let manager = ServiceManager::new(
            Arc::new(StubServiceDefinitionCatalog {
                launches: BTreeMap::new(),
            }),
            backend.clone(),
        );
        let key = super::super::types::TenantServiceKey::new(&tenant_id, "db");
        let expected = backend.sandbox_handle(&tenant_id, "db", SandboxStatus::Ready);
        backend
            .handles
            .lock()
            .expect("backend handle map should not be poisoned")
            .insert(expected.id.as_str().to_owned(), expected.clone());
        manager
            .state
            .lock()
            .expect("manager state should not be poisoned")
            .handles
            .insert(key.clone(), expected.clone());

        let mut crossed = expected.clone();
        match case {
            "sandbox-id" => crossed.id = SandboxId::new("crossed-service-sandbox"),
            "tenant" => {
                crossed.tenant_id =
                    TenantId::new("crossed-tenant").expect("crossed tenant should be valid");
            }
            "service-name" => crossed.name = "crossed-service".to_owned(),
            "backend" => crossed.backend = SandboxBackendKind::Container,
            _ => unreachable!("the identity table is exhaustive"),
        }
        backend.report_inspection_for(&expected.id, SandboxInspection::provider_reported(crossed));

        let error = manager
            .inspect_service_lifecycle_for_context_async(&isolation, "db")
            .await
            .expect_err("crossed service inspection identity must fail closed");

        assert!(
            error.to_string().contains("crossed inspection identity"),
            "{case}: rejection must name the backend contract failure: {error}"
        );
        assert_eq!(
            manager
                .state
                .lock()
                .expect("manager state should not be poisoned")
                .handles
                .get(&key),
            Some(&expected),
            "{case}: rejected evidence must not replace or evict the cached handle"
        );
        assert_eq!(backend.image_starts.load(Ordering::SeqCst), 0, "{case}");
        assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 0, "{case}");
    }
}

#[tokio::test]
async fn restart_service_for_context_async_stops_then_starts_service() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "db".to_owned(),
                image_service_backend("db", "postgres:16"),
            )]),
        }),
        backend.clone(),
    )
    .with_activation_poll_interval(Duration::from_millis(1))
    .with_activation_timeout(Duration::from_secs(1));
    let isolation = TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");

    manager
        .start_service_for_context_async(&isolation, "db", HostCallCancellation::default())
        .await
        .expect("initial service start should succeed")
        .expect("initial handle should exist");
    let restarted = manager
        .restart_service_for_context_async(&isolation, "db", HostCallCancellation::default())
        .await
        .expect("restart should succeed")
        .expect("restarted handle should exist");

    assert_eq!(restarted.status, SandboxStatus::Ready);
    assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        backend.image_starts.load(Ordering::SeqCst),
        2,
        "restart should materialize a fresh sandbox-backed service"
    );
}

#[tokio::test]
async fn restart_service_for_decision_async_requires_exact_service_grant_before_stop() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "db".to_owned(),
                image_service_backend("db", "postgres:16"),
            )]),
        }),
        backend.clone(),
    )
    .with_activation_poll_interval(Duration::from_millis(1))
    .with_activation_timeout(Duration::from_secs(1));
    let isolation = TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");
    manager
        .start_service_for_context_async(&isolation, "db", HostCallCancellation::default())
        .await
        .expect("service should start")
        .expect("active handle should exist");
    let denied_decision = isolation
        .admit_decision(
            TenantIsolationPolicyInput::new(WorkloadAttributes::service("db")).with_image(
                nimbus_tenant::TenantImagePolicyDecision::default().allow_local_build(),
            ),
        )
        .expect("decision without service grant should still build");

    manager
        .restart_service_for_decision_async(&denied_decision, "db", HostCallCancellation::default())
        .await
        .expect_err("restart must require an exact service grant before stopping");

    assert_eq!(
        backend.stop_calls.load(Ordering::SeqCst),
        0,
        "denied restart must not stop the active sandbox first"
    );
    assert_eq!(
        backend.image_starts.load(Ordering::SeqCst),
        1,
        "denied restart must not materialize a replacement sandbox"
    );
}

#[tokio::test]
async fn ensure_service_binding_async_rejects_backend_handle_for_wrong_tenant() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1).with_handle_tenant_override(
        TenantId::new("tenant-b").expect("tenant id should be valid"),
    ));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "db".to_owned(),
                image_service_backend("db", "postgres:16"),
            )]),
        }),
        backend.clone(),
    )
    .with_activation_poll_interval(Duration::from_millis(1))
    .with_activation_timeout(Duration::from_secs(1));

    let error = manager
        .ensure_service_binding_async(&tenant_id, "db", HostCallCancellation::default())
        .await
        .expect_err("backend handle from another tenant should be rejected");

    assert!(
        error
            .to_string()
            .contains("backend returned handle for tenant tenant-b"),
        "error should name the backend tenant mismatch: {error}"
    );
    assert_eq!(
        backend.stop_calls.load(Ordering::SeqCst),
        1,
        "rejecting the mismatched handle must stop the untracked sandbox it started"
    );
    assert!(
        backend
            .handles
            .lock()
            .expect("backend lock should not be poisoned")
            .is_empty(),
        "cleanup should remove the orphaned started handle from the backend"
    );
}

#[tokio::test]
async fn ensure_service_binding_async_rejects_backend_handle_for_wrong_service() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1).with_handle_name_override("not-db"));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "db".to_owned(),
                image_service_backend("db", "postgres:16"),
            )]),
        }),
        backend.clone(),
    )
    .with_activation_poll_interval(Duration::from_millis(1))
    .with_activation_timeout(Duration::from_secs(1));

    let error = manager
        .ensure_service_binding_async(&tenant_id, "db", HostCallCancellation::default())
        .await
        .expect_err("backend handle for another service should be rejected");

    assert!(
        error
            .to_string()
            .contains("backend returned handle for service not-db"),
        "error should name the backend service mismatch: {error}"
    );
    assert_eq!(
        backend.stop_calls.load(Ordering::SeqCst),
        1,
        "rejecting the mismatched handle must stop the untracked sandbox it started"
    );
    assert!(
        backend
            .handles
            .lock()
            .expect("backend lock should not be poisoned")
            .is_empty(),
        "cleanup should remove the orphaned started handle from the backend"
    );
}

#[tokio::test]
async fn ensure_service_binding_async_uses_build_launch_for_build_backed_service() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "api".to_owned(),
                build_service_backend("api", "nimbus-api", "/workspace/Dockerfile", "/workspace"),
            )]),
        }),
        backend.clone(),
    )
    .with_local_build_admission(LocalBuildAdmission::Allowed)
    .with_activation_poll_interval(Duration::from_millis(1))
    .with_activation_timeout(Duration::from_secs(1));

    let binding = manager
        .ensure_service_binding_async(&tenant_id, "api", HostCallCancellation::default())
        .await
        .expect("build-backed service activation should succeed")
        .expect("api binding should exist");

    assert_eq!(binding.port, 15432);
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 0);
    assert_eq!(backend.build_starts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn start_service_for_decision_rejects_build_backed_service_under_production_build_admission()
{
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    // No `.with_local_build_admission(...)` call: the manager defaults to the
    // fail-closed production posture (`LocalBuildAdmission::Denied`).
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "api".to_owned(),
                build_service_backend("api", "nimbus-api", "/workspace/Dockerfile", "/workspace"),
            )]),
        }),
        backend.clone(),
    )
    .with_activation_poll_interval(Duration::from_millis(1))
    .with_activation_timeout(Duration::from_secs(1));
    let isolation = TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");

    let error = manager
        .start_service_for_context_async(&isolation, "api", HostCallCancellation::default())
        .await
        .expect_err("production build admission must reject a local-build sandbox root");

    assert!(
        error.to_string().contains("permission denied"),
        "build rejection should map to permission denial: {error}"
    );
    assert!(
        error.to_string().contains("local build"),
        "error should name the rejected local build admission boundary: {error}"
    );
    assert!(
        error.to_string().contains("nimbus-api"),
        "error should name the rejected local build image: {error}"
    );
    assert_eq!(
        backend.build_starts.load(Ordering::SeqCst),
        0,
        "build root must be rejected at the manager admission boundary before any backend start"
    );
    assert_eq!(
        backend.image_starts.load(Ordering::SeqCst),
        0,
        "a rejected build must not reach any sandbox materialization path"
    );
}

#[tokio::test]
async fn start_service_for_decision_admits_reference_image_under_production_build_admission() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    // Default (Denied) build admission: the fail-closed posture must still admit
    // a plain reference image so it does not over-block non-build roots.
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "db".to_owned(),
                image_service_backend("db", "postgres:16"),
            )]),
        }),
        backend.clone(),
    )
    .with_activation_poll_interval(Duration::from_millis(1))
    .with_activation_timeout(Duration::from_secs(1));
    let isolation = TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");

    let handle = manager
        .start_service_for_context_async(&isolation, "db", HostCallCancellation::default())
        .await
        .expect("reference-image service should start under the fail-closed default")
        .expect("ready handle should be returned");

    assert_eq!(handle.status, SandboxStatus::Ready);
    assert_eq!(
        backend.image_starts.load(Ordering::SeqCst),
        1,
        "reference image must materialize exactly once under the production default"
    );
    assert_eq!(
        backend.build_starts.load(Ordering::SeqCst),
        0,
        "a reference image must never take the build start path"
    );
}

#[tokio::test]
async fn start_service_for_decision_admits_build_backed_service_under_dev_build_admission() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    // Operator opt-in to local development: the manager admits local builds.
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "api".to_owned(),
                build_service_backend("api", "nimbus-api", "/workspace/Dockerfile", "/workspace"),
            )]),
        }),
        backend.clone(),
    )
    .with_local_build_admission(LocalBuildAdmission::Allowed)
    .with_activation_poll_interval(Duration::from_millis(1))
    .with_activation_timeout(Duration::from_secs(1));
    let isolation = TenantIsolationContext::system(tenant_id.clone(), "runtime_service_registry");

    let handle = manager
        .start_service_for_context_async(&isolation, "api", HostCallCancellation::default())
        .await
        .expect("dev build admission should admit a local-build sandbox root")
        .expect("ready handle should be returned");

    assert_eq!(handle.status, SandboxStatus::Ready);
    assert_eq!(
        backend.build_starts.load(Ordering::SeqCst),
        1,
        "the local-build root must materialize through the build start path"
    );
    assert_eq!(
        backend.image_starts.load(Ordering::SeqCst),
        0,
        "a local-build root must never take the reference-image start path"
    );
}

#[tokio::test]
async fn ensure_service_binding_sync_lookup_stays_snapshot_only_for_missing_service() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "db".to_owned(),
                image_service_backend("db", "postgres:16"),
            )]),
        }),
        backend.clone(),
    );

    let binding = manager
        .resolve_service_binding(&tenant_id, "db")
        .expect("sync lookup should not fail");
    assert!(
        binding.is_none(),
        "missing in-memory bindings stay unresolved"
    );
    assert_eq!(
        backend.image_starts.load(Ordering::SeqCst),
        0,
        "sync lookup should not trigger sandbox activation"
    );
}

#[tokio::test]
async fn ensure_service_binding_async_can_be_cancelled_while_waiting_for_readiness() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(usize::MAX));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "db".to_owned(),
                image_service_backend("db", "postgres:16"),
            )]),
        }),
        backend.clone(),
    )
    .with_activation_poll_interval(Duration::from_millis(5))
    .with_activation_timeout(Duration::from_secs(1));
    let cancellation = HostCallCancellation::default();
    let cancellation_handle = cancellation.clone();

    let task = tokio::spawn(async move {
        manager
            .ensure_service_binding_async(&tenant_id, "db", cancellation)
            .await
    });

    tokio::time::sleep(Duration::from_millis(20)).await;
    cancellation_handle.cancel();

    let result = task
        .await
        .expect("cancellation task should join")
        .expect_err("cancellation should interrupt activation");
    assert!(matches!(result, Error::Cancelled));
    assert_eq!(
        backend.image_starts.load(Ordering::SeqCst),
        1,
        "activation should still start before the readiness wait is canceled"
    );
}
