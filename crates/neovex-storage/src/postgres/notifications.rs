use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresProviderNotification {
    pub tenant_id: TenantId,
    pub journal_changed: bool,
    pub scheduler_changed: bool,
    pub schema_changed: bool,
}

pub struct PostgresNotificationListener {
    pub(super) _client: tokio_postgres::Client,
    pub(super) receiver: mpsc::UnboundedReceiver<Result<PostgresProviderNotification>>,
    pub(super) pump_task: JoinHandle<()>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct PendingPostgresNotification {
    pub(super) journal_changed: bool,
    pub(super) scheduler_changed: bool,
    pub(super) schema_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PostgresProviderNotificationPayload {
    pub(super) tenant_id: String,
    pub(super) journal_changed: bool,
    pub(super) scheduler_changed: bool,
    pub(super) schema_changed: bool,
}

impl PendingPostgresNotification {
    pub(super) fn has_any(self) -> bool {
        self.journal_changed || self.scheduler_changed || self.schema_changed
    }
}

impl PostgresNotificationListener {
    pub async fn recv(&mut self) -> Option<Result<PostgresProviderNotification>> {
        self.receiver.recv().await
    }
}

impl Drop for PostgresNotificationListener {
    fn drop(&mut self) {
        self.pump_task.abort();
    }
}

pub(super) fn parse_postgres_notification(
    notification: tokio_postgres::Notification,
) -> Result<PostgresProviderNotification> {
    let payload: PostgresProviderNotificationPayload = serde_json::from_str(notification.payload())
        .map_err(|error| Error::Serialization(error.to_string()))?;
    Ok(PostgresProviderNotification {
        tenant_id: TenantId::new(payload.tenant_id)?,
        journal_changed: payload.journal_changed,
        scheduler_changed: payload.scheduler_changed,
        schema_changed: payload.schema_changed,
    })
}
