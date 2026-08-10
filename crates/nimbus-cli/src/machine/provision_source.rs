//! Pure default-machine source preparation and fenced activation.
//!
//! Preparation reads exact persisted machine intent under the authenticated
//! machine lock and derives provider facts without opening provider journals,
//! reserving ports, or starting the machine. Activation re-reads that same
//! source under the lock, then either adopts the exact running authority or
//! starts the stopped machine with the exact prepared next generation.

use std::sync::Arc;

use nimbus::Error;
use nimbus_machine::{
    MachineConnectivityCapabilities, MachineForwarderAuthority, MachineLifecycle, MachineProvider,
    api::{MACHINE_API_ROLE, PROTOCOL_VERSION},
};
use nimbus_network::{
    NetworkCapabilityBundle, NetworkCapabilityRequirements, NetworkCapabilitySelection,
    NetworkManagementMode, NetworkSovereigntyRequirements,
};
use nimbus_sandbox::backends::container::OciMachinePortForwarderConfig;
use nimbus_workloads::{NodeIdentity, WorkloadExecutionProviderId};

use super::DEFAULT_MACHINE_NAME;
use super::backend::provision::{
    ForwardedMachineProvisionAdapter, ForwardedMachineProvisionSourcePlan,
};
use super::client::MachineApiClient;
use super::files::{
    load_machine_provision_source_snapshot, read_machine_config_snapshot_if_exists,
    with_exact_authenticated_default_machine_lock,
};
use super::manager::{
    next_machine_forwarder_authority, start_machine_with_expected_forwarder_authority,
};
use super::network_composition::HostMachineNetworkAuthority;
use super::record::{MachineConfigRecord, MachinePaths, MachineRootLayout, MachineStateRecord};

/// Effect-free, exact source facts for the default managed machine.
#[derive(Clone)]
pub(crate) struct PreparedDefaultMachineProvisionSource {
    roots: MachineRootLayout,
    network: HostMachineNetworkAuthority,
    paths: MachinePaths,
    config: MachineConfigRecord,
    state: MachineStateRecord,
    disposition: PreparedMachineDisposition,
    expected_authority: MachineForwarderAuthority,
    connectivity: MachineConnectivityCapabilities,
    source_plan: ForwardedMachineProvisionSourcePlan,
}

/// Activated client and the one adapter that owns every forwarded provision
/// capability for the exact prepared source.
pub(crate) struct ActivatedDefaultMachineProvisionSource {
    client: MachineApiClient,
    adapter: Arc<ForwardedMachineProvisionAdapter>,
}

