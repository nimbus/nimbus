//! Read-only, typed view of durable OCI provider-attempt evidence.
//!
//! The IPAM journal remains the sole sandbox-owned attempt authority. This
//! adapter validates and enumerates that authority without constructing a
//! layout from filenames, invoking a provider, or mutating any partition.

use std::path::Path;

use nimbus_core::TenantId;
use nimbus_network::{NetworkAttachmentId, NetworkReservationClaim, NetworkSegmentId};

use super::OciIpamAuthority;
use super::authenticate_ipam_allocation_identity;
use super::provider_operation::validate_netavark_provider_operation_evidence;
use crate::backends::oci::network::dto::{IpamAllocation, NetavarkProviderOperation};
use crate::backends::oci::network::provider_locator::{
    OciArtifactRealmId, OciAttachmentProviderKind, OciAttachmentProviderLocator,
};
use crate::error::{Result, SandboxError};

/// Whether the IPAM entry still owns live provider-attempt authority or is an
/// exact terminal retry witness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::backends::oci::network) enum OciIpamEvidenceLifecycle {
    Live,
    Terminal,
}

/// One validated, tenant-qualified durable OCI provider attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::backends::oci::network) struct OciAttachmentProviderEvidence {
    tenant_id: TenantId,
    attachment_id: NetworkAttachmentId,
    segment_id: NetworkSegmentId,
    reservation_claim: NetworkReservationClaim,
    locator: OciAttachmentProviderLocator,
    provider_operation: NetavarkProviderOperation,
    lifecycle: OciIpamEvidenceLifecycle,
}

impl OciAttachmentProviderEvidence {
    pub(in crate::backends::oci::network) fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub(in crate::backends::oci::network) fn attachment_id(&self) -> &NetworkAttachmentId {
        &self.attachment_id
    }

    pub(in crate::backends::oci::network) fn segment_id(&self) -> &NetworkSegmentId {
        &self.segment_id
    }

    pub(in crate::backends::oci::network) fn reservation_claim(&self) -> &NetworkReservationClaim {
        &self.reservation_claim
    }

    pub(in crate::backends::oci::network) fn sandbox_id(&self) -> &crate::instance::SandboxId {
        self.locator.sandbox_id()
    }

    pub(in crate::backends::oci::network) fn provider_kind(&self) -> OciAttachmentProviderKind {
        self.locator.provider_kind()
    }

    pub(in crate::backends::oci::network) fn artifact_realm_id(&self) -> &OciArtifactRealmId {
        self.locator.artifact_realm_id()
    }

    pub(in crate::backends::oci::network) fn authenticates_workload_root(
        &self,
        workload_root: &Path,
    ) -> Result<bool> {
        self.locator.authenticates_workload_root(workload_root)
    }

    pub(in crate::backends::oci::network) fn authenticates_open_directory(
        &self,
        directory: &cap_std::fs::Dir,
    ) -> Result<bool> {
        self.locator.authenticates_open_directory(directory)
    }

    pub(in crate::backends::oci::network) fn provider_operation(
        &self,
    ) -> &NetavarkProviderOperation {
        &self.provider_operation
    }

    pub(in crate::backends::oci::network) fn lifecycle(&self) -> OciIpamEvidenceLifecycle {
        self.lifecycle
    }
}

impl OciIpamAuthority {
    /// Enumerate every validated provider-attempt record in stable order.
    ///
    /// The returned records include other artifact realms so the caller can
    /// retain them as unmatched evidence rather than silently hiding them.
    pub(in crate::backends::oci::network) fn list_attachment_provider_evidence(
        &self,
    ) -> Result<Vec<OciAttachmentProviderEvidence>> {
        let mut evidence = Vec::new();
        for tenant_id in self.tenant_ipam_tenants()? {
            let state = self.read_tenant(&tenant_id)?;
            for attachment_key in state.allocations.keys() {
                if state.released_allocations.contains_key(attachment_key) {
                    return Err(corrupt_ipam_evidence(format!(
                        "tenant {} contains attachment {attachment_key} in both live and terminal \
                         IPAM authority",
                        tenant_id.as_str()
                    )));
                }
            }
            for (attachment_key, allocation) in state.allocations {
                evidence.push(validate_evidence(
                    &tenant_id,
                    &attachment_key,
                    allocation,
                    OciIpamEvidenceLifecycle::Live,
                )?);
            }
            for (attachment_key, allocation) in state.released_allocations {
                evidence.push(validate_evidence(
                    &tenant_id,
                    &attachment_key,
                    allocation,
                    OciIpamEvidenceLifecycle::Terminal,
                )?);
            }
        }
        evidence.sort_by(|left, right| {
            (
                left.tenant_id.as_str(),
                left.attachment_id.as_str(),
                left.lifecycle,
            )
                .cmp(&(
                    right.tenant_id.as_str(),
                    right.attachment_id.as_str(),
                    right.lifecycle,
                ))
        });
        Ok(evidence)
    }

