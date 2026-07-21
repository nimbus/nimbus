use std::sync::Arc;

use nimbus_core::Result;
use nimbus_storage::{
    EmbeddedRedbControlPlaneProvider, MonthlyActiveUsersSnapshot, ObjectPlacement,
    ObjectPlacementStore, UsageStorage,
};

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
                .usage_store()?
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
                    .usage_storage()?
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
                .usage_store()?
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
                    .usage_storage()?
                    .execute(move |usage_store| {
                        usage_store.monthly_active_users_for(observed_at_unix_ms)
                    })
                    .await
            }
        }
    }

    pub(crate) fn object_placement_store(&self) -> Result<Arc<ObjectPlacementStore>> {
        match self {
            Self::EmbeddedRedb(provider) => provider.object_placement_store(),
        }
    }

    pub(crate) fn set_object_placement(&self, placement: &ObjectPlacement) -> Result<()> {
        self.object_placement_store()?.set(placement)
    }

    pub(crate) fn get_object_placement(
        &self,
        tenant_id: &nimbus_core::TenantId,
    ) -> Result<Option<ObjectPlacement>> {
        self.object_placement_store()?.get(tenant_id)
    }

    pub(crate) fn delete_object_placement(
        &self,
        tenant_id: &nimbus_core::TenantId,
    ) -> Result<Option<ObjectPlacement>> {
        self.object_placement_store()?.delete(tenant_id)
    }

    pub(crate) fn list_object_placements(&self) -> Result<Vec<ObjectPlacement>> {
        self.object_placement_store()?.list()
    }

    pub(crate) fn advance_tenant_incarnation(
        &self,
        tenant_id: &nimbus_core::TenantId,
    ) -> Result<u64> {
        match self {
            Self::EmbeddedRedb(provider) => provider.tenant_incarnation_store()?.advance(tenant_id),
        }
    }

    pub(crate) fn tenant_incarnation(&self, tenant_id: &nimbus_core::TenantId) -> Result<u64> {
        match self {
            Self::EmbeddedRedb(provider) => provider.tenant_incarnation_store()?.current(tenant_id),
        }
    }

    pub(crate) async fn advance_tenant_incarnation_async(
        &self,
        tenant_id: nimbus_core::TenantId,
    ) -> Result<u64> {
        match self {
            Self::EmbeddedRedb(provider) => provider.advance_tenant_incarnation(tenant_id).await,
        }
    }

    pub(crate) async fn tenant_incarnation_async(
        &self,
        tenant_id: nimbus_core::TenantId,
    ) -> Result<u64> {
        match self {
            Self::EmbeddedRedb(provider) => provider.tenant_incarnation(tenant_id).await,
        }
    }
}
