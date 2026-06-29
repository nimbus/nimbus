//! Per-tenant object placement policy stored in the control database.

use std::sync::Arc;

use nimbus_core::{Error, Result, TenantId};
use redb::{ReadableTable, TableDefinition, TableError};
use serde::{Deserialize, Serialize};

use crate::UsageStore;
use crate::store::map_redb_error;

const OBJECT_PLACEMENTS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("object_placements");

/// Cloud/object-store credential source. Raw secrets are intentionally not
/// persisted in placement policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectStoreProviderCredentials {
    Anonymous,
    Environment,
    SecretRef { id: String },
}

/// Object-store provider family for a placement target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectStoreProviderKind {
    S3,
    Gcs,
    Azure,
    Local,
    Memory,
}

/// Configured object-store target used by mirror/tier/cloud-primary placement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectStorePlacementTarget {
    pub provider: ObjectStoreProviderKind,
    pub bucket: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prefix: String,
    pub credentials: ObjectStoreProviderCredentials,
}

impl ObjectStorePlacementTarget {
    pub fn new(
        provider: ObjectStoreProviderKind,
        bucket: impl Into<String>,
        credentials: ObjectStoreProviderCredentials,
    ) -> Result<Self> {
        let bucket = bucket.into();
        if bucket.trim().is_empty() {
            return Err(Error::InvalidInput(
                "object placement target bucket is required".to_string(),
            ));
        }
        Ok(Self {
            provider,
            bucket,
            region: None,
            endpoint: None,
            prefix: String::new(),
            credentials,
        })
    }

    pub fn with_region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }
}

/// Per-tenant placement policy for the object byte plane.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub enum PlacementPolicy {
    #[default]
    LocalOnly,
    Mirror {
        target: ObjectStorePlacementTarget,
        require_ack: bool,
    },
    Tier {
        target: ObjectStorePlacementTarget,
    },
    CloudPrimary {
        target: ObjectStorePlacementTarget,
    },
}

/// Persisted placement policy for one tenant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectPlacement {
    pub tenant_id: TenantId,
    pub policy: PlacementPolicy,
    pub updated_at_unix_ms: u64,
}

impl ObjectPlacement {
    pub fn new(tenant_id: TenantId, policy: PlacementPolicy, updated_at_unix_ms: u64) -> Self {
        Self {
            tenant_id,
            policy,
            updated_at_unix_ms,
        }
    }
}

/// Typed placement facade over the encrypted control redb database.
#[derive(Clone)]
pub struct ObjectPlacementStore {
    usage_store: Arc<UsageStore>,
}

impl ObjectPlacementStore {
    pub(crate) fn new(usage_store: Arc<UsageStore>) -> Self {
        Self { usage_store }
    }

    pub fn set(&self, placement: &ObjectPlacement) -> Result<()> {
        let encoded = serde_json::to_vec(placement)
            .map_err(|error| Error::Serialization(format!("encode object placement: {error}")))?;
        let write_txn = self
            .usage_store
            .database()
            .begin_write()
            .map_err(map_redb_error)?;
        {
            let mut table = write_txn
                .open_table(OBJECT_PLACEMENTS)
                .map_err(map_redb_error)?;
            table
                .insert(placement.tenant_id.as_str().as_bytes(), encoded.as_slice())
                .map_err(map_redb_error)?;
        }
        write_txn.commit().map_err(map_redb_error)
    }

    pub fn get(&self, tenant_id: &TenantId) -> Result<Option<ObjectPlacement>> {
        let read_txn = self
            .usage_store
            .database()
            .begin_read()
            .map_err(map_redb_error)?;
        let table = match read_txn.open_table(OBJECT_PLACEMENTS) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(error) => return Err(map_redb_error(error)),
        };
        table
            .get(tenant_id.as_str().as_bytes())
            .map_err(map_redb_error)?
            .map(|value| decode_placement(value.value()))
            .transpose()
    }

    pub fn delete(&self, tenant_id: &TenantId) -> Result<Option<ObjectPlacement>> {
        let write_txn = self
            .usage_store
            .database()
            .begin_write()
            .map_err(map_redb_error)?;
        let removed = {
            let mut table = match write_txn.open_table(OBJECT_PLACEMENTS) {
                Ok(table) => table,
                Err(TableError::TableDoesNotExist(_)) => return Ok(None),
                Err(error) => return Err(map_redb_error(error)),
            };
            table
                .remove(tenant_id.as_str().as_bytes())
                .map_err(map_redb_error)?
                .map(|value| decode_placement(value.value()))
                .transpose()?
        };
        write_txn.commit().map_err(map_redb_error)?;
        Ok(removed)
    }

    pub fn list(&self) -> Result<Vec<ObjectPlacement>> {
        let read_txn = self
            .usage_store
            .database()
            .begin_read()
            .map_err(map_redb_error)?;
        let table = match read_txn.open_table(OBJECT_PLACEMENTS) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(map_redb_error(error)),
        };
        let mut placements = Vec::new();
        for item in table.iter().map_err(map_redb_error)? {
            let (_, value) = item.map_err(map_redb_error)?;
            placements.push(decode_placement(value.value())?);
        }
        placements.sort_by(|a, b| a.tenant_id.cmp(&b.tenant_id));
        Ok(placements)
    }
}

fn decode_placement(bytes: &[u8]) -> Result<ObjectPlacement> {
    serde_json::from_slice(bytes)
        .map_err(|error| Error::Serialization(format!("decode object placement: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> ObjectPlacementStore {
        ObjectPlacementStore::new(Arc::new(
            UsageStore::create_in_memory().expect("usage store should create"),
        ))
    }

    #[test]
    fn object_placement_round_trips_through_control_db() {
        let store = store();
        let tenant = TenantId::new("tenant-a").expect("tenant should parse");
        let target = ObjectStorePlacementTarget::new(
            ObjectStoreProviderKind::S3,
            "tenant-bucket",
            ObjectStoreProviderCredentials::SecretRef {
                id: "secret/s3".to_string(),
            },
        )
        .unwrap()
        .with_region("us-east-1")
        .with_prefix("objects/tenant-a");
        let placement = ObjectPlacement::new(
            tenant.clone(),
            PlacementPolicy::Mirror {
                target,
                require_ack: true,
            },
            1_776_960_000_000,
        );

        store.set(&placement).unwrap();

        assert_eq!(store.get(&tenant).unwrap(), Some(placement.clone()));
        assert_eq!(store.list().unwrap(), vec![placement.clone()]);
        assert_eq!(store.delete(&tenant).unwrap(), Some(placement));
        assert_eq!(store.get(&tenant).unwrap(), None);
    }

    #[test]
    fn placement_target_requires_bucket() {
        let err = ObjectStorePlacementTarget::new(
            ObjectStoreProviderKind::S3,
            " ",
            ObjectStoreProviderCredentials::Environment,
        )
        .unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));
    }
}
