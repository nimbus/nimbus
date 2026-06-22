use crate::backends::v8::embedder::{JsRealm, JsRuntime};

pub(crate) fn destroy_fresh_realm(runtime: &mut JsRuntime, realm: JsRealm) {
    #[cfg(test)]
    test_probe::record_fresh_realm_destroy();

    realm.destroy(runtime.v8_isolate());
}

#[cfg(test)]
pub(crate) mod test_probe {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread::ThreadId;

    #[derive(Clone)]
    struct ActiveFreshRealmDestroyProbe {
        owner: ThreadId,
        count: Arc<AtomicU64>,
    }

    pub(crate) struct FreshRealmDestroyProbe {
        owner: ThreadId,
        count: Arc<AtomicU64>,
    }

    fn active_probe() -> &'static Mutex<Option<ActiveFreshRealmDestroyProbe>> {
        static ACTIVE_PROBE: OnceLock<Mutex<Option<ActiveFreshRealmDestroyProbe>>> =
            OnceLock::new();
        ACTIVE_PROBE.get_or_init(|| Mutex::new(None))
    }

    pub(crate) fn start_fresh_realm_destroy_probe() -> FreshRealmDestroyProbe {
        let owner = std::thread::current().id();
        let count = Arc::new(AtomicU64::new(0));
        let mut active = active_probe()
            .lock()
            .expect("fresh realm destroy probe lock should not be poisoned");
        assert!(
            active.is_none(),
            "fresh realm destroy probe already active in this test process"
        );
        *active = Some(ActiveFreshRealmDestroyProbe {
            owner,
            count: count.clone(),
        });
        FreshRealmDestroyProbe { owner, count }
    }

    impl FreshRealmDestroyProbe {
        pub(crate) fn count(&self) -> u64 {
            self.count.load(Ordering::SeqCst)
        }
    }

    impl Drop for FreshRealmDestroyProbe {
        fn drop(&mut self) {
            let mut active = active_probe()
                .lock()
                .expect("fresh realm destroy probe lock should not be poisoned");
            if active.as_ref().is_some_and(|probe| {
                probe.owner == self.owner && Arc::ptr_eq(&probe.count, &self.count)
            }) {
                *active = None;
            }
        }
    }

    pub(super) fn record_fresh_realm_destroy() {
        let owner = std::thread::current().id();
        let active = active_probe()
            .lock()
            .expect("fresh realm destroy probe lock should not be poisoned");
        if let Some(probe) = active.as_ref()
            && probe.owner == owner
        {
            probe.count.fetch_add(1, Ordering::SeqCst);
        }
    }
}
