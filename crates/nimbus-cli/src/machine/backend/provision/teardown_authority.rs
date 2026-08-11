//! Parent publication authority transferred from provision to teardown.

use nimbus_compute::workload_saga::ConfirmedWorkloadTeardownCommand;
use nimbus_compute::workload_saga::teardown_provider_command::ConfirmedTeardownProviderCommand;
use nimbus_machine::api::{
    MachineApiWorkloadTeardownPhaseRequest, MachineApiWorkloadTeardownPhaseResponse,
};
use nimbus_network::{PortLeasePhase, PortLeaseRecord};
use nimbus_sandbox::{
    MachinePortForwardOutcome, MachinePortForwardReceipt, MachinePortForwardingRetirement,
    MachinePortForwardingRetirementObservation, ProviderCommandObservationKind,
};
use serde::Serialize;

use crate::machine::publication_authority::ConfirmedMachinePublicationRetirementPhase;

use super::*;

impl ForwardedMachineProvisionAdapter {
    pub(in crate::machine::backend) fn authenticate_teardown_retirement(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
        authority: &MachineForwarderAuthority,
    ) -> Result<ConfirmedMachinePublicationRetirement, Error> {
        self.publication_journal
            .authenticate_teardown_command(command, authority)
    }

    pub(in crate::machine::backend) fn reconcile_withdrawn_parent_batch(
        &self,
        retirement: &ConfirmedMachinePublicationRetirement,
        command: &ConfirmedWorkloadTeardownCommand,
        provider: &ConfirmedTeardownProviderCommand,
        forwarding: &dyn MachinePortForwardingRetirement,
    ) -> Result<Vec<MachinePortForwardReceipt>, Error> {
        self.authenticate_forwarding_retirement(retirement, forwarding)?;
        let withdrawing = self
            .publication_journal
            .begin_parent_publication_withdrawal(retirement, command, provider)?;
        if withdrawing.members().is_empty() {
            let receipts = require_forwarding_absent(
                forwarding
                    .inspect_batch(
                        withdrawing.tenant_id(),
                        withdrawing.sandbox_id(),
                        withdrawing.expected_guest_bindings(),
                    )
                    .map_err(forwarding_error)?,
                &withdrawing,
            )?;
            self.publication_journal
                .record_parent_publication_withdrawn_retained(
                    &withdrawing,
                    command,
                    provider,
                    &receipts,
                    &[],
                )?;
            return Ok(receipts);
        }

        let requests = publication_requests(withdrawing.members());
        let plan_id = exact_parent_plan_id(&requests)?;
        let records = self
            .port_leases
            .list_plan(plan_id)
            .map_err(port_authority_error)?;
        authenticate_exact_durable_plan(&requests, &records)?;

        let live = self.take_live_publication_batch(plan_id, withdrawing.members())?;
        let (receipts, retained) = if let Some(live) = live {
            let receipts = withdraw_and_prove_absent(forwarding, &withdrawing)?;
            self.port_leases
                .withdraw_provider_managed_batch_with_lifetimes(&requests, live.lifetimes())
                .map_err(port_authority_error)?;
            drop(live);
            let recoveries = recover_dead_batch(&self.port_leases, &requests)?;
            let retained = self
                .port_leases
                .retain_provider_managed_batch_after_confirmed_absence(&requests, &recoveries)
                .map_err(port_authority_error)?;
            (receipts, retained)
        } else if exact_retained_batch(&records) {
            let receipts = require_forwarding_absent(
                forwarding
                    .inspect_batch(
                        withdrawing.tenant_id(),
                        withdrawing.sandbox_id(),
                        withdrawing.expected_guest_bindings(),
                    )
                    .map_err(forwarding_error)?,
                &withdrawing,
            )?;
            (receipts, records)
        } else {
            let recoveries = recover_dead_batch(&self.port_leases, &requests)?;
            let receipts = withdraw_and_prove_absent(forwarding, &withdrawing)?;
            let retained = self
                .port_leases
                .retain_provider_managed_batch_after_confirmed_absence(&requests, &recoveries)
                .map_err(port_authority_error)?;
            (receipts, retained)
        };

        self.publication_journal
            .record_parent_publication_withdrawn_retained(
                &withdrawing,
                command,
                provider,
                &receipts,
                &retained,
            )?;
        Ok(receipts)
    }

