use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use futures::executor::block_on;
use neovex_core::{Error, TenantId};
use neovex_runtime::{InvocationServiceBinding, InvocationServices};
use neovex_sandbox::{SandboxBackend, SandboxError, SandboxHandle, SandboxStatus};

use crate::sandbox::{SandboxCatalog, SandboxServiceCatalog, SandboxServiceLaunch};
use crate::service_registry::{RuntimeServiceRegistry, service_binding_from_handle};

const DEFAULT_ACTIVATION_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_ACTIVATION_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TenantServiceKey {
    tenant_id: TenantId,
    service_name: String,
}

impl TenantServiceKey {
    fn new(tenant_id: &TenantId, service_name: &str) -> Self {
        Self {
            tenant_id: tenant_id.clone(),
            service_name: service_name.to_owned(),
        }
    }
}

#[derive(Default)]
struct SandboxServiceManagerState {
    handles: BTreeMap<TenantServiceKey, SandboxHandle>,
    activations_in_progress: BTreeSet<TenantServiceKey>,
}

pub struct SandboxServiceManager {
    service_catalog: Arc<dyn SandboxServiceCatalog>,
    sandbox_backend: Arc<dyn SandboxBackend>,
    activation_timeout: Duration,
    activation_poll_interval: Duration,
    state: Mutex<SandboxServiceManagerState>,
    activation_cv: Condvar,
}

impl SandboxServiceManager {
    pub fn new(
        service_catalog: Arc<dyn SandboxServiceCatalog>,
        sandbox_backend: Arc<dyn SandboxBackend>,
    ) -> Self {
        Self {
            service_catalog,
            sandbox_backend,
            activation_timeout: DEFAULT_ACTIVATION_TIMEOUT,
            activation_poll_interval: DEFAULT_ACTIVATION_POLL_INTERVAL,
            state: Mutex::new(SandboxServiceManagerState::default()),
            activation_cv: Condvar::new(),
        }
    }

    pub fn with_activation_timeout(mut self, activation_timeout: Duration) -> Self {
        self.activation_timeout = activation_timeout;
        self
    }

    pub fn with_activation_poll_interval(mut self, activation_poll_interval: Duration) -> Self {
        self.activation_poll_interval = activation_poll_interval;
        self
    }

    fn current_handle(&self, key: &TenantServiceKey) -> Option<SandboxHandle> {
        self.state
            .lock()
            .expect("manager lock should not be poisoned")
            .handles
            .get(key)
            .cloned()
    }

    fn refresh_handle(&self, key: &TenantServiceKey) -> Result<Option<SandboxHandle>, Error> {
        let Some(handle) = self.current_handle(key) else {
            return Ok(None);
        };
        let inspected = block_on(self.sandbox_backend.inspect(&handle.id))
            .map_err(|error| sandbox_backend_error(key, "inspect", &error))?;
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        match inspected {
            Some(handle) => {
                if matches!(
                    handle.status,
                    SandboxStatus::Stopped | SandboxStatus::Failed
                ) {
                    state.handles.remove(key);
                } else {
                    state.handles.insert(key.clone(), handle.clone());
                }
                Ok(Some(handle))
            }
            None => {
                state.handles.remove(key);
                Ok(None)
            }
        }
    }

    fn claim_activation(&self, key: &TenantServiceKey) -> ActivationClaim {
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        loop {
            if state.handles.contains_key(key) {
                return ActivationClaim::AlreadyActive;
            }
            if state.activations_in_progress.insert(key.clone()) {
                return ActivationClaim::Claimed;
            }
            state = self
                .activation_cv
                .wait(state)
                .expect("manager lock should not be poisoned");
        }
    }

    fn release_activation(&self, key: &TenantServiceKey) {
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        state.activations_in_progress.remove(key);
        self.activation_cv.notify_all();
    }

