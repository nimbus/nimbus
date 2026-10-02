//! Exact durable-authority checks for listener and provider bindings.
//!
//! Each check compares one observed effect with the current durable lease. A
//! mismatch fails closed before the adapter acts on the effect.

use std::net::SocketAddr;
use std::num::NonZeroU16;

use nimbus_core::TenantId;
use nimbus_network::{
    ListenerId, LocalPortLeaseAuthority, NetworkResourceId, PortBindRealm, PortBindTarget,
    PortExposure, PortLeaseAccounting, PortLeaseId, PortLeasePhase, PortLeaseRecord,
    PortLeaseRequest, PortProtocol, PortPublicationIntent, PortRequestMode,
};

use super::{
    INITIAL_LEASE_EPOCH, INITIAL_RESOURCE_GENERATION, OciPortProvider, inspect_exact,
    provider_binding, target_for_ip,
};
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

/// Verify that a request exactly matches current durable bind authority.
///
/// `Active` is accepted for idempotent provider reconstruction after a
/// confirmed local teardown; NNC3.8 owns explicit crash/ambiguity
/// reconciliation for that reconstruction window.
pub(crate) fn require_current_bind_authority(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
) -> Result<PortLeaseRecord> {
    let record = inspect_exact(authority, request)?;
    if !matches!(
        record.phase(),
        PortLeasePhase::Reserved | PortLeasePhase::Binding | PortLeasePhase::Active
    ) {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox port lease {} is not current bind authority in phase {:?}",
                request.lease_id(),
                record.phase()
            ),
        });
    }
    Ok(record)
}

/// Logical listener identity and binding intent expected by one effect caller.
pub(crate) struct ExpectedListenerAuthority<'a> {
    tenant_id: &'a TenantId,
    sandbox_id: &'a SandboxId,
    listener_name: String,
    target: PortBindTarget,
    publication: PortPublicationIntent,
    exposure: PortExposure,
    accounting: PortLeaseAccounting,
    port: Option<NonZeroU16>,
}

impl<'a> ExpectedListenerAuthority<'a> {
    /// Expected authority for one externally published sandbox endpoint.
    pub(crate) fn published(
        tenant_id: &'a TenantId,
        sandbox_id: &'a SandboxId,
        listener_name: impl Into<String>,
        target: PortBindTarget,
        publication: PortPublicationIntent,
        exposure: PortExposure,
        port: NonZeroU16,
    ) -> Self {
        Self {
            tenant_id,
            sandbox_id,
            listener_name: listener_name.into(),
            target,
            publication,
            exposure,
            accounting: PortLeaseAccounting::TenantPublished,
            port: Some(port),
        }
    }

    /// Expected authority for the private per-sandbox egress PEP.
    pub(crate) fn egress_pep(
        tenant_id: &'a TenantId,
        sandbox_id: &'a SandboxId,
        bind_addr: SocketAddr,
    ) -> Result<Self> {
        Ok(Self {
            tenant_id,
            sandbox_id,
            listener_name: "egress-pep".to_owned(),
            target: target_for_ip(bind_addr.ip())?,
            publication: PortPublicationIntent::Unpublished,
            exposure: PortExposure::Private,
            accounting: PortLeaseAccounting::HostInternal,
            port: NonZeroU16::new(bind_addr.port()),
        })
    }
}

/// Verify that a persisted request belongs to the named sandbox listener.
///
/// Durable record equality alone is not enough: a corrupted or cross-tenant
/// manifest must not borrow another listener's otherwise-current authority.
/// The expected port is absent only for a provider-assigned port-zero bind.
pub(crate) fn require_listener_authority(
    authority: &LocalPortLeaseAuthority,
    expected: ExpectedListenerAuthority<'_>,
    request: &PortLeaseRequest,
) -> Result<PortLeaseRecord> {
    let listener_id = ListenerId::for_tenant_workload_listener(
        expected.tenant_id,
        expected.sandbox_id.as_str(),
        &expected.listener_name,
    );
    let expected_lease_id = PortLeaseId::for_listener(&listener_id);
    let expected_owner = NetworkResourceId::Listener(listener_id);
    let binding = request.binding();
    let identity_matches = request.lease_id() == &expected_lease_id
        && request.owner_id() == &expected_owner
        && request.tenant_id() == Some(expected.tenant_id)
        && request.generation() == INITIAL_RESOURCE_GENERATION
        && request.lease_epoch() == INITIAL_LEASE_EPOCH
        && request.accounting() == expected.accounting;
    let binding_matches = binding.protocol() == PortProtocol::Tcp
        && binding.realm() == &PortBindRealm::Host
        && binding.target() == &expected.target
        && request.publication() == &expected.publication
        && binding.exposure() == expected.exposure
        && match (expected.port, binding.port()) {
            (Some(port), PortRequestMode::Exact(expected)) => *expected == port,
            (Some(port), PortRequestMode::Range(range)) => {
                range.start() <= port && port <= range.end()
            }
            // Provider-assigned intent deliberately carries no port in the
            // request. Authenticate the concrete port against the durable
            // record below instead of rejecting the original request shape.
            (Some(_), PortRequestMode::ProviderAssigned) => true,
            (None, PortRequestMode::ProviderAssigned) => true,
            _ => false,
        };
    if !identity_matches || !binding_matches {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox listener {:?} rejected port lease {} because its owner, tenant, \
                generation, epoch, accounting, or binding intent does not match the caller",
                expected.listener_name,
                request.lease_id()
            ),
        });
    }

    let record = inspect_exact(authority, request)?;
    if let Some(expected_port) = expected.port
        && record.reserved_port() != Some(expected_port)
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox listener {:?} port lease {} does not own expected port {}",
                expected.listener_name,
                request.lease_id(),
                expected_port
            ),
        });
    }
    Ok(record)
}

