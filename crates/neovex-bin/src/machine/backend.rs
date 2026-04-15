use neovex::{
    Error, SandboxBackend, SandboxBackendKind, SandboxBuildLaunchSpec, SandboxError, SandboxHandle,
    SandboxId, SandboxImageLaunchSpec, SandboxSpec,
};
use neovex_sandbox::SandboxFuture;

use super::client::MachineApiClient;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForwardedMachineApiSandboxBackend {
    client: MachineApiClient,
}

impl ForwardedMachineApiSandboxBackend {
    pub(crate) fn new(client: MachineApiClient) -> Self {
        Self { client }
    }
}

impl SandboxBackend for ForwardedMachineApiSandboxBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn start(&self, spec: SandboxSpec) -> SandboxFuture<SandboxHandle> {
        let message = format!(
            "forwarded machine API backend requires image/build launches, not bare spec {}",
            spec.name
        );
        Box::pin(async move { Err(SandboxError::InvalidSpec { message }) })
    }

    fn start_from_image(&self, launch: SandboxImageLaunchSpec) -> SandboxFuture<SandboxHandle> {
        spawn_machine_api_operation(self.client.clone(), "image-start", move |client| {
            client.start_service_sandbox_from_image(launch)
        })
    }

    fn start_from_build(&self, launch: SandboxBuildLaunchSpec) -> SandboxFuture<SandboxHandle> {
        spawn_machine_api_operation(self.client.clone(), "build-start", move |client| {
            client.start_service_sandbox_from_build(launch)
        })
    }

    fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<SandboxHandle>> {
        let sandbox_id = id.clone();
        spawn_machine_api_operation(self.client.clone(), "inspect", move |client| {
            client.inspect_service_sandbox(&sandbox_id)
        })
    }

    fn stop(&self, id: &SandboxId) -> SandboxFuture<()> {
        let sandbox_id = id.clone();
        spawn_machine_api_operation(self.client.clone(), "stop", move |client| {
            client.stop_service_sandbox(&sandbox_id)
        })
    }
}

fn spawn_machine_api_operation<T, F>(
    client: MachineApiClient,
    operation: &'static str,
    callback: F,
) -> SandboxFuture<T>
where
    T: Send + 'static,
    F: FnOnce(MachineApiClient) -> Result<T, Error> + Send + 'static,
{
    Box::pin(async move {
        tokio::task::spawn_blocking(move || callback(client))
            .await
            .map_err(|error| SandboxError::OperationFailed {
                message: format!("forwarded machine API {operation} task failed to join: {error}"),
            })?
            .map_err(machine_client_error_to_sandbox_error)
    })
}

