use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use nimbus_core::{Cidr, TenantId};
use nimbus_network::{
    AllocatedSegment, NetworkAttachmentId, NetworkLeaseEpoch, NetworkSegmentAllocator,
    NetworkSegmentReleaseOutcome,
};

use crate::error::SandboxError;

use super::OciSegmentRealization;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SegmentAllocatorOperation {
    SegmentFor(TenantId),
    Acquire(TenantId, NetworkAttachmentId),
    Release(TenantId, NetworkAttachmentId),
    GrowBlock(TenantId),
    Reconcile(BTreeSet<(TenantId, NetworkAttachmentId)>),
}

/// Behavior-recording substitute for proving OCI backends consume only the
/// portable allocator capability.
pub(crate) struct RecordingSegmentAllocator {
    segment: OciSegmentRealization,
    operations: Arc<Mutex<Vec<SegmentAllocatorOperation>>>,
}

impl RecordingSegmentAllocator {
    pub(crate) fn new(tenant: TenantId, cidr: &str, local_slot: u32) -> Self {
        let allocation = AllocatedSegment::new(
            "netsegment_01ARZ3NDEKTSV4RRFFQ69G5FAV"
                .parse()
                .expect("recording segment ID should parse"),
            tenant,
            Cidr::parse(cidr).expect("recording segment CIDR should parse"),
            NetworkLeaseEpoch::new(41),
        );
        Self {
            segment: OciSegmentRealization::from_local_slot(allocation, local_slot),
            operations: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub(crate) fn operations(&self) -> Vec<SegmentAllocatorOperation> {
        self.operations
            .lock()
            .expect("recording allocator lock should not be poisoned")
            .clone()
    }

    fn record(&self, operation: SegmentAllocatorOperation) {
        self.operations
            .lock()
            .expect("recording allocator lock should not be poisoned")
            .push(operation);
    }
}

impl NetworkSegmentAllocator for RecordingSegmentAllocator {
    type Segment = OciSegmentRealization;
    type Error = SandboxError;

    fn segment_for(&self, tenant: &TenantId) -> Result<Self::Segment, Self::Error> {
        self.record(SegmentAllocatorOperation::SegmentFor(tenant.clone()));
        Ok(self.segment.clone())
    }

    fn acquire(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
    ) -> Result<Self::Segment, Self::Error> {
        self.record(SegmentAllocatorOperation::Acquire(
            tenant.clone(),
            attachment_id.clone(),
        ));
        Ok(self.segment.clone())
    }

    fn release(
        &self,
        tenant: &TenantId,
        attachment_id: &NetworkAttachmentId,
    ) -> Result<NetworkSegmentReleaseOutcome<Self::Segment>, Self::Error> {
        self.record(SegmentAllocatorOperation::Release(
            tenant.clone(),
            attachment_id.clone(),
        ));
        Ok(NetworkSegmentReleaseOutcome::TenantDrained {
            segments: vec![self.segment.clone()],
        })
    }

    fn grow_block(&self, tenant: &TenantId) -> Result<Self::Segment, Self::Error> {
        self.record(SegmentAllocatorOperation::GrowBlock(tenant.clone()));
        Ok(self.segment.clone())
    }

    fn reconcile_orphans(
        &self,
        live: &BTreeSet<(TenantId, NetworkAttachmentId)>,
    ) -> Result<Vec<Self::Segment>, Self::Error> {
        self.record(SegmentAllocatorOperation::Reconcile(live.clone()));
        Ok(Vec::new())
    }
}
