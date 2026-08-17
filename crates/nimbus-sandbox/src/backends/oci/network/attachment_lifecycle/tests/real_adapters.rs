use super::*;

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
            ContractHostOperation::ProviderAttemptPrepared,
            ContractHostOperation::NamespaceCreated,
            ContractHostOperation::ProviderSetup,
            ContractHostOperation::ProviderTeardownPrepared,
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

#[test]
fn container_15_real_adapters_share_the_contract_owner() {
    real_adapters_share_the_contract_owner(ContractBackend::Container);
}

#[test]
fn krun_15_real_adapters_share_the_contract_owner() {
    real_adapters_share_the_contract_owner(ContractBackend::Krun);
}
