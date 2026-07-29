use std::time::Duration;

use mysql_async::prelude::Queryable;

use super::*;
use crate::{CommitterLease, CommitterLeaseError, CommitterLeaseResult};

impl MySqlTenantStore {
    pub fn read_committer_lease(&self) -> Result<Option<CommitterLease>> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        self.block_on(async move {
            let table = qualified_table(&database_name, "committer_lease");
            let query = lease_select_sql(table.as_str());
            let mut conn = provider.conn().await?;
            conn.exec_first::<Row, _, _>(query, ())
                .await
                .map_err(map_mysql_error)?
                .map(mysql_row_to_committer_lease)
                .transpose()
        })
    }

    pub fn acquire_committer_lease(
        &self,
        owner_id: &str,
        lease_duration: Duration,
    ) -> CommitterLeaseResult<CommitterLease> {
        let duration_micros = validate_lease_request(owner_id, lease_duration)?;
        let owner_id = owner_id.to_string();
        let requested_owner_id = owner_id.clone();
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        let (lease, acquired) = self.block_on(async move {
            let lease_table = qualified_table(&database_name, "committer_lease");
            let log_table = qualified_table(&database_name, "commit_log");
            let mut conn = provider.conn().await?;
            let mut transaction = conn
                .start_transaction(mysql_async::TxOpts::default())
                .await
                .map_err(map_mysql_error)?;
            let query = format!(
                "INSERT INTO {lease_table} \
                    (singleton, owner_id, epoch, expires_at, durable_sequence) \
                 VALUES (TRUE, ?, 1, TIMESTAMPADD(MICROSECOND, ?, CURRENT_TIMESTAMP(6)), \
                         (SELECT COALESCE(MAX(sequence), 0) FROM {log_table})) \
                 ON DUPLICATE KEY UPDATE \
                    epoch = IF(expires_at <= CURRENT_TIMESTAMP(6), epoch + 1, epoch), \
                    owner_id = IF(owner_id = ? OR expires_at <= CURRENT_TIMESTAMP(6), \
                                  ?, owner_id), \
                    expires_at = IF(owner_id = ?, \
                        GREATEST(expires_at, \
                                 TIMESTAMPADD(MICROSECOND, ?, CURRENT_TIMESTAMP(6))), \
                        expires_at)"
            );
            transaction
                .exec_drop(
                    query,
                    (
                        owner_id.as_str(),
                        duration_micros,
                        owner_id.as_str(),
                        owner_id.as_str(),
                        owner_id.as_str(),
                        duration_micros,
                    ),
                )
                .await
                .map_err(map_mysql_error)?;
            let row = transaction
                .exec_first::<Row, _, _>(lease_select_sql(lease_table.as_str()), ())
                .await
                .map_err(map_mysql_error)?
                .ok_or_else(|| {
                    Error::storage(
                        StorageErrorKind::Corruption,
                        "committer lease row disappeared after acquire",
                    )
                })?;
            let lease = mysql_row_to_committer_lease(row)?;
            let acquired = lease.owner_id == owner_id;
            transaction.commit().await.map_err(map_mysql_error)?;
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
        let duration_micros = validate_lease_request(owner_id, lease_duration)?;
        if epoch == 0 {
            return Err(Error::InvalidInput("lease epoch must be at least 1".to_string()).into());
        }
        let owner_id = owner_id.to_string();
        let fenced_owner_id = owner_id.clone();
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        let renewed = self.block_on(async move {
            let table = qualified_table(&database_name, "committer_lease");
            let mut conn = provider.conn().await?;
            let mut transaction = conn
                .start_transaction(mysql_async::TxOpts::default())
                .await
                .map_err(map_mysql_error)?;
            let update = format!(
                "UPDATE {table} \
                 SET expires_at = GREATEST(\
                     expires_at, TIMESTAMPADD(MICROSECOND, ?, CURRENT_TIMESTAMP(6))) \
                 WHERE singleton = TRUE AND owner_id = ? AND epoch = ? \
                       AND expires_at > CURRENT_TIMESTAMP(6)"
            );
            transaction
                .exec_drop(update, (duration_micros, owner_id.as_str(), epoch))
                .await
                .map_err(map_mysql_error)?;
            let select = format!(
                "{}, expires_at > CURRENT_TIMESTAMP(6) \
                 FROM {table} WHERE singleton = TRUE",
                lease_select_columns()
            );
            let row = transaction
                .exec_first::<Row, _, _>(select, ())
                .await
                .map_err(map_mysql_error)?;
            let renewed = row.map(mysql_row_to_renewed_committer_lease).transpose()?;
            transaction.commit().await.map_err(map_mysql_error)?;
            Ok(renewed)
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

fn validate_lease_request(owner_id: &str, duration: Duration) -> Result<u64> {
    if owner_id.is_empty() || owner_id.len() > 191 {
        return Err(Error::InvalidInput(
            "committer lease owner id must contain 1 through 191 bytes".to_string(),
        ));
    }
    // Canonical lease representation is milliseconds (provider parity, U3);
    // MySQL's TIMESTAMPADD(MICROSECOND, ...) edge multiplies at bind time.
    let millis = u64::try_from(duration.as_millis()).map_err(|_| {
        Error::InvalidInput("committer lease duration exceeds provider range".to_string())
    })?;
    millis.checked_mul(1_000).ok_or_else(|| {
        Error::InvalidInput("committer lease duration exceeds provider range".to_string())
    })
}

fn lease_select_columns() -> &'static str {
    "SELECT owner_id, epoch, \
            CAST(UNIX_TIMESTAMP(expires_at) * 1000 AS UNSIGNED), durable_sequence"
}

fn lease_select_sql(table: &str) -> String {
    format!(
        "{} FROM {table} WHERE singleton = TRUE",
        lease_select_columns()
    )
}

fn mysql_row_to_committer_lease(row: Row) -> Result<CommitterLease> {
    let (owner_id, epoch, expires_at, durable_sequence): (String, u64, u64, u64) =
        mysql_async::from_row(row);
    Ok(CommitterLease {
        owner_id,
        epoch,
        expires_at: Timestamp(expires_at),
        durable_sequence: SequenceNumber(durable_sequence),
    })
}

fn mysql_row_to_renewed_committer_lease(row: Row) -> Result<(CommitterLease, bool)> {
    let (owner_id, epoch, expires_at, durable_sequence, unexpired): (String, u64, u64, u64, u8) =
        mysql_async::from_row(row);
    Ok((
        CommitterLease {
            owner_id,
            epoch,
            expires_at: Timestamp(expires_at),
            durable_sequence: SequenceNumber(durable_sequence),
        },
        unexpired != 0,
    ))
}

#[cfg(test)]
mod validate_tests {
    use super::*;

    #[test]
    fn mysql_lease_validation_is_canonical_millis_bound_as_micros() {
        assert!(validate_lease_request("", Duration::from_secs(1)).is_err());
        assert!(validate_lease_request(&"x".repeat(192), Duration::from_secs(1)).is_err());
        assert_eq!(
            validate_lease_request("owner", Duration::from_millis(1500)).expect("valid request"),
            1_500_000,
            "milliseconds are canonical; the MICROSECOND SQL edge gets millis x 1000"
        );
        assert!(validate_lease_request("owner", Duration::MAX).is_err());
    }
}
