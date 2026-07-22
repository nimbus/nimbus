use super::*;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) const PPSC_OBSERVER: &str = "ppsc-scenario-recorder";

#[derive(Clone)]
pub(super) enum PpscEngineFactory {
    Memory(PathBuf),
    Embedded(PathBuf, EmbeddedProviderKind),
    Configured(Box<EnginePersistenceConfig>),
}

impl PpscEngineFactory {
    pub(super) async fn build(
        &self,
        wall_clock: Arc<ManualWallClock>,
        monotonic_clock: Arc<ManualMonotonicClock>,
        storage_faults: Arc<PpscStorageFaultInjector>,
        id_source: Arc<dyn IdSource>,
    ) -> Arc<Engine> {
        Arc::new(
            match self {
                Self::Memory(data_dir) => {
                    Engine::new_with_simulation_clocks_and_memory_persistence(
                        data_dir,
                        wall_clock,
                        monotonic_clock,
                        storage_faults,
                        id_source,
                    )
                }
                Self::Embedded(data_dir, provider) => {
                    Engine::new_with_simulation_clocks_id_source_and_embedded_provider(
                        data_dir,
                        wall_clock,
                        monotonic_clock,
                        storage_faults,
                        id_source,
                        *provider,
                    )
                }
                Self::Configured(config) => {
                    Engine::new_with_simulation_clocks_id_source_and_persistence_config(
                        config.as_ref().clone(),
                        wall_clock,
                        monotonic_clock,
                        storage_faults,
                        id_source,
                    )
                    .await
                }
            }
            .expect("PPSC Engine should construct from its retained factory"),
        )
    }
}

pub(super) struct PpscEngineBootstrap {
    pub(super) data_dir: Option<TempDir>,
    pub(super) engine: Arc<Engine>,
    pub(super) engine_factory: PpscEngineFactory,
    pub(super) wall_clock: Arc<ManualWallClock>,
    pub(super) monotonic_clock: Arc<ManualMonotonicClock>,
    pub(super) storage_faults: Arc<PpscStorageFaultInjector>,
    pub(super) id_source: Arc<dyn IdSource>,
}

pub(super) struct PpscEngineSlot(Option<Arc<Engine>>);

impl PpscEngineSlot {
    pub(super) fn new(engine: Arc<Engine>) -> Self {
        Self(Some(engine))
    }

    pub(super) fn is_running(&self) -> bool {
        self.0.is_some()
    }

    pub(super) fn current(&self) -> Arc<Engine> {
        self.0
            .as_ref()
            .expect("PPSC operation requires a running Engine")
            .clone()
    }

    pub(super) fn crash(&mut self) {
        let engine = self.0.take().expect("PPSC crash requires a running Engine");
        drop(engine);
    }

    pub(super) fn reopen(&mut self, engine: Arc<Engine>) {
        assert!(self.0.is_none(), "PPSC reopen requires a crashed Engine");
        self.0 = Some(engine);
    }

    pub(super) fn replace(&mut self, engine: Arc<Engine>) -> Arc<Engine> {
        self.0
            .replace(engine)
            .expect("PPSC takeover requires a running Engine")
    }
}

impl Deref for PpscEngineSlot {
    type Target = Arc<Engine>;

    fn deref(&self) -> &Self::Target {
        self.0
            .as_ref()
            .expect("PPSC operation requires a running Engine")
    }
}

#[derive(Default)]
pub(super) struct PpscPublicationRecorder {
    current_step: AtomicUsize,
    observer_publications: Mutex<Vec<(TenantId, SequenceNumber, usize)>>,
    published_prefix: Mutex<BTreeMap<TenantId, BTreeMap<SequenceNumber, usize>>>,
}

impl PpscPublicationRecorder {
    pub(super) fn enter_step(&self, step: usize) {
        self.current_step.store(step, Ordering::Release);
    }

    pub(super) fn observe_published_prefix(
        &self,
        tenant_id: &TenantId,
        published_head: SequenceNumber,
        step: usize,
    ) {
        let mut prefixes = self
            .published_prefix
            .lock()
            .expect("PPSC published-prefix recorder lock should not be poisoned");
        let tenant_prefix = prefixes.entry(tenant_id.clone()).or_default();
        for sequence in 1..=published_head.0 {
            tenant_prefix
                .entry(SequenceNumber(sequence))
                .or_insert(step);
        }
    }

    pub(super) fn published_for_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Vec<(SequenceNumber, usize)> {
        self.published_prefix
            .lock()
            .expect("PPSC published-prefix recorder lock should not be poisoned")
            .get(tenant_id)
            .map(|prefix| {
                prefix
                    .iter()
                    .map(|(sequence, step)| (*sequence, *step))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn observer_for_tenant(&self, tenant_id: &TenantId) -> Vec<(SequenceNumber, usize)> {
        self.observer_publications
            .lock()
            .expect("PPSC observer recorder lock should not be poisoned")
            .iter()
            .filter_map(|(candidate, sequence, step)| {
                (candidate == tenant_id).then_some((*sequence, *step))
            })
            .collect()
    }
}

impl crate::CommittedMutationObserver for PpscPublicationRecorder {
    fn committed_mutation_applied(&self, event: crate::CommittedMutationEvent) {
        self.observer_publications
            .lock()
            .expect("PPSC publication recorder lock should not be poisoned")
            .push((
                event.tenant_id,
                event.commit.sequence,
                self.current_step.load(Ordering::Acquire),
            ));
    }
}
