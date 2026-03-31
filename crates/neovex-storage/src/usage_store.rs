use std::path::Path;

use neovex_core::{Error, Result};
use redb::backends::InMemoryBackend;
use redb::{Database, ReadableTable, TableDefinition, TableError};
use serde::{Deserialize, Serialize};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime};

use crate::keys::prefix_end;
use crate::store::map_redb_error;

const MONTHLY_ACTIVE_IDENTITIES: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("monthly_active_identities");
const MONTHLY_ACTIVE_COUNTS: TableDefinition<u64, &[u8]> =
    TableDefinition::new("monthly_active_counts");
const MONTHLY_ACTIVE_LAST_RECORDED: TableDefinition<u64, &[u8]> =
    TableDefinition::new("monthly_active_last_recorded");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MonthlyActiveUsersSnapshot {
    pub month: String,
    pub month_start_unix_ms: u64,
    pub monthly_active_users: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_recorded_at_unix_ms: Option<u64>,
}

pub struct UsageStore {
    db: Database,
}

impl UsageStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let db = Database::create(path).map_err(map_redb_error)?;
        Ok(Self { db })
    }

    pub fn create_in_memory() -> Result<Self> {
        let db = Database::builder()
            .create_with_backend(InMemoryBackend::new())
            .map_err(map_redb_error)?;
        Ok(Self { db })
    }

    pub fn record_monthly_active_user(
        &self,
        token_identifier: &str,
        observed_at_unix_ms: u64,
    ) -> Result<bool> {
        let month_start_unix_ms = month_start_unix_ms(observed_at_unix_ms)?;
        let key = monthly_identity_key(month_start_unix_ms, token_identifier);
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;

        let is_new_identity = {
            let mut identities = write_txn
                .open_table(MONTHLY_ACTIVE_IDENTITIES)
                .map_err(map_redb_error)?;
            let existing = identities
                .get(key.as_slice())
                .map_err(map_redb_error)?
                .is_some();
            if !existing {
                let observed_at_bytes = encode_u64(observed_at_unix_ms);
                identities
                    .insert(key.as_slice(), observed_at_bytes.as_slice())
                    .map_err(map_redb_error)?;
            }
            !existing
        };

        if is_new_identity {
            let mut counts = write_txn
                .open_table(MONTHLY_ACTIVE_COUNTS)
                .map_err(map_redb_error)?;
            let next_count = counts
                .get(month_start_unix_ms)
                .map_err(map_redb_error)?
                .map(|value| decode_u64(value.value()))
                .transpose()?
                .unwrap_or(0)
                .saturating_add(1);
            let next_count_bytes = encode_u64(next_count);
            counts
                .insert(month_start_unix_ms, next_count_bytes.as_slice())
                .map_err(map_redb_error)?;
        }

        {
            let mut last_recorded = write_txn
                .open_table(MONTHLY_ACTIVE_LAST_RECORDED)
                .map_err(map_redb_error)?;
            let next_last_recorded = last_recorded
                .get(month_start_unix_ms)
                .map_err(map_redb_error)?
                .map(|value| decode_u64(value.value()))
                .transpose()?
                .unwrap_or(0)
                .max(observed_at_unix_ms);
            let next_last_recorded_bytes = encode_u64(next_last_recorded);
            last_recorded
                .insert(month_start_unix_ms, next_last_recorded_bytes.as_slice())
                .map_err(map_redb_error)?;
        }

        write_txn.commit().map_err(map_redb_error)?;
        Ok(is_new_identity)
    }

    pub fn monthly_active_users_for(
        &self,
        observed_at_unix_ms: u64,
    ) -> Result<MonthlyActiveUsersSnapshot> {
        let month_start_unix_ms = month_start_unix_ms(observed_at_unix_ms)?;
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        let count = read_u64_table_value(&read_txn, MONTHLY_ACTIVE_COUNTS, month_start_unix_ms)?;
        let last_recorded_at_unix_ms =
            read_u64_table_value(&read_txn, MONTHLY_ACTIVE_LAST_RECORDED, month_start_unix_ms)?;

        Ok(MonthlyActiveUsersSnapshot {
            month: month_label(month_start_unix_ms)?,
            month_start_unix_ms,
            monthly_active_users: count.unwrap_or(0),
            last_recorded_at_unix_ms,
        })
    }

    pub fn distinct_identities_for_month(&self, observed_at_unix_ms: u64) -> Result<Vec<String>> {
        let month_start_unix_ms = month_start_unix_ms(observed_at_unix_ms)?;
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        let identities = match read_txn.open_table(MONTHLY_ACTIVE_IDENTITIES) {
            Ok(identities) => identities,
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(map_redb_error(error)),
        };
        let prefix = month_start_unix_ms.to_be_bytes().to_vec();
        let mut values = Vec::new();
        match prefix_end(&prefix) {
            Some(end) => {
                let iter = identities
                    .range(prefix.as_slice()..end.as_slice())
                    .map_err(map_redb_error)?;
                for item in iter {
                    let (key, _) = item.map_err(map_redb_error)?;
                    values.push(monthly_identity_from_key(key.value())?);
                }
            }
            None => {
                let iter = identities
                    .range(prefix.as_slice()..)
                    .map_err(map_redb_error)?;
                for item in iter {
                    let (key, _) = item.map_err(map_redb_error)?;
                    if !key.value().starts_with(&prefix) {
                        break;
                    }
                    values.push(monthly_identity_from_key(key.value())?);
                }
            }
        }
        values.sort();
        Ok(values)
    }
}