    fn start_launch(
        &self,
        key: &TenantServiceKey,
        launch: SandboxServiceLaunch,
    ) -> Result<SandboxHandle, Error> {
        let requested_backend = launch.spec().backend;
        let actual_backend = self.sandbox_backend.kind();
        if requested_backend != actual_backend {
            return Err(Error::InvalidInput(format!(
                "sandbox service {} for tenant {} requested backend {:?}, but the configured manager backend is {:?}",
                key.service_name, key.tenant_id, requested_backend, actual_backend
            )));
        }
        if launch.spec().name != key.service_name {
            return Err(Error::InvalidInput(format!(
                "sandbox service catalog returned launch spec name {} for requested service {}",
                launch.spec().name,
                key.service_name
            )));
        }
        if launch.spec().tenant_id != key.tenant_id {
            return Err(Error::InvalidInput(format!(
                "sandbox service catalog returned tenant {} for requested tenant {}",
                launch.spec().tenant_id,
                key.tenant_id
            )));
        }

        let handle = match launch {
            SandboxServiceLaunch::Image(launch) => {
                block_on(self.sandbox_backend.start_from_image(launch))
            }
            SandboxServiceLaunch::Build(launch) => {
                block_on(self.sandbox_backend.start_from_build(launch))
            }
        }
        .map_err(|error| sandbox_backend_error(key, "start", &error))?;

        self.state
            .lock()
            .expect("manager lock should not be poisoned")
            .handles
            .insert(key.clone(), handle.clone());
        Ok(handle)
    }

    fn wait_for_binding(
        &self,
        key: &TenantServiceKey,
    ) -> Result<Option<InvocationServiceBinding>, Error> {
        let deadline = Instant::now() + self.activation_timeout;
        loop {
            let Some(handle) = self.refresh_handle(key)? else {
                return Ok(None);
            };
            if let Some(binding) = service_binding_from_handle(&handle) {
                return Ok(Some(binding));
            }
            if matches!(
                handle.status,
                SandboxStatus::Stopped | SandboxStatus::Failed
            ) {
                return Ok(None);
            }
            if Instant::now() >= deadline {
                return Err(Error::ResourceExhausted(format!(
                    "sandbox service {} for tenant {} did not become ready within {:?}",
                    key.service_name, key.tenant_id, self.activation_timeout
                )));
            }
            thread::sleep(self.activation_poll_interval);
        }
    }

    fn tenant_handles(&self, tenant_id: &TenantId) -> Vec<(TenantServiceKey, SandboxHandle)> {
        self.state
            .lock()
            .expect("manager lock should not be poisoned")
            .handles
            .iter()
            .filter(|(key, _)| &key.tenant_id == tenant_id)
            .map(|(key, handle)| (key.clone(), handle.clone()))
            .collect()
    }
}

impl SandboxCatalog for SandboxServiceManager {
    fn sandboxes_for_tenant(&self, tenant_id: &TenantId) -> BTreeMap<String, SandboxHandle> {
        let keys = {
            self.state
                .lock()
                .expect("manager lock should not be poisoned")
                .handles
                .keys()
                .filter(|key| &key.tenant_id == tenant_id)
                .cloned()
                .collect::<Vec<_>>()
        };

        keys.into_iter()
            .filter_map(|key| {
                self.refresh_handle(&key)
                    .ok()
                    .flatten()
                    .filter(|handle| {
                        !matches!(
                            handle.status,
                            SandboxStatus::Stopped | SandboxStatus::Failed
                        )
                    })
                    .map(|handle| (key.service_name.clone(), handle))
            })
            .collect()
    }
}

impl RuntimeServiceRegistry for SandboxServiceManager {
    fn snapshot_for_tenant(&self, tenant_id: &TenantId) -> InvocationServices {
        self.sandboxes_for_tenant(tenant_id)
            .into_iter()
            .filter_map(|(service_name, handle)| {
                service_binding_from_handle(&handle).map(|binding| (service_name, binding))
            })
            .collect()
    }

