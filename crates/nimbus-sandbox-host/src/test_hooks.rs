//! Narrow deterministic OCI host fixtures for upper-crate substitution tests.

use nimbus_network::NetworkPlan;
use nimbus_sandbox::{SandboxBackendKind, SandboxId, SandboxSpec};

use crate::network::{AttachmentBackendKind, oci_attachment_plan};

/// Supply test-only coarse-start fixtures with explicit attachment desired
/// state without changing their legacy port-reservation identities.
pub fn legacy_start_attachment_network_plan_fixture(
    spec: &SandboxSpec,
    sandbox_id: &SandboxId,
    _label: &str,
) -> NetworkPlan {
    let backend = match spec.backend {
        SandboxBackendKind::Container => AttachmentBackendKind::Container,
        SandboxBackendKind::Krun => AttachmentBackendKind::Krun,
    };
    oci_attachment_plan(&spec.tenant_id, sandbox_id, backend)
}
