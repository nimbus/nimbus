use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use nimbus_core::{
    CommitEntry, Document, DocumentId, Error, Result, SequenceNumber, TableName, TenantEventRecord,
    TenantId, Timestamp, WriteOp, WriteOpType,
};
use nimbus_storage::{
    OBJECT_MANIFEST_TABLE, OBJECT_MULTIPART_TABLE, ObjectManifest, ObjectMultipartUpload,
    multipart_upload_document_id, object_manifest_document_id,
};

use super::Engine;
use super::mutations::{begin_durable_recovery_eviction, durable_batch};
use crate::engine::execution_units::CommitFaultClient;
use crate::tenant::{TenantOperationGuard, TenantRuntime};

impl Engine {
    pub async fn ensure_object_tenant_async(self: &Arc<Self>, tenant_id: TenantId) -> Result<()> {
        self.ensure_tenant_ready_async(tenant_id).await.map(|_| ())
    }

    /// Resolves `tenant_id` to its object-metadata handle once, so callers
    /// that issue several manifest/multipart operations against the same
    /// tenant (an S3 or Convex-storage request) no longer re-resolve the
    /// tenant on every call.
    pub async fn tenant_object_meta(
        self: &Arc<Self>,
        tenant_id: TenantId,
    ) -> Result<TenantObjectMeta> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        Ok(TenantObjectMeta {
            engine: self.clone(),
            runtime,
            tenant_id,
        })
    }

    /// Enters a tenant operation guard for the object byte-plane without
    /// resolving any storage. The byte-plane resolver (`nimbus-object-storage`)
    /// depends on this crate, so it cannot be called from here; instead,
    /// callers that need guarded, lazy blob-plane resolution enter this guard
    /// first and resolve the blob store themselves while it is held. This
    /// mirrors [`TenantObjectMeta`]'s methods, which enter the same guard
    /// around each metadata-plane call: a tenant mid-deletion rejects with
    /// the same [`nimbus_core::Error::TenantNotFound`] either way.
    pub async fn enter_object_blob_operation(
        self: &Arc<Self>,
        tenant_id: &TenantId,
    ) -> Result<TenantOperationGuard> {
        let runtime = self.get_existing_tenant_async(tenant_id).await?;
        runtime.enter_operation(tenant_id)
    }
}

/// One tenant's object-metadata plane, resolved once via
/// [`Engine::tenant_object_meta`]. Each method still enters a fresh
/// [`TenantRuntime::enter_operation`] guard per call: resolution is hoisted,
/// but the deletion-blocking guard remains scoped to the individual
/// operation, matching every other tenant-scoped call in the engine.
///
/// Reads run on the tenant's read executor. Writes are real journal commits:
/// each one is sequenced inside the tenant committer actor, persisted through
/// the shared durable-batch core (fenced on provider-backed tenants), and
/// published through the write log, so manifest and multipart updates are
/// serialized against document mutations and visible to subscriptions like
/// any other commit.
pub struct TenantObjectMeta {
    engine: Arc<Engine>,
    runtime: Arc<TenantRuntime>,
    tenant_id: TenantId,
}

/// One object-metadata write, resolved to a document image inside the
/// committer actor.
enum ObjectMetaWrite {
    PutManifest(Box<ObjectManifest>),
    DeleteManifest { bucket: String, key: String },
    PutMultipart(Box<ObjectMultipartUpload>),
    DeleteMultipart { upload_id: String },
}

impl ObjectMetaWrite {
    /// Table plus document id the write addresses, and the new document image
    /// for puts (`None` for deletes).
    fn resolve_target(&self) -> Result<(TableName, DocumentId, Option<Document>)> {
        match self {
            Self::PutManifest(manifest) => {
                let document = manifest.to_document()?;
                Ok((document.table.clone(), document.id.clone(), Some(document)))
            }
            Self::DeleteManifest { bucket, key } => Ok((
                TableName::new(OBJECT_MANIFEST_TABLE)?,
                object_manifest_document_id(bucket, key)?,
                None,
            )),
            Self::PutMultipart(upload) => {
                let document = upload.to_document()?;
                Ok((document.table.clone(), document.id.clone(), Some(document)))
            }
            Self::DeleteMultipart { upload_id } => Ok((
                TableName::new(OBJECT_MULTIPART_TABLE)?,
                multipart_upload_document_id(upload_id)?,
                None,
            )),
        }
    }
}

