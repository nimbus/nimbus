use std::future::Future;
use std::pin::Pin;

use nimbus_core::{Error, TenantId};
use nimbus_sandbox::SandboxHandle;

use super::SandboxServiceManager;
use super::types::TenantServiceKey;

pub type ServiceEvidenceFuture<'a> = Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>>;

pub trait ServiceEvidenceWriter: Send + Sync + 'static {
    fn record_service_handle<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        handle: &'a SandboxHandle,
    ) -> ServiceEvidenceFuture<'a>;
}

#[derive(Debug, Default)]
pub struct NoopServiceEvidenceWriter;

impl ServiceEvidenceWriter for NoopServiceEvidenceWriter {
    fn record_service_handle<'a>(
        &'a self,
        _tenant_id: &'a TenantId,
        _handle: &'a SandboxHandle,
    ) -> ServiceEvidenceFuture<'a> {
        Box::pin(async { Ok(()) })
    }
}

impl SandboxServiceManager {
    pub(super) async fn record_service_handle(
        &self,
        key: &TenantServiceKey,
        handle: &SandboxHandle,
    ) -> Result<(), Error> {
        let writer = self
            .service_evidence_writer
            .lock()
            .expect("service evidence writer lock should not be poisoned")
            .clone();
        writer.record_service_handle(&key.tenant_id, handle).await
    }
}
