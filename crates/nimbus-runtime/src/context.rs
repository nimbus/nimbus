use std::sync::atomic::{AtomicU64, Ordering};

use crate::retained_state::{RuntimeDeploymentAuthorityLease, RuntimeOwnerLease};
use crate::runtime::InvocationRequest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeInvocationContext {
    pub invocation_id: u64,
    pub function_name: String,
    pub kind: &'static str,
    pub is_top_level: bool,
    pub bypasses_concurrency_limit: bool,
    pub tenant_label: Option<String>,
    pub(crate) runtime_owner_lease: Option<RuntimeOwnerLease>,
    pub(crate) deployment_authority_lease: Option<RuntimeDeploymentAuthorityLease>,
    pub server_request_id: Option<String>,
}

impl RuntimeInvocationContext {
    pub fn top_level(request: &InvocationRequest) -> Self {
        Self::new(request, None, None, None, true)
    }

    pub fn top_level_for_tenant(
        request: &InvocationRequest,
        tenant_label: impl Into<String>,
    ) -> Self {
        Self::new(request, Some(tenant_label.into()), None, None, true)
    }

    /// Creates a top-level invocation whose runtime reuse authority is
    /// independent of tenant-local worker routing.
    pub fn top_level_with_owner(
        request: &InvocationRequest,
        runtime_owner_lease: RuntimeOwnerLease,
    ) -> Self {
        Self::new(request, None, Some(runtime_owner_lease), None, true)
    }

    #[cfg(test)]
    pub(crate) fn top_level_for_tenant_for_test(
        request: &InvocationRequest,
        tenant_label: impl Into<String>,
    ) -> Self {
        let tenant_label = tenant_label.into();
        Self::top_level_for_tenant_with_owner(
            request,
            tenant_label.clone(),
            crate::test_support::runtime_owner_lease_for_test(&tenant_label),
        )
    }

    pub fn top_level_for_tenant_and_request(
        request: &InvocationRequest,
        tenant_label: impl Into<String>,
        server_request_id: impl Into<String>,
    ) -> Self {
        Self::new(
            request,
            Some(tenant_label.into()),
            None,
            Some(server_request_id.into()),
            true,
        )
    }

    /// Creates a request-correlated top-level invocation without introducing
    /// tenant routing locality.
    pub fn top_level_and_request_with_owner(
        request: &InvocationRequest,
        runtime_owner_lease: RuntimeOwnerLease,
        server_request_id: impl Into<String>,
    ) -> Self {
        Self::new(
            request,
            None,
            Some(runtime_owner_lease),
            Some(server_request_id.into()),
            true,
        )
    }

    #[cfg(test)]
    pub(crate) fn top_level_for_tenant_and_request_for_test(
        request: &InvocationRequest,
        tenant_label: impl Into<String>,
        server_request_id: impl Into<String>,
    ) -> Self {
        let tenant_label = tenant_label.into();
        Self::top_level_for_tenant_and_request_with_owner(
            request,
            tenant_label.clone(),
            crate::test_support::runtime_owner_lease_for_test(&tenant_label),
            server_request_id,
        )
    }

    pub fn top_level_for_tenant_with_owner(
        request: &InvocationRequest,
        tenant_label: impl Into<String>,
        runtime_owner_lease: RuntimeOwnerLease,
    ) -> Self {
        Self::new(
            request,
            Some(tenant_label.into()),
            Some(runtime_owner_lease),
            None,
            true,
        )
    }

    pub fn top_level_for_tenant_and_request_with_owner(
        request: &InvocationRequest,
        tenant_label: impl Into<String>,
        runtime_owner_lease: RuntimeOwnerLease,
        server_request_id: impl Into<String>,
    ) -> Self {
        Self::new(
            request,
            Some(tenant_label.into()),
            Some(runtime_owner_lease),
            Some(server_request_id.into()),
            true,
        )
    }

    pub fn nested_for_tenant(request: &InvocationRequest, tenant_label: impl Into<String>) -> Self {
        Self::new(request, Some(tenant_label.into()), None, None, false)
    }

    /// Creates a nested invocation whose runtime reuse authority does not
    /// imply tenant routing locality.
    pub fn nested_with_owner(
        request: &InvocationRequest,
        runtime_owner_lease: RuntimeOwnerLease,
    ) -> Self {
        Self::new(request, None, Some(runtime_owner_lease), None, false)
    }

    pub fn nested_for_tenant_and_request(
        request: &InvocationRequest,
        tenant_label: impl Into<String>,
        server_request_id: impl Into<String>,
    ) -> Self {
        Self::new(
            request,
            Some(tenant_label.into()),
            None,
            Some(server_request_id.into()),
            false,
        )
    }

    /// Creates a request-correlated nested invocation without introducing
    /// tenant routing locality.
    pub fn nested_and_request_with_owner(
        request: &InvocationRequest,
        runtime_owner_lease: RuntimeOwnerLease,
        server_request_id: impl Into<String>,
    ) -> Self {
        Self::new(
            request,
            None,
            Some(runtime_owner_lease),
            Some(server_request_id.into()),
            false,
        )
    }

    pub fn nested_for_tenant_with_owner(
        request: &InvocationRequest,
        tenant_label: impl Into<String>,
        runtime_owner_lease: RuntimeOwnerLease,
    ) -> Self {
        Self::new(
            request,
            Some(tenant_label.into()),
            Some(runtime_owner_lease),
            None,
            false,
        )
    }

    pub fn nested_for_tenant_and_request_with_owner(
        request: &InvocationRequest,
        tenant_label: impl Into<String>,
        runtime_owner_lease: RuntimeOwnerLease,
        server_request_id: impl Into<String>,
    ) -> Self {
        Self::new(
            request,
            Some(tenant_label.into()),
            Some(runtime_owner_lease),
            Some(server_request_id.into()),
            false,
        )
    }

    fn new(
        request: &InvocationRequest,
        tenant_label: Option<String>,
        runtime_owner_lease: Option<RuntimeOwnerLease>,
        server_request_id: Option<String>,
        is_top_level: bool,
    ) -> Self {
        static NEXT_INVOCATION_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            invocation_id: NEXT_INVOCATION_ID.fetch_add(1, Ordering::Relaxed),
            function_name: request.function_name.clone(),
            kind: request.kind.as_str(),
            is_top_level,
            bypasses_concurrency_limit: false,
            tenant_label,
            runtime_owner_lease,
            deployment_authority_lease: None,
            server_request_id,
        }
    }

    pub fn with_bypassed_concurrency_limit(mut self) -> Self {
        self.bypasses_concurrency_limit = true;
        self
    }

    pub fn with_deployment_authority(
        mut self,
        deployment_authority_lease: RuntimeDeploymentAuthorityLease,
    ) -> Self {
        self.deployment_authority_lease = Some(deployment_authority_lease);
        self
    }

    pub fn bypasses_concurrency_limit(&self) -> bool {
        self.bypasses_concurrency_limit
    }

    pub fn runtime_owner_lease(&self) -> Option<&RuntimeOwnerLease> {
        self.runtime_owner_lease.as_ref()
    }

    pub fn deployment_authority_lease(&self) -> Option<&RuntimeDeploymentAuthorityLease> {
        self.deployment_authority_lease.as_ref()
    }
}
