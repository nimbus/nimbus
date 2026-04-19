use std::sync::Arc;

use neovex_core::Result;
use neovex_storage::{EmbeddedRedbControlPlaneProvider, MonthlyActiveUsersSnapshot, UsageStorage};

#[derive(Clone)]
pub(crate) enum ControlPlaneProvider {
    EmbeddedRedb(Arc<EmbeddedRedbControlPlaneProvider>),
}

impl ControlPlaneProvider {
    pub(crate) fn record_monthly_active_user(
        &self,
        token_identifier: &str,
        observed_at_unix_ms: u64,
    ) -> Result<bool> {
        match self {
            Self::EmbeddedRedb(provider) => provider
                .usage_store()
                .record_monthly_active_user(token_identifier, observed_at_unix_ms),
        }
    }

    pub(crate) async fn record_monthly_active_user_async(
        &self,
        token_identifier: String,
        observed_at_unix_ms: u64,
    ) -> Result<bool> {
        match self {
            Self::EmbeddedRedb(provider) => {
                provider
                    .usage_storage()
                    .execute(move |usage_store| {
                        usage_store
                            .record_monthly_active_user(&token_identifier, observed_at_unix_ms)
                    })
                    .await
            }
        }
    }

    pub(crate) fn current_monthly_active_users(
        &self,
        observed_at_unix_ms: u64,
    ) -> Result<MonthlyActiveUsersSnapshot> {
        match self {
            Self::EmbeddedRedb(provider) => provider
                .usage_store()
                .monthly_active_users_for(observed_at_unix_ms),
        }
    }

    pub(crate) async fn current_monthly_active_users_async(
        &self,
        observed_at_unix_ms: u64,
    ) -> Result<MonthlyActiveUsersSnapshot> {
        match self {
            Self::EmbeddedRedb(provider) => {
                provider
                    .usage_storage()
                    .execute(move |usage_store| {
                        usage_store.monthly_active_users_for(observed_at_unix_ms)
                    })
                    .await
            }
        }
    }
}
