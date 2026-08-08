use super::*;

#[test]
fn durable_attachment_compilation_rejects_missing_compiled_plan_before_mutation() {
    let fixture = ContractFixture::new(ContractBackend::Krun, "missing-compiled-plan");
    let mut config = fixture.reserve_and_adopt();
    config.network_plan = None;
    let adapter = fixture.host_adapter(ContractBackend::Krun, &config, &[], &[]);
    let association = fixture
        .allocator
        .inspect_attachment_reservation(&fixture.tenant_id, &config.attachment_id, &fixture.claim)
        .expect("exact allocator association should inspect")
        .association()
        .expect("adopted fixture should retain its association")
        .clone();

    let error = state::OciAttachmentDurableState::compile(
        Some(&fixture.attachments),
        &adapter.context,
        association,
    )
    .err()
    .expect("missing compiled plan must not fall back to sandbox-derived identity");
    assert!(
        error
            .to_string()
            .contains("lacks its exact compiled network plan"),
        "{error}"
    );
    assert!(
        fixture
            .attachments
            .get(&fixture.tenant_id, &config.attachment_id)
            .expect("durable attachment authority should remain readable")
            .is_none(),
        "failed compilation must not reserve durable attachment authority"
    );
}
