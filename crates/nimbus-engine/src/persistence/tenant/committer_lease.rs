use std::time::Duration;

use nimbus_storage::{CommitterLease, CommitterLeaseError, CommitterLeaseResult};

use super::*;

// Every lease method below delegates to a remote-provider store and answers
// `Unsupported` for the embedded stores. A build with no remote provider
// compiled in keeps only the `Unsupported` arms, which leaves the fencing
// arguments with no reader; each such method therefore consumes them
// explicitly under `cfg(not(any(provider)))` rather than duplicating its
// signature in a second definition or suppressing the lint.
impl TenantPersistence {
    /// Whether this store needs a durable committer lease before assigning.
    ///
    /// Embedded stores retain process-local sequence authority and never enter
    /// the lease path. Provider stores share their tenant namespace across
    /// processes and therefore require fencing before assignment.
    pub(crate) fn requires_committer_lease(&self) -> bool {
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(_) => true,
            #[cfg(feature = "libsql")]
            Self::LibsqlReplica(_) => true,
            #[cfg(feature = "mysql")]
            Self::MySql(_) => true,
            Self::Redb(_) | Self::Sqlite(_) => false,
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => false,
        }
    }

    pub(crate) fn read_committer_lease(&self) -> Result<Option<CommitterLease>> {
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(store) => store.read_committer_lease(),
            #[cfg(feature = "libsql")]
            Self::LibsqlReplica(store) => store.read_committer_lease(),
            #[cfg(feature = "mysql")]
            Self::MySql(store) => store.read_committer_lease(),
            Self::Redb(_) | Self::Sqlite(_) => Ok(None),
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => Ok(None),
        }
    }

    pub(crate) fn acquire_committer_lease(
        &self,
        owner_id: &str,
        lease_duration: Duration,
    ) -> CommitterLeaseResult<CommitterLease> {
        #[cfg(not(any(feature = "libsql", feature = "mysql", feature = "postgres")))]
        let _ = (owner_id, lease_duration);
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(store) => store.acquire_committer_lease(owner_id, lease_duration),
            #[cfg(feature = "libsql")]
            Self::LibsqlReplica(store) => store.acquire_committer_lease(owner_id, lease_duration),
            #[cfg(feature = "mysql")]
            Self::MySql(store) => store.acquire_committer_lease(owner_id, lease_duration),
            Self::Redb(_) | Self::Sqlite(_) => Err(CommitterLeaseError::Unsupported),
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => Err(CommitterLeaseError::Unsupported),
        }
    }

    pub(crate) fn renew_committer_lease(
        &self,
        owner_id: &str,
        epoch: u64,
        lease_duration: Duration,
    ) -> CommitterLeaseResult<CommitterLease> {
        #[cfg(not(any(feature = "libsql", feature = "mysql", feature = "postgres")))]
        let _ = (owner_id, epoch, lease_duration);
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(store) => store.renew_committer_lease(owner_id, epoch, lease_duration),
            #[cfg(feature = "libsql")]
            Self::LibsqlReplica(store) => {
                store.renew_committer_lease(owner_id, epoch, lease_duration)
            }
            #[cfg(feature = "mysql")]
            Self::MySql(store) => store.renew_committer_lease(owner_id, epoch, lease_duration),
            Self::Redb(_) | Self::Sqlite(_) => Err(CommitterLeaseError::Unsupported),
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => Err(CommitterLeaseError::Unsupported),
        }
    }

    pub(crate) fn fenced_append_and_apply_durable_records_batch_cancellable<Check>(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: nimbus_core::SequenceNumber,
        records: &[nimbus_core::TenantEventRecord],
        check_cancel: Check,
    ) -> CommitterLeaseResult<()>
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        #[cfg(not(any(feature = "libsql", feature = "mysql", feature = "postgres")))]
        let _ = (owner_id, epoch, expected_previous, records, check_cancel);
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(store) => store
                .fenced_append_and_apply_durable_records_batch_cancellable(
                    owner_id,
                    epoch,
                    expected_previous,
                    records,
                    check_cancel,
                ),
            #[cfg(feature = "libsql")]
            Self::LibsqlReplica(store) => store
                .fenced_append_and_apply_durable_records_batch_cancellable(
                    owner_id,
                    epoch,
                    expected_previous,
                    records,
                    check_cancel,
                ),
            #[cfg(feature = "mysql")]
            Self::MySql(store) => store.fenced_append_and_apply_durable_records_batch_cancellable(
                owner_id,
                epoch,
                expected_previous,
                records,
                check_cancel,
            ),
            Self::Redb(_) | Self::Sqlite(_) => Err(CommitterLeaseError::Unsupported),
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => Err(CommitterLeaseError::Unsupported),
        }
    }

    pub(crate) fn fenced_apply_prepared_write_batch(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: nimbus_core::SequenceNumber,
        record: &nimbus_core::TenantEventRecord,
        schedule_ops: &[ResolvedScheduleOp],
        scheduled_execution_id: Option<&str>,
    ) -> CommitterLeaseResult<Option<nimbus_core::CommitEntry>> {
        #[cfg(not(any(feature = "libsql", feature = "mysql", feature = "postgres")))]
        let _ = (
            owner_id,
            epoch,
            expected_previous,
            record,
            schedule_ops,
            scheduled_execution_id,
        );
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(store) => store.fenced_apply_prepared_write_batch(
                owner_id,
                epoch,
                expected_previous,
                record,
                schedule_ops,
                scheduled_execution_id,
            ),
            #[cfg(feature = "libsql")]
            Self::LibsqlReplica(store) => store.fenced_apply_prepared_write_batch(
                owner_id,
                epoch,
                expected_previous,
                record,
                schedule_ops,
                scheduled_execution_id,
            ),
            #[cfg(feature = "mysql")]
            Self::MySql(store) => store.fenced_apply_prepared_write_batch(
                owner_id,
                epoch,
                expected_previous,
                record,
                schedule_ops,
                scheduled_execution_id,
            ),
            Self::Redb(_) | Self::Sqlite(_) => Err(CommitterLeaseError::Unsupported),
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => Err(CommitterLeaseError::Unsupported),
        }
    }

    pub(crate) fn fenced_replace_table_schema(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: nimbus_core::SequenceNumber,
        table_schema: &nimbus_core::TableSchema,
    ) -> CommitterLeaseResult<()> {
        #[cfg(not(any(feature = "libsql", feature = "mysql", feature = "postgres")))]
        let _ = (owner_id, epoch, expected_previous, table_schema);
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(store) => {
                store.fenced_replace_table_schema(owner_id, epoch, expected_previous, table_schema)
            }
            #[cfg(feature = "libsql")]
            Self::LibsqlReplica(store) => {
                store.fenced_replace_table_schema(owner_id, epoch, expected_previous, table_schema)
            }
            #[cfg(feature = "mysql")]
            Self::MySql(store) => {
                store.fenced_replace_table_schema(owner_id, epoch, expected_previous, table_schema)
            }
            Self::Redb(_) | Self::Sqlite(_) => Err(CommitterLeaseError::Unsupported),
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => Err(CommitterLeaseError::Unsupported),
        }
    }

    pub(crate) fn fenced_delete_table_schema(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: nimbus_core::SequenceNumber,
        table: &nimbus_core::TableName,
    ) -> CommitterLeaseResult<()> {
        #[cfg(not(any(feature = "libsql", feature = "mysql", feature = "postgres")))]
        let _ = (owner_id, epoch, expected_previous, table);
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(store) => {
                store.fenced_delete_table_schema(owner_id, epoch, expected_previous, table)
            }
            #[cfg(feature = "libsql")]
            Self::LibsqlReplica(store) => {
                store.fenced_delete_table_schema(owner_id, epoch, expected_previous, table)
            }
            #[cfg(feature = "mysql")]
            Self::MySql(store) => {
                store.fenced_delete_table_schema(owner_id, epoch, expected_previous, table)
            }
            Self::Redb(_) | Self::Sqlite(_) => Err(CommitterLeaseError::Unsupported),
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => Err(CommitterLeaseError::Unsupported),
        }
    }

    pub(crate) fn fenced_materialize_trigger_invocations(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: nimbus_core::SequenceNumber,
        records: &[nimbus_core::TriggerInvocationRecord],
        cursor: nimbus_core::TriggerDeliveryCursor,
    ) -> CommitterLeaseResult<()> {
        #[cfg(not(any(feature = "libsql", feature = "mysql", feature = "postgres")))]
        let _ = (owner_id, epoch, expected_previous, records, cursor);
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(store) => store.fenced_materialize_trigger_invocations(
                owner_id,
                epoch,
                expected_previous,
                records,
                cursor,
            ),
            #[cfg(feature = "libsql")]
            Self::LibsqlReplica(store) => store.fenced_materialize_trigger_invocations(
                owner_id,
                epoch,
                expected_previous,
                records,
                cursor,
            ),
            #[cfg(feature = "mysql")]
            Self::MySql(store) => store.fenced_materialize_trigger_invocations(
                owner_id,
                epoch,
                expected_previous,
                records,
                cursor,
            ),
            Self::Redb(_) | Self::Sqlite(_) => Err(CommitterLeaseError::Unsupported),
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => Err(CommitterLeaseError::Unsupported),
        }
    }

    pub(crate) fn fenced_import_point_in_time_restore_archive(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: nimbus_core::SequenceNumber,
        archive: &nimbus_storage::PointInTimeRestoreArchive,
    ) -> CommitterLeaseResult<nimbus_storage::JournalProgress> {
        #[cfg(not(any(feature = "libsql", feature = "mysql", feature = "postgres")))]
        let _ = (owner_id, epoch, expected_previous, archive);
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(store) => store.fenced_import_point_in_time_restore_archive(
                owner_id,
                epoch,
                expected_previous,
                archive,
            ),
            #[cfg(feature = "libsql")]
            Self::LibsqlReplica(store) => store.fenced_import_point_in_time_restore_archive(
                owner_id,
                epoch,
                expected_previous,
                archive,
            ),
            #[cfg(feature = "mysql")]
            Self::MySql(store) => store.fenced_import_point_in_time_restore_archive(
                owner_id,
                epoch,
                expected_previous,
                archive,
            ),
            Self::Redb(_) | Self::Sqlite(_) => Err(CommitterLeaseError::Unsupported),
            #[cfg(any(test, feature = "test-hooks"))]
            Self::Memory(_) => Err(CommitterLeaseError::Unsupported),
        }
    }
}
