//! Exact durable membership authentication for planned port-lease batches.

use std::collections::BTreeMap;

use nimbus_core::TenantId;

use super::{PortLeaseOperationError, PortLeaseRequest, PortLeaseState, exact_record};
use crate::{NetworkLeaseEpoch, NetworkPlanId, NetworkResourceGeneration, PortLeaseId};

#[derive(Debug)]
struct PlanBatchKey {
    plan_id: NetworkPlanId,
    tenant_id: TenantId,
    generation: NetworkResourceGeneration,
    lease_epoch: NetworkLeaseEpoch,
}

impl PlanBatchKey {
    fn matches(&self, request: &PortLeaseRequest) -> bool {
        request.plan_id() == Some(&self.plan_id)
            && request.tenant_id() == Some(&self.tenant_id)
            && request.generation() == self.generation
            && request.lease_epoch() == self.lease_epoch
    }
}

/// Authenticate an initial provider-managed plan or an exact durable replay.
///
/// The first reservation establishes immutable membership. Once any member is
/// durable, neither a subset nor an extension can redefine the plan.
pub(super) fn authenticate_new_or_exact_plan_batch(
    state: &PortLeaseState,
    requests: &[&PortLeaseRequest],
) -> Result<(), PortLeaseOperationError> {
    let key = required_plan_key(requests)?;
    let supplied = distinct_requests(requests)?;
    let durable = durable_plan_members(state, &key);
    if !durable.is_empty() && durable != supplied {
        return Err(PortLeaseOperationError::PlanMembershipConflict {
            plan_id: key.plan_id,
        });
    }
    Ok(())
}

/// Authenticate an initial optional plan batch or an exact durable replay.
///
/// Ordinary all-standalone batches bypass plan membership. If any request is
/// planned, every request must share one exact plan fence and the complete
/// durable membership rules apply.
pub(super) fn authenticate_new_or_exact_plan_batch_if_present(
    state: &PortLeaseState,
    requests: &[&PortLeaseRequest],
) -> Result<(), PortLeaseOperationError> {
    let Some(key) = optional_plan_key(requests)? else {
        return Ok(());
    };
    let supplied = distinct_requests(requests)?;
    let durable = durable_plan_members(state, &key);
    if !durable.is_empty() && durable != supplied {
        return Err(PortLeaseOperationError::PlanMembershipConflict {
            plan_id: key.plan_id,
        });
    }
    Ok(())
}

/// Authenticate complete durable membership when requests carry a plan.
///
/// Legacy standalone batches remain legal only when every request omits plan
/// identity. Mixing planned and standalone requests fails closed.
pub(super) fn authenticate_complete_plan_batch_if_present(
    state: &PortLeaseState,
    requests: &[&PortLeaseRequest],
) -> Result<(), PortLeaseOperationError> {
    let Some(key) = optional_plan_key(requests)? else {
        return Ok(());
    };
    authenticate_complete(state, requests, key)
}

/// Authenticate one exact member against a caller-supplied complete plan.
///
/// The witness proves immutable membership without forcing lifecycle effects
/// for unrelated members into the same provider phase. Initial reservation
/// still establishes the whole set atomically; this seam only authorizes a
/// later mutation of a member that is already durable in that exact set.
pub(super) fn authenticate_complete_plan_member(
    state: &PortLeaseState,
    plan_members: &[&PortLeaseRequest],
    member: &PortLeaseRequest,
) -> Result<(), PortLeaseOperationError> {
    let key = required_plan_key(plan_members)?;
    authenticate_complete(state, plan_members, key)?;
    if !plan_members.contains(&member) {
        return Err(PortLeaseOperationError::PlanMembershipConflict {
            plan_id: member.plan_id().cloned().unwrap_or_else(|| {
                plan_members[0]
                    .plan_id()
                    .expect("required plan key")
                    .clone()
            }),
        });
    }
    exact_record(state, member)?;
    Ok(())
}

/// Authenticate a provider-owned subset against one complete durable plan.
///
/// The witness establishes immutable membership. The member set is required
/// to be identity-distinct and every member must match an exact witness entry,
/// but unrelated plan members remain outside the caller's lifecycle effect.
pub(super) fn authenticate_complete_plan_members(
    state: &PortLeaseState,
    plan_members: &[&PortLeaseRequest],
    members: &[&PortLeaseRequest],
) -> Result<(), PortLeaseOperationError> {
    let key = required_plan_key(plan_members)?;
    authenticate_complete(state, plan_members, key)?;
    let witness = distinct_requests(plan_members)?;
    let members = distinct_requests(members)?;
    for (lease_id, member) in members {
        if witness.get(&lease_id) != Some(&member) {
            return Err(PortLeaseOperationError::PlanMembershipConflict {
                plan_id: member
                    .plan_id()
                    .cloned()
                    .or_else(|| plan_members[0].plan_id().cloned())
                    .expect("complete plan witness has a plan identity"),
            });
        }
        exact_record(state, &member)?;
    }
    Ok(())
}

