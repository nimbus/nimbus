use super::*;

// Row 12: stale provenance must fail before the first effect.
pub(super) fn stale_provenance_fails_before_effects(backend: ContractBackend) {
    let fixture = ContractFixture::new(backend, "stale");
    let config = fixture.reserve_and_adopt();
    let mut stale_generation = config.clone();
    stale_generation.reservation_claim =
        reservation_claim(&format!("{}-replacement", backend.label()));
    let before = fixture.allocator.operations();
    let error = fixture
        .host_adapter(backend, &stale_generation, &[], &[])
        .attach(
            &fixture.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
            |_| Ok(()),
        )
        .expect_err("stale generation must fail");
    assert!(
        error.to_string().contains("stale")
            || error.to_string().contains("does not match")
            || error.to_string().contains("diverge"),
        "stale generation diagnostic must identify the fence: {error}"
    );
    assert_eq!(fixture.allocator.operations(), before);
    assert!(!fixture.layout.netns_path.exists());

    let foreign_root = fixture
        .layout
        .workload_state_root
        .join("foreign-workload-root");
    let foreign_tenant =
        TenantId::new(format!("{}-foreign", fixture.tenant_id)).expect("tenant should validate");
    let root_adapter = backend.adapter(OciAttachmentInput {
        workload_state_root: &foreign_root,
        tenant_id: &foreign_tenant,
        sandbox_id: &fixture.sandbox_id,
        display_name: "stale provenance",
        hostname: "stale-provenance",
        bindings: &[],
        leases: &[],
        auxiliary_listener: None,
        layout: &fixture.layout,
        config: &config,
        launch_claim: Some(&fixture.claim),
    });
    root_adapter
        .attach(
            &fixture.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
            |_| Ok(()),
        )
        .expect_err("foreign tenant/root provenance must fail");
    assert!(
        !foreign_root.exists(),
        "foreign workload provenance must fail before filesystem effects"
    );

    for launch_claim in [
        None,
        Some(reservation_claim(&format!(
            "{}-foreign-launch-claim",
            backend.label()
        ))),
    ] {
        let row = if launch_claim.is_some() {
            "stale-launch-claim"
        } else {
            "missing-launch-claim"
        };
        let fixture = ContractFixture::new(backend, row);
        let config = fixture.reserve_and_adopt();
        let reserved = fixture.reserve_published_binding();
        let host = ContractHostEffects::default();
        let mut observer = ContractPhaseObserver::recording();
        let adapter = backend.adapter(OciAttachmentInput {
            workload_state_root: &fixture.layout.workload_state_root,
            tenant_id: &fixture.tenant_id,
            sandbox_id: &fixture.sandbox_id,
            display_name: "launch claim provenance",
            hostname: "launch-claim-provenance",
            bindings: &reserved.published_bindings,
            leases: &reserved.published_leases,
            auxiliary_listener: None,
            layout: &fixture.layout,
            config: &config,
            launch_claim: launch_claim.as_ref(),
        });
        let before = fixture.allocator.operations();

        let error = adapter
            .attach_with(
                &fixture.lifecycle(),
                AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
                &host,
                &mut observer,
                |_| Ok(()),
            )
            .expect_err("missing or stale launch claim must fail before effects");
        let rendered = error.to_string();
        assert!(
            rendered.contains("launch reservation claim")
                && (rendered.contains("missing") || rendered.contains("does not match")),
            "launch-claim diagnostic must identify the exact failed fence: {rendered}"
        );
        assert_eq!(
            observer.phases,
            [
                AttachmentAttachPhase::GenerationAuthenticated,
                AttachmentAttachPhase::LeasesAuthenticated,
            ],
            "launch-claim authority must fail at the final pre-effect checkpoint"
        );
        assert!(host.operations().is_empty());
        let after = fixture.allocator.operations();
        assert_eq!(&after[..before.len()], before.as_slice());
        assert!(
            matches!(
                after.get(before.len()),
                Some(SegmentAllocatorOperation::InspectAttachment(..))
            ) && after.len() == before.len() + 1,
            "claim authentication may inspect but must not mutate attachment authority: {after:?}"
        );
        assert!(!fixture.layout.netns_path.exists());
    }

    // A request from another sandbox can share the same launch coordinator,
    // so coordinator equality alone must not authorize this workload's PEP.
    let fixture = ContractFixture::new(backend, "foreign-fresh-pep");
    let config = fixture.reserve_and_adopt();
    let gateway = bridge_gateway_addr(&config).expect("contract bridge gateway should resolve");
    let foreign_sandbox = SandboxId::new(format!("{}-foreign-pep", fixture.sandbox_id));
    let reserved =
        fixture.reserve_auxiliary_listener_for(&foreign_sandbox, std::net::IpAddr::V4(gateway));
    let auxiliary = reserved
        .internal_listener
        .as_ref()
        .expect("contract PEP listener should be present");
    let host_string = gateway.to_string();
    let adapter = backend.adapter(OciAttachmentInput {
        workload_state_root: &fixture.layout.workload_state_root,
        tenant_id: &fixture.tenant_id,
        sandbox_id: &fixture.sandbox_id,
        display_name: "foreign fresh PEP",
        hostname: "foreign-fresh-pep",
        bindings: &[],
        leases: &[],
        auxiliary_listener: Some(OciAttachmentAuxiliaryListener::egress_pep(
            &auxiliary.lease,
            &host_string,
            auxiliary.port,
        )),
        layout: &fixture.layout,
        config: &config,
        launch_claim: Some(&fixture.claim),
    });
    let host = ContractHostEffects::default();
    let mut observer = ContractPhaseObserver::recording();
    let error = adapter
        .attach_with(
            &fixture.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
            &host,
            &mut observer,
            |_| Ok(()),
        )
        .expect_err("foreign same-claim PEP must fail before attachment effects");
    assert!(
        error.to_string().contains("rejected port lease"),
        "fresh PEP rejection must identify exact listener authority: {error}"
    );
    assert_eq!(
        observer.phases,
        [
            AttachmentAttachPhase::GenerationAuthenticated,
            AttachmentAttachPhase::LeasesAuthenticated,
        ]
    );
    assert!(host.operations().is_empty());
    assert!(!fixture.layout.netns_path.exists());

    // Restart evidence must bind the exact persisted assignment, not merely a
    // confirmed-stop receipt from the right provider.
    let fixture = ContractFixture::new(backend, "foreign-restart-pep");
    let config = fixture.reserve_and_adopt();
    let gateway = bridge_gateway_addr(&config).expect("contract bridge gateway should resolve");
    let foreign_sandbox = SandboxId::new(format!("{}-foreign-pep", fixture.sandbox_id));
    let reserved =
        fixture.reserve_auxiliary_listener_for(&foreign_sandbox, std::net::IpAddr::V4(gateway));
    let auxiliary = reserved
        .internal_listener
        .as_ref()
        .expect("contract PEP listener should be present");
    let bind_addr = std::net::SocketAddr::from((gateway, auxiliary.port));
    let authority = fixture
        .ports
        .authority()
        .expect("contract port authority should open");
    let claims = claim_bind_attempts(
        authority,
        std::slice::from_ref(&auxiliary.lease),
        OciPortProvider::EgressPep,
        Some(&fixture.claim),
    )
    .expect("foreign PEP provider claim should be established");
    adopt_claimed_and_activate(
        authority,
        &auxiliary.lease,
        Some(&fixture.claim),
        &claims[0],
        bind_addr,
        OciPortProvider::EgressPep,
    )
    .expect("foreign PEP should record its exact active binding");
    withdraw(authority, &auxiliary.lease).expect("foreign PEP should begin withdrawal");
    let expected_binding =
        provider_binding(&auxiliary.lease, bind_addr, OciPortProvider::EgressPep)
            .expect("foreign PEP binding should render");
    prepare_rebind_after_confirmed_stop(authority, &auxiliary.lease, &expected_binding)
        .expect("foreign PEP should retain confirmed-stop authority");
    let host_string = gateway.to_string();
    let adapter = backend.adapter(OciAttachmentInput {
        workload_state_root: &fixture.layout.workload_state_root,
        tenant_id: &fixture.tenant_id,
        sandbox_id: &fixture.sandbox_id,
        display_name: "foreign restart PEP",
        hostname: "foreign-restart-pep",
        bindings: &[],
        leases: &[],
        auxiliary_listener: Some(OciAttachmentAuxiliaryListener::egress_pep(
            &auxiliary.lease,
            &host_string,
            auxiliary.port,
        )),
        layout: &fixture.layout,
        config: &config,
        launch_claim: None,
    });
    let host = ContractHostEffects::default();
    let mut observer = ContractPhaseObserver::recording();
    let error = adapter
        .attach_with(
            &fixture.lifecycle(),
            AttachmentAttachAuthority::RestartRetained,
            &host,
            &mut observer,
            |_| Ok(()),
        )
        .expect_err("foreign retained PEP must fail before restart effects");
    assert!(
        error.to_string().contains("rejected port lease"),
        "restart PEP rejection must identify exact listener authority: {error}"
    );
    assert_eq!(
        observer.phases,
        [
            AttachmentAttachPhase::GenerationAuthenticated,
            AttachmentAttachPhase::LeasesAuthenticated,
        ]
    );
    assert!(host.operations().is_empty());
    assert!(!fixture.layout.netns_path.exists());
}
