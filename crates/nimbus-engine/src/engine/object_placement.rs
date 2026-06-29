use nimbus_core::{Result, TenantId};
use nimbus_storage::{ObjectPlacement, PlacementPolicy};

use crate::Engine;

impl Engine {
    /// Persists the object byte-plane placement policy for a tenant.
    pub fn set_object_placement(&self, placement: ObjectPlacement) -> Result<()> {
        self.control_plane_provider.set_object_placement(&placement)
    }

    /// Returns the explicit object placement for `tenant_id`, if configured.
    pub fn object_placement(&self, tenant_id: &TenantId) -> Result<Option<ObjectPlacement>> {
        self.control_plane_provider.get_object_placement(tenant_id)
    }

    /// Deletes an explicit object placement override for `tenant_id`.
    pub fn delete_object_placement(&self, tenant_id: &TenantId) -> Result<Option<ObjectPlacement>> {
        self.control_plane_provider
            .delete_object_placement(tenant_id)
    }

    /// Lists configured object placement overrides.
    pub fn list_object_placements(&self) -> Result<Vec<ObjectPlacement>> {
        self.control_plane_provider.list_object_placements()
    }

    /// Returns the effective object placement policy, defaulting to local-only.
    pub fn effective_object_placement_policy(
        &self,
        tenant_id: &TenantId,
    ) -> Result<PlacementPolicy> {
        Ok(self
            .object_placement(tenant_id)?
            .map(|placement| placement.policy)
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use nimbus_core::TenantId;
    use nimbus_storage::{ObjectPlacement, PlacementPolicy};

    use super::*;

    #[test]
    fn engine_object_placement_round_trips_through_control_plane() {
        let temp = tempdir().expect("tempdir should create");
        let engine = Engine::new(temp.path()).expect("engine should create");
        let tenant = TenantId::new("tenant-a").expect("tenant should parse");
        let placement = ObjectPlacement::new(tenant.clone(), PlacementPolicy::LocalOnly, 42);

        engine
            .set_object_placement(placement.clone())
            .expect("placement should persist");

        assert_eq!(
            engine.object_placement(&tenant).expect("placement reads"),
            Some(placement.clone())
        );
        assert_eq!(
            engine
                .effective_object_placement_policy(&tenant)
                .expect("effective placement reads"),
            PlacementPolicy::LocalOnly
        );
        assert_eq!(
            engine.list_object_placements().unwrap(),
            vec![placement.clone()]
        );
        assert_eq!(
            engine.delete_object_placement(&tenant).unwrap(),
            Some(placement)
        );
        assert_eq!(engine.object_placement(&tenant).unwrap(), None);
    }
}
