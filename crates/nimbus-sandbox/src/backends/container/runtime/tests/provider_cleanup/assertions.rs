//! Shared exact-authority assertions for provider-cleanup proofs.

use crate::backends::oci::network::OciMachinePortForwarderConfig;

use super::*;

pub(super) fn assert_machine_unexpose_request(
    request: &[u8],
    binding: &SandboxPortBinding,
    _forwarder: &OciMachinePortForwarderConfig,
) {
    let header_end = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("forwarder request should contain complete headers");
    let headers = std::str::from_utf8(&request[..header_end])
        .expect("forwarder request headers should be UTF-8");
    assert_eq!(
        headers.lines().next(),
        Some("POST /services/forwarder/unexpose HTTP/1.0"),
        "cleanup must target the exact persisted forwarder path"
    );
    let body: serde_json::Value = serde_json::from_slice(&request[header_end + 4..])
        .expect("forwarder request body should be valid JSON");
    assert_eq!(
        body,
        serde_json::json!({
            "local": format!("{}:{}", binding.host_address, binding.host_port),
            "protocol": "tcp",
        }),
        "cleanup must withdraw the exact persisted publication"
    );
}

pub(super) fn manifest_port_lease_records(
    state_root: &std::path::Path,
    manifest: &ContainerSandboxManifest,
) -> Vec<nimbus_network::PortLeaseRecord> {
    let authority = nimbus_network::LocalPortLeaseAuthority::open(state_root)
        .expect("manifest port authority should reopen");
    manifest
        .port_leases
        .iter()
        .chain(
            manifest
                .egress_proxy
                .as_ref()
                .map(|assignment| &assignment.port_lease),
        )
        .map(|request| {
            authority
                .inspect(request.lease_id())
                .expect("manifest lease should inspect")
                .expect("manifest lease record should remain durable")
        })
        .collect()
}

pub(super) fn assert_manifest_port_leases_released(
    state_root: &std::path::Path,
    manifest: &ContainerSandboxManifest,
) -> Vec<nimbus_network::PortLeaseRecord> {
    let records = manifest_port_lease_records(state_root, manifest);
    for record in &records {
        assert_eq!(
            record.phase(),
            nimbus_network::PortLeasePhase::Released,
            "terminal cleanup must release exact lease {}",
            record.request().lease_id()
        );
        assert!(record.confirmed_stopped_binding().is_none());
    }
    records
}