fn read_u64_table_value(
    read_txn: &redb::ReadTransaction,
    table: TableDefinition<u64, &[u8]>,
    key: u64,
) -> Result<Option<u64>> {
    let table = match read_txn.open_table(table) {
        Ok(table) => table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(None),
        Err(error) => return Err(map_redb_error(error)),
    };
    table
        .get(key)
        .map_err(map_redb_error)?
        .map(|value| decode_u64(value.value()))
        .transpose()
}

fn month_start_unix_ms(observed_at_unix_ms: u64) -> Result<u64> {
    let datetime = offset_datetime_from_unix_ms(observed_at_unix_ms)?;
    let date = Date::from_calendar_date(datetime.year(), datetime.month(), 1)
        .map_err(|error| Error::Internal(format!("invalid month bucket date: {error}")))?;
    let start = PrimitiveDateTime::new(date, time::Time::MIDNIGHT).assume_utc();
    let millis = start.unix_timestamp_nanos() / 1_000_000;
    u64::try_from(millis)
        .map_err(|_| Error::Internal("month bucket timestamp overflowed u64".to_string()))
}

fn month_label(month_start_unix_ms: u64) -> Result<String> {
    let datetime = offset_datetime_from_unix_ms(month_start_unix_ms)?;
    Ok(format!(
        "{:04}-{:02}",
        datetime.year(),
        month_number(datetime.month())
    ))
}

fn month_number(month: Month) -> u8 {
    u8::from(month)
}

fn offset_datetime_from_unix_ms(unix_ms: u64) -> Result<OffsetDateTime> {
    let nanos = i128::from(unix_ms) * 1_000_000;
    OffsetDateTime::from_unix_timestamp_nanos(nanos).map_err(|error| {
        Error::Internal(format!(
            "failed to build UTC timestamp from unix milliseconds {unix_ms}: {error}"
        ))
    })
}

fn monthly_identity_key(month_start_unix_ms: u64, token_identifier: &str) -> Vec<u8> {
    let mut key = month_start_unix_ms.to_be_bytes().to_vec();
    key.extend_from_slice(token_identifier.as_bytes());
    key
}

fn monthly_identity_from_key(key: &[u8]) -> Result<String> {
    if key.len() < 8 {
        return Err(Error::Internal(
            "monthly active identity key was shorter than 8 bytes".to_string(),
        ));
    }
    String::from_utf8(key[8..].to_vec())
        .map_err(|error| Error::Serialization(format!("invalid UTF-8 identity key: {error}")))
}

fn encode_u64(value: u64) -> [u8; 8] {
    value.to_be_bytes()
}

fn decode_u64(bytes: &[u8]) -> Result<u64> {
    let array: [u8; 8] = bytes
        .try_into()
        .map_err(|_| Error::Internal("expected 8 bytes when decoding u64 metadata".to_string()))?;
    Ok(u64::from_be_bytes(array))
}
