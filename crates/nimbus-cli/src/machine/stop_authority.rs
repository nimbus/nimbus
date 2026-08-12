//! Compute-coordinated authorization for physical-machine stop effects.

use std::sync::Arc;

use nimbus::{Engine, Error};
use nimbus_compute::machine_stop_authority::{
    ConfirmedMachineStopAuthorization, MachineStopAdmissionBarrier, MachineStopAuthorizationError,
    MachineWorkloadAuthorityStore, authorize_physical_machine_stop,
};
use nimbus_machine::MachineForwarderAuthority;
use nimbus_server::EngineWorkloadSagaStore;

use super::backend::provision::forwarded_machine_execution_provider_id;
use super::network_composition::HostMachineNetworkAuthority;
use super::publication_authority::{
    ConfirmedMachinePublicationJournal, ConfirmedMachineStopBarrierAuthority,
};

#[derive(Clone)]
pub(super) struct HostMachineStopAuthority {
    barriers: ConfirmedMachineStopBarrierAuthority,
    workloads: Arc<dyn MachineWorkloadAuthorityStore>,
}

impl HostMachineStopAuthority {
    pub(super) fn new(
        network: &HostMachineNetworkAuthority,
        engine: Arc<Engine>,
    ) -> Result<Self, Error> {
        let journal = ConfirmedMachinePublicationJournal::open(network.port_leases().state_root())?;
        Ok(Self {
            barriers: ConfirmedMachineStopBarrierAuthority::new(journal),
            workloads: Arc::new(EngineWorkloadSagaStore::new(engine)),
        })
    }

    pub(super) async fn authorize(
        &self,
        machine_name: &str,
        forwarder_authority: &MachineForwarderAuthority,
    ) -> Result<ConfirmedMachineStopAuthorization, Error> {
        authorize_physical_machine_stop(
            &self.barriers,
            self.workloads.as_ref(),
            machine_name,
            forwarder_authority,
            &forwarded_machine_execution_provider_id(),
        )
        .await
        .map_err(map_authorization_error)
    }

    pub(super) fn begin_physical_stop(
        &self,
        authorization: &ConfirmedMachineStopAuthorization,
    ) -> Result<MachineStopAdmissionBarrier, Error> {
        if authorization.execution_provider_id() != &forwarded_machine_execution_provider_id() {
            return Err(Error::conflict(
                "physical-machine stop authorization names another execution provider",
            ));
        }
        self.barriers
            .journal()
            .begin_physical_machine_stop(authorization.barrier())
    }

    pub(super) async fn cancel_effect_free_stop(
        &self,
        authorization: &ConfirmedMachineStopAuthorization,
    ) -> Result<(), Error> {
        use nimbus_compute::machine_stop_authority::MachineStopBarrierAuthority as _;

        self.barriers
            .clear_effect_free_barrier(authorization.barrier())
            .await
            .map_err(|error| {
                Error::Internal(format!(
                    "failed to clear unused physical-machine stop fence: {error}"
                ))
            })
    }

    pub(super) fn record_physical_stop_absent(
        &self,
        barrier: &MachineStopAdmissionBarrier,
    ) -> Result<(), Error> {
        self.barriers
            .journal()
            .record_physical_machine_stop_absent(barrier)?;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn from_port_leases_for_test(
        port_leases: nimbus_network::LocalPortLeaseAuthority,
    ) -> Result<Self, Error> {
        Self::from_port_leases_and_workloads_for_test(
            port_leases,
            Arc::new(EmptyMachineWorkloadAuthorityStore),
        )
    }

    #[cfg(test)]
    pub(super) fn from_port_leases_and_workloads_for_test(
        port_leases: nimbus_network::LocalPortLeaseAuthority,
        workloads: Arc<dyn MachineWorkloadAuthorityStore>,
    ) -> Result<Self, Error> {
        let journal = ConfirmedMachinePublicationJournal::open(port_leases.state_root())?;
        Ok(Self {
            barriers: ConfirmedMachineStopBarrierAuthority::new(journal),
            workloads,
        })
    }
}

fn map_authorization_error(error: MachineStopAuthorizationError) -> Error {
    Error::RejectedBeforeExecution {
        message: error.to_string(),
    }
}

#[cfg(test)]
struct EmptyMachineWorkloadAuthorityStore;

#[cfg(test)]
impl MachineWorkloadAuthorityStore for EmptyMachineWorkloadAuthorityStore {
    fn list_machine_workload_authority_from_engine<'a>(
        &'a self,
        _execution_provider_id: &'a nimbus_workloads::WorkloadExecutionProviderId,
    ) -> nimbus_compute::machine_stop_authority::MachineWorkloadAuthorityFuture<'a> {
        Box::pin(async { Ok(Vec::new()) })
    }
}