/// Verify logical listener ownership plus a phase that may create or
/// reconstruct the named provider effect.
pub(crate) fn require_current_listener_authority(
    authority: &LocalPortLeaseAuthority,
    expected: ExpectedListenerAuthority<'_>,
    request: &PortLeaseRequest,
) -> Result<PortLeaseRecord> {
    let record = require_listener_authority(authority, expected, request)?;
    if !matches!(
        record.phase(),
        PortLeasePhase::Reserved | PortLeasePhase::Binding | PortLeasePhase::Active
    ) {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox port lease {} is not current bind authority in phase {:?}",
                request.lease_id(),
                record.phase()
            ),
        });
    }
    Ok(record)
}

/// Verify logical listener ownership and one exact Active provider binding.
pub(crate) fn require_active_listener_binding(
    authority: &LocalPortLeaseAuthority,
    expected: ExpectedListenerAuthority<'_>,
    request: &PortLeaseRequest,
    actual_addr: SocketAddr,
    provider: OciPortProvider,
) -> Result<PortLeaseRecord> {
    let record = require_listener_authority(authority, expected, request)?;
    let expected_binding = provider_binding(request, actual_addr, provider)?;
    if record.phase() != PortLeasePhase::Active || record.binding() != Some(&expected_binding) {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox port lease {} does not carry the exact Active {:?} listener binding",
                request.lease_id(),
                provider
            ),
        });
    }
    Ok(record)
}

/// Verify that one sandbox-owned provider effect has exact active evidence.
pub(crate) fn require_active_provider_binding(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    actual_addr: SocketAddr,
    provider: OciPortProvider,
) -> Result<PortLeaseRecord> {
    let record = inspect_exact(authority, request)?;
    let expected = provider_binding(request, actual_addr, provider)?;
    if record.phase() != PortLeasePhase::Active || record.binding() != Some(&expected) {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox port lease {} cannot start provider {:?}: expected exact Active binding \
                 evidence, found phase {:?}",
                request.lease_id(),
                provider,
                record.phase()
            ),
        });
    }
    Ok(record)
}

/// Verify exact provider evidence for final cleanup after durable withdrawal.
///
/// Final release may begin from `Active` or resume from `Withdrawing`; every
/// other phase either lacks a live effect or belongs to a different lifecycle
/// disposition. The exact binding remains mandatory in both accepted phases.
pub(crate) fn require_releasable_provider_binding(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    actual_addr: SocketAddr,
    provider: OciPortProvider,
) -> Result<PortLeaseRecord> {
    let record = inspect_exact(authority, request)?;
    let expected = provider_binding(request, actual_addr, provider)?;
    if !matches!(
        record.phase(),
        PortLeasePhase::Active | PortLeasePhase::Withdrawing
    ) || record.binding() != Some(&expected)
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox port lease {} cannot release provider {:?}: expected exact Active or \
                 Withdrawing binding evidence, found phase {:?}",
                request.lease_id(),
                provider,
                record.phase()
            ),
        });
    }
    Ok(record)
}

/// Verify exact provider evidence that may require owner-death recovery.
pub(crate) fn require_provider_recovery_binding(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    actual_addr: SocketAddr,
    provider: OciPortProvider,
) -> Result<PortLeaseRecord> {
    let record = inspect_exact(authority, request)?;
    let expected = provider_binding(request, actual_addr, provider)?;
    if !matches!(
        record.phase(),
        PortLeasePhase::Active | PortLeasePhase::Withdrawing | PortLeasePhase::CleanupPending
    ) || record.binding() != Some(&expected)
        || record.active_lifetime().is_none()
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox port lease {} cannot recover provider {:?}: expected exact Active, \
                 Withdrawing, or CleanupPending lifetime-fenced binding evidence, found phase {:?}",
                request.lease_id(),
                provider,
                record.phase()
            ),
        });
    }
    Ok(record)
}
