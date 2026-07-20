use std::time::Duration;

use super::*;
use crate::{CommitterLease, CommitterLeaseError, CommitterLeaseResult};

impl PostgresTenantStore {
    pub fn read_committer_lease(&self) -> Result<Option<CommitterLease>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            let table = qualified_table(&schema_name, "committer_lease");
            let query = format!(
                "SELECT owner_id, epoch, \
                        FLOOR(EXTRACT(EPOCH FROM expires_at) * 1000)::BIGINT, durable_sequence \
                 FROM {table} WHERE singleton = TRUE"
            );
            let row = client
                .query_opt(query.as_str(), &[])
                .await
                .map_err(map_postgres_error)?;
            row.map(postgres_row_to_committer_lease).transpose()
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
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let lease = self.block_on(async move {
            let client = provider.client().await?;
            let lease_table = qualified_table(&schema_name, "committer_lease");
            let log_table = qualified_table(&schema_name, "commit_log");
            let query = format!(
                "INSERT INTO {lease_table} AS lease \
                    (singleton, owner_id, epoch, expires_at, durable_sequence) \
                 SELECT TRUE, $1::TEXT, 1, \
                        CURRENT_TIMESTAMP + ($2::BIGINT * INTERVAL '1 millisecond'), \
                        COALESCE(MAX(sequence), 0) \
                 FROM {log_table} \
                 ON CONFLICT (singleton) DO UPDATE SET \
                    owner_id = CASE \
                        WHEN lease.expires_at <= CURRENT_TIMESTAMP \
                             OR lease.owner_id = EXCLUDED.owner_id \
                        THEN EXCLUDED.owner_id ELSE lease.owner_id END, \
                    epoch = CASE WHEN lease.expires_at <= CURRENT_TIMESTAMP \
                                 THEN lease.epoch + 1 ELSE lease.epoch END, \
                    expires_at = CASE \
                        WHEN lease.expires_at <= CURRENT_TIMESTAMP THEN EXCLUDED.expires_at \
                        WHEN lease.owner_id = EXCLUDED.owner_id \
                            THEN GREATEST(lease.expires_at, EXCLUDED.expires_at) \
                        ELSE lease.expires_at END \
                 WHERE lease.expires_at <= CURRENT_TIMESTAMP \
                       OR lease.owner_id = EXCLUDED.owner_id \
                 RETURNING owner_id, epoch, \
                           FLOOR(EXTRACT(EPOCH FROM expires_at) * 1000)::BIGINT, \
                           durable_sequence"
            );
            client
                .query_opt(query.as_str(), &[&owner_id, &duration_ms])
                .await
                .map_err(map_postgres_error)?
                .map(postgres_row_to_committer_lease)
                .transpose()
        })?;
        lease.ok_or(CommitterLeaseError::Held).and_then(|lease| {
            if lease.owner_id == requested_owner_id {
                Ok(lease)
            } else {
                Err(CommitterLeaseError::Held)
            }
        })
    }

    pub fn renew_committer_lease(
        &self,
        owner_id: &str,
        epoch: u64,
        lease_duration: Duration,
    ) -> CommitterLeaseResult<CommitterLease> {
        let duration_ms = validate_lease_request(owner_id, lease_duration)?;
        let epoch_i64 = i64::try_from(epoch)
            .map_err(|_| Error::InvalidInput(format!("lease epoch {epoch} exceeds BIGINT")))?;
        if epoch == 0 {
            return Err(Error::InvalidInput("lease epoch must be at least 1".to_string()).into());
        }
        let owner_id = owner_id.to_string();
        let fenced_owner_id = owner_id.clone();
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let lease = self.block_on(async move {
            let table = qualified_table(&schema_name, "committer_lease");
            let query = format!(
                "UPDATE {table} \
                 SET expires_at = GREATEST(\
                     expires_at, \
                     CURRENT_TIMESTAMP + ($3::BIGINT * INTERVAL '1 millisecond')) \
                 WHERE singleton = TRUE AND owner_id = $1::TEXT AND epoch = $2::BIGINT \
                       AND expires_at > CURRENT_TIMESTAMP \
                 RETURNING owner_id, epoch, \
                           FLOOR(EXTRACT(EPOCH FROM expires_at) * 1000)::BIGINT, \
                           durable_sequence"
            );
            let client = provider.client().await?;
            client
                .query_opt(query.as_str(), &[&owner_id, &epoch_i64, &duration_ms])
                .await
                .map_err(map_postgres_error)?
                .map(postgres_row_to_committer_lease)
                .transpose()
        })?;
        lease.ok_or(CommitterLeaseError::Fenced {
            owner_id: fenced_owner_id,
            epoch,
        })
    }
}

fn validate_lease_request(owner_id: &str, duration: Duration) -> Result<i64> {
    if owner_id.is_empty() {
        return Err(Error::InvalidInput(
            "committer lease owner id cannot be empty".to_string(),
        ));
    }
    i64::try_from(duration.as_millis()).map_err(|_| {
        Error::InvalidInput("committer lease duration exceeds provider range".to_string())
    })
}

fn postgres_row_to_committer_lease(row: tokio_postgres::Row) -> Result<CommitterLease> {
    let epoch = u64::try_from(row.get::<_, i64>(1)).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            "negative committer lease epoch",
        )
    })?;
    let expires_at = u64::try_from(row.get::<_, i64>(2)).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            "negative committer lease expiration",
        )
    })?;
    let durable_sequence = u64::try_from(row.get::<_, i64>(3)).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            "negative committer lease durable sequence",
        )
    })?;
    Ok(CommitterLease {
        owner_id: row.get(0),
        epoch,
        expires_at: Timestamp(expires_at),
        durable_sequence: SequenceNumber(durable_sequence),
    })
}
