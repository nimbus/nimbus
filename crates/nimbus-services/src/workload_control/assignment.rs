use nimbus_core::{Error, TenantId};

use super::{WorkloadExecutionStatus, validate_component};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeAssignment {
    tenant_id: TenantId,
    node_id: String,
    workload_id: String,
    generation: u64,
}

impl NodeAssignment {
    pub fn new(
        tenant_id: TenantId,
        node_id: impl Into<String>,
        workload_id: impl Into<String>,
        generation: u64,
    ) -> Result<Self, Error> {
        Ok(Self {
            tenant_id,
            node_id: validate_component("node id", node_id)?,
            workload_id: validate_component("workload id", workload_id)?,
            generation,
        })
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    pub fn workload_id(&self) -> &str {
        &self.workload_id
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn accept_status(
        &self,
        update: WorkloadStatusUpdate,
    ) -> Result<WorkloadExecutionStatus, Error> {
        if update.tenant_id != self.tenant_id {
            return Err(Error::PermissionDenied(format!(
                "status tenant {} does not match assignment tenant {}",
                update.tenant_id, self.tenant_id
            )));
        }
        if update.node_id != self.node_id {
            return Err(Error::PermissionDenied(format!(
                "status node `{}` does not match assigned node `{}`",
                update.node_id, self.node_id
            )));
        }
        if update.workload_id != self.workload_id {
            return Err(Error::PermissionDenied(format!(
                "status workload `{}` does not match assigned workload `{}`",
                update.workload_id, self.workload_id
            )));
        }
        if update.status.workload_id() != self.workload_id {
            return Err(Error::PermissionDenied(format!(
                "status payload workload `{}` does not match assigned workload `{}`",
                update.status.workload_id(),
                self.workload_id
            )));
        }
        if update.observed_generation < self.generation {
            return Err(Error::Conflict(format!(
                "status generation {} is stale for assignment generation {}",
                update.observed_generation, self.generation
            )));
        }
        if update.observed_generation > self.generation {
            return Err(Error::Conflict(format!(
                "status generation {} is newer than assignment generation {}",
                update.observed_generation, self.generation
            )));
        }
        Ok(update.status)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadStatusUpdate {
    tenant_id: TenantId,
    node_id: String,
    workload_id: String,
    observed_generation: u64,
    status: WorkloadExecutionStatus,
}

impl WorkloadStatusUpdate {
    pub fn new(
        tenant_id: TenantId,
        node_id: impl Into<String>,
        workload_id: impl Into<String>,
        observed_generation: u64,
        status: WorkloadExecutionStatus,
    ) -> Result<Self, Error> {
        Ok(Self {
            tenant_id,
            node_id: validate_component("node id", node_id)?,
            workload_id: validate_component("workload id", workload_id)?,
            observed_generation,
            status,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WorkloadExecutionPhase;

    fn tenant_id() -> TenantId {
        TenantId::new("tenant-a").expect("tenant id should parse")
    }

    fn assignment() -> NodeAssignment {
        NodeAssignment::new(tenant_id(), "node-a", "service:api", 7)
            .expect("assignment should build")
    }

    fn status_update(
        tenant_id: TenantId,
        node_id: &str,
        workload_id: &str,
        observed_generation: u64,
    ) -> WorkloadStatusUpdate {
        WorkloadStatusUpdate::new(
            tenant_id,
            node_id,
            workload_id,
            observed_generation,
            WorkloadExecutionStatus::ready(workload_id).expect("status should build"),
        )
        .expect("status update should build")
    }

    #[test]
    fn status_acceptance_accepts_matching_assignment() {
        let accepted = assignment()
            .accept_status(status_update(tenant_id(), "node-a", "service:api", 7))
            .expect("matching status should be accepted");

        assert_eq!(accepted.workload_id(), "service:api");
        assert_eq!(accepted.phase(), WorkloadExecutionPhase::Ready);
    }

    #[test]
    fn status_acceptance_rejects_wrong_tenant() {
        let error = assignment()
            .accept_status(status_update(
                TenantId::new("tenant-b").expect("tenant id should parse"),
                "node-a",
                "service:api",
                7,
            ))
            .expect_err("status from another tenant must be rejected");

        assert!(
            error.to_string().contains("tenant-b"),
            "tenant mismatch should be visible: {error}"
        );
    }

    #[test]
    fn status_acceptance_rejects_wrong_node() {
        let error = assignment()
            .accept_status(status_update(tenant_id(), "node-b", "service:api", 7))
            .expect_err("status from another node must be rejected");

        assert!(
            error.to_string().contains("node-b"),
            "node mismatch should be visible: {error}"
        );
    }

    #[test]
    fn status_acceptance_rejects_wrong_workload() {
        let error = assignment()
            .accept_status(status_update(tenant_id(), "node-a", "service:worker", 7))
            .expect_err("status for another workload must be rejected");

        assert!(
            error.to_string().contains("service:worker"),
            "workload mismatch should be visible: {error}"
        );
    }

    #[test]
    fn status_acceptance_rejects_stale_generation() {
        let error = assignment()
            .accept_status(status_update(tenant_id(), "node-a", "service:api", 6))
            .expect_err("stale status generation must be rejected");

        assert!(
            error.to_string().contains("stale"),
            "stale generation should be visible: {error}"
        );
    }
}
