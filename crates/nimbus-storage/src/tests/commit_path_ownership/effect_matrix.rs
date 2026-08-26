//! The storage writer ownership matrix: one row per writer, one column per
//! cross-cutting commit effect, every cell an explicit decision.
//!
//! `sql/commit_effects.rs` forces the three composite SQL commit paths to name
//! every effect, and its module doc records the gap that leaves: "adding a field
//! here does not force the direct path to declare a position on it." A direct or
//! internal writer can therefore acquire a new effect, or silently lose one,
//! without any compile or structural failure. That is finding F3.
//!
//! Decision U8 already rejected the two ways of closing the gap inside
//! `SqlCommitEffects`: a `Default` reintroduces the silence the type exists to
//! remove, and boxed per-writer closures replace reviewer-visible variants with
//! opaque callbacks. So the matrix sits beside the composite type instead of
//! inside it, and `effect_gate` checks each row against the writer's own source.
//!
//! Nothing here is optional. There is no `Default`, no `Option`, and no callback:
//! every row names all twelve effects, and "this writer does not touch that
//! concept" is spelled as a variant a reviewer can see, never as an omission.
//!
//! The plan requires eleven effects: admission, lease, condition, document,
//! index, version, scheduler, trigger, journal, watermark, and outcome. This
//! matrix declares a twelfth, `catalog`. Without it the schema, table-lifecycle,
//! resource-path, object-metadata, and usage writers would declare eleven
//! no-ops each, and the matrix would state that they have no effects at all —
//! exactly the silence SIC3 removes.

/// How a writer reaches storage. The gate derives the same classification from
/// source and fails when a row disagrees with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shape {
    /// Opens its own write transaction through `execute_write` or
    /// `execute_write_cancellable`.
    Direct,
    /// Has no transaction of its own; delegates to the named `SqlStoreCore`
    /// writers. Its effects must appear in at least one delegate.
    Composes(&'static [&'static str]),
    /// A `SqlStoreCore` method with no default body. The shared body is a free
    /// function gated on `#[cfg(any(feature = "mysql", feature = "postgres"))]`,
    /// which is why this matrix scans source rather than registering writers at
    /// run time: under the bare feature set those bodies do not compile in, and
    /// a runtime registry would go vacuous exactly where coverage matters.
    ProviderBodied,
    /// A writer outside `SqlStoreCore`, pinned to the file and symbol that owns
    /// it so a rename or deletion fails this gate instead of dropping the row.
    External {
        path: &'static str,
        symbol: &'static str,
    },
}

/// Whether the writer can refuse a duplicate attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Admission {
    /// Guarded by a scheduled-execution id: a replay commits nothing twice.
    Deduplicated,
    AlwaysAdmitted,
}

/// The writer's relationship to the per-tenant committer lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Lease {
    /// Validates `(owner_id, epoch, expected head)` in the same transaction.
    Fenced,
    NotFenced,
    /// Chooses the fenced writer when the tenant holds a lease, the unfenced one
    /// otherwise.
    FencedWhenLeaseHeld,
    /// Acquires or renews the lease itself.
    AcquiredOrRenewed,
}

/// What the writer requires of the current state before it commits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Condition {
    /// A typed expected state decided inside the commit authority before the
    /// sequence is assigned.
    ExpectedState,
    /// A caller-supplied validator run against the current document.
    CallerValidator,
    Unconditional,
}

/// The writer's effect on documents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DocumentEffect {
    PreparedRecord,
    ResolvedExecutionUnit,
    PointInsert,
    PointUpdate,
    PointDelete,
    /// Dispatches across the point-write family rather than one member of it.
    PointWriteFamily,
    /// Applies records that are already durable.
    ReplayedRecords,
    None,
}

/// The writer's effect on index entries. Index maintenance is not a field of
/// `SqlCommitEffects`: it runs inside the document statements, which is why a
/// per-writer declaration is the only place it can be stated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IndexEffect {
    MaintainedWithDocument,
    PrunedWithVersions,
    None,
}

