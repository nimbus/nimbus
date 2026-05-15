use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::executor::block_on;
use nimbus_core::{Error, TenantId};
use nimbus_engine::Service;
use nimbus_runtime::{HostCallCancellation, InvocationServiceBinding, InvocationServices};
use nimbus_sandbox::{SandboxBackend, SandboxError, SandboxHandle, SandboxStatus};
use tokio::sync::Notify;
use tokio::time::sleep;

use crate::sandbox::{SandboxCatalog, SandboxServiceCatalog, SandboxServiceLaunch};
use crate::service_registry::{
    RuntimeServiceBindingFuture, RuntimeServiceRegistry, service_binding_from_handle,
};

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
    system_state_service: Mutex<Option<Arc<Service>>>,
    activation_notify: Notify,
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
            system_state_service: Mutex::new(None),
            activation_notify: Notify::new(),
        }
    }

    pub(crate) fn attach_system_state_service(&self, service: Arc<Service>) {
        *self
            .system_state_service
            .lock()
            .expect("system state service lock should not be poisoned") = Some(service);
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

    async fn refresh_handle_async(
        &self,
        key: &TenantServiceKey,
    ) -> Result<Option<SandboxHandle>, Error> {
        let Some(handle) = self.current_handle(key) else {
            return Ok(None);
        };
        let inspected = self
            .sandbox_backend
            .inspect(&handle.id)
            .await
            .map_err(|error| sandbox_backend_error(key, "inspect", &error))?;
        let refreshed = {
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
                    Some(handle)
                }
                None => {
                    state.handles.remove(key);
                    None
                }
            }
        };

        if let Some(handle) = refreshed.as_ref() {
            self.record_service_handle(key, handle).await?;
        }

        Ok(refreshed)
    }

    async fn claim_activation(&self, key: &TenantServiceKey) -> ActivationClaim {
        loop {
            let notified = self.activation_notify.notified();
            {
                let mut state = self
                    .state
                    .lock()
                    .expect("manager lock should not be poisoned");
                if state.handles.contains_key(key) {
                    return ActivationClaim::AlreadyActive;
                }
                if state.activations_in_progress.insert(key.clone()) {
                    return ActivationClaim::Claimed;
                }
            }
            notified.await;
        }
    }

    fn release_activation(&self, key: &TenantServiceKey) {
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        state.activations_in_progress.remove(key);
        self.activation_notify.notify_waiters();
    }

    async fn start_launch_async(
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
                self.sandbox_backend.start_from_image(launch).await
            }
            SandboxServiceLaunch::Build(launch) => {
                self.sandbox_backend.start_from_build(launch).await
            }
        }
        .map_err(|error| sandbox_backend_error(key, "start", &error))?;

        self.state
            .lock()
            .expect("manager lock should not be poisoned")
            .handles
            .insert(key.clone(), handle.clone());
        self.record_service_handle(key, &handle).await?;
        Ok(handle)
    }

    async fn wait_for_ready_handle_async(
        &self,
        key: &TenantServiceKey,
        cancellation: &HostCallCancellation,
    ) -> Result<Option<SandboxHandle>, Error> {
        let deadline = Instant::now() + self.activation_timeout;
        loop {
            if cancellation.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let Some(handle) = self.refresh_handle_async(key).await? else {
                return Ok(None);
            };
            if handle.status == SandboxStatus::Ready
                || service_binding_from_handle(&handle).is_some()
            {
                return Ok(Some(handle));
            }
            if matches!(
                handle.status,
                SandboxStatus::Stopped | SandboxStatus::Failed
            ) {
                return Ok(Some(handle));
            }
            if Instant::now() >= deadline {
                return Err(Error::ResourceExhausted(format!(
                    "sandbox service {} for tenant {} did not become ready within {:?}",
                    key.service_name, key.tenant_id, self.activation_timeout
                )));
            }
            tokio::select! {
                _ = cancellation.cancelled() => return Err(Error::Cancelled),
                _ = sleep(self.activation_poll_interval) => {}
            }
        }
    }

    pub(crate) async fn start_service_async(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
        cancellation: HostCallCancellation,
    ) -> Result<Option<SandboxHandle>, Error> {
        let key = TenantServiceKey::new(tenant_id, service_name);
        if let Some(handle) = self.refresh_handle_async(&key).await?
            && !matches!(
                handle.status,
                SandboxStatus::Stopped | SandboxStatus::Failed
            )
        {
            return self.wait_for_ready_handle_async(&key, &cancellation).await;
        }

        match self.claim_activation(&key).await {
            ActivationClaim::AlreadyActive => {
                self.wait_for_ready_handle_async(&key, &cancellation).await
            }
            ActivationClaim::Claimed => {
                let Some(launch) = self
                    .service_catalog
                    .sandbox_service_for_tenant(tenant_id, service_name)
                else {
                    self.release_activation(&key);
                    return Ok(None);
                };
                let start_result = self.start_launch_async(&key, launch).await;
                self.release_activation(&key);
                start_result?;
                self.wait_for_ready_handle_async(&key, &cancellation).await
            }
        }
    }

    pub(crate) async fn stop_service_async(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Result<Option<SandboxHandle>, Error> {
        let key = TenantServiceKey::new(tenant_id, service_name);
        let previous_handle = self.current_handle(&key);
        let refreshed_handle = self.refresh_handle_async(&key).await?;
        let handle_existed_in_backend = refreshed_handle.is_some();
        let Some(handle) = refreshed_handle.or(previous_handle) else {
            return Ok(None);
        };

        if handle_existed_in_backend
            && !matches!(
                handle.status,
                SandboxStatus::Stopped | SandboxStatus::Stopping
            )
        {
            self.sandbox_backend
                .stop(&handle.id)
                .await
                .map_err(|error| sandbox_backend_error(&key, "stop", &error))?;
        }

        let mut stopped_handle = handle;
        stopped_handle.status = SandboxStatus::Stopped;
        stopped_handle.published_endpoints.clear();

        {
            let mut state = self
                .state
                .lock()
                .expect("manager lock should not be poisoned");
            state.handles.remove(&key);
            state.activations_in_progress.remove(&key);
        }
        self.activation_notify.notify_waiters();
        self.record_service_handle(&key, &stopped_handle).await?;

        Ok(Some(stopped_handle))
    }

    pub(crate) async fn restart_service_async(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
        cancellation: HostCallCancellation,
    ) -> Result<Option<SandboxHandle>, Error> {
        self.stop_service_async(tenant_id, service_name).await?;
        self.start_service_async(tenant_id, service_name, cancellation)
            .await
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

    async fn record_service_handle(
        &self,
        key: &TenantServiceKey,
        handle: &SandboxHandle,
    ) -> Result<(), Error> {
        let service = self
            .system_state_service
            .lock()
            .expect("system state service lock should not be poisoned")
            .clone();
        let Some(service) = service else {
            return Ok(());
        };
        crate::system_tenant::record_service_handle_async(&service, &key.tenant_id, handle).await
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
            .current_handle(&key)
            .and_then(|handle| service_binding_from_handle(&handle)))
    }

    fn ensure_service_binding_async<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        service_name: &'a str,
        cancellation: HostCallCancellation,
    ) -> RuntimeServiceBindingFuture<'a> {
        Box::pin(async move {
            if let Some(binding) = self.resolve_service_binding(tenant_id, service_name)? {
                return Ok(Some(binding));
            }
            let Some(handle) = self
                .start_service_async(tenant_id, service_name, cancellation)
                .await?
            else {
                return Ok(None);
            };
            Ok(service_binding_from_handle(&handle))
        })
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
        self.activation_notify.notify_waiters();
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

    use axum::http::StatusCode;
    use nimbus_sandbox::{
        PublishedEndpoint, PublishedEndpointProtocol, SandboxBackendKind, SandboxBuildLaunchSpec,
        SandboxFilesystemSpec, SandboxFuture, SandboxHandle, SandboxId, SandboxImageLaunchSpec,
        SandboxProcessSpec, SandboxSpec,
    };
    use nimbus_testing::ServerFixture;
    use serde_json::json;

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
                vec![
                    PublishedEndpoint::new(
                        "postgres",
                        PublishedEndpointProtocol::Tcp,
                        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 15432),
                    )
                    .with_guest_port(5432),
                ]
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

    #[tokio::test]
    async fn ensure_service_binding_async_starts_declared_image_service_once() {
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
            .ensure_service_binding_async(&tenant_id, "db", HostCallCancellation::default())
            .await
            .expect("image-backed service activation should succeed")
            .expect("db binding should exist");

        assert_eq!(binding.host, "127.0.0.1");
        assert_eq!(binding.port, 15432);
        assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);
        assert_eq!(backend.build_starts.load(Ordering::SeqCst), 0);

        let second = manager
            .ensure_service_binding_async(&tenant_id, "db", HostCallCancellation::default())
            .await
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

    #[tokio::test]
    async fn ensure_service_binding_async_records_system_tenant_service_state() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let service = Arc::new(Service::new(temp.path()).expect("service should create"));
        crate::system_tenant::prepare_system_tenant_async(&service, None)
            .await
            .expect("system tenant should prepare");
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
            backend,
        )
        .with_activation_poll_interval(Duration::from_millis(1))
        .with_activation_timeout(Duration::from_secs(1));
        manager.attach_system_state_service(service.clone());

        manager
            .ensure_service_binding_async(&tenant_id, "db", HostCallCancellation::default())
            .await
            .expect("service activation should succeed")
            .expect("db binding should exist");

        let documents = service
            .list_documents_async(
                crate::system_tenant::system_tenant_id().expect("system id should parse"),
                nimbus_core::TableName::new("services").expect("table should parse"),
            )
            .await
            .expect("service state documents should list");
        assert_eq!(documents.len(), 1);
        let fields = &documents[0].fields;
        assert_eq!(fields.get("name"), Some(&serde_json::json!("db")));
        assert_eq!(fields.get("kind"), Some(&serde_json::json!("sandbox")));
        assert_eq!(fields.get("state"), Some(&serde_json::json!("ready")));
        assert_eq!(
            fields
                .get("health")
                .and_then(serde_json::Value::as_object)
                .and_then(|health| health.get("backend")),
            Some(&serde_json::json!("krun"))
        );
        assert_eq!(
            fields
                .get("endpoints")
                .and_then(serde_json::Value::as_array)
                .and_then(|endpoints| endpoints.first())
                .and_then(|endpoint| endpoint.get("port")),
            Some(&serde_json::json!(15432))
        );

        let ports = service
            .list_documents_async(
                crate::system_tenant::system_tenant_id().expect("system id should parse"),
                nimbus_core::TableName::new("ports").expect("table should parse"),
            )
            .await
            .expect("service ports should list");
        assert_eq!(ports.len(), 1);
        assert_eq!(
            ports[0].fields.get("serviceId"),
            Some(&json!("service:tenant:db"))
        );
        assert_eq!(ports[0].fields.get("hostPort"), Some(&json!(15432)));
        assert_eq!(ports[0].fields.get("guestPort"), Some(&json!(5432)));
        assert_eq!(ports[0].fields.get("state"), Some(&json!("ready")));
    }

    #[tokio::test]
    async fn local_admin_service_lifecycle_routes_start_stop_and_project_system_state() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let service = Arc::new(Service::new(temp.path()).expect("service should create"));
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = Arc::new(
            SandboxServiceManager::new(
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
            .with_activation_timeout(Duration::from_secs(1)),
        );
        let server = ServerFixture::start(
            crate::router::RouterBuildConfig::core(service.clone())
                .with_sandbox_service_manager(manager)
                .without_deploy_admin_token()
                .build(),
        )
        .await;

        let start = server
            .client()
            .post(server.http_url("/api/tenants/tenant/services/db/start"))
            .send()
            .await
            .expect("service start request should send");
        assert_eq!(start.status(), StatusCode::OK);
        let start_body = start
            .json::<serde_json::Value>()
            .await
            .expect("service start response should parse");
        assert_eq!(start_body["tenantId"], json!("tenant"));
        assert_eq!(start_body["name"], json!("db"));
        assert_eq!(start_body["state"], json!("ready"));
        assert_eq!(start_body["backend"], json!("krun"));
        assert_eq!(start_body["endpoints"][0]["port"], json!(15432));
        assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);

        let system_services = service
            .list_documents_async(
                crate::system_tenant::system_tenant_id().expect("system id should parse"),
                nimbus_core::TableName::new("services").expect("table should parse"),
            )
            .await
            .expect("system services should list after start");
        assert_eq!(system_services.len(), 1);
        assert_eq!(
            system_services[0].fields.get("tenantId"),
            Some(&json!("tenant"))
        );
        assert_eq!(
            system_services[0].fields.get("state"),
            Some(&json!("ready"))
        );

        let system_ports = service
            .list_documents_async(
                crate::system_tenant::system_tenant_id().expect("system id should parse"),
                nimbus_core::TableName::new("ports").expect("table should parse"),
            )
            .await
            .expect("system ports should list after start");
        assert_eq!(system_ports.len(), 1);
        assert_eq!(system_ports[0].fields.get("hostPort"), Some(&json!(15432)));
        assert_eq!(system_ports[0].fields.get("guestPort"), Some(&json!(5432)));
        assert_eq!(system_ports[0].fields.get("state"), Some(&json!("ready")));

        let stop = server
            .client()
            .post(server.http_url("/api/tenants/tenant/services/db/stop"))
            .send()
            .await
            .expect("service stop request should send");
        assert_eq!(stop.status(), StatusCode::OK);
        let stop_body = stop
            .json::<serde_json::Value>()
            .await
            .expect("service stop response should parse");
        assert_eq!(stop_body["state"], json!("stopped"));
        assert_eq!(stop_body["endpoints"], json!([]));
        assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 1);

        let system_services = service
            .list_documents_async(
                crate::system_tenant::system_tenant_id().expect("system id should parse"),
                nimbus_core::TableName::new("services").expect("table should parse"),
            )
            .await
            .expect("system services should list after stop");
        assert_eq!(system_services.len(), 1);
        assert_eq!(
            system_services[0].fields.get("state"),
            Some(&json!("stopped"))
        );
        assert_eq!(system_services[0].fields.get("endpoints"), Some(&json!([])));

        let system_ports = service
            .list_documents_async(
                crate::system_tenant::system_tenant_id().expect("system id should parse"),
                nimbus_core::TableName::new("ports").expect("table should parse"),
            )
            .await
            .expect("system ports should list after stop");
        assert!(
            system_ports.is_empty(),
            "stopping the service should remove stale service port documents: {system_ports:?}"
        );

        let events = service
            .list_documents_async(
                crate::system_tenant::system_tenant_id().expect("system id should parse"),
                nimbus_core::TableName::new("events").expect("table should parse"),
            )
            .await
            .expect("system events should list after service lifecycle actions");
        assert_eq!(events.len(), 2);
        let mut actual_events = events
            .iter()
            .map(|event| {
                assert_eq!(event.fields.get("source"), Some(&json!("service")));
                assert_eq!(event.fields.get("level"), Some(&json!("info")));
                assert!(
                    event
                        .fields
                        .get("createdAt")
                        .and_then(serde_json::Value::as_u64)
                        .is_some(),
                    "service lifecycle event should include createdAt: {event:?}"
                );
                (
                    event.fields["category"]
                        .as_str()
                        .expect("category should be a string")
                        .to_owned(),
                    event.fields["data"]["tenantId"]
                        .as_str()
                        .expect("tenantId should be a string")
                        .to_owned(),
                    event.fields["data"]["serviceName"]
                        .as_str()
                        .expect("serviceName should be a string")
                        .to_owned(),
                    event.fields["data"]["action"]
                        .as_str()
                        .expect("action should be a string")
                        .to_owned(),
                    event.fields["data"]["state"]
                        .as_str()
                        .expect("state should be a string")
                        .to_owned(),
                )
            })
            .collect::<Vec<_>>();
        actual_events.sort();
        assert_eq!(
            actual_events,
            vec![
                (
                    "service.lifecycle".to_owned(),
                    "tenant".to_owned(),
                    "db".to_owned(),
                    "start".to_owned(),
                    "ready".to_owned(),
                ),
                (
                    "service.lifecycle".to_owned(),
                    "tenant".to_owned(),
                    "db".to_owned(),
                    "stop".to_owned(),
                    "stopped".to_owned(),
                ),
            ]
        );
    }

    #[tokio::test]
    async fn ensure_service_binding_async_uses_build_launch_for_build_backed_service() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(1));
        let manager = SandboxServiceManager::new(
            Arc::new(StubSandboxServiceCatalog {
                launches: BTreeMap::from([(
                    "api".to_owned(),
                    SandboxServiceLaunch::build(SandboxBuildLaunchSpec::new(
                        sparse_image_spec("api"),
                        "nimbus-api",
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
            .ensure_service_binding_async(&tenant_id, "api", HostCallCancellation::default())
            .await
            .expect("build-backed service activation should succeed")
            .expect("api binding should exist");

        assert_eq!(binding.port, 15432);
        assert_eq!(backend.image_starts.load(Ordering::SeqCst), 0);
        assert_eq!(backend.build_starts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn ensure_service_binding_sync_lookup_stays_snapshot_only_for_missing_service() {
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
        );

        let binding = manager
            .resolve_service_binding(&tenant_id, "db")
            .expect("sync lookup should not fail");
        assert!(
            binding.is_none(),
            "missing in-memory bindings stay unresolved"
        );
        assert_eq!(
            backend.image_starts.load(Ordering::SeqCst),
            0,
            "sync lookup should not trigger sandbox activation"
        );
    }

    #[tokio::test]
    async fn ensure_service_binding_async_can_be_cancelled_while_waiting_for_readiness() {
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(StubSandboxBackend::new(usize::MAX));
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
        .with_activation_poll_interval(Duration::from_millis(5))
        .with_activation_timeout(Duration::from_secs(1));
        let cancellation = HostCallCancellation::default();
        let cancellation_handle = cancellation.clone();

        let task = tokio::spawn(async move {
            manager
                .ensure_service_binding_async(&tenant_id, "db", cancellation)
                .await
        });

        tokio::time::sleep(Duration::from_millis(20)).await;
        cancellation_handle.cancel();

        let result = task
            .await
            .expect("cancellation task should join")
            .expect_err("cancellation should interrupt activation");
        assert!(matches!(result, Error::Cancelled));
        assert_eq!(
            backend.image_starts.load(Ordering::SeqCst),
            1,
            "activation should still start before the readiness wait is canceled"
        );
    }

    #[tokio::test]
    async fn teardown_tenant_stops_tracked_sandboxes_and_clears_snapshot() {
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
            .ensure_service_binding_async(&tenant_id, "db", HostCallCancellation::default())
            .await
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
