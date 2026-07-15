use std::fmt::Display;
use std::path::Path;
use std::sync::Arc;

use nimbus_core::TenantId;
use nimbus_storage::DeterministicHarness;
use tempfile::{TempDir, tempdir};

pub struct EngineFixture<S> {
    _data_dir: TempDir,
    engine: Arc<S>,
}

impl<S> EngineFixture<S> {
    pub fn new<F, E>(builder: F) -> Self
    where
        F: FnOnce(&Path) -> Result<S, E>,
        E: Display,
    {
        let data_dir = tempdir().expect("tempdir should create");
        let engine = Arc::new(
            builder(data_dir.path())
                .unwrap_or_else(|error| panic!("engine should create: {error}")),
        );
        Self {
            _data_dir: data_dir,
            engine,
        }
    }

    pub fn engine(&self) -> Arc<S> {
        Arc::clone(&self.engine)
    }

    pub fn new_with_harness<F, E>(harness: DeterministicHarness, builder: F) -> Self
    where
        F: FnOnce(&Path, &DeterministicHarness) -> Result<S, E>,
        E: Display,
    {
        let data_dir = tempdir().expect("tempdir should create");
        let engine = Arc::new(
            builder(data_dir.path(), &harness)
                .unwrap_or_else(|error| panic!("engine should create: {error}")),
        );
        Self {
            _data_dir: data_dir,
            engine,
        }
    }

    /// Builds an engine through its test-only memory-persistence constructor.
    ///
    /// The generic builder keeps this fixture crate independent of the engine
    /// crate and mirrors `new_with_harness` without changing existing callers.
    pub fn new_with_memory_persistence<F, E>(harness: DeterministicHarness, builder: F) -> Self
    where
        F: FnOnce(&Path, &DeterministicHarness) -> Result<S, E>,
        E: Display,
    {
        Self::new_with_harness(harness, builder)
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
        create(self.engine.as_ref(), tenant_id.clone())
            .unwrap_or_else(|error| panic!("tenant should create: {error}"));
        tenant_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nimbus_core::Timestamp;
    use nimbus_storage::Clock;

    #[derive(Debug)]
    struct DummyEngine {
        scenario_name: String,
        seed: u64,
        now_ms: u64,
    }

    #[test]
    fn new_with_harness_passes_scenario_context_to_the_builder() {
        let harness = DeterministicHarness::scenario("fixture-builder", 19, Timestamp(12_345));
        let fixture = EngineFixture::new_with_harness(harness.clone(), |path, harness| {
            assert!(path.exists(), "fixture tempdir should already exist");
            Ok::<DummyEngine, std::convert::Infallible>(DummyEngine {
                scenario_name: harness.name().to_string(),
                seed: harness.seed(),
                now_ms: harness.clock().now().0,
            })
        });

        let engine = fixture.engine();
        assert_eq!(engine.scenario_name, "fixture-builder");
        assert_eq!(engine.seed, 19);
        assert_eq!(engine.now_ms, 12_345);
        assert!(fixture.data_dir().exists());
    }
}
