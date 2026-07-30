//! Least-authority read adapters for orphan evidence collection.
//!
//! The collector receives only these object-safe inspection capabilities. The
//! concrete attachment, IPAM, and allocator authorities remain outside its
//! interface, so later edits cannot accidentally gain a mutation path.

use std::path::Path;

use nimbus_core::TenantId;
use nimbus_network::{
    DurableNetworkAttachmentState, LocalNetworkAttachmentAuthority, NetworkAttachmentId,
    NetworkAttachmentReservationObservation, NetworkReservationClaim, NetworkSegmentAllocator,
};

use super::super::ipam::{OciAttachmentProviderEvidence, OciIpamAuthority};
use super::super::realization::OciSegmentRealization;
use crate::error::{Result, SandboxError};

pub(in crate::backends::oci::network) trait OciDesiredAttachmentEvidenceReader {
    fn list_desired_attachment_evidence(&self) -> Result<Vec<DurableNetworkAttachmentState>>;
}

impl OciDesiredAttachmentEvidenceReader for LocalNetworkAttachmentAuthority {
    fn list_desired_attachment_evidence(&self) -> Result<Vec<DurableNetworkAttachmentState>> {
        self.list().map_err(super::attachment_state_error)
    }
}

pub(in crate::backends::oci::network) trait OciProviderAttemptEvidenceReader {
    fn list_provider_attempt_evidence(&self) -> Result<Vec<OciAttachmentProviderEvidence>>;
    fn network_state_root(&self) -> &Path;
}

impl OciProviderAttemptEvidenceReader for OciIpamAuthority {
    fn list_provider_attempt_evidence(&self) -> Result<Vec<OciAttachmentProviderEvidence>> {
        self.list_attachment_provider_evidence()
    }

    fn network_state_root(&self) -> &Path {
        self.state_root()
    }
}

pub(in crate::backends::oci::network) trait OciExactAllocatorEvidenceReader {
    fn inspect_exact_attachment_reservation(
        &self,
        tenant_id: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<NetworkAttachmentReservationObservation>;
}

impl<T> OciExactAllocatorEvidenceReader for T
where
    T: NetworkSegmentAllocator<Segment = OciSegmentRealization, Error = SandboxError> + ?Sized,
{
    fn inspect_exact_attachment_reservation(
        &self,
        tenant_id: &TenantId,
        attachment_id: &NetworkAttachmentId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<NetworkAttachmentReservationObservation> {
        self.inspect_attachment_reservation(tenant_id, attachment_id, reservation_claim)
    }
}
