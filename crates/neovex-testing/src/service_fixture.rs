use std::fmt::Display;
use std::path::Path;
use std::sync::Arc;

use neovex_core::TenantId;
use neovex_storage::DeterministicHarness;
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

    pub fn new_with_harness<F, E>(harness: DeterministicHarness, builder: F) -> Self
    where
        F: FnOnce(&Path, &DeterministicHarness) -> Result<S, E>,
        E: Display,
    {
        let data_dir = tempdir().expect("tempdir should create");
        let service = Arc::new(
            builder(data_dir.path(), &harness)
                .unwrap_or_else(|error| panic!("service should create: {error}")),
        );
        Self {
            _data_dir: data_dir,
            service,
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use neovex_core::Timestamp;
    use neovex_storage::Clock;

    #[derive(Debug)]
    struct DummyService {
        scenario_name: String,
        seed: u64,
        now_ms: u64,
    }

    #[test]
    fn new_with_harness_passes_scenario_context_to_the_builder() {
        let harness = DeterministicHarness::scenario("fixture-builder", 19, Timestamp(12_345));
        let fixture = ServiceFixture::new_with_harness(harness.clone(), |path, harness| {
            assert!(path.exists(), "fixture tempdir should already exist");
            Ok::<DummyService, std::convert::Infallible>(DummyService {
                scenario_name: harness.name().to_string(),
                seed: harness.seed(),
                now_ms: harness.clock().now().0,
            })
        });

        let service = fixture.service();
        assert_eq!(service.scenario_name, "fixture-builder");
        assert_eq!(service.seed, 19);
        assert_eq!(service.now_ms, 12_345);
        assert!(fixture.data_dir().exists());
    }
}