    pub(in crate::machine::backend) fn inspect_parent_withdrawal(
        &self,
        retirement: &ConfirmedMachinePublicationRetirement,
        forwarding: &dyn MachinePortForwardingRetirement,
    ) -> Result<(ProviderCommandObservationKind, Vec<u8>), Error> {
        self.authenticate_forwarding_retirement(retirement, forwarding)?;
        let forwarding_observation = forwarding
            .inspect_batch(
                retirement.tenant_id(),
                retirement.sandbox_id(),
                retirement.expected_guest_bindings(),
            )
            .map_err(forwarding_error)?;
        let requests = publication_requests(retirement.members());
        let records = if requests.is_empty() {
            Vec::new()
        } else {
            let plan_id = exact_parent_plan_id(&requests)?;
            self.port_leases
                .list_plan(plan_id)
                .map_err(port_authority_error)?
        };
        let forwarding_evidence = ForwardingInspectionEvidence::from(&forwarding_observation);
        let evidence = serde_json::to_vec(&(retirement.phase(), forwarding_evidence, &records))
            .map_err(|error| {
                Error::Internal(format!(
                    "failed to encode parent withdrawal inspection: {error}"
                ))
            })?;
        let kind = match retirement.phase() {
            ConfirmedMachinePublicationRetirementPhase::Active => match forwarding_observation {
                MachinePortForwardingRetirementObservation::Partial { .. }
                | MachinePortForwardingRetirementObservation::Present(_)
                | MachinePortForwardingRetirementObservation::Absent(_) => {
                    ProviderCommandObservationKind::RetryAuthorized
                }
            },
            ConfirmedMachinePublicationRetirementPhase::WithdrawalMayExist => {
                authenticate_exact_durable_plan(&requests, &records)?;
                match forwarding_observation {
                    MachinePortForwardingRetirementObservation::Partial { .. }
                    | MachinePortForwardingRetirementObservation::Present(_)
                    | MachinePortForwardingRetirementObservation::Absent(_) => {
                        ProviderCommandObservationKind::RetryAuthorized
                    }
                }
            }
            ConfirmedMachinePublicationRetirementPhase::WithdrawnRetained
            | ConfirmedMachinePublicationRetirementPhase::ReleaseMayExist => {
                authenticate_exact_durable_plan(&requests, &records)?;
                require_forwarding_absent(forwarding_observation, retirement)?;
                if !exact_retained_or_empty(&records, retirement.members()) {
                    return Err(Error::PreconditionFailed(
                        "parent publication inspection crosses its retained port phase".to_owned(),
                    ));
                }
                ProviderCommandObservationKind::Succeeded
            }
            ConfirmedMachinePublicationRetirementPhase::Released => {
                authenticate_exact_durable_plan(&requests, &records)?;
                require_forwarding_absent(forwarding_observation, retirement)?;
                if !exact_released_or_empty(&records, retirement.members()) {
                    return Err(Error::PreconditionFailed(
                        "parent publication inspection crosses its released port phase".to_owned(),
                    ));
                }
                ProviderCommandObservationKind::Succeeded
            }
        };
        Ok((kind, evidence))
    }

    pub(in crate::machine::backend) fn release_parent_batch_after_guest_release(
        &self,
        retirement: &ConfirmedMachinePublicationRetirement,
        command: &ConfirmedWorkloadTeardownCommand,
        provider: &ConfirmedTeardownProviderCommand,
        request: &MachineApiWorkloadTeardownPhaseRequest,
        response: &MachineApiWorkloadTeardownPhaseResponse,
        forwarding: &dyn MachinePortForwardingRetirement,
    ) -> Result<Vec<PortLeaseRecord>, Error> {
        self.authenticate_forwarding_retirement(retirement, forwarding)?;
        let releasing = self
            .publication_journal
            .begin_parent_publication_release(retirement, command, provider, request, response)?;
        require_forwarding_absent(
            forwarding
                .inspect_batch(
                    releasing.tenant_id(),
                    releasing.sandbox_id(),
                    releasing.expected_guest_bindings(),
                )
                .map_err(forwarding_error)?,
            &releasing,
        )?;
        let requests = publication_requests(releasing.members());
        let records = if requests.is_empty() {
            Vec::new()
        } else {
            let plan_id = exact_parent_plan_id(&requests)?;
            let current = self
                .port_leases
                .list_plan(plan_id)
                .map_err(port_authority_error)?;
            authenticate_exact_durable_plan(&requests, &current)?;
            if current
                .iter()
                .all(|record| record.phase() == PortLeasePhase::Released)
            {
                current
            } else if exact_retained_batch(&current) {
                self.port_leases
                    .release_retained_provider_managed_batch_after_confirmed_absence(&requests)
                    .map_err(port_authority_error)?
            } else {
                return Err(Error::PreconditionFailed(
                    "parent publication ports lack a complete cleanup-pending batch".to_owned(),
                ));
            }
        };
        self.publication_journal
            .record_parent_publication_released(&releasing, &records)?;
        Ok(records)
    }

