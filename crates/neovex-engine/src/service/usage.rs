use std::sync::Arc;

use neovex_core::Result;
use neovex_storage::{MonthlyActiveUsersSnapshot, UsageStorage};

use crate::Service;

impl Service {
    /// Records an authenticated identity in the global monthly active user ledger.
    pub fn record_monthly_active_user(&self, token_identifier: &str) -> Result<bool> {
        self.usage_store
            .record_monthly_active_user(token_identifier, self.now().0)
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
        self.usage_store.monthly_active_users_for(self.now().0)
    }

    /// Returns the current month's global monthly active user snapshot asynchronously.
    pub async fn current_monthly_active_users_async(
        self: &Arc<Self>,
    ) -> Result<MonthlyActiveUsersSnapshot> {
        let now = self.now().0;
        self.usage_read_storage
            .execute(move |usage_store| usage_store.monthly_active_users_for(now))
            .await
    }
}