/// Outcome of a fenced object-metadata write.
enum ObjectMetaWriteOutcome {
    Committed {
        commit: CommitEntry,
        previous: Option<Document>,
    },
    /// Delete of an absent target: no sequence consumed, nothing committed.
    AbsentTarget,
}

impl TenantObjectMeta {
    pub async fn put_manifest(&self, manifest: ObjectManifest) -> Result<CommitEntry> {
        match self
            .commit_meta_write(ObjectMetaWrite::PutManifest(Box::new(manifest)))
            .await?
        {
            ObjectMetaWriteOutcome::Committed { commit, .. } => Ok(commit),
            ObjectMetaWriteOutcome::AbsentTarget => Err(Error::Internal(
                "object manifest put must always commit".to_string(),
            )),
        }
    }

    pub async fn get_manifest(
        &self,
        bucket: String,
        key: String,
    ) -> Result<Option<ObjectManifest>> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        self.runtime
            .read_storage()
            .execute(move |store| store.get_object_manifest(&bucket, &key))
            .await
    }

    pub async fn delete_manifest(
        &self,
        bucket: String,
        key: String,
    ) -> Result<Option<(CommitEntry, ObjectManifest)>> {
        match self
            .commit_meta_write(ObjectMetaWrite::DeleteManifest { bucket, key })
            .await?
        {
            ObjectMetaWriteOutcome::Committed { commit, previous } => {
                let previous = previous.ok_or_else(|| {
                    Error::Internal(
                        "committed manifest delete must carry the removed document".to_string(),
                    )
                })?;
                Ok(Some((commit, ObjectManifest::from_document(&previous)?)))
            }
            ObjectMetaWriteOutcome::AbsentTarget => Ok(None),
        }
    }

    pub async fn list_manifests(
        &self,
        bucket: String,
        prefix: String,
        limit: usize,
    ) -> Result<Vec<ObjectManifest>> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        self.runtime
            .read_storage()
            .execute(move |store| store.list_object_manifests(&bucket, &prefix, limit))
            .await
    }

    pub async fn put_multipart_upload(&self, upload: ObjectMultipartUpload) -> Result<CommitEntry> {
        match self
            .commit_meta_write(ObjectMetaWrite::PutMultipart(Box::new(upload)))
            .await?
        {
            ObjectMetaWriteOutcome::Committed { commit, .. } => Ok(commit),
            ObjectMetaWriteOutcome::AbsentTarget => Err(Error::Internal(
                "multipart upload put must always commit".to_string(),
            )),
        }
    }

    pub async fn get_multipart_upload(
        &self,
        upload_id: String,
    ) -> Result<Option<ObjectMultipartUpload>> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        self.runtime
            .read_storage()
            .execute(move |store| store.get_multipart_upload(&upload_id))
            .await
    }

    pub async fn delete_multipart_upload(
        &self,
        upload_id: String,
    ) -> Result<Option<(CommitEntry, ObjectMultipartUpload)>> {
        match self
            .commit_meta_write(ObjectMetaWrite::DeleteMultipart { upload_id })
            .await?
        {
            ObjectMetaWriteOutcome::Committed { commit, previous } => {
                let previous = previous.ok_or_else(|| {
                    Error::Internal(
                        "committed multipart delete must carry the removed document".to_string(),
                    )
                })?;
                Ok(Some((
                    commit,
                    ObjectMultipartUpload::from_document(&previous)?,
                )))
            }
            ObjectMetaWriteOutcome::AbsentTarget => Ok(None),
        }
    }

    pub async fn list_multipart_uploads(
        &self,
        bucket: String,
        prefix: String,
        limit: usize,
    ) -> Result<Vec<ObjectMultipartUpload>> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        self.runtime
            .read_storage()
            .execute(move |store| store.list_multipart_uploads(&bucket, &prefix, limit))
            .await
    }

    /// Commits one object-metadata write through the tenant committer actor.
    ///
    /// Mirrors the scheduler-write shape: the actor task owns sequencing and
    /// persistence; an ambiguous durable outcome begins crash-recovery
    /// eviction inside the task, and this caller awaits that eviction before
    /// surfacing the error. On success the commit fans out to subscriptions
    /// and committed-mutation observers exactly like a journal batch.
    async fn commit_meta_write(&self, write: ObjectMetaWrite) -> Result<ObjectMetaWriteOutcome> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let now = self.engine.now();
        let commit_faults = self.engine.commit_faults.clone();
        let initiated_eviction = Arc::new(AtomicBool::new(false));
        let initiated_eviction_for_commit = initiated_eviction.clone();
        let runtime_for_commit = self.runtime.clone();
        let result = self
            .runtime
            .submit_internal_committer_async(move || {
                commit_object_meta_write_in_actor(
                    &runtime_for_commit,
                    &commit_faults,
                    now,
                    write,
                    initiated_eviction_for_commit,
                )
            })
            .await;
        let eviction_completion = initiated_eviction
            .load(Ordering::Acquire)
            .then(|| self.runtime.eviction_completion());
        if let Some(completion) = eviction_completion {
            completion.wait().await;
        }
        let outcome = result?;
        if let ObjectMetaWriteOutcome::Committed { commit, .. } = &outcome {
            let applied = std::slice::from_ref(commit);
            self.engine.process_applied_commit_batch_fanout(
                self.runtime.clone(),
                applied,
                Some(commit.clone()),
                true,
            );
            self.engine
                .enqueue_applied_commit_batch_observers(self.runtime.clone(), applied);
        }
        Ok(outcome)
    }
}

