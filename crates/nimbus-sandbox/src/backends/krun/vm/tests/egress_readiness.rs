//! Fail-before coverage for PEP dependency withdrawal during live inspection.

use super::support::*;
use std::sync::Arc;

use crate::backends::oci::egress::{
    PepPreAdoptionReleaseAuthority, ensure_egress_proxy_running_with_release_authority,
};
use crate::backends::oci::network::{
    AttachmentAttachAuthority, FixedOciEgressPinProvider, default_network_attachment_id,
};

#[test]
fn krun_inspect_withdraws_ready_projection_when_pep_dependency_is_absent_or_not_ready() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let endpoint_listener =
        TcpListener::bind("127.0.0.1:0").expect("published endpoint fixture should bind");
    let endpoint_port = endpoint_listener
        .local_addr()
        .expect("published endpoint fixture should report its address")
        .port();
    let pep_reservation = TcpListener::bind("127.0.0.1:0").expect("PEP port fixture should bind");
    let pep_port = pep_reservation
        .local_addr()
        .expect("PEP port fixture should report its address")
        .port();
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.published_port_range = pep_port..=pep_port;
    let pin_provider = Arc::new(FixedOciEgressPinProvider::ready());
    let backend = KrunSandboxBackend::new(config).with_egress_pin_provider(pin_provider.clone());
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
    drop(pep_reservation);
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
    let initially_ready = backend
        .inspect_sync(&manifest.handle.id)
        .expect("live runtime inspection should succeed")
        .expect("live runtime should remain inspectable");
    assert_eq!(
        initially_ready.status,
        SandboxStatus::Ready,
        "precondition: a live runtime with its PEP must project Ready"
    );
    assert_eq!(
        initially_ready.published_endpoints.len(),
        1,
        "precondition: Ready must visibly publish the desired endpoint"
    );
    let published_endpoint = initially_ready.published_endpoints[0].clone();
    assert_eq!(published_endpoint.name, "published-api");
    assert_eq!(published_endpoint.address.port(), endpoint_port);

    let status_bytes =
        fs::read(&manifest.network_layout.status_path).expect("exact Netavark status should read");
    fs::remove_file(&manifest.network_layout.status_path)
        .expect("Netavark status facet should be removable");
    let attachment_withdrawn = backend
        .inspect_sync(&manifest.handle.id)
        .expect("live runtime with lost attachment evidence should inspect")
        .expect("live runtime should remain inspectable");
    assert_eq!(
        attachment_withdrawn.status,
        SandboxStatus::NotReady,
        "losing one required attachment facet must withdraw Ready"
    );
    assert!(
        attachment_withdrawn.published_endpoints.is_empty(),
        "attachment evidence loss must withdraw every endpoint"
    );
    fs::write(&manifest.network_layout.status_path, status_bytes)
        .expect("exact Netavark status facet should restore");
    let attachment_restored = backend
        .inspect_sync(&manifest.handle.id)
        .expect("restored exact attachment evidence should inspect")
        .expect("live runtime should remain inspectable");
    assert_eq!(
        attachment_restored.status,
        SandboxStatus::Ready,
        "restoring exact evidence should permit application readiness to recover"
    );
    assert_eq!(
        attachment_restored.published_endpoints,
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
        observed.status,
        SandboxStatus::NotReady,
        "NNC4.5: inspection must withdraw Ready when the required PEP is absent or not ready"
    );
    assert!(
        !observed.published_endpoints.contains(&published_endpoint)
            && observed.published_endpoints.is_empty(),
        "PEP dependency loss must withdraw the same endpoint that was visible while Ready"
    );

    drop(endpoint_listener);
}
