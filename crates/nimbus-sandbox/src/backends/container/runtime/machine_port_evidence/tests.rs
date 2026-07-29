use super::*;
use crate::backend::SandboxBackendKind;
use crate::backends::container::ContainerSandboxBackendConfig;
use crate::spec::{
    SandboxOwnerSpec, SandboxProcessSpec, SandboxRootSpec, SandboxRootfsSpec, SandboxSpec,
};
use std::net::TcpListener;
use std::path::PathBuf;

fn fixture() -> (
    tempfile::TempDir,
    PathBuf,
    PathBuf,
    TenantId,
    SandboxId,
    OciMachinePortForwarderConfig,
    Vec<SandboxPortBinding>,
) {
    let temp = tempfile::tempdir().expect("temporary root should exist");
    let root = temp.path().join("state");
    let state_dir = root.join("tenant/sandbox/container");
    let tenant_id = TenantId::new("tenant-evidence").expect("tenant should validate");
    let sandbox_id = SandboxId::new("machine-api:test-plan-evidence");
    let forwarder = OciMachinePortForwarderConfig::for_provider_instance(
        "127.0.0.1",
        80,
        "/services/forwarder",
        "parent-provider-evidence",
        NetworkResourceGeneration::new(17),
    )
    .expect("forwarder fixture should validate");
    let bindings = vec![
        SandboxPortBinding::tcp("http", 18_080, 8_080),
        SandboxPortBinding::tcp("metrics", 19_090, 9_090),
    ];
    (
        temp, root, state_dir, tenant_id, sandbox_id, forwarder, bindings,
    )
}

fn receipts(
    outcome: MachinePortForwardOutcome,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    forwarder: &OciMachinePortForwarderConfig,
    bindings: &[SandboxPortBinding],
) -> Vec<MachinePortForwardReceipt> {
    bindings
        .iter()
        .map(|binding| MachinePortForwardReceipt {
            outcome,
            tenant_id: tenant_id.clone(),
            sandbox_id: sandbox_id.clone(),
            binding: binding.clone(),
            provider_instance: forwarder.provider_instance().clone(),
            provider_generation: forwarder.provider_generation(),
        })
        .collect()
}

#[test]
fn complete_observed_batches_publish_atomically_and_reload_in_canonical_order() {
    let (_temp, root, state_dir, tenant_id, sandbox_id, forwarder, bindings) = fixture();
    let expectation =
        MachinePortEvidenceExpectation::new(&tenant_id, &sandbox_id, &bindings, &forwarder);
    let exposed = receipts(
        MachinePortForwardOutcome::Exposed,
        &tenant_id,
        &sandbox_id,
        &forwarder,
        &bindings,
    );
    expectation
        .validate(MachinePortEvidencePhase::Exposed, &exposed)
        .expect("complete exposure should validate");
    publish_record(
        &root,
        &state_dir,
        record(
            MachinePortEvidencePhase::Exposed,
            expectation,
            exposed.clone(),
        ),
    )
    .expect("complete exposure should publish");

    let reloaded = read_record(&state_dir).expect("evidence should reload");
    assert_eq!(reloaded.phase, MachinePortEvidencePhase::Exposed);
    assert_eq!(reloaded.receipts, exposed);

    let absent = receipts(
        MachinePortForwardOutcome::ExactAlreadyAbsent,
        &tenant_id,
        &sandbox_id,
        &forwarder,
        &bindings,
    );
    let expectation =
        MachinePortEvidenceExpectation::new(&tenant_id, &sandbox_id, &bindings, &forwarder);
    publish_record(
        &root,
        &state_dir,
        record(
            MachinePortEvidencePhase::Absent,
            expectation,
            absent.clone(),
        ),
    )
    .expect("complete absence should replace exposure atomically");
    let reloaded = read_record(&state_dir).expect("absence evidence should reload");
    assert_eq!(reloaded.phase, MachinePortEvidencePhase::Absent);
    assert_eq!(reloaded.receipts, absent);
}

#[test]
fn zero_binding_batches_are_complete_exact_evidence_for_both_phases() {
    let (_temp, root, state_dir, tenant_id, sandbox_id, forwarder, _bindings) = fixture();
    let bindings = Vec::new();

    for phase in [
        MachinePortEvidencePhase::Exposed,
        MachinePortEvidencePhase::Absent,
    ] {
        let expectation =
            MachinePortEvidenceExpectation::new(&tenant_id, &sandbox_id, &bindings, &forwarder);
        expectation
            .validate(phase, &[])
            .expect("an empty receipt batch exactly covers an empty binding set");
        publish_record(&root, &state_dir, record(phase, expectation, Vec::new()))
            .expect("empty exact evidence should publish atomically");

        let reloaded = read_record(&state_dir).expect("empty exact evidence should reload");
        assert_eq!(reloaded.phase, phase);
        assert!(reloaded.receipts.is_empty());
    }
}