/// The writer's effect on retained document versions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VersionEffect {
    RetainsNewVersion,
    PrunesRetained,
    None,
}

/// The writer's effect on catalog state: schema, table identity, resource-path
/// bindings, object metadata, and the cross-tenant usage database.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CatalogEffect {
    TableSchemaReplaced,
    TableSchemaDeleted,
    ObjectMetadata,
    TableLifecycle,
    ResourcePathBindings,
    UsageControlDatabase,
    RetentionCheckpoint,
    None,
}

/// The writer's effect on scheduler state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SchedulerEffect {
    /// Applies resolved schedule operations inside the commit transaction.
    ResolvedOps,
    JobInserted,
    JobsClaimed,
    JobCompleted,
    JobCancelled,
    JobResultRecorded,
    CronSaved,
    CronDeleted,
    RunningJobsRecovered,
    None,
}

/// The writer's effect on trigger state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TriggerEffect {
    OriginTransactionDefault,
    OriginExplicitOrDefault,
    InvocationsMaterialized,
    InvocationSaved,
    CandidatesRecorded,
    None,
}

/// The writer's effect on the journal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum JournalEffect {
    PreparedRecord,
    CommitEntryFromBufferedWrites,
    DurableRecordsAppended,
    DurableRecordsApplied,
    DurableRecordsReplayed,
    DurableRecordsAppendedAndApplied,
    DurableRecordsPruned,
    None,
}

/// The writer's effect on the applied watermark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WatermarkEffect {
    AdvancedByRecordApply,
    NotAdvanced,
}

/// What the caller observes. For a `SqlStoreCore` writer the gate checks this
/// against the method's own return type, so an outcome cannot drift from the
/// signature it describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Unit,
    Boolean,
    CommitEntry,
    OptionalCommitEntry,
    CommitAndRemovedDocument,
    OptionalCommitAndRemovedDocument,
    ClaimedJobs,
    JournalProgress,
    RetentionSummary,
    RetentionHistorySummary,
    ObjectCondition,
}

/// One writer's complete declaration. Every field is required: adding a
/// thirteenth effect fails compilation at all rows at once, which is the point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct WriterEffects {
    pub(super) writer: &'static str,
    pub(super) shape: Shape,
    pub(super) admission: Admission,
    pub(super) lease: Lease,
    pub(super) condition: Condition,
    pub(super) document: DocumentEffect,
    pub(super) index: IndexEffect,
    pub(super) version: VersionEffect,
    pub(super) catalog: CatalogEffect,
    pub(super) scheduler: SchedulerEffect,
    pub(super) trigger: TriggerEffect,
    pub(super) journal: JournalEffect,
    pub(super) watermark: WatermarkEffect,
    pub(super) outcome: Outcome,
}

/// One writer's contract with a bounded materialized-verification session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VerificationEffect {
    /// The route exposes its successfully applied `TenantEventRecord`, whose
    /// canonical deltas can advance a session root.
    ExactAppliedRecord,
    /// The route changes covered state without an exact journal payload. A
    /// live session must discard its root and rebuild from materialized state.
    Invalidate,
    /// The route advances durability only. It must not advance an applied root.
    DurableOnly,
    /// The route does not change a state family covered by the root.
    NoMaterializedState,
}