    fn resolve_service_binding(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Result<Option<InvocationServiceBinding>, Error> {
        let key = TenantServiceKey::new(tenant_id, service_name);
        Ok(self
            .refresh_handle(&key)?
            .and_then(|handle| service_binding_from_handle(&handle)))
    }

    fn ensure_service_binding(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Result<Option<InvocationServiceBinding>, Error> {
        if let Some(binding) = self.resolve_service_binding(tenant_id, service_name)? {
            return Ok(Some(binding));
        }

        let key = TenantServiceKey::new(tenant_id, service_name);
        match self.claim_activation(&key) {
            ActivationClaim::AlreadyActive => self.wait_for_binding(&key),
            ActivationClaim::Claimed => {
                let Some(launch) = self
                    .service_catalog
                    .sandbox_service_for_tenant(tenant_id, service_name)
                else {
                    self.release_activation(&key);
                    return Ok(None);
                };
                let start_result = self.start_launch(&key, launch);
                self.release_activation(&key);
                start_result?;
                self.wait_for_binding(&key)
            }
        }
    }

    fn teardown_tenant(&self, tenant_id: &TenantId) -> Result<(), Error> {
        let tenant_handles = self.tenant_handles(tenant_id);
        for (key, handle) in &tenant_handles {
            block_on(self.sandbox_backend.stop(&handle.id))
                .map_err(|error| sandbox_backend_error(key, "stop", &error))?;
        }

        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        for (key, _) in tenant_handles {
            state.handles.remove(&key);
            state.activations_in_progress.remove(&key);
        }
        self.activation_cv.notify_all();
        Ok(())
    }
}

enum ActivationClaim {
    Claimed,
    AlreadyActive,
}

fn sandbox_backend_error(key: &TenantServiceKey, operation: &str, error: &SandboxError) -> Error {
    Error::Internal(format!(
        "failed to {operation} sandbox service {} for tenant {}: {error}",
        key.service_name, key.tenant_id
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use neovex_sandbox::{
        PublishedEndpoint, PublishedEndpointProtocol, SandboxBackendKind, SandboxBuildLaunchSpec,
        SandboxFilesystemSpec, SandboxFuture, SandboxHandle, SandboxId, SandboxImageLaunchSpec,
        SandboxProcessSpec, SandboxSpec,
    };

    use super::*;

    struct StubSandboxServiceCatalog {
        launches: BTreeMap<String, SandboxServiceLaunch>,
    }

    impl SandboxServiceCatalog for StubSandboxServiceCatalog {
        fn sandbox_service_for_tenant(
            &self,
            _tenant_id: &TenantId,
            service_name: &str,
        ) -> Option<SandboxServiceLaunch> {
            self.launches.get(service_name).cloned()
        }
    }

    struct StubSandboxBackend {
        image_starts: AtomicUsize,
        build_starts: AtomicUsize,
        stop_calls: AtomicUsize,
        inspect_calls: AtomicUsize,
        ready_after_inspects: usize,
        handles: Mutex<BTreeMap<String, SandboxHandle>>,
    }

    impl StubSandboxBackend {
        fn new(ready_after_inspects: usize) -> Self {
            Self {
                image_starts: AtomicUsize::new(0),
                build_starts: AtomicUsize::new(0),
                stop_calls: AtomicUsize::new(0),
                inspect_calls: AtomicUsize::new(0),
                ready_after_inspects,
                handles: Mutex::new(BTreeMap::new()),
            }
        }

        fn sandbox_handle(&self, service_name: &str, status: SandboxStatus) -> SandboxHandle {
            let endpoints = if status == SandboxStatus::Ready {
                vec![PublishedEndpoint::new(
                    "postgres",
                    PublishedEndpointProtocol::Tcp,
                    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 15432),
                )]
            } else {
                Vec::new()
            };
            SandboxHandle::new(
                SandboxId::new(format!("sandbox-{service_name}")),
                service_name,
                SandboxBackendKind::Krun,
                status,
                endpoints,
            )
        }
    }

    impl SandboxBackend for StubSandboxBackend {
        fn kind(&self) -> SandboxBackendKind {
            SandboxBackendKind::Krun
        }

        fn start(&self, spec: SandboxSpec) -> SandboxFuture<SandboxHandle> {
            Box::pin(async move {
                Err(SandboxError::InvalidSpec {
                    message: format!("rootfs launch unsupported for {}", spec.name),
                })
            })
        }

        fn start_from_image(&self, launch: SandboxImageLaunchSpec) -> SandboxFuture<SandboxHandle> {
            self.image_starts.fetch_add(1, Ordering::SeqCst);
            let handle = self.sandbox_handle(&launch.spec.name, SandboxStatus::Starting);
            self.handles
                .lock()
                .expect("backend lock should not be poisoned")
                .insert(handle.id.as_str().to_owned(), handle.clone());
            Box::pin(async move { Ok(handle) })
        }

        fn start_from_build(&self, launch: SandboxBuildLaunchSpec) -> SandboxFuture<SandboxHandle> {
            self.build_starts.fetch_add(1, Ordering::SeqCst);
            let handle = self.sandbox_handle(&launch.spec.name, SandboxStatus::Starting);
            self.handles
                .lock()
                .expect("backend lock should not be poisoned")
                .insert(handle.id.as_str().to_owned(), handle.clone());
            Box::pin(async move { Ok(handle) })
        }

        fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<SandboxHandle>> {
            let inspect_call = self.inspect_calls.fetch_add(1, Ordering::SeqCst) + 1;
            let mut handles = self
                .handles
                .lock()
                .expect("backend lock should not be poisoned");
            let handle = handles.get_mut(id.as_str()).cloned().map(|mut handle| {
                if inspect_call >= self.ready_after_inspects {
                    handle = self.sandbox_handle(&handle.name, SandboxStatus::Ready);
                    handles.insert(id.as_str().to_owned(), handle.clone());
                }
                handle
            });
            Box::pin(async move { Ok(handle) })
        }

        fn stop(&self, id: &SandboxId) -> SandboxFuture<()> {
            self.stop_calls.fetch_add(1, Ordering::SeqCst);
            self.handles
                .lock()
                .expect("backend lock should not be poisoned")
                .remove(id.as_str());
            Box::pin(async move { Ok(()) })
        }
    }

    fn sparse_image_spec(name: &str) -> SandboxSpec {
        SandboxSpec::new(
            TenantId::new("tenant").expect("tenant id should be valid"),
            name,
            SandboxBackendKind::Krun,
            SandboxFilesystemSpec::new(""),
            SandboxProcessSpec::new(Vec::<String>::new()),
        )
    }

    #[test]
    fn ensure_service_binding_starts_declared_image_service_once() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(2));
        let manager = SandboxServiceManager::new(
            Arc::new(StubSandboxServiceCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
                        sparse_image_spec("db"),
                        "postgres:16",
                    )),
                )]),
            }),
            backend.clone(),
        )
        .with_activation_poll_interval(Duration::from_millis(1))
        .with_activation_timeout(Duration::from_secs(1));

        let binding = manager
            .ensure_service_binding(&tenant_id, "db")
            .expect("image-backed service activation should succeed")
            .expect("db binding should exist");

        assert_eq!(binding.host, "127.0.0.1");
        assert_eq!(binding.port, 15432);
        assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);
        assert_eq!(backend.build_starts.load(Ordering::SeqCst), 0);

        let second = manager
            .ensure_service_binding(&tenant_id, "db")
            .expect("cached service activation should succeed")
            .expect("db binding should still exist");
        assert_eq!(second.port, 15432);
        assert_eq!(
            backend.image_starts.load(Ordering::SeqCst),
            1,
            "existing active handle should prevent duplicate starts"
        );

        let snapshot = manager.snapshot_for_tenant(&tenant_id);
        assert_eq!(
            snapshot
                .get("db")
                .expect("db binding should be in snapshot")
                .port,
            15432
        );
    }

    #[test]
    fn ensure_service_binding_uses_build_launch_for_build_backed_service() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = SandboxServiceManager::new(
            Arc::new(StubSandboxServiceCatalog {
                launches: BTreeMap::from([(
                    "api".to_owned(),
                    SandboxServiceLaunch::build(SandboxBuildLaunchSpec::new(
                        sparse_image_spec("api"),
                        "neovex-api",
                        "/workspace/Dockerfile",
                        "/workspace",
                    )),
                )]),
            }),
            backend.clone(),
        )
        .with_activation_poll_interval(Duration::from_millis(1))
        .with_activation_timeout(Duration::from_secs(1));

        let binding = manager
            .ensure_service_binding(&tenant_id, "api")
            .expect("build-backed service activation should succeed")
            .expect("api binding should exist");

        assert_eq!(binding.port, 15432);
        assert_eq!(backend.image_starts.load(Ordering::SeqCst), 0);
        assert_eq!(backend.build_starts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn teardown_tenant_stops_tracked_sandboxes_and_clears_snapshot() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = SandboxServiceManager::new(
            Arc::new(StubSandboxServiceCatalog {
                launches: BTreeMap::from([(
                    "db".to_owned(),
                    SandboxServiceLaunch::image(SandboxImageLaunchSpec::new(
                        sparse_image_spec("db"),
                        "postgres:16",
                    )),
                )]),
            }),
            backend.clone(),
        )
        .with_activation_poll_interval(Duration::from_millis(1))
        .with_activation_timeout(Duration::from_secs(1));

        manager
            .ensure_service_binding(&tenant_id, "db")
            .expect("service activation should succeed")
            .expect("db binding should exist");
        assert!(manager.snapshot_for_tenant(&tenant_id).contains_key("db"));

        manager
            .teardown_tenant(&tenant_id)
            .expect("tenant teardown should stop tracked sandboxes");

        assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 1);
        assert!(
            manager.snapshot_for_tenant(&tenant_id).is_empty(),
            "tenant teardown should clear manager snapshots"
        );
    }
}
