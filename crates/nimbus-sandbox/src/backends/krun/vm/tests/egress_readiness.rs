//! Fail-before coverage for PEP dependency withdrawal during live inspection.

use super::support::*;
use std::sync::Arc;

use nimbus_process_harness::PortWindow;

use crate::backends::oci::egress::{
    PepPreAdoptionReleaseAuthority, ensure_egress_proxy_running_with_release_authority,
};
use crate::backends::oci::network::{
    AttachmentAttachAuthority, FixedOciEgressPinProvider, default_network_attachment_id,
};

#[test]
fn krun_inspect_withdraws_ready_projection_when_pep_dependency_is_absent_or_not_ready() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    // One claimed window owns both fixture ports, partitioned so the published
    // endpoint and the PEP can never be handed the same number. The endpoint
    // listener still occupies its port for the whole test; the PEP binds its
    // own port for real, which the window keeps free of other test processes.
    let port_window = PortWindow::claim();
    let endpoint_port = port_window.port(0);
    let endpoint_listener = TcpListener::bind(("127.0.0.1", endpoint_port))
        .expect("published endpoint fixture should bind");
    let pep_port = port_window.port(1);
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.published_port_range = pep_port..=pep_port;
    let pin_provider = Arc::new(FixedOciEgressPinProvider::ready());
    let readiness_provider = Arc::new(FixedReadinessProbeProvider::ready());
    let backend = KrunSandboxBackend::new(config)
        .with_egress_pin_provider(pin_provider.clone())
        .with_readiness_probe_provider(readiness_provider.clone());
    let sandbox_id = SandboxId::new("krun-pep-readiness-withdrawal");
    let spec = sample_spec_for_tenant("krun-pep-readiness-withdrawal", "live-runtime")
        .with_port_bindings([SandboxPortBinding::tcp(
            "published-api",
            endpoint_port,
            8080,
        )]);

    let mut manifest = backend
        .plan_start_with_id(&spec, &sandbox_id, None, None)
        .expect("execute planning should reserve exact network and PEP authority")
        .manifest;
    let launch_claim = manifest
        .require_reserved_claim()
        .expect("execute planning should retain its reservation claim")
        .clone();
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &launch_claim,
        )
        .expect("fixture should adopt the exact network attachment");
    let network_config = manifest
        .require_network_config()
        .expect("execute manifest should retain network config");
    let ports = backend.port_lease_coordinator();
    let hostname = crate::backends::krun::vm::start::hostname_for(&manifest.spec);
    backend
        .attachment_adapter(&manifest, network_config, &hostname)
        .attach_with_test_host(
            &backend.attachment_lifecycle(&ports),
            AttachmentAttachAuthority::FreshLaunch(&launch_claim),
            |_| {
                backend.egress_pin_provider.apply(
                    &manifest.network_layout,
                    manifest
                        .egress_proxy
                        .as_ref()
                        .expect("execute manifest should retain its PEP assignment"),
                )
            },
        )
        .expect("fixture should realize the complete host-managed attachment");
    assert_eq!(
        pin_provider.apply_count(),
        1,
        "complete initial attachment should apply the exact egress pin once"
    );
    ensure_egress_proxy_running_with_release_authority(
        &backend.egress_proxies,
        &manifest.spec.tenant_id,
        &manifest.handle.id,
        manifest.egress_proxy.as_ref(),
        &manifest.spec.egress,
        PepPreAdoptionReleaseAuthority::FreshLaunch(&launch_claim),
    )
    .expect("fixture should start the exact desired egress PEP");
    manifest.launch_authority = KrunLaunchAuthority::ProviderOwned;
    manifest.creator_handoff = KrunCreatorHandoffState::RuntimeObserved {
        receipt: crate::backends::conmon::creator::CreatorAttemptReceipt::for_test(
            "krun-pep-readiness-live-runtime",
        ),
    };
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' '{{\"id\":\"{}\",\"status\":\"running\"}}'",
            manifest.handle.id
        ),
    ]);
    let _ = fs::remove_file(&manifest.conmon_layout.exit_status_file);
    super::super::readiness::synchronize_handle_status(&mut manifest, SandboxStatus::Ready);
    backend
        .write_manifest(&manifest)
        .expect("live Ready fixture should persist");

    let readiness = backend
        .egress_proxies
        .readiness(&manifest.spec.tenant_id, &manifest.handle.id)
        .expect("PEP readiness should inspect")
        .expect("the exact PEP should be registered");
    assert!(
        readiness.is_ready() && readiness.audit_healthy(),
        "precondition: the exact registered PEP must be ready: {readiness:?}"
    );
    backend
        .ensure_complete_host_managed_attachment_readiness_for_test(&manifest)
        .expect("complete evidence should reach the existing VMM-spawn boundary");
    let status: serde_json::Value = serde_json::from_slice(
        &fs::read(&manifest.network_layout.status_path).expect("exact Netavark status should read"),
    )
    .expect("exact Netavark status should decode");
    let assigned_ip = status["assigned_ips"][0]
        .as_str()
        .expect("status should contain one assigned IPv4 address")
        .parse::<std::net::Ipv4Addr>()
        .expect("assigned IPv4 address should parse");
    let initially_ready = backend
        .inspect_sync(&manifest.handle.id)
        .expect("live runtime inspection should succeed")
        .expect("live runtime should remain inspectable");
    assert_eq!(
        initially_ready.handle.status,
        SandboxStatus::Ready,
        "precondition: a live runtime with its PEP must project Ready"
    );
    assert_eq!(
        initially_ready.handle.published_endpoints.len(),
        1,
        "precondition: Ready must visibly publish the desired endpoint"
    );
    let published_endpoint = initially_ready.handle.published_endpoints[0].clone();
    assert_eq!(published_endpoint.name, "published-api");
    assert_eq!(published_endpoint.address.port(), endpoint_port);
    assert_eq!(
        readiness_provider.calls().last().map(|(target, _)| *target),
        Some(ReadinessProbeTarget::Tcp(std::net::SocketAddr::new(
            assigned_ip.into(),
            endpoint_port,
        ))),
        "sandbox execution readiness must inspect the provider-private attachment, not the public ingress endpoint whose withdrawal is ordered before restart",
    );

    let status_bytes =
        fs::read(&manifest.network_layout.status_path).expect("exact Netavark status should read");
    fs::remove_file(&manifest.network_layout.status_path)
        .expect("Netavark status facet should be removable");
    let attachment_withdrawn = backend
        .inspect_sync(&manifest.handle.id)
        .expect("live runtime with lost attachment evidence should inspect")
        .expect("live runtime should remain inspectable");
    assert_eq!(
        attachment_withdrawn.handle.status,
        SandboxStatus::NotReady,
        "losing one required attachment facet must withdraw Ready"
    );
    assert!(
        attachment_withdrawn.handle.published_endpoints.is_empty(),
        "attachment evidence loss must withdraw every endpoint"
    );
    fs::write(&manifest.network_layout.status_path, status_bytes)
        .expect("exact Netavark status facet should restore");
    let attachment_restored = backend
        .inspect_sync(&manifest.handle.id)
        .expect("restored exact attachment evidence should inspect")
        .expect("live runtime should remain inspectable");
    assert_eq!(
        attachment_restored.handle.status,
        SandboxStatus::Ready,
        "restoring exact evidence should permit application readiness to recover"
    );
    assert_eq!(
        attachment_restored.handle.published_endpoints,
        vec![published_endpoint.clone()]
    );

    backend
        .egress_proxies
        .stop_with_assignment(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            manifest.egress_proxy.as_ref(),
        )
        .expect("fixture should stop and retire the exact registered PEP");
    assert!(
        backend
            .egress_proxies
            .readiness(&manifest.spec.tenant_id, &manifest.handle.id)
            .expect("post-stop PEP readiness should inspect")
            .is_none(),
        "precondition: the workload stays live after its exact PEP is absent"
    );
    let manifest_before_missing_pep_inspection =
        fs::read(&manifest.conmon_layout.manifest_path).expect("manifest bytes should read");
    let attachment_before_missing_pep_inspection =
        fs::read(&manifest.network_layout.status_path).expect("attachment status should read");
    let pin_effects_before_missing_pep_inspection = pin_provider.apply_count();
    assert!(matches!(
        backend
            .inspect_provision_network_attachment(
                &manifest.handle.id,
                &manifest.execution_attempt_id
            )
            .expect("the live attachment with a missing PEP should inspect without replay"),
        crate::SandboxProvisionPhaseObservation::InProgress { .. }
    ));
    let launch_error = backend
        .require_authenticated_egress_readiness(&manifest)
        .expect_err("the exact pre-spawn gate must reject the missing PEP dependency");
    assert!(
        launch_error.to_string().contains("denied launch")
            && launch_error
                .to_string()
                .contains("egress PEP dependency is not ready"),
        "the pre-spawn gate must fail before any runtime creator effect: {launch_error}"
    );

    let observed = backend
        .inspect_sync(&manifest.handle.id)
        .expect("live runtime inspection should remain available")
        .expect("live runtime should remain inspectable");
    assert_eq!(
        observed.handle.status,
        SandboxStatus::NotReady,
        "NNC4.5: inspection must withdraw Ready when the required PEP is absent or not ready"
    );
    assert!(
        !observed
            .handle
            .published_endpoints
            .contains(&published_endpoint)
            && observed.handle.published_endpoints.is_empty(),
        "PEP dependency loss must withdraw the same endpoint that was visible while Ready"
    );
    assert!(
        backend
            .egress_proxies
            .readiness(&manifest.spec.tenant_id, &manifest.handle.id)
            .expect("post-inspection PEP readiness should inspect")
            .is_none(),
        "inspection must not repair or start the missing PEP"
    );
    assert_eq!(
        fs::read(&manifest.conmon_layout.manifest_path)
            .expect("manifest bytes should remain readable"),
        manifest_before_missing_pep_inspection,
        "missing-PEP inspection must not persist a projection or repair checkpoint"
    );
    assert_eq!(
        fs::read(&manifest.network_layout.status_path)
            .expect("attachment status should remain readable"),
        attachment_before_missing_pep_inspection,
        "missing-PEP inspection must not replay attachment effects"
    );
    assert_eq!(
        pin_provider.apply_count(),
        pin_effects_before_missing_pep_inspection,
        "missing-PEP inspection must not replay the egress-pin provider"
    );

    drop(endpoint_listener);
}
