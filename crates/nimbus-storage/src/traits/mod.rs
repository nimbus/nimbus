//! Focused storage capability traits.
//!
//! These traits are the MBA2 capability split over the current concrete store
//! families. They do not replace the async executor seam in `async_storage`;
//! that seam still owns blocking work and cancellation. The traits here make
//! backend support explicit so future providers can implement only the
//! capability families they actually support.
#![allow(async_fn_in_trait)]

mod committer_lease;
mod core;
mod kv;
mod object_metadata;
mod provider_impls;

pub use committer_lease::{
    CommitterLease, CommitterLeaseError, CommitterLeaseResult, CommitterLeaseStore,
};
pub use core::{
    ControlPlaneUsage, DurableJournal, KeyProviderSurface, MaterializedRebuild, ReadCapabilities,
    ResourcePathScan, ResourcePathSnapshot, SchedulerStore, StorageEngine, TenantLifecycle,
    TenantPointRead, TenantPointWrite, TenantRangeScan,
};
pub use kv::{
    KvBatchOp, KvBatchOutcome, KvEntry, KvMutation, KvPut, KvScanPage, KvStorageEngine,
    KvSweepOutcome, TenantKvStore,
};
pub use object_metadata::{
    OBJECT_MANIFEST_TABLE, OBJECT_MULTIPART_TABLE, ObjectBlobLayout, ObjectChecksums,
    ObjectChunkRef, ObjectConditionOutcome, ObjectExpectedState, ObjectManifest,
    ObjectManifestAttributes, ObjectMetaRead, ObjectMultipartPart, ObjectMultipartUpload,
    multipart_upload_document_id, object_manifest_document_id,
};
// Object metadata has no production writer in this crate; these seed the
// metadata plane for the crate's own read-half coverage.
#[cfg(test)]
pub(crate) use object_metadata::{
    delete_multipart_upload_direct, delete_object_manifest_direct, put_multipart_upload_direct,
    put_object_manifest_direct,
};