impl ActivatedDefaultMachineProvisionSource {
    pub(crate) fn into_parts(self) -> (MachineApiClient, Arc<ForwardedMachineProvisionAdapter>) {
        (self.client, self.adapter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreparedMachineDisposition {
    AdoptRunning,
    StartStopped,
}

struct PreparedMachineActivation {
    paths: MachinePaths,
    authority: MachineForwarderAuthority,
    source_plan: ForwardedMachineProvisionSourcePlan,
}

/// Prepare the initialized default machine as a provision-capability source.
///
/// The caller supplies the neutral compute node identity. Provider selection,
/// forwarder generation, connectivity, and sovereignty come only from the
/// authenticated persisted machine source and the concrete provider report.
pub(crate) fn prepare_default_machine_provision_source(
    network: &HostMachineNetworkAuthority,
    node_identity: NodeIdentity,
) -> Result<PreparedDefaultMachineProvisionSource, Error> {
    let roots = MachineRootLayout::resolve()?;
    prepare_default_machine_provision_source_at(&roots, network, node_identity)
}

fn prepare_default_machine_provision_source_at(
    roots: &MachineRootLayout,
    network: &HostMachineNetworkAuthority,
    node_identity: NodeIdentity,
) -> Result<PreparedDefaultMachineProvisionSource, Error> {
    prepare_default_machine_provision_source_at_with(roots, network, node_identity, |provider| {
        provider
            .connectivity_capabilities()
            .map_err(|error| Error::InvalidInput(error.to_string()))
    })
}

fn prepare_default_machine_provision_source_at_with<Connectivity>(
    roots: &MachineRootLayout,
    network: &HostMachineNetworkAuthority,
    node_identity: NodeIdentity,
    connectivity_for: Connectivity,
) -> Result<PreparedDefaultMachineProvisionSource, Error>
where
    Connectivity: FnOnce(MachineProvider) -> Result<MachineConnectivityCapabilities, Error>,
{
    let paths = roots.paths(DEFAULT_MACHINE_NAME);
    let preflight = read_machine_config_snapshot_if_exists(&paths.config_path)?.ok_or_else(|| {
        Error::InvalidInput(format!(
            "machine '{}' is not initialized; run `nimbus machine start` to create it with defaults or `nimbus machine init` to configure it first",
            DEFAULT_MACHINE_NAME
        ))
    })?;
    reject_provider_managed(preflight.provider)?;

    with_exact_authenticated_default_machine_lock(roots, network, || {
        let (paths, config, state) =
            load_machine_provision_source_snapshot(roots, network, DEFAULT_MACHINE_NAME)?;
        let connectivity = connectivity_for(config.provider)?;
        PreparedDefaultMachineProvisionSource::from_locked_snapshot(
            roots.clone(),
            network.clone(),
            paths,
            config,
            state,
            node_identity,
            connectivity,
        )
    })
}

impl PreparedDefaultMachineProvisionSource {
    #[allow(clippy::too_many_arguments)]
    fn from_locked_snapshot(
        roots: MachineRootLayout,
        network: HostMachineNetworkAuthority,
        paths: MachinePaths,
        config: MachineConfigRecord,
        state: MachineStateRecord,
        node_identity: NodeIdentity,
        connectivity: MachineConnectivityCapabilities,
    ) -> Result<Self, Error> {
        reject_provider_managed(config.provider)?;
        let (disposition, expected_authority) = prepared_authority(&config, &state)?;
        let source_plan = ForwardedMachineProvisionSourcePlan::new(
            config.provider,
            expected_authority.clone(),
            node_identity,
            connectivity.clone(),
            parent_forwarder_config(&paths, &expected_authority)?,
        )?;
        Ok(Self {
            roots,
            network,
            paths,
            config,
            state,
            disposition,
            expected_authority,
            connectivity,
            source_plan,
        })
    }

    pub(crate) fn bundle(&self) -> &NetworkCapabilityBundle {
        self.source_plan.bundle()
    }

    pub(crate) fn selection(&self) -> &NetworkCapabilitySelection {
        self.source_plan.selection()
    }

    pub(crate) fn requirements(&self) -> &NetworkCapabilityRequirements {
        self.source_plan.requirements()
    }

    pub(crate) fn sovereignty(&self) -> &NetworkSovereigntyRequirements {
        self.source_plan.sovereignty()
    }

    pub(crate) fn node_identity(&self) -> &NodeIdentity {
        self.source_plan.node_identity()
    }

    pub(crate) fn execution_provider_id(&self) -> &WorkloadExecutionProviderId {
        self.source_plan.execution_provider_id()
    }

    /// Activate the exact prepared source, then open provider journals only
    /// after the guest health response and intrinsic source plan authenticate.
    pub(crate) fn activate(self) -> Result<ActivatedDefaultMachineProvisionSource, Error> {
        let activation =
            self.activate_machine_with(|network, paths, config, state, expected_authority| {
                start_machine_with_expected_forwarder_authority(
                    network,
                    paths,
                    config,
                    state,
                    expected_authority,
                )
            })?;
        if !activation.paths.api_socket_path.exists() {
            return Err(Error::InvalidInput(format!(
                "machine '{}' is active but guest machine API socket {} is missing; run `nimbus machine status` or restart the machine",
                DEFAULT_MACHINE_NAME,
                activation.paths.api_socket_path.display()
            )));
        }
        if activation.source_plan != self.source_plan {
            return Err(Error::PreconditionFailed(
                "activated machine provision source plan differs from its prepared source"
                    .to_owned(),
            ));
        }
        activation
            .source_plan
            .forwarder_config()
            .require_reachable()
            .map_err(|error| {
                Error::InvalidInput(format!(
                    "machine '{}' parent gvproxy control endpoint is not reachable after exact activation: {error}",
                    DEFAULT_MACHINE_NAME
                ))
            })?;
        let client = MachineApiClient::new(activation.paths.api_socket_path)
            .with_forwarder_authority(activation.authority);
        let health = client.health().map_err(|error| {
            Error::InvalidInput(format!(
                "machine '{}' guest machine API is not reachable after exact activation: {error}",
                DEFAULT_MACHINE_NAME
            ))
        })?;
        if health.status != "ok"
            || health.role != MACHINE_API_ROLE
            || health.protocol_version != PROTOCOL_VERSION
        {
            return Err(Error::PreconditionFailed(format!(
                "machine '{}' guest health identity is not the expected {} {} protocol",
                DEFAULT_MACHINE_NAME, MACHINE_API_ROLE, PROTOCOL_VERSION
            )));
        }
        let adapter = self.source_plan.activate(client.clone(), &self.network)?;
        Ok(ActivatedDefaultMachineProvisionSource { client, adapter })
    }

    fn activate_machine_with<Start>(&self, start: Start) -> Result<PreparedMachineActivation, Error>
    where
        Start: FnOnce(
            &HostMachineNetworkAuthority,
            &MachinePaths,
            &mut MachineConfigRecord,
            &mut MachineStateRecord,
            &MachineForwarderAuthority,
        ) -> Result<(), Error>,
    {
        with_exact_authenticated_default_machine_lock(&self.roots, &self.network, || {
            let (paths, mut config, mut state) = load_machine_provision_source_snapshot(
                &self.roots,
                &self.network,
                DEFAULT_MACHINE_NAME,
            )?;
            if paths != self.paths || config != self.config || state != self.state {
                return Err(Error::PreconditionFailed(
                    "default machine provision source changed after preparation".to_owned(),
                ));
            }
            reject_provider_managed(config.provider)?;
            let (disposition, authority) = prepared_authority(&config, &state)?;
            if disposition != self.disposition || authority != self.expected_authority {
                return Err(Error::PreconditionFailed(
                    "default machine forwarder authority changed after preparation".to_owned(),
                ));
            }
            let source_plan = ForwardedMachineProvisionSourcePlan::new(
                config.provider,
                authority.clone(),
                self.source_plan.node_identity().clone(),
                self.connectivity.clone(),
                parent_forwarder_config(&paths, &authority)?,
            )?;
            if source_plan != self.source_plan {
                return Err(Error::PreconditionFailed(
                    "default machine provider facts changed after preparation".to_owned(),
                ));
            }

            match disposition {
                PreparedMachineDisposition::AdoptRunning => {}
                PreparedMachineDisposition::StartStopped => {
                    start(&self.network, &paths, &mut config, &mut state, &authority)?;
                    let running = running_authority(&config, &state)?;
                    authority.authenticate(&running).map_err(|error| {
                        Error::PreconditionFailed(format!(
                            "machine start did not retain its exact prepared authority: {error}"
                        ))
                    })?;
                }
            }

            Ok(PreparedMachineActivation {
                paths,
                authority,
                source_plan,
            })
        })
    }
}

fn parent_forwarder_config(
    paths: &MachinePaths,
    authority: &MachineForwarderAuthority,
) -> Result<OciMachinePortForwarderConfig, Error> {
    OciMachinePortForwarderConfig::for_unix_services_socket(
        paths.gvproxy_services_socket_path(),
        "/services/forwarder",
        authority.provider_instance().expose_to_provider(),
        authority.generation(),
    )
    .map_err(|error| {
        Error::PreconditionFailed(format!(
            "failed to compose the exact parent gvproxy control endpoint: {error}"
        ))
    })
}

fn reject_provider_managed(provider: MachineProvider) -> Result<(), Error> {
    if provider.network_management_mode() == NetworkManagementMode::ProviderManaged {
        return Err(provider.unavailable_error());
    }
    Ok(())
}

fn prepared_authority(
    config: &MachineConfigRecord,
    state: &MachineStateRecord,
) -> Result<(PreparedMachineDisposition, MachineForwarderAuthority), Error> {
    match state.lifecycle {
        MachineLifecycle::Running => Ok((
            PreparedMachineDisposition::AdoptRunning,
            running_authority(config, state)?,
        )),
        MachineLifecycle::Stopped => Ok((
            PreparedMachineDisposition::StartStopped,
            next_machine_forwarder_authority(config, state)?,
        )),
        lifecycle => Err(Error::conflict(format!(
            "machine '{}' must be stopped or running to prepare workload provision; found {}",
            config.name,
            lifecycle.as_str()
        ))),
    }
}

fn running_authority(
    config: &MachineConfigRecord,
    state: &MachineStateRecord,
) -> Result<MachineForwarderAuthority, Error> {
    let runtime = state.runtime.as_ref().ok_or_else(|| {
        Error::conflict(format!(
            "machine '{}' is running without a parent-issued forwarder authority",
            config.name
        ))
    })?;
    if runtime.forwarder_authority.provider_instance()
        != config.network_authority.provider_instance()
    {
        return Err(Error::conflict(format!(
            "machine '{}' running authority belongs to a different provider instance",
            config.name
        )));
    }
    Ok(runtime.forwarder_authority.clone())
}

#[cfg(test)]
#[path = "provision_source/tests.rs"]
mod tests;