/// Authenticate one complete provider-managed plan.
pub(super) fn authenticate_complete_plan_batch(
    state: &PortLeaseState,
    requests: &[&PortLeaseRequest],
) -> Result<(), PortLeaseOperationError> {
    let key = required_plan_key(requests)?;
    authenticate_complete(state, requests, key)
}

/// Allow a scalar operation only for standalone or durable singleton plans.
///
/// Planned membership must first be established by an atomic batch
/// reservation. This prevents a scalar reserve from durably declaring the
/// first member of a larger intended plan and poisoning every complete retry.
pub(super) fn authenticate_scalar_plan_if_present(
    state: &PortLeaseState,
    request: &PortLeaseRequest,
) -> Result<(), PortLeaseOperationError> {
    let Some(plan_id) = request.plan_id() else {
        return Ok(());
    };
    let durable = state
        .leases
        .values()
        .filter(|record| record.request.plan_id() == Some(plan_id))
        .collect::<Vec<_>>();
    if durable.len() != 1 || durable[0].request() != request {
        return Err(PortLeaseOperationError::PlanMembershipConflict {
            plan_id: plan_id.clone(),
        });
    }
    Ok(())
}

fn authenticate_complete(
    state: &PortLeaseState,
    requests: &[&PortLeaseRequest],
    key: PlanBatchKey,
) -> Result<(), PortLeaseOperationError> {
    let supplied = distinct_requests(requests)?;
    if durable_plan_members(state, &key) != supplied {
        return Err(PortLeaseOperationError::PlanMembershipConflict {
            plan_id: key.plan_id,
        });
    }
    for request in requests {
        exact_record(state, request)?;
    }
    Ok(())
}

fn required_plan_key(
    requests: &[&PortLeaseRequest],
) -> Result<PlanBatchKey, PortLeaseOperationError> {
    let Some(first) = requests.first() else {
        return Err(PortLeaseOperationError::CorruptAuthority {
            reason: "provider-managed plan batch must contain at least one lease".to_owned(),
        });
    };
    let Some(plan_id) = first.plan_id().cloned() else {
        return Err(PortLeaseOperationError::PlanRequired {
            lease_id: first.lease_id().clone(),
        });
    };
    let Some(tenant_id) = first.tenant_id().cloned() else {
        return Err(PortLeaseOperationError::TenantAttributionRequired {
            lease_id: first.lease_id().clone(),
        });
    };
    let key = PlanBatchKey {
        plan_id,
        tenant_id,
        generation: first.generation(),
        lease_epoch: first.lease_epoch(),
    };
    for request in &requests[1..] {
        if request.plan_id().is_none() {
            return Err(PortLeaseOperationError::PlanRequired {
                lease_id: request.lease_id().clone(),
            });
        }
        if !key.matches(request) {
            return Err(PortLeaseOperationError::PlanMembershipConflict {
                plan_id: key.plan_id,
            });
        }
    }
    Ok(key)
}

fn optional_plan_key(
    requests: &[&PortLeaseRequest],
) -> Result<Option<PlanBatchKey>, PortLeaseOperationError> {
    let Some(first) = requests.first() else {
        return Ok(None);
    };
    if first.plan_id().is_some() {
        return required_plan_key(requests).map(Some);
    }
    if let Some(planned) = requests.iter().find(|request| request.plan_id().is_some()) {
        return Err(PortLeaseOperationError::PlanMembershipConflict {
            plan_id: planned
                .plan_id()
                .expect("planned request was selected")
                .clone(),
        });
    }
    Ok(None)
}

fn distinct_requests(
    requests: &[&PortLeaseRequest],
) -> Result<BTreeMap<PortLeaseId, PortLeaseRequest>, PortLeaseOperationError> {
    let mut distinct = BTreeMap::new();
    for request in requests {
        if distinct
            .insert(request.lease_id().clone(), (*request).clone())
            .is_some()
        {
            return Err(PortLeaseOperationError::IdentityConflict {
                lease_id: request.lease_id().clone(),
            });
        }
    }
    Ok(distinct)
}

fn durable_plan_members(
    state: &PortLeaseState,
    key: &PlanBatchKey,
) -> BTreeMap<PortLeaseId, PortLeaseRequest> {
    state
        .leases
        .iter()
        .filter(|(_, record)| record.request.plan_id() == Some(&key.plan_id))
        .map(|(lease_id, record)| (lease_id.clone(), record.request.clone()))
        .collect()
}