#[test]
fn partial_stale_crossed_and_wrong_outcome_batches_never_reach_the_commit_point() {
    let (_temp, root, state_dir, tenant_id, sandbox_id, forwarder, bindings) = fixture();
    let exact = receipts(
        MachinePortForwardOutcome::Exposed,
        &tenant_id,
        &sandbox_id,
        &forwarder,
        &bindings,
    );
    let cases = [
        exact[..1].to_vec(),
        {
            let mut stale = exact.clone();
            stale[0].provider_generation = NetworkResourceGeneration::new(16);
            stale
        },
        {
            let mut crossed = exact.clone();
            crossed.swap(0, 1);
            crossed
        },
        {
            let mut absent = exact.clone();
            absent[0].outcome = MachinePortForwardOutcome::Withdrawn;
            absent
        },
    ];
    for candidate in cases {
        let expectation =
            MachinePortEvidenceExpectation::new(&tenant_id, &sandbox_id, &bindings, &forwarder);
        assert!(
            expectation
                .validate(MachinePortEvidencePhase::Exposed, &candidate)
                .is_err(),
            "invalid batch must be rejected before publication"
        );
        assert!(
            !state_dir.join(MACHINE_PORT_EVIDENCE_FILE).exists(),
            "validation failure must leave the canonical observation absent"
        );
    }
    assert!(
        !root.exists(),
        "pre-publication rejection performs no filesystem effects"
    );
}

#[test]
fn crash_stage_is_not_evidence_and_next_complete_write_reconciles_it() {
    let (_temp, root, state_dir, tenant_id, sandbox_id, forwarder, bindings) = fixture();
    let exposed = receipts(
        MachinePortForwardOutcome::Exposed,
        &tenant_id,
        &sandbox_id,
        &forwarder,
        &bindings,
    );
    let expectation =
        MachinePortEvidenceExpectation::new(&tenant_id, &sandbox_id, &bindings, &forwarder);
    publish_record(
        &root,
        &state_dir,
        record(
            MachinePortEvidencePhase::Exposed,
            expectation,
            exposed.clone(),
        ),
    )
    .expect("baseline exposure should publish");

    fs::write(
        state_dir.join(MACHINE_PORT_EVIDENCE_STAGE_FILE),
        br#"{"phase":"absent","receipts":[]}"#,
    )
    .expect("crash-cut stage should write");
    assert_eq!(
        read_record(&state_dir)
            .expect("reader must ignore an uncommitted stage")
            .receipts,
        exposed
    );

    let absent = receipts(
        MachinePortForwardOutcome::Withdrawn,
        &tenant_id,
        &sandbox_id,
        &forwarder,
        &bindings,
    );
    let expectation =
        MachinePortEvidenceExpectation::new(&tenant_id, &sandbox_id, &bindings, &forwarder);
    publish_record(
        &root,
        &state_dir,
        record(
            MachinePortEvidencePhase::Absent,
            expectation,
            absent.clone(),
        ),
    )
    .expect("fresh owner should remove the stage and publish the complete batch");
    assert!(!state_dir.join(MACHINE_PORT_EVIDENCE_STAGE_FILE).exists());
    assert_eq!(
        read_record(&state_dir)
            .expect("fresh observation should reload")
            .receipts,
        absent
    );
}

