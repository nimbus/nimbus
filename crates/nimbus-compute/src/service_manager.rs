use std::sync::Arc;

use nimbus_core::TenantId;
use nimbus_engine::Engine;
use nimbus_sandbox::SandboxHandle;
use nimbus_services::{ServiceEvidenceFuture, ServiceEvidenceWriter, ServiceManager};

struct SystemTenantServiceEvidenceWriter {
    engine: Arc<Engine>,
}

impl SystemTenantServiceEvidenceWriter {
    fn new(engine: Arc<Engine>) -> Self {
        Self { engine }
    }
}

impl ServiceEvidenceWriter for SystemTenantServiceEvidenceWriter {
    fn record_service_handle<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        handle: &'a SandboxHandle,
    ) -> ServiceEvidenceFuture<'a> {
        Box::pin(async move {
            nimbus_system::record_service_handle_async(&self.engine, tenant_id, handle).await
        })
    }
}

pub fn attach_system_state_engine(manager: &ServiceManager, engine: Arc<Engine>) {
    manager
        .set_service_evidence_writer_arc(Arc::new(SystemTenantServiceEvidenceWriter::new(engine)));
}
