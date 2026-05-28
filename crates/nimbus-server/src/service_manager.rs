use std::sync::Arc;

use nimbus_core::TenantId;
use nimbus_engine::Service;
use nimbus_sandbox::SandboxHandle;
use nimbus_services::{SandboxServiceManager, ServiceEvidenceFuture, ServiceEvidenceWriter};

struct SystemTenantServiceEvidenceWriter {
    service: Arc<Service>,
}

impl SystemTenantServiceEvidenceWriter {
    fn new(service: Arc<Service>) -> Self {
        Self { service }
    }
}

impl ServiceEvidenceWriter for SystemTenantServiceEvidenceWriter {
    fn record_service_handle<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        handle: &'a SandboxHandle,
    ) -> ServiceEvidenceFuture<'a> {
        Box::pin(async move {
            nimbus_system::record_service_handle_async(&self.service, tenant_id, handle).await
        })
    }
}

pub(crate) fn attach_system_state_service(manager: &SandboxServiceManager, service: Arc<Service>) {
    manager
        .set_service_evidence_writer_arc(Arc::new(SystemTenantServiceEvidenceWriter::new(service)));
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use axum::http::StatusCode;
    use nimbus_core::{TableName, TenantId};
    use nimbus_engine::Service;
    use nimbus_runtime::HostCallCancellation;
    use nimbus_sandbox::{
        PublishedEndpoint, PublishedEndpointProtocol, SandboxBackend, SandboxBackendKind,
        SandboxError, SandboxFilesystemSpec, SandboxFuture, SandboxHandle, SandboxId,
        SandboxImageLaunchSpec, SandboxProcessSpec, SandboxSpec, SandboxStatus,
    };
    use nimbus_services::{
        RuntimeServiceRegistry, SandboxServiceCatalog, SandboxServiceLaunch, SandboxServiceManager,
    };
    use nimbus_testing::ServerFixture;
    use serde_json::json;

    use super::attach_system_state_service;

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

    struct ReadySandboxBackend {
        image_starts: AtomicUsize,
        stop_calls: AtomicUsize,
    }

    impl ReadySandboxBackend {
        fn handle(
            tenant_id: &TenantId,
            service_name: &str,
            status: SandboxStatus,
        ) -> SandboxHandle {
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
                tenant_id.clone(),
                SandboxId::new(format!("sandbox-{tenant_id}-{service_name}")),
                service_name,
                SandboxBackendKind::Krun,
                status,
                endpoints,
            )
        }
    }

    impl SandboxBackend for ReadySandboxBackend {
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
            Box::pin(async move {
                Ok(Self::handle(
                    &launch.spec.tenant_id,
                    &launch.spec.name,
                    SandboxStatus::Ready,
                ))
            })
        }

        fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<SandboxHandle>> {
            let id = id.as_str().to_owned();
            Box::pin(async move {
                let parts = id.strip_prefix("sandbox-").unwrap_or(&id);
                let (tenant, service) = parts.rsplit_once('-').unwrap_or(("tenant", "db"));
                let tenant_id = TenantId::new(tenant).expect("test tenant id should parse");
                Ok(Some(Self::handle(
                    &tenant_id,
                    service,
                    SandboxStatus::Ready,
                )))
            })
        }

        fn stop(&self, _id: &SandboxId) -> SandboxFuture<()> {
            self.stop_calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(()) })
        }

        fn remove_tenant_artifacts(&self, _tenant_id: TenantId) -> SandboxFuture<()> {
            Box::pin(async { Ok(()) })
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

    fn service_manager(backend: Arc<ReadySandboxBackend>) -> Arc<SandboxServiceManager> {
        Arc::new(
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
                backend,
            )
            .with_activation_poll_interval(std::time::Duration::from_millis(1))
            .with_activation_timeout(std::time::Duration::from_secs(1)),
        )
    }

    #[tokio::test]
    async fn service_evidence_writer_records_observed_state_to_system_tenant() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let service = Arc::new(Service::new(temp.path()).expect("service should create"));
        nimbus_system::prepare_system_tenant_async(&service, None)
            .await
            .expect("system tenant should prepare");
        let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
        let backend = Arc::new(ReadySandboxBackend {
            image_starts: AtomicUsize::new(0),
            stop_calls: AtomicUsize::new(0),
        });
        let manager = service_manager(backend);
        attach_system_state_service(&manager, service.clone());

        manager
            .ensure_service_binding_async(&tenant_id, "db", HostCallCancellation::default())
            .await
            .expect("service activation should succeed")
            .expect("db binding should exist");

        let documents = service
            .list_documents_async(
                nimbus_system::system_tenant_id().expect("system id should parse"),
                TableName::new("services").expect("table should parse"),
            )
            .await
            .expect("service state documents should list");
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].fields.get("name"), Some(&json!("db")));
        assert_eq!(documents[0].fields.get("state"), Some(&json!("ready")));

        let ports = service
            .list_documents_async(
                nimbus_system::system_tenant_id().expect("system id should parse"),
                TableName::new("ports").expect("table should parse"),
            )
            .await
            .expect("service ports should list");
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].fields.get("tenantId"), Some(&json!("tenant")));
        assert_eq!(ports[0].fields.get("serviceName"), Some(&json!("db")));
        assert_eq!(ports[0].fields.get("hostPort"), Some(&json!(15432)));
        assert_eq!(ports[0].fields.get("guestPort"), Some(&json!(5432)));
    }

    #[tokio::test]
    async fn local_admin_service_lifecycle_routes_remain_server_owned() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let service = Arc::new(Service::new(temp.path()).expect("service should create"));
        let backend = Arc::new(ReadySandboxBackend {
            image_starts: AtomicUsize::new(0),
            stop_calls: AtomicUsize::new(0),
        });
        let server = ServerFixture::start(
            crate::router::RouterBuildConfig::core(service.clone())
                .with_sandbox_service_manager(service_manager(backend.clone()))
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
        assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);

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
        assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 1);
    }
}