    fn authenticate_forwarding_retirement(
        &self,
        retirement: &ConfirmedMachinePublicationRetirement,
        forwarding: &dyn MachinePortForwardingRetirement,
    ) -> Result<(), Error> {
        if retirement.forwarder_authority()
            != self
                .client
                .forwarder_authority()
                .map_err(|error| Error::PreconditionFailed(error.to_string()))?
            || forwarding.provider_instance()
                != retirement.forwarder_authority().provider_instance()
            || forwarding.provider_generation() != retirement.forwarder_authority().generation()
        {
            return Err(Error::PreconditionFailed(
                "parent forwarding retirement is crossed with its exact provider incarnation"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum ForwardingInspectionEvidence<'a> {
    Present {
        receipts: &'a [MachinePortForwardReceipt],
    },
    Partial {
        present: &'a [MachinePortForwardReceipt],
        absent: &'a [MachinePortForwardReceipt],
    },
    Absent {
        receipts: &'a [MachinePortForwardReceipt],
    },
}

impl<'a> From<&'a MachinePortForwardingRetirementObservation> for ForwardingInspectionEvidence<'a> {
    fn from(observation: &'a MachinePortForwardingRetirementObservation) -> Self {
        match observation {
            MachinePortForwardingRetirementObservation::Present(receipts) => {
                Self::Present { receipts }
            }
            MachinePortForwardingRetirementObservation::Partial { present, absent } => {
                Self::Partial { present, absent }
            }
            MachinePortForwardingRetirementObservation::Absent(receipts) => {
                Self::Absent { receipts }
            }
        }
    }
}

fn withdraw_and_prove_absent(
    forwarding: &dyn MachinePortForwardingRetirement,
    retirement: &ConfirmedMachinePublicationRetirement,
) -> Result<Vec<MachinePortForwardReceipt>, Error> {
    forwarding
        .withdraw_batch(
            retirement.tenant_id(),
            retirement.sandbox_id(),
            retirement.expected_guest_bindings(),
        )
        .map_err(forwarding_error)?;
    require_forwarding_absent(
        forwarding
            .inspect_batch(
                retirement.tenant_id(),
                retirement.sandbox_id(),
                retirement.expected_guest_bindings(),
            )
            .map_err(forwarding_error)?,
        retirement,
    )
}

fn forwarding_error(error: nimbus_sandbox::SandboxError) -> Error {
    Error::Internal(format!("parent forwarding retirement failed: {error}"))
}

fn require_forwarding_absent(
    observation: MachinePortForwardingRetirementObservation,
    retirement: &ConfirmedMachinePublicationRetirement,
) -> Result<Vec<MachinePortForwardReceipt>, Error> {
    match observation {
        MachinePortForwardingRetirementObservation::Absent(receipts)
            if exact_forwarding_absence_receipts(retirement, &receipts) =>
        {
            Ok(receipts)
        }
        MachinePortForwardingRetirementObservation::Present(_)
        | MachinePortForwardingRetirementObservation::Partial { .. }
        | MachinePortForwardingRetirementObservation::Absent(_) => Err(Error::PreconditionFailed(
            "parent forwarding batch is not completely and exactly absent".to_owned(),
        )),
    }
}

fn exact_forwarding_absence_receipts(
    retirement: &ConfirmedMachinePublicationRetirement,
    receipts: &[MachinePortForwardReceipt],
) -> bool {
    receipts.len() == retirement.expected_guest_bindings().len()
        && receipts.iter().all(|receipt| {
            matches!(
                receipt.outcome,
                MachinePortForwardOutcome::Withdrawn
                    | MachinePortForwardOutcome::ExactAlreadyAbsent
            ) && receipt.tenant_id == *retirement.tenant_id()
                && receipt.sandbox_id == *retirement.sandbox_id()
                && receipt.provider_instance
                    == *retirement.forwarder_authority().provider_instance()
                && receipt.provider_generation == retirement.forwarder_authority().generation()
                && retirement
                    .expected_guest_bindings()
                    .contains(&receipt.binding)
        })
        && retirement
            .expected_guest_bindings()
            .iter()
            .all(|binding| receipts.iter().any(|receipt| receipt.binding == *binding))
}

fn exact_parent_plan_id(
    requests: &[nimbus_network::PortLeaseRequest],
) -> Result<&NetworkPlanId, Error> {
    let plan_id = requests
        .first()
        .and_then(|request| request.plan_id())
        .ok_or_else(|| {
            Error::PreconditionFailed(
                "parent publication retirement lacks a canonical network plan".to_owned(),
            )
        })?;
    if requests
        .iter()
        .any(|request| request.plan_id() != Some(plan_id))
    {
        return Err(Error::PreconditionFailed(
            "parent publication retirement crosses canonical network plans".to_owned(),
        ));
    }
    Ok(plan_id)
}

fn exact_retained_batch(records: &[PortLeaseRecord]) -> bool {
    !records.is_empty()
        && records.iter().all(|record| {
            record.phase() == PortLeasePhase::CleanupPending
                && record.active_lifetime().is_none()
                && record.failure().is_none()
        })
}

fn exact_retained_or_empty(
    records: &[PortLeaseRecord],
    members: &[ConfirmedMachinePublicationMember],
) -> bool {
    (members.is_empty() && records.is_empty()) || exact_retained_batch(records)
}

fn exact_released_or_empty(
    records: &[PortLeaseRecord],
    members: &[ConfirmedMachinePublicationMember],
) -> bool {
    (members.is_empty() && records.is_empty())
        || (!records.is_empty()
            && records.iter().all(|record| {
                record.phase() == PortLeasePhase::Released && record.active_lifetime().is_none()
            }))
}