/// Every storage writer, client and internal.
pub(super) const MATRIX: &[WriterEffects] = &[
    // Queued journal route, unfenced batch.
    WriterEffects {
        writer: "apply_prepared_write_batch",
        shape: Shape::Direct,
        admission: Admission::Deduplicated,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::PreparedRecord,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::ResolvedOps,
        trigger: TriggerEffect::OriginTransactionDefault,
        journal: JournalEffect::PreparedRecord,
        watermark: WatermarkEffect::AdvancedByRecordApply,
        outcome: Outcome::OptionalCommitEntry,
    },
    // Queued journal route under a held committer lease.
    WriterEffects {
        writer: "fenced_apply_prepared_write_batch",
        shape: Shape::Direct,
        admission: Admission::Deduplicated,
        lease: Lease::Fenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::PreparedRecord,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::ResolvedOps,
        trigger: TriggerEffect::OriginTransactionDefault,
        journal: JournalEffect::PreparedRecord,
        watermark: WatermarkEffect::AdvancedByRecordApply,
        outcome: Outcome::OptionalCommitEntry,
    },
    // Retention GC prunes versions and never commits.
    WriterEffects {
        writer: "compact_retained_versions",
        shape: Shape::Direct,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::PrunedWithVersions,
        version: VersionEffect::PrunesRetained,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::RetentionSummary,
    },
    // Provider history retention publishes the checkpoint and deletes the
    // corresponding journal/MVCC prefixes in one lease-fenced transaction.
    WriterEffects {
        writer: "fenced_finalize_retained_history",
        shape: Shape::Direct,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::Fenced,
        condition: Condition::ExpectedState,
        document: DocumentEffect::None,
        index: IndexEffect::PrunedWithVersions,
        version: VersionEffect::PrunesRetained,
        catalog: CatalogEffect::RetentionCheckpoint,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::DurableRecordsPruned,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::RetentionHistorySummary,
    },
    // Compatibility wrapper: preparation is read-only, then finalization owns
    // the lease-fenced storage effects above.
    WriterEffects {
        writer: "fenced_compact_retained_history",
        shape: Shape::Composes(&["fenced_finalize_retained_history"]),
        admission: Admission::AlwaysAdmitted,
        lease: Lease::Fenced,
        condition: Condition::ExpectedState,
        document: DocumentEffect::None,
        index: IndexEffect::PrunedWithVersions,
        version: VersionEffect::PrunesRetained,
        catalog: CatalogEffect::RetentionCheckpoint,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::DurableRecordsPruned,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::RetentionHistorySummary,
    },
    WriterEffects {
        writer: "replace_table_schema",
        shape: Shape::Direct,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::TableSchemaReplaced,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::Unit,
    },
    WriterEffects {
        writer: "fenced_replace_table_schema",
        shape: Shape::Direct,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::Fenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::TableSchemaReplaced,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::Unit,
    },
    WriterEffects {
        writer: "delete_table_schema",
        shape: Shape::Direct,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::TableSchemaDeleted,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::Unit,
    },
    WriterEffects {
        writer: "fenced_delete_table_schema",
        shape: Shape::Direct,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::Fenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::TableSchemaDeleted,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::Unit,
    },
    WriterEffects {
        writer: "insert_scheduled_job",
        shape: Shape::Direct,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::JobInserted,
        trigger: TriggerEffect::None,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::Unit,
    },
    WriterEffects {
        writer: "claim_due_jobs",
        shape: Shape::Direct,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::JobsClaimed,
        trigger: TriggerEffect::None,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::ClaimedJobs,
    },
    WriterEffects {
        writer: "complete_scheduled_job",
        shape: Shape::Direct,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::JobCompleted,
        trigger: TriggerEffect::None,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::Unit,
    },
    WriterEffects {
        writer: "cancel_scheduled_job",
        shape: Shape::Direct,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::JobCancelled,
        trigger: TriggerEffect::None,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::Boolean,
    },
    WriterEffects {
        writer: "record_scheduled_job_result",
        shape: Shape::Direct,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::JobResultRecorded,
        trigger: TriggerEffect::None,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::Unit,
    },
    WriterEffects {
        writer: "save_cron_job",
        shape: Shape::Direct,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::CronSaved,
        trigger: TriggerEffect::None,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::Unit,
    },
    WriterEffects {
        writer: "delete_cron_job",
        shape: Shape::Direct,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::CronDeleted,
        trigger: TriggerEffect::None,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::Unit,
    },
    WriterEffects {
        writer: "recover_running_jobs",
        shape: Shape::Direct,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::RunningJobsRecovered,
        trigger: TriggerEffect::None,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::Unit,
    },
    WriterEffects {
        writer: "materialize_trigger_invocations",
        shape: Shape::Direct,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::InvocationsMaterialized,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::Unit,
    },
    WriterEffects {
        writer: "fenced_materialize_trigger_invocations",
        shape: Shape::Direct,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::Fenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::InvocationsMaterialized,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::Unit,
    },
    WriterEffects {
        writer: "save_trigger_invocation",
        shape: Shape::Direct,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::InvocationSaved,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::Unit,
    },
    WriterEffects {
        writer: "fenced_save_trigger_invocation",
        shape: Shape::Direct,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::Fenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::InvocationSaved,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::Unit,
    },
    // Execution-unit route: one invocation, one transaction.
    WriterEffects {
        writer: "apply_execution_unit_batch_with_origin",
        shape: Shape::Direct,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::ResolvedExecutionUnit,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::ResolvedOps,
        trigger: TriggerEffect::OriginExplicitOrDefault,
        journal: JournalEffect::CommitEntryFromBufferedWrites,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::OptionalCommitEntry,
    },
    WriterEffects {
        writer: "insert_once",
        shape: Shape::Direct,
        admission: Admission::Deduplicated,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::PointInsert,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::CommitEntryFromBufferedWrites,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::OptionalCommitEntry,
    },
    WriterEffects {
        writer: "insert_with_indexes_once_at",
        shape: Shape::Direct,
        admission: Admission::Deduplicated,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::PointInsert,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::CommitEntryFromBufferedWrites,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::OptionalCommitEntry,
    },
    WriterEffects {
        writer: "update_validated_once",
        shape: Shape::Direct,
        admission: Admission::Deduplicated,
        lease: Lease::NotFenced,
        condition: Condition::CallerValidator,
        document: DocumentEffect::PointUpdate,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::CommitEntryFromBufferedWrites,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::OptionalCommitEntry,
    },
    WriterEffects {
        writer: "update_with_indexes_validated_once_at",
        shape: Shape::Direct,
        admission: Admission::Deduplicated,
        lease: Lease::NotFenced,
        condition: Condition::CallerValidator,
        document: DocumentEffect::PointUpdate,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::CommitEntryFromBufferedWrites,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::OptionalCommitEntry,
    },
    WriterEffects {
        writer: "delete_validated_once",
        shape: Shape::Direct,
        admission: Admission::Deduplicated,
        lease: Lease::NotFenced,
        condition: Condition::CallerValidator,
        document: DocumentEffect::PointDelete,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::CommitEntryFromBufferedWrites,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::OptionalCommitAndRemovedDocument,
    },
    WriterEffects {
        writer: "delete_with_indexes_validated_once_at",
        shape: Shape::Direct,
        admission: Admission::Deduplicated,
        lease: Lease::NotFenced,
        condition: Condition::CallerValidator,
        document: DocumentEffect::PointDelete,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::CommitEntryFromBufferedWrites,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::OptionalCommitAndRemovedDocument,
    },
    // Appends durable records; applies nothing.
    WriterEffects {
        writer: "append_durable_records_batch",
        shape: Shape::ProviderBodied,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::DurableRecordsAppended,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::Unit,
    },
    WriterEffects {
        writer: "apply_durable_records_batch",
        shape: Shape::ProviderBodied,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::ReplayedRecords,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::DurableRecordsApplied,
        watermark: WatermarkEffect::AdvancedByRecordApply,
        outcome: Outcome::Unit,
    },
    // Replay makes nothing newly durable, so it notes no durable records.
    WriterEffects {
        writer: "replay_durable_records_batch",
        shape: Shape::ProviderBodied,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::ReplayedRecords,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::DurableRecordsReplayed,
        watermark: WatermarkEffect::AdvancedByRecordApply,
        outcome: Outcome::Unit,
    },
    WriterEffects {
        writer: "fenced_append_and_apply_durable_records_batch_cancellable",
        shape: Shape::ProviderBodied,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::Fenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::ReplayedRecords,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::DurableRecordsAppendedAndApplied,
        watermark: WatermarkEffect::AdvancedByRecordApply,
        outcome: Outcome::Unit,
    },
    WriterEffects {
        writer: "recover_durable_journal",
        shape: Shape::ProviderBodied,
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::ReplayedRecords,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::DurableRecordsReplayed,
        watermark: WatermarkEffect::AdvancedByRecordApply,
        outcome: Outcome::JournalProgress,
    },
    WriterEffects {
        writer: "insert",
        shape: Shape::Composes(&["insert_once"]),
        admission: Admission::Deduplicated,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::PointInsert,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::CommitEntryFromBufferedWrites,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::CommitEntry,
    },
    WriterEffects {
        writer: "insert_with_indexes",
        shape: Shape::Composes(&["insert"]),
        admission: Admission::Deduplicated,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::PointInsert,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::CommitEntryFromBufferedWrites,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::CommitEntry,
    },
    WriterEffects {
        writer: "insert_with_indexes_once",
        shape: Shape::Composes(&["insert_once"]),
        admission: Admission::Deduplicated,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::PointInsert,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::CommitEntryFromBufferedWrites,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::OptionalCommitEntry,
    },
    WriterEffects {
        writer: "update_validated",
        shape: Shape::Composes(&["update_validated_once"]),
        admission: Admission::Deduplicated,
        lease: Lease::NotFenced,
        condition: Condition::CallerValidator,
        document: DocumentEffect::PointUpdate,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::CommitEntryFromBufferedWrites,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::CommitEntry,
    },
    WriterEffects {
        writer: "update_with_indexes_validated",
        shape: Shape::Composes(&["update_validated"]),
        admission: Admission::Deduplicated,
        lease: Lease::NotFenced,
        condition: Condition::CallerValidator,
        document: DocumentEffect::PointUpdate,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::CommitEntryFromBufferedWrites,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::CommitEntry,
    },
    WriterEffects {
        writer: "update_with_indexes_validated_once",
        shape: Shape::Composes(&["update_validated_once"]),
        admission: Admission::Deduplicated,
        lease: Lease::NotFenced,
        condition: Condition::CallerValidator,
        document: DocumentEffect::PointUpdate,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::CommitEntryFromBufferedWrites,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::OptionalCommitEntry,
    },
    WriterEffects {
        writer: "delete_validated_returning_document",
        shape: Shape::Composes(&["delete_validated_once"]),
        admission: Admission::Deduplicated,
        lease: Lease::NotFenced,
        condition: Condition::CallerValidator,
        document: DocumentEffect::PointDelete,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::CommitEntryFromBufferedWrites,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::CommitAndRemovedDocument,
    },
    WriterEffects {
        writer: "delete_with_indexes_validated_returning_document",
        shape: Shape::Composes(&["delete_validated_returning_document"]),
        admission: Admission::Deduplicated,
        lease: Lease::NotFenced,
        condition: Condition::CallerValidator,
        document: DocumentEffect::PointDelete,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::CommitEntryFromBufferedWrites,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::CommitAndRemovedDocument,
    },
    WriterEffects {
        writer: "delete_with_indexes_validated_once",
        shape: Shape::Composes(&["delete_validated_once"]),
        admission: Admission::Deduplicated,
        lease: Lease::NotFenced,
        condition: Condition::CallerValidator,
        document: DocumentEffect::PointDelete,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::CommitEntryFromBufferedWrites,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::OptionalCommitAndRemovedDocument,
    },
    WriterEffects {
        writer: "apply_execution_unit_batch",
        shape: Shape::Composes(&["apply_execution_unit_batch_with_origin"]),
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::ResolvedExecutionUnit,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::ResolvedOps,
        trigger: TriggerEffect::OriginExplicitOrDefault,
        journal: JournalEffect::CommitEntryFromBufferedWrites,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::OptionalCommitEntry,
    },
    WriterEffects {
        writer: "fenced_append_and_apply_durable_records_batch",
        shape: Shape::Composes(&["fenced_append_and_apply_durable_records_batch_cancellable"]),
        admission: Admission::AlwaysAdmitted,
        lease: Lease::Fenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::ReplayedRecords,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::DurableRecordsAppendedAndApplied,
        watermark: WatermarkEffect::AdvancedByRecordApply,
        outcome: Outcome::Unit,
    },
    // Appends the archived tail, then recovers it into applied state.
    WriterEffects {
        writer: "import_point_in_time_restore_archive",
        shape: Shape::Composes(&["append_durable_records_batch", "recover_durable_journal"]),
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::ReplayedRecords,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::DurableRecordsAppended,
        watermark: WatermarkEffect::AdvancedByRecordApply,
        outcome: Outcome::JournalProgress,
    },
    WriterEffects {
        writer: "fenced_import_point_in_time_restore_archive",
        shape: Shape::Composes(&["fenced_append_and_apply_durable_records_batch"]),
        admission: Admission::AlwaysAdmitted,
        lease: Lease::Fenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::ReplayedRecords,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::DurableRecordsAppendedAndApplied,
        watermark: WatermarkEffect::AdvancedByRecordApply,
        outcome: Outcome::JournalProgress,
    },
    // Client route 2 of 3. It reaches the point-write family above rather than a
    // `SqlCommitEffects` witness, which is decision U8 and the reason this matrix
    // exists: the composite type cannot force this route to declare a position.
    WriterEffects {
        writer: "apply_mutation_with_mode",
        shape: Shape::External {
            path: "crates/nimbus-engine/src/engine/mutations/direct/execution.rs",
            symbol: "apply_mutation_with_mode",
        },
        admission: Admission::Deduplicated,
        lease: Lease::NotFenced,
        condition: Condition::CallerValidator,
        document: DocumentEffect::PointWriteFamily,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::CommitEntryFromBufferedWrites,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::OptionalCommitEntry,
    },
    // Client route 1 of 3. Picks the fenced batch writer when the tenant holds a
    // committer lease and the unfenced one otherwise.
    WriterEffects {
        writer: "persist_assigned_batch_once",
        shape: Shape::External {
            path: "crates/nimbus-engine/src/engine/mutations/publisher.rs",
            symbol: "persist_assigned_batch_once",
        },
        admission: Admission::Deduplicated,
        lease: Lease::FencedWhenLeaseHeld,
        condition: Condition::Unconditional,
        document: DocumentEffect::PreparedRecord,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::ResolvedOps,
        trigger: TriggerEffect::OriginTransactionDefault,
        journal: JournalEffect::PreparedRecord,
        watermark: WatermarkEffect::AdvancedByRecordApply,
        outcome: Outcome::OptionalCommitEntry,
    },
    // Client route 3 of 3.
    WriterEffects {
        writer: "MutationExecutionUnit",
        shape: Shape::External {
            path: "crates/nimbus-engine/src/engine/execution_units/mod.rs",
            symbol: "MutationExecutionUnit",
        },
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::ResolvedExecutionUnit,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::ResolvedOps,
        trigger: TriggerEffect::OriginExplicitOrDefault,
        journal: JournalEffect::CommitEntryFromBufferedWrites,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::OptionalCommitEntry,
    },
    // Internal committer route. SIC1 and SIC2 moved the condition decision here, so
    // it is the one writer that decides an expected state before sequencing.
    WriterEffects {
        writer: "commit_object_meta_write_in_actor",
        shape: Shape::External {
            path: "crates/nimbus-engine/src/engine/objects.rs",
            symbol: "commit_object_meta_write_in_actor",
        },
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::ExpectedState,
        document: DocumentEffect::PointWriteFamily,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::ObjectMetadata,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::CommitEntryFromBufferedWrites,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::ObjectCondition,
    },
    WriterEffects {
        writer: "ensure_committer_lease_for_assignment",
        shape: Shape::External {
            path: "crates/nimbus-engine/src/tenant/committer_lease.rs",
            symbol: "ensure_committer_lease_for_assignment",
        },
        admission: Admission::AlwaysAdmitted,
        lease: Lease::AcquiredOrRenewed,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::Unit,
    },
    WriterEffects {
        writer: "enqueue_trigger_commit_batch",
        shape: Shape::External {
            path: "crates/nimbus-engine/src/tenant/trigger_candidates.rs",
            symbol: "enqueue_trigger_commit_batch",
        },
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::CandidatesRecorded,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::Unit,
    },
    WriterEffects {
        writer: "activate_hidden_table_identity",
        shape: Shape::External {
            path: "crates/nimbus-storage/src/store/table_lifecycle.rs",
            symbol: "activate_hidden_table_identity",
        },
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::TableLifecycle,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::Unit,
    },
    WriterEffects {
        writer: "upsert_resource_path_binding",
        shape: Shape::External {
            path: "crates/nimbus-storage/src/store/resource_paths.rs",
            symbol: "upsert_resource_path_binding",
        },
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::ResourcePathBindings,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::Unit,
    },
    // Local replica cache only. It materializes an authoritative snapshot and is
    // never the authority itself.
    WriterEffects {
        writer: "materialize_snapshot_to_replica_cache",
        shape: Shape::External {
            path: "crates/nimbus-storage/src/libsql/provider.rs",
            symbol: "materialize_snapshot_to_replica_cache",
        },
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::ReplayedRecords,
        index: IndexEffect::MaintainedWithDocument,
        version: VersionEffect::RetainsNewVersion,
        catalog: CatalogEffect::None,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::DurableRecordsReplayed,
        watermark: WatermarkEffect::AdvancedByRecordApply,
        outcome: Outcome::Unit,
    },
    // Cross-tenant control database. It touches no tenant journal or watermark.
    WriterEffects {
        writer: "record_monthly_active_user",
        shape: Shape::External {
            path: "crates/nimbus-storage/src/usage_store.rs",
            symbol: "record_monthly_active_user",
        },
        admission: Admission::AlwaysAdmitted,
        lease: Lease::NotFenced,
        condition: Condition::Unconditional,
        document: DocumentEffect::None,
        index: IndexEffect::None,
        version: VersionEffect::None,
        catalog: CatalogEffect::UsageControlDatabase,
        scheduler: SchedulerEffect::None,
        trigger: TriggerEffect::None,
        journal: JournalEffect::None,
        watermark: WatermarkEffect::NotAdvanced,
        outcome: Outcome::Unit,
    },
];

