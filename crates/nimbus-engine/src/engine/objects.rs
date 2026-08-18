use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use nimbus_core::{
    CommitEntry, Document, DocumentId, Error, Result, SequenceNumber, TableName, TenantEventRecord,
    TenantId, Timestamp, WriteOp, WriteOpType,
};
use nimbus_storage::{
    OBJECT_MANIFEST_TABLE, OBJECT_MULTIPART_TABLE, ObjectConditionOutcome, ObjectExpectedState,
    ObjectManifest, ObjectMultipartUpload, ObjectUploadConditionOutcome, ObjectUploadExpectedState,
    multipart_upload_document_id, object_manifest_document_id,
};

use super::Engine;
use super::mutations::durable_outcome::{
    DurableWriteOutcome, DurableWriteRoute, classify_durable_write_error,
};
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
/// `TenantRuntime::enter_operation` guard per call: resolution is hoisted,
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
    PutManifest {
        manifest: Box<ObjectManifest>,
        /// Every clause the committer must find true before it assigns a
        /// sequence. Empty means an unconditional write.
        expected: Vec<ObjectExpectedState>,
    },
    DeleteManifest {
        bucket: String,
        key: String,
    },
    PutMultipart {
        upload: Box<ObjectMultipartUpload>,
        /// Every clause the committer must find true before it assigns a
        /// sequence. Empty means an unconditional write.
        expected: Vec<ObjectUploadExpectedState>,
    },
    DeleteMultipart {
        upload_id: String,
        expected: Vec<ObjectUploadExpectedState>,
    },
}

