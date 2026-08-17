//! Provider-neutral terminal authentication for separately owned publication.
//!
//! The attachment owner must not infer that every published listener belongs
//! to Netavark. When an upper composition owner publishes and withdraws the
//! listener, attachment teardown consumes only the terminal durable lease
//! evidence. It does not execute, release, or reinterpret that provider effect.

use nimbus_core::TenantId;
use nimbus_network::{PortLeasePhase, PortLeaseRecord, PortLeaseRequest};

use super::OciPortLeaseCoordinator;
use crate::error::{Result, SandboxError};
use crate::spec::SandboxPortBinding;

impl OciPortLeaseCoordinator {
    /// Authenticate an exact published-listener subset as terminal under its
    /// separate effect owner.
    ///
    /// Complete plan membership, tenant, binding intent, and selected port are
    /// checked in one host-global authority snapshot. Historical provider
    /// evidence is deliberately provider-neutral: terminal lifecycle state is
    /// the proof that the separate owner withdrew its effect.
    pub(crate) fn authenticate_separate_owner_publication_terminal(
        &self,
        plan_members: &[PortLeaseRequest],
        tenant_id: &TenantId,
        bindings: &[SandboxPortBinding],
        leases: &[PortLeaseRequest],
    ) -> Result<Vec<PortLeaseRecord>> {
        let records =
            self.planned_published_listener_records(plan_members, tenant_id, bindings, leases)?;
        for record in &records {
            if !matches!(
                record.phase(),
                PortLeasePhase::Released | PortLeasePhase::Failed
            ) || record.active_lifetime().is_some()
            {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "separately-owned publication lease {} remains {:?}; its effect owner must publish terminal authority before attachment detach",
                        record.request().lease_id(),
                        record.phase()
                    ),
                });
            }
        }
        Ok(records)
    }
}
