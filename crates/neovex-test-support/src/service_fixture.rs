use std::fmt::Display;
use std::path::Path;
use std::sync::Arc;

use neovex_core::TenantId;
use tempfile::{TempDir, tempdir};

pub struct ServiceFixture<S> {
    _data_dir: TempDir,
    service: Arc<S>,
}

impl<S> ServiceFixture<S> {
    pub fn new<F, E>(builder: F) -> Self
    where
        F: FnOnce(&Path) -> Result<S, E>,
        E: Display,
    {
        let data_dir = tempdir().expect("tempdir should create");
        let service = Arc::new(
            builder(data_dir.path())
                .unwrap_or_else(|error| panic!("service should create: {error}")),
        );
        Self {
            _data_dir: data_dir,
            service,
        }
    }

    pub fn service(&self) -> Arc<S> {
        Arc::clone(&self.service)
    }

    pub fn data_dir(&self) -> &Path {
        self._data_dir.path()
    }

    pub fn create_tenant<F, E>(&self, name: &str, create: F) -> TenantId
    where
        F: FnOnce(&S, TenantId) -> Result<(), E>,
        E: Display,
    {
        let tenant_id = TenantId::new(name).expect("tenant id should be valid");
        create(self.service.as_ref(), tenant_id.clone())
            .unwrap_or_else(|error| panic!("tenant should create: {error}"));
        tenant_id
    }
}
