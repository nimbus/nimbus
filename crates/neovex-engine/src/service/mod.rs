mod mutations;
mod queries;
mod scheduler;
mod schema;
mod subscriptions;
mod tenants;
mod usage;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

use neovex_core::{Document, Error, Result, TenantId};
use neovex_storage::UsageStore;

use crate::tenant::TenantRuntime;

/// Top-level Neovex engine service.
pub struct Service {
    data_dir: PathBuf,
    tenants: RwLock<HashMap<TenantId, Arc<TenantRuntime>>>,
    usage_store: Arc<UsageStore>,
}

impl Service {
    /// Creates a new service for the provided data directory.
    pub fn new(data_dir: impl Into<PathBuf>) -> Result<Self> {
        let data_dir = data_dir.into();
        std::fs::create_dir_all(&data_dir).map_err(|error| Error::Internal(error.to_string()))?;
        let usage_store = Arc::new(UsageStore::open(data_dir.join("neovex-control.db"))?);
        Ok(Self {
            data_dir,
            tenants: RwLock::new(HashMap::new()),
            usage_store,
        })
    }
}

fn documents_to_json(documents: Vec<Document>) -> Vec<serde_json::Value> {
    documents
        .into_iter()
        .map(|document| document.to_json())
        .collect()
}
