//! Compute-owned entrypoint for issued node-workload commands.
//!
//! The local reconcile state machine remains in `nimbus-node`. This owner is
//! deliberately thin until NNC6.1b-e add durable workload-saga vocabulary and
//! recovery. It cannot evaluate desired state, acquire network authority, or
//! write system projections.

use std::sync::Arc;

use nimbus_core::Result;
use nimbus_node::{
    HostLifecycleBackendCapabilities, HostLifecycleStatus, NodeAgentAssignment,
    NodeAgentReconcileReport, NodeWorkloadReconcileCapability, NodeWorkloadReconcileOutcome,
};

pub struct NodeWorkloadCoordinator {
    capability: Arc<dyn NodeWorkloadReconcileCapability>,
}

impl NodeWorkloadCoordinator {
    pub fn new(capability: Arc<dyn NodeWorkloadReconcileCapability>) -> Self {
        Self { capability }
    }

    pub fn backend_capabilities(&self) -> Vec<HostLifecycleBackendCapabilities> {
        self.capability.backend_capabilities()
    }

    pub async fn reconcile_assignment(
        &self,
        assignment: NodeAgentAssignment,
    ) -> Result<NodeWorkloadReconcileOutcome> {
        self.capability.reconcile_assignment(assignment).await
    }

    pub async fn reconcile_assignments(
        &self,
        assignments: impl IntoIterator<Item = NodeAgentAssignment>,
    ) -> Result<NodeAgentReconcileReport> {
        self.capability
            .reconcile_assignments(assignments.into_iter().collect())
            .await
    }

    pub async fn inspect_assignment(
        &self,
        assignment: &NodeAgentAssignment,
    ) -> Result<HostLifecycleStatus> {
        self.capability.inspect_assignment(assignment).await
    }
}