fn machine_client_error_to_sandbox_error(error: Error) -> SandboxError {
    let rendered = error.to_string();
    match error {
        Error::InvalidInput(_)
        | Error::SchemaValidation(_)
        | Error::SchemaNotFound(_)
        | Error::Serialization(_) => SandboxError::InvalidSpec { message: rendered },
        Error::ResourceExhausted(_) | Error::PermissionDenied(_) | Error::Storage { .. } => {
            SandboxError::BackendUnavailable { message: rendered }
        }
        Error::Internal(message)
            if message.contains("failed to connect to machine API socket")
                || message.contains("failed to read machine API response")
                || message.contains("machine API response from")
                || message.contains("machine API request") =>
        {
            SandboxError::BackendUnavailable { message: rendered }
        }
        Error::AlreadyExists(_)
        | Error::Conflict(_)
        | Error::Cancelled
        | Error::TenantNotFound(_)
        | Error::DocumentNotFound(_)
        | Error::ScheduledJobNotFound(_)
        | Error::Internal(_) => SandboxError::OperationFailed { message: rendered },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use neovex::{
        PublishedEndpoint, PublishedEndpointProtocol, SandboxBackend, SandboxBackendKind,
        SandboxBuildLaunchSpec, SandboxError, SandboxFilesystemSpec, SandboxHandle, SandboxId,
        SandboxImageLaunchSpec, SandboxPortBinding, SandboxProcessSpec, SandboxSpec, SandboxStatus,
        TenantId,
    };
    use neovex_sandbox::SandboxFuture;
    use tempfile::{Builder, TempDir};

    use super::{ForwardedMachineApiSandboxBackend, MachineApiClient};
    use crate::machine::{
        MachineApiListenMode, MachineApiState, bind_direct_listener, serve_machine_api,
    };

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn backend_round_trips_image_build_inspect_and_stop_over_machine_api() {
        let temp_dir = short_socket_tempdir();
        let socket_path = temp_dir.path().join("default-api.sock");
        let listener = bind_direct_listener(&socket_path).expect("listener should bind");
        let state = MachineApiState {
            control_data_dir: temp_dir.path().join("control"),
            listen_mode: MachineApiListenMode::DirectSocket,
            binary_lookup_path: None,
            service_backend: Some(std::sync::Arc::new(StubMachineApiSandboxBackend::default())),
            machine_port_forwarder: None,
        };
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(serve_machine_api(listener, state, async move {
            let _ = shutdown_rx.await;
        }));

        let backend = ForwardedMachineApiSandboxBackend::new(MachineApiClient::new_for_test(
            socket_path.clone(),
        ));
        let tenant = TenantId::new("tenant").expect("tenant should be valid");
        let image_handle = backend
            .start_from_image(SandboxImageLaunchSpec::new(
                sample_spec(&tenant, "db"),
                "docker://busybox:latest",
            ))
            .await
            .expect("image-backed start should succeed");
        assert_eq!(image_handle.backend, SandboxBackendKind::Container);
        assert_eq!(image_handle.status, SandboxStatus::Ready);
        assert_eq!(image_handle.published_endpoints.len(), 1);

        let inspected = backend
            .inspect(&image_handle.id)
            .await
            .expect("inspect should succeed")
            .expect("handle should exist");
        assert_eq!(inspected, image_handle);

        backend
            .stop(&image_handle.id)
            .await
            .expect("stop should succeed");
        assert!(
            backend
                .inspect(&image_handle.id)
                .await
                .expect("inspect after stop should succeed")
                .is_none()
        );

        let build_handle = backend
            .start_from_build(SandboxBuildLaunchSpec::new(
                sample_spec(&tenant, "api"),
                "api-image",
                "/Users/jack/src/github.com/agentstation/neovex/Dockerfile",
                "/Users/jack/src/github.com/agentstation/neovex",
            ))
            .await
            .expect("build-backed start should succeed");
        assert_eq!(build_handle.name, "api");

        let _ = shutdown_tx.send(());
        server
            .await
            .expect("machine API server task should join")
            .expect("machine API server should shut down cleanly");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn backend_maps_missing_socket_to_backend_unavailable() {
        let backend = ForwardedMachineApiSandboxBackend::new(MachineApiClient::new(
            "/tmp/neovex-missing.sock",
        ));
        let tenant = TenantId::new("tenant").expect("tenant should be valid");
        let error = backend
            .start_from_image(SandboxImageLaunchSpec::new(
                sample_spec(&tenant, "db"),
                "docker://busybox:latest",
            ))
            .await
            .expect_err("missing socket should fail");
        assert!(
            matches!(error, SandboxError::BackendUnavailable { .. }),
            "{error:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn backend_rejects_bare_specs() {
        let backend = ForwardedMachineApiSandboxBackend::new(MachineApiClient::new(
            "/tmp/neovex-unused.sock",
        ));
        let tenant = TenantId::new("tenant").expect("tenant should be valid");
        let error = backend
            .start(sample_spec(&tenant, "db"))
            .await
            .expect_err("bare specs should fail");
        assert!(
            matches!(error, SandboxError::InvalidSpec { .. }),
            "{error:?}"
        );
    }

    fn sample_spec(tenant: &TenantId, name: &str) -> SandboxSpec {
        SandboxSpec::new(
            tenant.clone(),
            name,
            SandboxBackendKind::Container,
            SandboxFilesystemSpec::new("/"),
            SandboxProcessSpec::new(["sleep", "60"]),
        )
        .with_port_binding(SandboxPortBinding::new(
            "http",
            PublishedEndpointProtocol::Http,
            18080,
            8080,
        ))
    }

    fn short_socket_tempdir() -> TempDir {
        Builder::new()
            .prefix("neovex-mac-")
            .tempdir_in("/tmp")
            .expect("short temp dir should exist")
    }

    #[derive(Default)]
    struct StubMachineApiSandboxBackend {
        next_id: AtomicUsize,
        handles: Mutex<BTreeMap<String, SandboxHandle>>,
    }

    impl StubMachineApiSandboxBackend {
        fn start_with_spec(&self, spec: &SandboxSpec) -> SandboxHandle {
            let sequence = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
            let sandbox_id = SandboxId::new(format!("{}-{sequence}", spec.name));
            let endpoints = spec
                .port_bindings
                .iter()
                .map(|binding| {
                    PublishedEndpoint::new(
                        binding.name.clone(),
                        binding.protocol,
                        SocketAddr::new(
                            IpAddr::V4(Ipv4Addr::LOCALHOST),
                            binding.host_socket_addr().port(),
                        ),
                    )
                })
                .collect::<Vec<_>>();
            let handle = SandboxHandle::new(
                sandbox_id.clone(),
                spec.name.clone(),
                SandboxBackendKind::Container,
                SandboxStatus::Ready,
                endpoints,
            );
            self.handles
                .lock()
                .expect("stub backend lock should not be poisoned")
                .insert(sandbox_id.as_str().to_owned(), handle.clone());
            handle
        }
    }

    impl SandboxBackend for StubMachineApiSandboxBackend {
        fn kind(&self) -> SandboxBackendKind {
            SandboxBackendKind::Container
        }

        fn start(&self, spec: SandboxSpec) -> SandboxFuture<SandboxHandle> {
            let message = format!(
                "stub backend expects image/build launch, not bare spec {}",
                spec.name
            );
            Box::pin(async move { Err(SandboxError::InvalidSpec { message }) })
        }

        fn start_from_image(&self, launch: SandboxImageLaunchSpec) -> SandboxFuture<SandboxHandle> {
            let handle = self.start_with_spec(&launch.spec);
            Box::pin(async move { Ok(handle) })
        }

        fn start_from_build(&self, launch: SandboxBuildLaunchSpec) -> SandboxFuture<SandboxHandle> {
            let handle = self.start_with_spec(&launch.spec);
            Box::pin(async move { Ok(handle) })
        }

        fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<SandboxHandle>> {
            let handle = self
                .handles
                .lock()
                .expect("stub backend lock should not be poisoned")
                .get(id.as_str())
                .cloned();
            Box::pin(async move { Ok(handle) })
        }

        fn stop(&self, id: &SandboxId) -> SandboxFuture<()> {
            self.handles
                .lock()
                .expect("stub backend lock should not be poisoned")
                .remove(id.as_str());
            Box::pin(async move { Ok(()) })
        }
    }
}
