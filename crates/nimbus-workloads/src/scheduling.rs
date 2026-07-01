use std::collections::{BTreeMap, BTreeSet};

use nimbus_core::Error;

use super::{DesiredWorkload, validate_component};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeCapacity {
    node_id: String,
    available_slots: u32,
    binding_keys: BTreeSet<String>,
    /// Per-tenant network segments the node can still carve from its super-net
    /// (the MTN6 address-pool dimension). `u32::MAX` means unbounded / not
    /// segment-gated; `0` means the node's super-net is exhausted and it must not
    /// admit a workload that needs a fresh tenant segment (fail-closed placement).
    remaining_segments: u32,
}

impl NodeCapacity {
    pub fn new(node_id: impl Into<String>, available_slots: u32) -> Result<Self, Error> {
        Ok(Self {
            node_id: validate_component("node id", node_id)?,
            available_slots,
            binding_keys: BTreeSet::new(),
            remaining_segments: u32::MAX,
        })
    }

    pub fn with_binding_key(mut self, binding_key: impl Into<String>) -> Result<Self, Error> {
        self.binding_keys
            .insert(validate_component("binding key", binding_key)?);
        Ok(self)
    }

    /// Set the number of per-tenant network segments the node can still carve
    /// (the MTN6 remaining-segment dimension). A node reporting `0` is rejected
    /// for placement because it cannot allocate a new tenant a bridge subnet.
    pub fn with_remaining_segments(mut self, remaining_segments: u32) -> Self {
        self.remaining_segments = remaining_segments;
        self
    }

    pub fn remaining_segments(&self) -> u32 {
        self.remaining_segments
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementPlan {
    workload_id: String,
    node_id: Option<String>,
    explanation: SchedulingExplanation,
}

impl PlacementPlan {
    pub fn workload_id(&self) -> &str {
        &self.workload_id
    }

    pub fn node_id(&self) -> Option<&str> {
        self.node_id.as_deref()
    }

    pub fn explanation(&self) -> &SchedulingExplanation {
        &self.explanation
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulingExplanation {
    selected_node: Option<String>,
    rejected_nodes: BTreeMap<String, String>,
    reason: String,
}

impl SchedulingExplanation {
    pub fn selected_node(&self) -> Option<&str> {
        self.selected_node.as_deref()
    }

    pub fn rejected_nodes(&self) -> &BTreeMap<String, String> {
        &self.rejected_nodes
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Debug, Default)]
pub struct WorkloadPlacementEngine;

impl WorkloadPlacementEngine {
    pub fn place(&self, workload: &DesiredWorkload, nodes: &[NodeCapacity]) -> PlacementPlan {
        let mut selected_node = None;
        let mut rejected_nodes = BTreeMap::new();
        for node in nodes {
            if node.available_slots == 0 {
                rejected_nodes.insert(node.node_id.clone(), "node has no free slots".to_owned());
                continue;
            }
            if node.remaining_segments == 0 {
                rejected_nodes.insert(
                    node.node_id.clone(),
                    "node network-segment pool is exhausted (no per-tenant subnet available)"
                        .to_owned(),
                );
                continue;
            }
            if let Some(binding_key) = workload.binding_key()
                && node.binding_keys.contains(binding_key)
            {
                rejected_nodes.insert(
                    node.node_id.clone(),
                    format!("binding key `{binding_key}` is already reserved"),
                );
                continue;
            }
            match selected_node.as_ref() {
                Some(current) if current <= &node.node_id => {}
                _ => selected_node = Some(node.node_id.clone()),
            }
        }
        let reason = selected_node
            .as_ref()
            .map(|node| format!("selected node `{node}` by deterministic id order"))
            .unwrap_or_else(|| "no feasible node".to_owned());
        PlacementPlan {
            workload_id: workload.workload_id().to_owned(),
            node_id: selected_node.clone(),
            explanation: SchedulingExplanation {
                selected_node,
                rejected_nodes,
                reason,
            },
        }
    }
}

#[derive(Debug, Default)]
pub struct WorkloadScheduler {
    placement: WorkloadPlacementEngine,
}

impl WorkloadScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn schedule(&self, workload: &DesiredWorkload, nodes: &[NodeCapacity]) -> PlacementPlan {
        self.placement.place(workload, nodes)
    }
}

#[cfg(test)]
mod tests {
    use nimbus_core::TenantId;

    use super::*;
    use crate::{DesiredWorkload, DesiredWorkloadState};

    fn tenant_id() -> TenantId {
        TenantId::new("tenant-a").expect("tenant id should parse")
    }

    #[test]
    fn generated_workload_placement() {
        let workload =
            DesiredWorkload::service(tenant_id(), "api", DesiredWorkloadState::Running, 1)
                .expect("desired workload should build");
        let scheduler = WorkloadScheduler::new();
        let plan = scheduler.schedule(
            &workload,
            &[
                NodeCapacity::new("node-c", 0).expect("node should build"),
                NodeCapacity::new("node-b", 2)
                    .expect("node should build")
                    .with_binding_key("service:api")
                    .expect("binding key should build"),
                NodeCapacity::new("node-a", 1).expect("node should build"),
            ],
        );

        assert_eq!(plan.workload_id(), "service:api");
        assert_eq!(plan.node_id(), Some("node-a"));
        assert_eq!(plan.explanation().selected_node(), Some("node-a"));
        assert_eq!(plan.explanation().rejected_nodes().len(), 2);
        assert!(
            plan.explanation()
                .rejected_nodes()
                .get("node-b")
                .expect("node-b should be rejected")
                .contains("already reserved")
        );
    }

    #[test]
    fn segment_exhausted_node_is_rejected_fail_closed() {
        let workload =
            DesiredWorkload::service(tenant_id(), "api", DesiredWorkloadState::Running, 1)
                .expect("desired workload should build");
        let scheduler = WorkloadScheduler::new();
        let plan = scheduler.schedule(
            &workload,
            &[
                // Free slots, but its per-tenant network-segment pool is exhausted:
                // it cannot carve a bridge subnet for a new tenant, so it is
                // rejected fail-closed even though it has slots.
                NodeCapacity::new("node-full", 4)
                    .expect("node should build")
                    .with_remaining_segments(0),
                // Has free segments -> eligible.
                NodeCapacity::new("node-ok", 1)
                    .expect("node should build")
                    .with_remaining_segments(3),
            ],
        );

        assert_eq!(plan.node_id(), Some("node-ok"));
        assert!(
            plan.explanation()
                .rejected_nodes()
                .get("node-full")
                .expect("segment-exhausted node should be rejected")
                .contains("segment pool is exhausted")
        );
        // The dimension defaults to unbounded so non-segment-aware callers are
        // unaffected.
        assert_eq!(
            NodeCapacity::new("n", 1)
                .expect("node should build")
                .remaining_segments(),
            u32::MAX
        );
    }
}
