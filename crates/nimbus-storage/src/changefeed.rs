use nimbus_core::{
    DurableMutationRecord, Error, HistoricalReadErrorKind, Result, SequenceNumber, TenantEventKind,
    TenantEventRecord, Timestamp,
};
use serde::{Deserialize, Serialize};

use crate::store::{
    DurableJournalBootstrap, DurableJournalPage, MaterializedJournalSnapshot, TenantStore,
};
use crate::{LibsqlReplicaTenantStore, MySqlTenantStore, PostgresTenantStore, SqliteTenantStore};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangefeedHandle {
    pub id: String,
    pub snapshot_cut: SequenceNumber,
    pub cursor_floor: SequenceNumber,
}

impl ChangefeedHandle {
    pub fn new(snapshot_cut: SequenceNumber, cursor_floor: SequenceNumber) -> Self {
        Self {
            id: format!("cdc:{}:{}", snapshot_cut.0, cursor_floor.0),
            snapshot_cut,
            cursor_floor,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangefeedCursor {
    pub handle: ChangefeedHandle,
    pub after: SequenceNumber,
}

impl ChangefeedCursor {
    pub fn rotate_handle(&self, handle: ChangefeedHandle) -> Result<Self> {
        ensure_cursor_at_or_above_floor(self.after, handle.cursor_floor)?;
        Ok(Self {
            handle,
            after: self.after,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangefeedEvent {
    pub sequence: SequenceNumber,
    pub timestamp: Timestamp,
    pub events: Vec<TenantEventKind>,
    pub record: TenantEventRecord,
}

impl TryFrom<DurableMutationRecord> for ChangefeedEvent {
    type Error = Error;

    fn try_from(record: DurableMutationRecord) -> Result<Self> {
        record.validate_integrity()?;
        Ok(Self {
            sequence: record.sequence,
            timestamp: record.timestamp,
            events: record.events().to_vec(),
            record,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangefeedBootstrap {
    pub handle: ChangefeedHandle,
    pub cursor: ChangefeedCursor,
    pub snapshot: MaterializedJournalSnapshot,
    pub latest_sequence: SequenceNumber,
    pub cursor_floor: SequenceNumber,
}

impl ChangefeedBootstrap {
    pub fn from_durable_bootstrap(bootstrap: DurableJournalBootstrap) -> Result<Self> {
        ensure_cursor_at_or_above_floor(bootstrap.resume_after, bootstrap.cursor_floor)?;
        let handle = ChangefeedHandle::new(bootstrap.bootstrap_cut, bootstrap.cursor_floor);
        let cursor = ChangefeedCursor {
            handle: handle.clone(),
            after: bootstrap.resume_after,
        };
        Ok(Self {
            handle,
            cursor,
            snapshot: bootstrap.snapshot,
            latest_sequence: bootstrap.bootstrap_cut,
            cursor_floor: bootstrap.cursor_floor,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangefeedPage {
    pub handle: ChangefeedHandle,
    pub events: Vec<ChangefeedEvent>,
    pub next_cursor: ChangefeedCursor,
    pub latest_sequence: SequenceNumber,
    pub cursor_floor: SequenceNumber,
    pub has_more: bool,
}

impl ChangefeedPage {
    pub fn from_durable_page(handle: ChangefeedHandle, page: DurableJournalPage) -> Result<Self> {
        ensure_cursor_at_or_above_floor(page.next_cursor, page.cursor_floor)?;
        let events = page
            .records
            .into_iter()
            .map(ChangefeedEvent::try_from)
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            next_cursor: ChangefeedCursor {
                handle: handle.clone(),
                after: page.next_cursor,
            },
            handle,
            events,
            latest_sequence: page.latest_sequence,
            cursor_floor: page.cursor_floor,
            has_more: page.has_more,
        })
    }
}

pub(crate) fn map_changefeed_journal_error(error: Error) -> Error {
    match error {
        Error::InvalidInput(message) if message.contains("behind the retention floor") => {
            Error::historical_read(HistoricalReadErrorKind::RetentionExpired, message)
        }
        other => other,
    }
}

fn ensure_cursor_at_or_above_floor(
    after: SequenceNumber,
    cursor_floor: SequenceNumber,
) -> Result<()> {
    if after.0 < cursor_floor.0 {
        return Err(Error::historical_read(
            HistoricalReadErrorKind::RetentionExpired,
            format!(
                "changefeed cursor {} is behind the retention floor {}",
                after.0, cursor_floor.0
            ),
        ));
    }
    Ok(())
}

macro_rules! impl_changefeed_journal {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl $ty {
                pub fn export_changefeed_bootstrap(&self) -> Result<ChangefeedBootstrap> {
                    ChangefeedBootstrap::from_durable_bootstrap(
                        self.export_durable_journal_bootstrap()?,
                    )
                }

                pub fn stream_changefeed(
                    &self,
                    cursor: &ChangefeedCursor,
                    limit: usize,
                ) -> Result<ChangefeedPage> {
                    ensure_cursor_at_or_above_floor(cursor.after, cursor.handle.cursor_floor)?;
                    let page = self
                        .stream_durable_journal(cursor.after, limit)
                        .map_err(map_changefeed_journal_error)?;
                    ChangefeedPage::from_durable_page(cursor.handle.clone(), page)
                }
            }
        )+
    };
}

impl_changefeed_journal!(
    TenantStore,
    SqliteTenantStore,
    PostgresTenantStore,
    MySqlTenantStore,
    LibsqlReplicaTenantStore,
);
