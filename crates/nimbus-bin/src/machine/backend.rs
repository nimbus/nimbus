use nimbus::{
    Error, SandboxBackend, SandboxBackendKind, SandboxError, SandboxHandle, SandboxId,
    SandboxOciImageSource, SandboxRootSpec, SandboxSpec,
};
use nimbus_sandbox::SandboxFuture;

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
        if spec.service_name().is_none() {
            let message = format!(
                "forwarded machine API backend requires service-owned sandbox metadata for {}; standalone sandboxes are not supported through this backing-plane API",
                spec.display_name()
            );
            return Box::pin(async move { Err(SandboxError::InvalidSpec { message }) });
        }

        match &spec.root {
            SandboxRootSpec::Rootfs(_) => {
                let message = format!(
                    "forwarded machine API backend requires an OCI image root for service sandbox {}; rootfs starts are not supported through this backing-plane API",
                    spec.display_name()
                );
                Box::pin(async move { Err(SandboxError::InvalidSpec { message }) })
            }
            SandboxRootSpec::OciImage(image) => match &image.source {
                SandboxOciImageSource::Reference(_) => {
                    spawn_machine_api_operation(self.client.clone(), "image-start", move |client| {
                        client.start_service_sandbox_from_image(spec)
                    })
                }
                SandboxOciImageSource::Build(_) => {
                    spawn_machine_api_operation(self.client.clone(), "build-start", move |client| {
                        client.start_service_sandbox_from_build(spec)
                    })
                }
            },
        }
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
        | Error::HistoricalRead { .. }
        | Error::Serialization(_) => SandboxError::InvalidSpec { message: rendered },
        Error::ResourceExhausted(_)
        | Error::PermissionDenied(_)
        | Error::Storage { .. }
        | Error::Transport(_) => SandboxError::BackendUnavailable { message: rendered },
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
        | Error::NotFound(_)
        | Error::Internal(_) => SandboxError::OperationFailed { message: rendered },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use nimbus::{
        PublishedEndpoint, PublishedEndpointProtocol, SandboxBackend, SandboxBackendKind,
        SandboxError, SandboxHandle, SandboxId, SandboxOwnerSpec, SandboxPortBinding,
        SandboxProcessSpec, SandboxRootSpec, SandboxSpec, SandboxStatus, TenantId,
    };
    use nimbus_sandbox::SandboxFuture;
    use serde_json::json;
    use tempfile::{Builder, TempDir};

    use super::{ForwardedMachineApiSandboxBackend, MachineApiClient};
    use crate::machine::{
        MachineApiListenMode, MachineApiState, bind_direct_listener,
        default_guest_helper_binary_dirs, serve_machine_api,
    };

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn backend_round_trips_image_build_inspect_and_stop_over_machine_api() {
        let temp_dir = short_socket_tempdir();
        let socket_path = temp_dir.path().join("default-api.sock");
        let listener = bind_direct_listener(&socket_path).expect("listener should bind");
        let control_data_dir = temp_dir.path().join("control");
        let state_root = machine_api_container_state_root(&control_data_dir);
        let state = MachineApiState {
            control_data_dir,
            listen_mode: MachineApiListenMode::DirectSocket,
            binary_lookup_path: None,
            helper_binary_dirs: default_guest_helper_binary_dirs(),
            service_backend: Some(std::sync::Arc::new(
                StubMachineApiSandboxBackend::with_state_root(state_root),
            )),
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
            .start(image_spec(&tenant, "db", "docker://busybox:latest"))
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
            .start(build_spec(
                &tenant,
                "api",
                "api-image",
                "/Users/jack/src/github.com/nimbus/nimbus/Dockerfile",
                "/Users/jack/src/github.com/nimbus/nimbus",
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
            "/tmp/nimbus-missing.sock",
        ));
        let tenant = TenantId::new("tenant").expect("tenant should be valid");
        let error = backend
            .start(image_spec(&tenant, "db", "docker://busybox:latest"))
            .await
            .expect_err("missing socket should fail");
        assert!(
            matches!(error, SandboxError::BackendUnavailable { .. }),
            "{error:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn backend_rejects_rootfs_specs() {
        let backend = ForwardedMachineApiSandboxBackend::new(MachineApiClient::new(
            "/tmp/nimbus-unused.sock",
        ));
        let tenant = TenantId::new("tenant").expect("tenant should be valid");
        let error = backend
            .start(rootfs_spec(&tenant, "db"))
            .await
            .expect_err("rootfs specs should fail");
        assert!(
            matches!(error, SandboxError::InvalidSpec { .. }),
            "{error:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn backend_rejects_standalone_specs_before_machine_api_io() {
        let backend = ForwardedMachineApiSandboxBackend::new(MachineApiClient::new(
            "/tmp/nimbus-unused.sock",
        ));
        let tenant = TenantId::new("tenant").expect("tenant should be valid");
        let mut spec = image_spec(&tenant, "db", "docker://busybox:latest");
        spec.owner = SandboxOwnerSpec::standalone_named("scratch-db");

        let error = backend
            .start(spec)
            .await
            .expect_err("standalone specs should fail before machine API I/O");

        let SandboxError::InvalidSpec { message } = error else {
            panic!("expected InvalidSpec for standalone spec, got {error:?}");
        };
        assert!(
            message.contains("requires service-owned sandbox metadata"),
            "{message}"
        );
    }

    fn rootfs_spec(tenant: &TenantId, name: &str) -> SandboxSpec {
        SandboxSpec::new(
            tenant.clone(),
            SandboxOwnerSpec::service(name),
            SandboxBackendKind::Container,
            SandboxRootSpec::rootfs("/"),
            SandboxProcessSpec::new(["sleep", "60"]),
        )
        .with_port_binding(SandboxPortBinding::new(
            "http",
            PublishedEndpointProtocol::Http,
            18080,
            8080,
        ))
    }

    fn image_spec(tenant: &TenantId, name: &str, image_reference: &str) -> SandboxSpec {
        let mut spec = rootfs_spec(tenant, name);
        spec.root = SandboxRootSpec::oci_image_reference(image_reference);
        spec
    }

    fn build_spec(
        tenant: &TenantId,
        name: &str,
        image_name: &str,
        dockerfile_path: impl Into<std::path::PathBuf>,
        context_path: impl Into<std::path::PathBuf>,
    ) -> SandboxSpec {
        let mut spec = rootfs_spec(tenant, name);
        spec.root = SandboxRootSpec::oci_image_build(image_name, dockerfile_path, context_path);
        spec
    }

    fn short_socket_tempdir() -> TempDir {
        Builder::new()
            .prefix("nimbus-mac-")
            .tempdir_in("/tmp")
            .expect("short temp dir should exist")
    }

    #[derive(Default)]
    struct StubMachineApiSandboxBackend {
        next_id: AtomicUsize,
        handles: Mutex<BTreeMap<String, SandboxHandle>>,
        state_root: Option<PathBuf>,
    }

    fn machine_api_container_state_root(control_data_dir: &std::path::Path) -> PathBuf {
        control_data_dir
            .join("service-sandboxes")
            .join("container")
            .join("state")
    }

    impl StubMachineApiSandboxBackend {
        fn with_state_root(state_root: PathBuf) -> Self {
            Self {
                state_root: Some(state_root),
                ..Self::default()
            }
        }

        fn start_with_spec(&self, spec: &SandboxSpec) -> SandboxHandle {
            let sequence = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
            let service_name = spec.display_name().to_owned();
            let sandbox_id = SandboxId::new(format!("{service_name}-{sequence}"));
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
                spec.tenant_id.clone(),
                sandbox_id.clone(),
                service_name,
                SandboxBackendKind::Container,
                SandboxStatus::Ready,
                endpoints,
            );
            self.handles
                .lock()
                .expect("stub backend lock should not be poisoned")
                .insert(sandbox_id.as_str().to_owned(), handle.clone());
            if let Some(state_root) = self.state_root.as_ref() {
                write_stub_container_manifest(state_root, &handle, spec);
            }
            handle
        }

        fn remove_manifest(&self, id: &SandboxId) {
            let Some(state_root) = self.state_root.as_ref() else {
                return;
            };
            let Some(handle) = self
                .handles
                .lock()
                .expect("stub backend lock should not be poisoned")
                .get(id.as_str())
                .cloned()
            else {
                return;
            };
            let manifest_path = state_root
                .join("tenants")
                .join(handle.tenant_id.as_str())
                .join("sandboxes")
                .join(id.as_str())
                .join("state")
                .join("containers")
                .join(id.as_str())
                .join("manifest.json");
            let _ = fs::remove_file(manifest_path);
        }
    }

    impl SandboxBackend for StubMachineApiSandboxBackend {
        fn kind(&self) -> SandboxBackendKind {
            SandboxBackendKind::Container
        }

        fn start(&self, spec: SandboxSpec) -> SandboxFuture<SandboxHandle> {
            let handle = self.start_with_spec(&spec);
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
            self.remove_manifest(id);
            self.handles
                .lock()
                .expect("stub backend lock should not be poisoned")
                .remove(id.as_str());
            Box::pin(async move { Ok(()) })
        }
    }

    fn write_stub_container_manifest(
        state_root: &std::path::Path,
        handle: &SandboxHandle,
        spec: &SandboxSpec,
    ) {
        let container_dir = state_root
            .join("tenants")
            .join(handle.tenant_id.as_str())
            .join("sandboxes")
            .join(handle.id.as_str())
            .join("state")
            .join("containers")
            .join(handle.id.as_str());
        fs::create_dir_all(&container_dir).expect("stub manifest directory should exist");
        let manifest = json!({
            "handle": handle,
            "spec": spec,
            "conmon_layout": {
                "container_state_dir": container_dir,
                "ctr_log": container_dir.join("ctr.log"),
                "oci_log": container_dir.join("oci.log")
            },
            "last_exit_code": null,
            "shutdown_requested": false,
            "status": handle.status
        });

        fs::write(
            container_dir.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).expect("stub manifest should serialize"),
        )
        .expect("stub manifest should write");
    }
}