/// Verification ownership for every row in [`MATRIX`], in the same order.
///
/// This is separate from `WriterEffects` because it is a session-consumer
/// decision, not another physical commit effect. Exact name-and-order parity
/// is tested so adding or renaming a writer cannot silently omit this contract.
pub(super) const VERIFICATION_MATRIX: &[(&str, VerificationEffect)] = &[
    (
        "apply_prepared_write_batch",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "fenced_apply_prepared_write_batch",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "compact_retained_versions",
        VerificationEffect::NoMaterializedState,
    ),
    (
        "fenced_finalize_retained_history",
        VerificationEffect::NoMaterializedState,
    ),
    (
        "fenced_compact_retained_history",
        VerificationEffect::NoMaterializedState,
    ),
    (
        "replace_table_schema",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "fenced_replace_table_schema",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "delete_table_schema",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "fenced_delete_table_schema",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "insert_scheduled_job",
        VerificationEffect::NoMaterializedState,
    ),
    ("claim_due_jobs", VerificationEffect::NoMaterializedState),
    (
        "complete_scheduled_job",
        VerificationEffect::NoMaterializedState,
    ),
    (
        "cancel_scheduled_job",
        VerificationEffect::NoMaterializedState,
    ),
    (
        "record_scheduled_job_result",
        VerificationEffect::NoMaterializedState,
    ),
    ("save_cron_job", VerificationEffect::NoMaterializedState),
    ("delete_cron_job", VerificationEffect::NoMaterializedState),
    (
        "recover_running_jobs",
        VerificationEffect::NoMaterializedState,
    ),
    (
        "materialize_trigger_invocations",
        VerificationEffect::NoMaterializedState,
    ),
    (
        "fenced_materialize_trigger_invocations",
        VerificationEffect::NoMaterializedState,
    ),
    (
        "save_trigger_invocation",
        VerificationEffect::NoMaterializedState,
    ),
    (
        "fenced_save_trigger_invocation",
        VerificationEffect::NoMaterializedState,
    ),
    (
        "apply_execution_unit_batch_with_origin",
        VerificationEffect::ExactAppliedRecord,
    ),
    ("insert_once", VerificationEffect::ExactAppliedRecord),
    (
        "insert_with_indexes_once_at",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "update_validated_once",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "update_with_indexes_validated_once_at",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "delete_validated_once",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "delete_with_indexes_validated_once_at",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "append_durable_records_batch",
        VerificationEffect::DurableOnly,
    ),
    (
        "apply_durable_records_batch",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "replay_durable_records_batch",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "fenced_append_and_apply_durable_records_batch_cancellable",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "recover_durable_journal",
        VerificationEffect::ExactAppliedRecord,
    ),
    ("insert", VerificationEffect::ExactAppliedRecord),
    (
        "insert_with_indexes",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "insert_with_indexes_once",
        VerificationEffect::ExactAppliedRecord,
    ),
    ("update_validated", VerificationEffect::ExactAppliedRecord),
    (
        "update_with_indexes_validated",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "update_with_indexes_validated_once",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "delete_validated_returning_document",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "delete_with_indexes_validated_returning_document",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "delete_with_indexes_validated_once",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "apply_execution_unit_batch",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "fenced_append_and_apply_durable_records_batch",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "import_point_in_time_restore_archive",
        VerificationEffect::Invalidate,
    ),
    (
        "fenced_import_point_in_time_restore_archive",
        VerificationEffect::Invalidate,
    ),
    (
        "apply_mutation_with_mode",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "persist_assigned_batch_once",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "MutationExecutionUnit",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "commit_object_meta_write_in_actor",
        VerificationEffect::ExactAppliedRecord,
    ),
    (
        "ensure_committer_lease_for_assignment",
        VerificationEffect::NoMaterializedState,
    ),
    (
        "enqueue_trigger_commit_batch",
        VerificationEffect::NoMaterializedState,
    ),
    (
        "activate_hidden_table_identity",
        VerificationEffect::Invalidate,
    ),
    (
        "upsert_resource_path_binding",
        VerificationEffect::NoMaterializedState,
    ),
    (
        "materialize_snapshot_to_replica_cache",
        VerificationEffect::Invalidate,
    ),
    (
        "record_monthly_active_user",
        VerificationEffect::NoMaterializedState,
    ),
];

