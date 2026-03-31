use std::sync::Arc;

use neovex_core::{Result, Timestamp};
use neovex_storage::MonthlyActiveUsersSnapshot;

use crate::Service;

impl Service {
    /// Records an authenticated identity in the global monthly active user ledger.
    pub fn record_monthly_active_user(&self, token_identifier: &str) -> Result<bool> {
        self.usage_store
            .record_monthly_active_user(token_identifier, Timestamp::now().0)
    }

    /// Records an authenticated identity in the global monthly active user ledger asynchronously.
    pub async fn record_monthly_active_user_async(
        self: &Arc<Self>,
        token_identifier: String,
    ) -> Result<bool> {
        self.call_blocking(move |service| service.record_monthly_active_user(&token_identifier))
            .await
    }

    /// Returns the current month's global monthly active user snapshot.
    pub fn current_monthly_active_users(&self) -> Result<MonthlyActiveUsersSnapshot> {
        self.usage_store
            .monthly_active_users_for(Timestamp::now().0)
    }

    /// Returns the current month's global monthly active user snapshot asynchronously.
    pub async fn current_monthly_active_users_async(
        self: &Arc<Self>,
    ) -> Result<MonthlyActiveUsersSnapshot> {
        self.call_blocking(move |service| service.current_monthly_active_users())
            .await
    }
}