    /// Inspect one exact tenant-qualified provider attempt without mutation.
    pub(in crate::backends::oci::network) fn get_attachment_provider_evidence(
        &self,
        tenant_id: &TenantId,
        attachment_id: &NetworkAttachmentId,
    ) -> Result<Option<OciAttachmentProviderEvidence>> {
        let state = self.read_tenant(tenant_id)?;
        let live = state.allocations.get(attachment_id.as_str());
        let terminal = state.released_allocations.get(attachment_id.as_str());
        match (live, terminal) {
            (Some(_), Some(_)) => Err(corrupt_ipam_evidence(format!(
                "tenant {} contains attachment {} in both live and terminal IPAM authority",
                tenant_id.as_str(),
                attachment_id.as_str()
            ))),
            (Some(allocation), None) => validate_evidence(
                tenant_id,
                attachment_id.as_str(),
                allocation.clone(),
                OciIpamEvidenceLifecycle::Live,
            )
            .map(Some),
            (None, Some(allocation)) => validate_evidence(
                tenant_id,
                attachment_id.as_str(),
                allocation.clone(),
                OciIpamEvidenceLifecycle::Terminal,
            )
            .map(Some),
            (None, None) => Ok(None),
        }
    }
}

fn validate_evidence(
    tenant_id: &TenantId,
    attachment_key: &str,
    allocation: IpamAllocation,
    lifecycle: OciIpamEvidenceLifecycle,
) -> Result<OciAttachmentProviderEvidence> {
    if allocation.provider_locator.tenant_id() != tenant_id {
        return Err(corrupt_ipam_evidence(format!(
            "tenant {} IPAM partition does not match provider locator tenant {}",
            tenant_id.as_str(),
            allocation.provider_locator.tenant_id().as_str()
        )));
    }
    let attachment_id = attachment_key
        .parse::<NetworkAttachmentId>()
        .map_err(|error| {
            corrupt_ipam_evidence(format!(
                "tenant {} contains invalid attachment key {attachment_key:?}: {error}",
                tenant_id.as_str()
            ))
        })?;
    authenticate_ipam_allocation_identity(tenant_id, &attachment_id, &allocation).map_err(
        |error| {
            corrupt_ipam_evidence(format!(
                "tenant {} attachment {} failed immutable allocation identity authentication: \
                 {error}",
                tenant_id.as_str(),
                attachment_id.as_str()
            ))
        },
    )?;
    allocation.provider_locator.validate()?;
    let segment_id = allocation
        .segment_id
        .parse::<NetworkSegmentId>()
        .map_err(|error| {
            corrupt_ipam_evidence(format!(
                "tenant {} attachment {} contains invalid segment identity {:?}: {error}",
                tenant_id.as_str(),
                attachment_id.as_str(),
                allocation.segment_id
            ))
        })?;
    if allocation.ips.is_empty() {
        return Err(corrupt_ipam_evidence(format!(
            "tenant {} attachment {} has no allocated addresses",
            tenant_id.as_str(),
            attachment_id.as_str()
        )));
    }
    for ip in &allocation.ips {
        ip.parse::<std::net::Ipv4Addr>().map_err(|error| {
            corrupt_ipam_evidence(format!(
                "tenant {} attachment {} contains invalid IPv4 evidence {ip:?}: {error}",
                tenant_id.as_str(),
                attachment_id.as_str()
            ))
        })?;
    }
    if lifecycle == OciIpamEvidenceLifecycle::Terminal
        && !allocation
            .provider_operation
            .permits_terminal_ipam_release()
    {
        return Err(corrupt_ipam_evidence(format!(
            "tenant {} attachment {} terminal IPAM authority carries provider phase {}; only a \
             no-effect reserved or detached generation may be terminal",
            tenant_id.as_str(),
            attachment_id.as_str(),
            allocation.provider_operation.label()
        )));
    }
    validate_netavark_provider_operation_evidence(tenant_id, &attachment_id, &allocation)?;
    Ok(OciAttachmentProviderEvidence {
        tenant_id: tenant_id.clone(),
        attachment_id,
        segment_id,
        reservation_claim: allocation.reservation_claim,
        locator: allocation.provider_locator,
        provider_operation: allocation.provider_operation,
        lifecycle,
    })
}

fn corrupt_ipam_evidence(reason: String) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!("OCI IPAM provider evidence is corrupt: {reason}"),
    }
}