/// Snapshot and replica-reconciliation writers outside the SIC `SqlStoreCore`
/// census. These paths do not create a new client mutation route, but a live
/// verification session must still account for them.
pub(super) const VERIFICATION_OUT_OF_BAND_WRITERS: &[(&str, &str, VerificationEffect)] = &[
    (
        "store/journal_snapshot.rs",
        "restore_materialized_journal_from_snapshot",
        VerificationEffect::Invalidate,
    ),
    (
        "store/journal_snapshot.rs",
        "rebuild_materialized_journal_from_snapshot",
        VerificationEffect::Invalidate,
    ),
    (
        "memory/journal.rs",
        "restore_materialized_journal_from_snapshot",
        VerificationEffect::Invalidate,
    ),
    (
        "memory/journal.rs",
        "rebuild_materialized_journal_from_snapshot",
        VerificationEffect::Invalidate,
    ),
    (
        "sqlite/journal.rs",
        "restore_materialized_journal_from_snapshot",
        VerificationEffect::Invalidate,
    ),
    (
        "sqlite/journal.rs",
        "rebuild_materialized_journal_from_snapshot",
        VerificationEffect::Invalidate,
    ),
    (
        "sqlite/replica_cache.rs",
        "reconcile_replica_durable_records_batch",
        VerificationEffect::DurableOnly,
    ),
    (
        "libsql.rs",
        "refresh_local_cache_from_snapshot",
        VerificationEffect::Invalidate,
    ),
    (
        "libsql.rs",
        "catch_up_local_cache_from_remote_durable_journal",
        VerificationEffect::Invalidate,
    ),
];