/// Runs inside the tenant committer actor.
///
/// The actor excludes every other sequence assigner (journal batches, trigger
/// cursor advances, schema commits), so reading the durable head and
/// assigning the next sequence here is race-free, and the read-modify-write
/// against the previous document image cannot interleave with a concurrent
/// put or delete of the same object.
fn commit_object_meta_write_in_actor(
    runtime: &Arc<TenantRuntime>,
    commit_faults: &CommitFaultClient,
    now: Timestamp,
    write: ObjectMetaWrite,
    initiated_eviction: Arc<AtomicBool>,
) -> Result<ObjectMetaWriteOutcome> {
    runtime.ensure_committer_lease_for_assignment()?;
    let (table, doc_id, current) = write.resolve_target()?;
    let previous = runtime.store.get(&table, &doc_id)?;
    let mut current = current;
    let op_type = match (&previous, &current) {
        (None, Some(_)) => WriteOpType::Insert,
        (Some(_), Some(_)) => WriteOpType::Update,
        (Some(_), None) => WriteOpType::Delete,
        (None, None) => return Ok(ObjectMetaWriteOutcome::AbsentTarget),
    };
    if let (Some(previous_document), Some(current_document)) = (&previous, &mut current) {
        // Replacement keeps the stored row's creation identity, matching what
        // a patch-style update would have preserved.
        current_document.creation_time = previous_document.creation_time;
    }
    let snapshot = runtime.store.read_snapshot()?;
    let table_id = runtime.prepared_table_id(&table, snapshot.table_id(&table)?);
    drop(snapshot);
    let write_op = WriteOp {
        table,
        table_id,
        op_type,
        doc_id,
        resource_path_binding: None,
        trigger_write_origin: None,
        previous: previous.clone(),
        current,
    };
    let sequence = SequenceNumber(runtime.durable_head().0.saturating_add(1));
    let record = TenantEventRecord::new(sequence, now, vec![write_op], None)?;
    let commit = record.as_commit_entry();
    runtime.stage_pending_write_log_commits([commit.clone()], now);
    match durable_batch::persist_and_apply_assigned_batch(
        runtime.as_ref(),
        std::slice::from_ref(&record),
        commit_faults,
        || {},
    ) {
        Ok(_) => Ok(ObjectMetaWriteOutcome::Committed { commit, previous }),
        Err(durable_batch::DurableBatchFailure::Persistence { error, .. }) => {
            runtime.discard_unpersisted_write_log_suffix(sequence);
            Err(error)
        }
        Err(durable_batch::DurableBatchFailure::Ambiguous(error)) => {
            runtime.publisher_record_ambiguous_error();
            begin_durable_recovery_eviction(runtime.as_ref(), &error);
            runtime.fail_and_drain_mutation_queues(&error);
            runtime.close_committed_mutation_observers();
            initiated_eviction.store(true, Ordering::Release);
            Err(error)
        }
    }
}