#[test]
fn crossed_or_stale_forwarder_is_fenced_before_io_and_cannot_overwrite_evidence() {
    let temp = tempfile::tempdir().expect("temporary root should exist");
    let canonical = OciMachinePortForwarderConfig::for_provider_instance(
        "127.0.0.1",
        9,
        "/services/forwarder",
        "canonical-provider",
        NetworkResourceGeneration::new(17),
    )
    .expect("canonical forwarder should validate");
    let mut config = ContainerSandboxBackendConfig::plan_only(
        temp.path().join("bundles"),
        temp.path().join("state"),
    );
    config.machine_port_forwarder = Some(canonical.clone());
    let backend = ContainerSandboxBackend::new(config);
    let tenant_id = TenantId::new("tenant-authority").expect("tenant should validate");
    let sandbox_id = SandboxId::new("machine-api:authority-plan");
    let binding = SandboxPortBinding::tcp("http", 18_080, 8_080);
    let spec = SandboxSpec::new(
        tenant_id.clone(),
        SandboxOwnerSpec::service("api"),
        SandboxBackendKind::Container,
        SandboxRootSpec::Rootfs(SandboxRootfsSpec::new("/tmp/rootfs")),
        SandboxProcessSpec::new(["sleep", "60"]),
    )
    .with_port_binding(binding.clone());
    backend
        .prepare_plan_only_service_workload_with_id(spec, sandbox_id.clone())
        .expect("fixture manifest should publish");
    let canonical_receipts = receipts(
        MachinePortForwardOutcome::ExactAlreadyAbsent,
        &tenant_id,
        &sandbox_id,
        &canonical,
        std::slice::from_ref(&binding),
    );
    backend
        .persist_absent_machine_port_receipts(
            &tenant_id,
            &sandbox_id,
            std::slice::from_ref(&binding),
            &canonical,
            canonical_receipts,
        )
        .expect("canonical absence should publish");
    let manifest = backend
        .read_manifest(&sandbox_id)
        .expect("manifest lookup should succeed")
        .expect("manifest should remain present");
    let evidence_path = manifest
        .conmon_layout
        .container_state_dir
        .join(MACHINE_PORT_EVIDENCE_FILE);
    let canonical_bytes = fs::read(&evidence_path).expect("canonical evidence should read");

    let listener = TcpListener::bind("127.0.0.1:0").expect("provider tripwire should bind");
    listener
        .set_nonblocking(true)
        .expect("provider tripwire should be nonblocking");
    let port = listener
        .local_addr()
        .expect("tripwire address should inspect")
        .port();
    for (provider, generation) in [
        ("crossed-provider", NetworkResourceGeneration::new(17)),
        ("canonical-provider", NetworkResourceGeneration::new(16)),
    ] {
        let crossed = OciMachinePortForwarderConfig::for_provider_instance(
            "127.0.0.1",
            port,
            "/services/forwarder",
            provider,
            generation,
        )
        .expect("crossed forwarder fixture should validate");
        let error = backend
            .authenticate_machine_port_forwarder(
                &tenant_id,
                &sandbox_id,
                std::slice::from_ref(&binding),
                &crossed,
            )
            .expect_err("crossed or stale authority must fail before provider I/O");
        assert!(
            error.to_string().contains("crossed or stale"),
            "authority rejection should be explicit: {error}"
        );
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "authority preflight must not contact the supplied provider endpoint"
        );

        let crossed_receipts = receipts(
            MachinePortForwardOutcome::Withdrawn,
            &tenant_id,
            &sandbox_id,
            &crossed,
            std::slice::from_ref(&binding),
        );
        backend
            .persist_absent_machine_port_receipts(
                &tenant_id,
                &sandbox_id,
                std::slice::from_ref(&binding),
                &crossed,
                crossed_receipts,
            )
            .expect_err("crossed or stale authority must not overwrite durable evidence");
        assert_eq!(
            fs::read(&evidence_path).expect("canonical evidence should remain readable"),
            canonical_bytes,
            "crossed or stale authority must preserve canonical evidence byte-for-byte"
        );
    }
}

#[test]
fn detached_absence_reopens_exact_identity_including_zero_binding_header() {
    let temp = tempfile::tempdir().expect("temporary root should exist");
    let forwarder = OciMachinePortForwarderConfig::for_provider_instance(
        "127.0.0.1",
        9,
        "/services/forwarder",
        "detached-provider",
        NetworkResourceGeneration::new(23),
    )
    .expect("forwarder fixture should validate");
    let mut config = ContainerSandboxBackendConfig::plan_only(
        temp.path().join("bundles"),
        temp.path().join("state"),
    );
    config.machine_port_forwarder = Some(forwarder.clone());
    let backend = ContainerSandboxBackend::new(config);
    let tenant_id = TenantId::new("tenant-detached").expect("tenant should validate");
    let sandbox_id = SandboxId::new("machine-api:detached-plan");
    let spec = SandboxSpec::new(
        tenant_id.clone(),
        SandboxOwnerSpec::service("worker"),
        SandboxBackendKind::Container,
        SandboxRootSpec::Rootfs(SandboxRootfsSpec::new("/tmp/rootfs")),
        SandboxProcessSpec::new(["sleep", "60"]),
    );
    backend
        .prepare_plan_only_service_workload_with_id(spec, sandbox_id.clone())
        .expect("zero-binding fixture manifest should publish");
    backend
        .persist_absent_machine_port_receipts(&tenant_id, &sandbox_id, &[], &forwarder, Vec::new())
        .expect("zero-binding absence should publish");
    let manifest = backend
        .read_manifest(&sandbox_id)
        .expect("manifest lookup should succeed")
        .expect("manifest should exist before crash-cut simulation");
    fs::remove_file(&manifest.conmon_layout.manifest_path)
        .expect("lost-manifest crash cut should remove only desired state");

    let evidence = backend
        .absent_machine_port_evidence(&sandbox_id)
        .expect("detached evidence lookup should succeed")
        .expect("exact detached absence should be found");
    assert_eq!(evidence.tenant_id, tenant_id);
    assert_eq!(evidence.sandbox_id, sandbox_id);
    assert!(evidence.receipts.is_empty());
    assert!(
        backend
            .absent_machine_port_evidence(&SandboxId::new("machine-api:unrelated-plan"))
            .expect("unrelated lookup should remain well-formed")
            .is_none(),
        "unrelated identities must not inherit another sandbox's absence"
    );
}
