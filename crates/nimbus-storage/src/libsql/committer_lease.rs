use std::time::Duration;

use super::*;
use crate::{CommitterLease, CommitterLeaseError, CommitterLeaseResult};

impl LibsqlReplicaTenantStore {
    pub fn read_committer_lease(&self) -> Result<Option<CommitterLease>> {
        self.block_on(async move {
            let conn = self.remote_connection()?;
            load_committer_lease(&conn).await
        })
    }

    pub fn acquire_committer_lease(
        &self,
        owner_id: &str,
        lease_duration: Duration,
    ) -> CommitterLeaseResult<CommitterLease> {
        let duration_ms = validate_lease_request(owner_id, lease_duration)?;
        let owner_id = owner_id.to_string();
        let requested_owner_id = owner_id.clone();
        let (lease, acquired) = self.block_on(async move {
            let conn = self.remote_write_connection()?;
            let transaction = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .await
                .map_err(map_libsql_error)?;
            transaction
                .execute(
                    "INSERT INTO committer_lease
                        (singleton, owner_id, epoch, expires_at, durable_sequence)
                     SELECT 1, ?1, 1,
                            CAST(unixepoch('subsec') * 1000 AS INTEGER) + ?2,
                            COALESCE(MAX(sequence), 0)
                     FROM commit_log WHERE TRUE
                     ON CONFLICT(singleton) DO UPDATE SET
                        owner_id = CASE
                            WHEN committer_lease.expires_at <=
                                     CAST(unixepoch('subsec') * 1000 AS INTEGER)
                                 OR committer_lease.owner_id = excluded.owner_id
                            THEN excluded.owner_id ELSE committer_lease.owner_id END,
                        epoch = CASE
                            WHEN committer_lease.expires_at <=
                                 CAST(unixepoch('subsec') * 1000 AS INTEGER)
                            THEN committer_lease.epoch + 1 ELSE committer_lease.epoch END,
                        expires_at = CASE
                            WHEN committer_lease.expires_at <=
                                 CAST(unixepoch('subsec') * 1000 AS INTEGER)
                            THEN excluded.expires_at
                            WHEN committer_lease.owner_id = excluded.owner_id
                            THEN MAX(committer_lease.expires_at, excluded.expires_at)
                            ELSE committer_lease.expires_at END
                     WHERE committer_lease.expires_at <=
                               CAST(unixepoch('subsec') * 1000 AS INTEGER)
                           OR committer_lease.owner_id = excluded.owner_id",
                    libsql::params![owner_id.as_str(), duration_ms],
                )
                .await
                .map_err(map_libsql_error)?;
            let lease = load_committer_lease(&transaction).await?.ok_or_else(|| {
                Error::storage(
                    StorageErrorKind::Corruption,
                    "committer lease row disappeared after acquire",
                )
            })?;
            let acquired = lease.owner_id == owner_id;
            transaction.commit().await.map_err(map_libsql_error)?;
            Ok((lease, acquired))
        })?;
        if acquired && lease.owner_id == requested_owner_id {
            Ok(lease)
        } else {
            Err(CommitterLeaseError::Held)
        }
    }

