use std::sync::Arc;

use neovex_core::Result;
use neovex_storage::MonthlyActiveUsersSnapshot;

use crate::Service;

impl Service {
    /// Records an authenticated identity in the global monthly active user ledger.
    pub fn record_monthly_active_user(&self, token_identifier: &str) -> Result<bool> {
        self.control_plane_provider
            .record_monthly_active_user(token_identifier, self.now().0)
    }

    /// Records an authenticated identity in the global monthly active user ledger asynchronously.
    pub async fn record_monthly_active_user_async(
        self: &Arc<Self>,
        token_identifier: String,
    ) -> Result<bool> {
        self.control_plane_provider
            .record_monthly_active_user_async(token_identifier, self.now().0)
            .await
    }

    /// Returns the current month's global monthly active user snapshot.
    pub fn current_monthly_active_users(&self) -> Result<MonthlyActiveUsersSnapshot> {
        self.control_plane_provider
            .current_monthly_active_users(self.now().0)
    }

    /// Returns the current month's global monthly active user snapshot asynchronously.
    pub async fn current_monthly_active_users_async(
        self: &Arc<Self>,
    ) -> Result<MonthlyActiveUsersSnapshot> {
        self.control_plane_provider
            .current_monthly_active_users_async(self.now().0)
            .await
    }
}