/// The clause list a write carries, tagged by the row concept it addresses.
///
/// Manifests are conditioned on an opaque `ETag`, uploads on a revision. Two
/// concepts, two clause vocabularies, one committer that decides both before
/// it assigns a sequence.
enum ObjectMetaCondition<'a> {
    Manifest(&'a [ObjectExpectedState]),
    Upload(&'a [ObjectUploadExpectedState]),
}

/// The first clause that did not hold, tagged the same way.
enum ObjectMetaUnmet {
    Manifest(ObjectExpectedState),
    Upload(ObjectUploadExpectedState),
}

impl ObjectMetaWrite {
    /// Expected-state clauses this write carries, in the vocabulary of the
    /// row it addresses. Manifest deletes are unconditional by shape, not by
    /// a silent default: `CompleteMultipartUpload` and `DeleteObject` take no
    /// preconditions on the wire.
    fn condition(&self) -> ObjectMetaCondition<'_> {
        match self {
            Self::PutManifest { expected, .. } => ObjectMetaCondition::Manifest(expected),
            Self::DeleteManifest { .. } => ObjectMetaCondition::Manifest(&[]),
            Self::PutMultipart { expected, .. } | Self::DeleteMultipart { expected, .. } => {
                ObjectMetaCondition::Upload(expected)
            }
        }
    }

    /// Table plus document id the write addresses, and the new document image
    /// for puts (`None` for deletes).
    fn resolve_target(&self) -> Result<(TableName, DocumentId, Option<Document>)> {
        match self {
            Self::PutManifest { manifest, .. } => {
                let document = manifest.to_document()?;
                Ok((document.table.clone(), document.id.clone(), Some(document)))
            }
            Self::DeleteManifest { bucket, key } => Ok((
                TableName::new(OBJECT_MANIFEST_TABLE)?,
                object_manifest_document_id(bucket, key)?,
                None,
            )),
            Self::PutMultipart { upload, .. } => {
                let document = upload.to_document()?;
                Ok((document.table.clone(), document.id.clone(), Some(document)))
            }
            Self::DeleteMultipart { upload_id, .. } => Ok((
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
    /// An expected-state clause did not hold against the committer's own read.
    /// No sequence consumed, no journal record, no fan-out, nothing written.
    ConditionRejected {
        unmet: ObjectMetaUnmet,
        current: Option<Document>,
    },
}

impl TenantObjectMeta {
    /// Writes a manifest, but only if every clause in `expected` holds
    /// against the committer's own read of the current row.
    ///
    /// The condition is decided inside the committer actor, before sequence
    /// assignment, so it cannot interleave with another writer on the same
    /// key. Pass an empty `expected` for an unconditional write; there is no
    /// separate unconditional entry point, so every caller states which one
    /// it wants.
    pub async fn put_manifest_conditional(
        &self,
        manifest: ObjectManifest,
        expected: Vec<ObjectExpectedState>,
    ) -> Result<ObjectConditionOutcome> {
        match self
            .commit_meta_write(ObjectMetaWrite::PutManifest {
                manifest: Box::new(manifest),
                expected,
            })
            .await?
        {
            ObjectMetaWriteOutcome::Committed { commit, previous } => {
                Ok(ObjectConditionOutcome::Committed {
                    commit,
                    previous: previous
                        .as_ref()
                        .map(ObjectManifest::from_document)
                        .transpose()?,
                })
            }
            ObjectMetaWriteOutcome::ConditionRejected { unmet, current } => {
                let ObjectMetaUnmet::Manifest(unmet) = unmet else {
                    return Err(Error::Internal(
                        "a manifest write can only be rejected by a manifest clause".to_string(),
                    ));
                };
                Ok(ObjectConditionOutcome::Rejected {
                    unmet,
                    current: current
                        .as_ref()
                        .map(ObjectManifest::from_document)
                        .transpose()?,
                })
            }
            ObjectMetaWriteOutcome::AbsentTarget => Err(Error::Internal(
                "object manifest put must always commit or reject its condition".to_string(),
            )),
        }
    }

    /// Writes a manifest with no expected state.
    ///
    /// Named rather than defaulted: a caller that wants an unconditional
    /// write says so here, and a caller that needs a condition cannot reach
    /// this entry point by forgetting to pass one.
    ///
    /// # Errors
    /// Fails if the commit fails. A clause-free write cannot be rejected, so
    /// a rejection is an internal contract violation.
    pub async fn put_manifest_unconditional(
        &self,
        manifest: ObjectManifest,
    ) -> Result<CommitEntry> {
        match self.put_manifest_conditional(manifest, Vec::new()).await? {
            ObjectConditionOutcome::Committed { commit, .. } => Ok(commit),
            ObjectConditionOutcome::Rejected { .. } => Err(Error::Internal(
                "a manifest write with no expected state cannot be rejected".to_string(),
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
            ObjectMetaWriteOutcome::ConditionRejected { .. } => Err(Error::Internal(
                "manifest delete carries no condition and cannot be rejected".to_string(),
            )),
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

    /// Writes a multipart-upload row, but only if every clause in `expected`
    /// holds against the committer's own read of the current row.
    ///
    /// `UploadPart` has no conditional headers on the wire, so this is the
    /// only place a multipart merge can be fenced. The caller states the
    /// revision it merged onto; the committer decides against its own read,
    /// before sequence assignment, and the caller reloads and re-merges when
    /// the row has moved on.
    ///
    /// # Errors
    /// Rejects the write as an internal contract violation when the image's
    /// revision is not the successor of the revision its clause names. A
    /// merge that does not advance the revision would let the next writer
    /// merge onto an image it never observed.
    pub async fn put_multipart_upload_conditional(
        &self,
        upload: ObjectMultipartUpload,
        expected: Vec<ObjectUploadExpectedState>,
    ) -> Result<ObjectUploadConditionOutcome> {
        if let [clause] = expected.as_slice()
            && upload.revision != clause.successor_revision()
        {
            return Err(Error::Internal(format!(
                "a fenced multipart write must publish revision {}, not {}",
                clause.successor_revision(),
                upload.revision
            )));
        }
        match self
            .commit_meta_write(ObjectMetaWrite::PutMultipart {
                upload: Box::new(upload),
                expected,
            })
            .await?
        {
            ObjectMetaWriteOutcome::Committed { commit, previous } => {
                Ok(ObjectUploadConditionOutcome::Committed {
                    commit,
                    previous: previous
                        .as_ref()
                        .map(ObjectMultipartUpload::from_document)
                        .transpose()?,
                })
            }
            ObjectMetaWriteOutcome::ConditionRejected { unmet, current } => {
                let ObjectMetaUnmet::Upload(unmet) = unmet else {
                    return Err(Error::Internal(
                        "a multipart write can only be rejected by an upload clause".to_string(),
                    ));
                };
                Ok(ObjectUploadConditionOutcome::Rejected {
                    unmet,
                    current: current
                        .as_ref()
                        .map(ObjectMultipartUpload::from_document)
                        .transpose()?,
                })
            }
            ObjectMetaWriteOutcome::AbsentTarget => Err(Error::Internal(
                "multipart upload put must always commit or reject its condition".to_string(),
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

    /// Removes a multipart-upload row, but only if every clause in
    /// `expected` holds against the committer's own read.
    ///
    /// `CompleteMultipartUpload` and `AbortMultipartUpload` both consume an
    /// upload they have already read. Fencing the delete on that read is what
    /// stops either one from discarding a part that landed in between.
    ///
    /// A fenced delete of an absent row is a rejection, not a silent success:
    /// [`ObjectUploadExpectedState::AtRevision`] does not hold for an absent
    /// row, so the committer refuses it before sequence assignment.
    ///
    /// # Errors
    /// Fails when the caller passes no clauses. Every multipart delete
    /// consumes an upload the caller already read, so it always has a
    /// revision to fence on; a clause-free delete would silently discard
    /// whatever landed after that read.
    pub async fn delete_multipart_upload_conditional(
        &self,
        upload_id: String,
        expected: Vec<ObjectUploadExpectedState>,
    ) -> Result<ObjectUploadConditionOutcome> {
        if expected.is_empty() {
            return Err(Error::Internal(
                "a multipart delete must name the revision it observed".to_string(),
            ));
        }
        match self
            .commit_meta_write(ObjectMetaWrite::DeleteMultipart {
                upload_id,
                expected,
            })
            .await?
        {
            ObjectMetaWriteOutcome::Committed { commit, previous } => {
                let previous = previous.ok_or_else(|| {
                    Error::Internal(
                        "committed multipart delete must carry the removed document".to_string(),
                    )
                })?;
                Ok(ObjectUploadConditionOutcome::Committed {
                    commit,
                    previous: Some(ObjectMultipartUpload::from_document(&previous)?),
                })
            }
            ObjectMetaWriteOutcome::AbsentTarget => Err(Error::Internal(
                "a multipart delete must name the revision it observed".to_string(),
            )),
            ObjectMetaWriteOutcome::ConditionRejected { unmet, current } => {
                let ObjectMetaUnmet::Upload(unmet) = unmet else {
                    return Err(Error::Internal(
                        "a multipart delete can only be rejected by an upload clause".to_string(),
                    ));
                };
                Ok(ObjectUploadConditionOutcome::Rejected {
                    unmet,
                    current: current
                        .as_ref()
                        .map(ObjectMultipartUpload::from_document)
                        .transpose()?,
                })
            }
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
    /// Mirrors the direct-mutation shape: the actor task owns sequencing,
    /// persistence, evidence-based failure classification, and the ordered
    /// fan-out boundary; an ambiguous durable outcome begins crash-recovery
    /// eviction inside the task, and this caller awaits that eviction before
    /// surfacing the error.
    async fn commit_meta_write(&self, write: ObjectMetaWrite) -> Result<ObjectMetaWriteOutcome> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let now = self.engine.now();
        let engine = self.engine.clone();
        let commit_faults = self.engine.commit_faults.clone();
        let initiated_eviction = Arc::new(AtomicBool::new(false));
        let initiated_eviction_for_commit = initiated_eviction.clone();
        let runtime_for_commit = self.runtime.clone();
        let result = self
            .runtime
            .submit_internal_committer_async(move || {
                commit_object_meta_write_in_actor(
                    &engine,
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
        result
    }
}

/// Runs inside the tenant committer actor.
///
/// The actor excludes every other sequence assigner (journal batches, trigger
/// cursor advances, schema commits), so reading the durable head and
/// assigning the next sequence here is race-free, and the read-modify-write
/// against the previous document image cannot interleave with a concurrent
/// put or delete of the same object.
///
/// This is an engine-internal committer route in the same family as the
/// scheduler-state and trigger-cursor writes, not a fourth client mutation
/// path: sequencing, persistence, failure classification
/// (`classify_durable_write_error`), and fan-out are the same seams the
/// direct route uses. Representing an upsert-with-previous-image as a client
/// `Mutation` would expand the client mutation and authorization surface for
/// no additional safety.
fn commit_object_meta_write_in_actor(
    engine: &Arc<Engine>,
    runtime: &Arc<TenantRuntime>,
    commit_faults: &CommitFaultClient,
    now: Timestamp,
    write: ObjectMetaWrite,
    initiated_eviction: Arc<AtomicBool>,
) -> Result<ObjectMetaWriteOutcome> {
    runtime.ensure_committer_lease_for_assignment()?;
    let (table, doc_id, current) = write.resolve_target()?;
    let previous = runtime.store.get(&table, &doc_id)?;
    // Decided here, against the actor's own read, and before any sequence is
    // assigned: a rejected condition leaves no sequence, no journal record,
    // no fan-out, and nothing for the caller to clean up but its own blob.
    if let Some(unmet) = evaluate_object_condition(write.condition(), previous.as_ref())? {
        return Ok(ObjectMetaWriteOutcome::ConditionRejected {
            unmet,
            current: previous,
        });
    }
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
    let previous_sequence = runtime.durable_head();
    let sequence = SequenceNumber(previous_sequence.0.saturating_add(1));
    let record = TenantEventRecord::new(sequence, now, vec![write_op], None)?;
    let commit = record.as_commit_entry();
    runtime.stage_pending_write_log_commits([commit.clone()], now);
    match durable_batch::persist_and_apply_assigned_batch(
        runtime.as_ref(),
        std::slice::from_ref(&record),
        commit_faults,
        || {},
    ) {
        Ok(outcome) => {
            // The core reports success even when a failed apply recovered to
            // an applied head below this record: the write is durable but not
            // yet visible, and acknowledging or fanning it out would expose a
            // commit above the published frontier. Only a returned `applied`
            // entry at this sequence proves visibility.
            if !outcome
                .applied
                .iter()
                .any(|entry| entry.sequence == commit.sequence)
            {
                let error = Error::Internal(format!(
                    "object metadata write became durable at {sequence} but recovery reports an applied head below it; crash-and-replay required"
                ));
                begin_object_meta_durable_recovery(runtime, &error, &initiated_eviction);
                return Err(error);
            }
            // Ordered fan-out boundary: the actor cannot start the next
            // commit until this returns, so subscriptions and observers see
            // object commits in sequence order (same seams as the direct
            // route's post-commit stage).
            engine.process_commit_fanout(runtime.clone(), &commit);
            engine.enqueue_applied_commit_batch_observers(
                runtime.clone(),
                std::slice::from_ref(&commit),
            );
            Ok(ObjectMetaWriteOutcome::Committed { commit, previous })
        }
        // A failed persistence call is not proof the provider did not commit.
        // Classify on durable evidence: rollback is safe only when the
        // authoritative durable head is exactly the pre-write head.
        Err(durable_batch::DurableBatchFailure::Persistence { error, .. }) => {
            match classify_durable_write_error(
                runtime.as_ref(),
                DurableWriteRoute::ObjectMetadata,
                previous_sequence,
                error,
            ) {
                DurableWriteOutcome::Definitive(error) => {
                    runtime.discard_unpersisted_write_log_suffix(sequence);
                    Err(error)
                }
                DurableWriteOutcome::Ambiguous(recovery_error) => {
                    begin_object_meta_durable_recovery(
                        runtime,
                        &recovery_error,
                        &initiated_eviction,
                    );
                    Err(recovery_error)
                }
            }
        }
        Err(durable_batch::DurableBatchFailure::Ambiguous(error)) => {
            begin_object_meta_durable_recovery(runtime, &error, &initiated_eviction);
            Err(error)
        }
    }
}

/// Returns the first expected-state clause that does not hold against
/// `current`, or `None` when every clause holds.
///
/// Decodes the current row only when at least one clause needs it. An empty
/// clause list is an unconditional write and reads nothing.
fn evaluate_object_condition(
    condition: ObjectMetaCondition<'_>,
    current: Option<&Document>,
) -> Result<Option<ObjectMetaUnmet>> {
    match condition {
        ObjectMetaCondition::Manifest(expected) => {
            if expected.is_empty() {
                return Ok(None);
            }
            let current_etag = current
                .map(ObjectManifest::from_document)
                .transpose()?
                .map(|manifest| manifest.etag);
            Ok(
                ObjectExpectedState::first_unmet(expected, current_etag.as_deref())
                    .cloned()
                    .map(ObjectMetaUnmet::Manifest),
            )
        }
        ObjectMetaCondition::Upload(expected) => {
            if expected.is_empty() {
                return Ok(None);
            }
            let current_revision = current
                .map(ObjectMultipartUpload::from_document)
                .transpose()?
                .map(|upload| upload.revision);
            Ok(
                ObjectUploadExpectedState::first_unmet(expected, current_revision)
                    .cloned()
                    .map(ObjectMetaUnmet::Upload),
            )
        }
    }
}

fn begin_object_meta_durable_recovery(
    runtime: &Arc<TenantRuntime>,
    error: &Error,
    initiated_eviction: &AtomicBool,
) {
    runtime.publisher_record_ambiguous_error();
    begin_durable_recovery_eviction(runtime.as_ref(), error);
    runtime.fail_and_drain_mutation_queues(error);
    runtime.close_committed_mutation_observers();
    initiated_eviction.store(true, Ordering::Release);
}