    pub fn renew_committer_lease(
        &self,
        owner_id: &str,
        epoch: u64,
        lease_duration: Duration,
    ) -> CommitterLeaseResult<CommitterLease> {
        let duration_ms = validate_lease_request(owner_id, lease_duration)?;
        let epoch_i64 = i64::try_from(epoch)
            .map_err(|_| Error::InvalidInput(format!("lease epoch {epoch} exceeds INTEGER")))?;
        if epoch == 0 {
            return Err(Error::InvalidInput("lease epoch must be at least 1".to_string()).into());
        }
        let owner_id = owner_id.to_string();
        let fenced_owner_id = owner_id.clone();
        let renewed = self.block_on(async move {
            let conn = self.remote_write_connection()?;
            let transaction = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .await
                .map_err(map_libsql_error)?;
            transaction
                .execute(
                    "UPDATE committer_lease
                     SET expires_at = MAX(
                         expires_at,
                         CAST(unixepoch('subsec') * 1000 AS INTEGER) + ?3)
                     WHERE singleton = 1 AND owner_id = ?1 AND epoch = ?2
                           AND expires_at >
                               CAST(unixepoch('subsec') * 1000 AS INTEGER)",
                    libsql::params![owner_id.as_str(), epoch_i64, duration_ms],
                )
                .await
                .map_err(map_libsql_error)?;
            let lease = load_committer_lease_with_validity(&transaction).await?;
            transaction.commit().await.map_err(map_libsql_error)?;
            Ok(lease)
        })?;
        match renewed {
            Some((lease, true)) if lease.owner_id == fenced_owner_id && lease.epoch == epoch => {
                Ok(lease)
            }
            _ => Err(CommitterLeaseError::Fenced {
                owner_id: fenced_owner_id,
                epoch,
            }),
        }
    }
}

fn validate_lease_request(owner_id: &str, duration: Duration) -> Result<i64> {
    if owner_id.is_empty() || owner_id.len() > 191 {
        return Err(Error::InvalidInput(
            "committer lease owner id must contain 1 through 191 bytes".to_string(),
        ));
    }
    i64::try_from(duration.as_millis()).map_err(|_| {
        Error::InvalidInput("committer lease duration exceeds provider range".to_string())
    })
}

async fn load_committer_lease(conn: &Connection) -> Result<Option<CommitterLease>> {
    let rows = conn
        .query(
            "SELECT owner_id, epoch, expires_at, durable_sequence
             FROM committer_lease WHERE singleton = 1",
            (),
        )
        .await
        .map_err(map_libsql_error)?;
    take_single_remote_row(rows)
        .await?
        .map(libsql_row_to_committer_lease)
        .transpose()
}

async fn load_committer_lease_with_validity(
    conn: &Connection,
) -> Result<Option<(CommitterLease, bool)>> {
    let rows = conn
        .query(
            "SELECT owner_id, epoch, expires_at, durable_sequence,
                    expires_at > CAST(unixepoch('subsec') * 1000 AS INTEGER)
             FROM committer_lease WHERE singleton = 1",
            (),
        )
        .await
        .map_err(map_libsql_error)?;
    take_single_remote_row(rows)
        .await?
        .map(|row| {
            let unexpired = row.get::<i64>(4).map_err(map_libsql_error)? != 0;
            Ok((libsql_row_to_committer_lease(row)?, unexpired))
        })
        .transpose()
}

fn libsql_row_to_committer_lease(row: libsql::Row) -> Result<CommitterLease> {
    let epoch = u64::try_from(row.get::<i64>(1).map_err(map_libsql_error)?).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            "negative committer lease epoch",
        )
    })?;
    let expires_at = u64::try_from(row.get::<i64>(2).map_err(map_libsql_error)?).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            "negative committer lease expiration",
        )
    })?;
    let durable_sequence =
        u64::try_from(row.get::<i64>(3).map_err(map_libsql_error)?).map_err(|_| {
            Error::storage(
                StorageErrorKind::Corruption,
                "negative committer lease durable sequence",
            )
        })?;
    Ok(CommitterLease {
        owner_id: row.get::<String>(0).map_err(map_libsql_error)?,
        epoch,
        expires_at: Timestamp(expires_at),
        durable_sequence: SequenceNumber(durable_sequence),
    })
}

#[cfg(test)]
mod validate_tests {
    use super::*;

    #[test]
    fn libsql_lease_owner_id_guard_matches_provider_parity() {
        assert!(validate_lease_request("", Duration::from_secs(1)).is_err());
        assert!(validate_lease_request(&"x".repeat(192), Duration::from_secs(1)).is_err());
        assert_eq!(
            validate_lease_request("owner", Duration::from_millis(1500)).expect("valid request"),
            1500
        );
    }
}
