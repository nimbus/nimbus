use std::sync::{Arc, Mutex, OnceLock};

use pingora_core::server::configuration::ServerConf;
use tokio::runtime::{Builder, Handle, Runtime};

/// Shared scheduling infrastructure for egress proxy workers.
///
/// A substrate owns only runtime capacity and Pingora server configuration. It
/// must never carry policy, credential, CA, tenant, or other PEP identity state;
/// those isolation boundaries stay on each `EgressProxy`.
#[derive(Clone)]
pub struct ProxySubstrate {
    inner: Arc<ProxySubstrateInner>,
}

struct ProxySubstrateInner {
    handle: Handle,
    server_conf: Arc<ServerConf>,
    // Node-wide budget for concurrently *running* blocking DNS resolutions.
    // System resolvers are uncancellable once running, so without a budget a
    // wedged resolver could eat the shared blocking pool and starve sibling
    // PEPs. Capacity, not tenant state — it belongs to the substrate.
    dns_limiter: Arc<tokio::sync::Semaphore>,
    runtime: ProxySubstrateRuntime,
}

/// Concurrent blocking DNS resolutions allowed per substrate. Well above any
/// healthy steady state (resolution is milliseconds), low enough that wedged
/// resolver threads cannot exhaust tokio's blocking pool.
const SUBSTRATE_DNS_CONCURRENCY: usize = 32;

enum ProxySubstrateRuntime {
    // The runtime is held only to keep the shared executor alive for the
    // process lifetime; it is driven exclusively through `handle`.
    Shared { _runtime: Runtime },
    Dedicated(Mutex<Option<Runtime>>),
}

impl ProxySubstrate {
    pub fn shared() -> Self {
        static SHARED: OnceLock<ProxySubstrate> = OnceLock::new();
        SHARED
            .get_or_init(|| Self::new_shared(shared_worker_threads()))
            .clone()
    }

    pub fn dedicated(worker_threads: usize) -> Self {
        let runtime = build_runtime(worker_threads.max(1));
        let handle = runtime.handle().clone();
        Self {
            inner: Arc::new(ProxySubstrateInner {
                handle,
                server_conf: Arc::new(ServerConf::default()),
                dns_limiter: Arc::new(tokio::sync::Semaphore::new(SUBSTRATE_DNS_CONCURRENCY)),
                runtime: ProxySubstrateRuntime::Dedicated(Mutex::new(Some(runtime))),
            }),
        }
    }

    pub fn handle(&self) -> Handle {
        self.inner.handle.clone()
    }

    pub fn server_conf(&self) -> Arc<ServerConf> {
        Arc::clone(&self.inner.server_conf)
    }

    pub(crate) fn dns_limiter(&self) -> Arc<tokio::sync::Semaphore> {
        Arc::clone(&self.inner.dns_limiter)
    }

    fn new_shared(worker_threads: usize) -> Self {
        let runtime = build_runtime(worker_threads);
        let handle = runtime.handle().clone();
        Self {
            inner: Arc::new(ProxySubstrateInner {
                handle,
                server_conf: Arc::new(ServerConf::default()),
                dns_limiter: Arc::new(tokio::sync::Semaphore::new(SUBSTRATE_DNS_CONCURRENCY)),
                runtime: ProxySubstrateRuntime::Shared { _runtime: runtime },
            }),
        }
    }
}

impl Default for ProxySubstrate {
    fn default() -> Self {
        Self::shared()
    }
}

impl Drop for ProxySubstrateInner {
    fn drop(&mut self) {
        match &self.runtime {
            // The shared substrate lives in a process-wide static and is never
            // dropped; nothing to do here.
            ProxySubstrateRuntime::Shared { .. } => {}
            ProxySubstrateRuntime::Dedicated(runtime) => {
                if let Some(runtime) = runtime
                    .lock()
                    .expect("proxy substrate runtime lock should not be poisoned")
                    .take()
                {
                    // Never a blocking drop: a dedicated substrate may be
                    // dropped from inside an async context.
                    runtime.shutdown_background();
                }
            }
        }
    }
}

fn shared_worker_threads() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(2)
        .clamp(2, 8)
}

fn build_runtime(worker_threads: usize) -> Runtime {
    // Runtime construction only fails when the OS refuses to spawn threads —
    // process-level resource exhaustion with no sane degraded mode for a PEP.
    // Panicking here is the fail-closed choice: no runtime means no proxy, and
    // every sandbox launch gate already treats an absent PEP as deny.
    Builder::new_multi_thread()
        .worker_threads(worker_threads)
        .thread_name("nimbus-egress-proxy")
        .enable_all()
        .build()
        .expect("failed to start egress proxy runtime")
}
