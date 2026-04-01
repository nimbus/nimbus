use std::sync::atomic::{AtomicU64, Ordering};

use crate::runtime::InvocationRequest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeInvocationContext {
    pub invocation_id: u64,
    pub function_name: String,
    pub kind: &'static str,
    pub is_top_level: bool,
    pub tenant_label: Option<String>,
    pub server_request_id: Option<String>,
}

impl RuntimeInvocationContext {
    pub fn top_level(request: &InvocationRequest) -> Self {
        Self::new(request, None, None)
    }

    pub fn top_level_for_tenant(
        request: &InvocationRequest,
        tenant_label: impl Into<String>,
    ) -> Self {
        Self::new(request, Some(tenant_label.into()), None)
    }

    pub fn top_level_for_tenant_and_request(
        request: &InvocationRequest,
        tenant_label: impl Into<String>,
        server_request_id: impl Into<String>,
    ) -> Self {
        Self::new(
            request,
            Some(tenant_label.into()),
            Some(server_request_id.into()),
        )
    }

    fn new(
        request: &InvocationRequest,
        tenant_label: Option<String>,
        server_request_id: Option<String>,
    ) -> Self {
        static NEXT_INVOCATION_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            invocation_id: NEXT_INVOCATION_ID.fetch_add(1, Ordering::Relaxed),
            function_name: request.function_name.clone(),
            kind: request.kind.as_str(),
            is_top_level: true,
            tenant_label,
            server_request_id,
        }
    }
}
