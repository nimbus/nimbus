//! Focused storage capability traits.
//!
//! These traits are the MBA2 capability split over the current concrete store
//! families. They do not replace the async executor seam in `async_storage`;
//! that seam still owns blocking work and cancellation. The traits here make
//! backend support explicit so future providers can implement only the
//! capability families they actually support.
#![allow(async_fn_in_trait)]

mod core;
mod kv;
mod object_metadata;
mod provider_impls;

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
    ObjectChunkRef, ObjectManifest, ObjectManifestAttributes, ObjectMetaStore, ObjectMultipartPart,
    ObjectMultipartUpload,
};
