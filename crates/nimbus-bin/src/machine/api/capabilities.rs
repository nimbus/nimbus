use super::binaries::resolve_binary_statuses;
use super::*;

pub(super) fn machine_api_capability_response(
    state: &MachineApiState,
) -> MachineApiCapabilityResponse {
    let binary_statuses = resolve_binary_statuses(
        state.binary_lookup_path.as_deref(),
        &state.helper_binary_dirs,
    );
    let node_workload_blockers = node_workload_service_blockers(state);
    let node_workload_available = node_workload_blockers.is_empty();
    let state_operations_available = state.service_workloads.is_some() && node_workload_available;
    let mut shared_blockers = node_workload_blockers;
    if state.service_workloads.is_none() {
        shared_blockers.push(MACHINE_API_OPERATION_BLOCKER.to_owned());
    }
    if state_operations_available
        && let Some(forwarder) = state.machine_port_forwarder.as_ref()
        && let Err(error) = probe_machine_port_forwarder(forwarder)
    {
        shared_blockers.push(error);
    }

    let image_start_blockers = merge_operation_blockers(
        &shared_blockers,
        missing_binary_blockers(&binary_statuses, MACHINE_API_IMAGE_START_OPERATION),
    );
    let build_start_blockers = merge_operation_blockers(
        &image_start_blockers,
        missing_binary_blockers(&binary_statuses, MACHINE_API_BUILD_START_OPERATION),
    );
    let bootc_status_blockers =
        missing_binary_blockers(&binary_statuses, MACHINE_API_BOOTC_STATUS_OPERATION);
    let bootc_switch_blockers =
        missing_binary_blockers(&binary_statuses, MACHINE_API_BOOTC_SWITCH_OPERATION);
    let bootc_upgrade_blockers =
        missing_binary_blockers(&binary_statuses, MACHINE_API_BOOTC_UPGRADE_OPERATION);
    let bootc_rollback_blockers =
        missing_binary_blockers(&binary_statuses, MACHINE_API_BOOTC_ROLLBACK_OPERATION);
    let operation_statuses = vec![
        machine_api_operation_status(MACHINE_API_BOOTC_STATUS_OPERATION, bootc_status_blockers),
        machine_api_operation_status(MACHINE_API_BOOTC_SWITCH_OPERATION, bootc_switch_blockers),
        machine_api_operation_status(MACHINE_API_BOOTC_UPGRADE_OPERATION, bootc_upgrade_blockers),
        machine_api_operation_status(
            MACHINE_API_BOOTC_ROLLBACK_OPERATION,
            bootc_rollback_blockers,
        ),
        machine_api_operation_status(
            MACHINE_API_LIST_OPERATION,
            shared_operation_blockers(state_operations_available, &shared_blockers),
        ),
        machine_api_operation_status(
            MACHINE_API_INSPECT_OPERATION,
            shared_operation_blockers(state_operations_available, &shared_blockers),
        ),
        machine_api_operation_status(
            MACHINE_API_INSPECT_CURRENT_OPERATION,
            shared_operation_blockers(state_operations_available, &shared_blockers),
        ),
        machine_api_operation_status(
            MACHINE_API_LOGS_OPERATION,
            shared_operation_blockers(state_operations_available, &shared_blockers),
        ),
        machine_api_operation_status(
            MACHINE_API_PS_OPERATION,
            shared_operation_blockers(state_operations_available, &shared_blockers),
        ),
        machine_api_operation_status(
            MACHINE_API_IMAGE_START_OPERATION,
            image_start_blockers.clone(),
        ),
        machine_api_operation_status(MACHINE_API_STOP_OPERATION, image_start_blockers.clone()),
        machine_api_operation_status(MACHINE_API_BUILD_START_OPERATION, build_start_blockers),
    ];
    let service_execution_blockers = operation_statuses
        .iter()
        .find(|status| status.name == MACHINE_API_IMAGE_START_OPERATION)
        .map(|status| status.blockers.clone())
        .unwrap_or_default();
    let service_execution_ready = service_execution_blockers.is_empty();
    let mut supported_operations = vec!["healthz".to_owned(), "capabilities".to_owned()];
    supported_operations.extend(
        operation_statuses
            .iter()
            .filter(|status| status.available)
            .map(|status| status.name.clone()),
    );
    let supported_service_backends = state
        .service_workloads
        .as_ref()
        .map(|workloads| vec![workloads.kind()])
        .unwrap_or_else(|| vec![SandboxBackendKind::Container]);

    MachineApiCapabilityResponse {
        protocol_version: PROTOCOL_VERSION.to_owned(),
        service_execution_ready,
        service_execution_mode: MachineApiServiceExecutionMode::StandardContainers,
        service_execution_driver: if state.service_workloads.is_some() && node_workload_available {
            MachineApiServiceExecutionDriver::GuestNodeAgentSystemdTransientUnit
        } else {
            MachineApiServiceExecutionDriver::Unavailable
        },
        supported_service_backends,
        supported_operations,
        binary_statuses,
        operation_statuses,
        service_execution_blockers,
    }
}

fn node_workload_service_blockers(state: &MachineApiState) -> Vec<String> {
    state
        .service_workloads
        .as_ref()
        .map(|workloads| workloads.service_execution_blockers())
        .unwrap_or_default()
}

fn machine_api_operation_status(name: &str, blockers: Vec<String>) -> MachineApiOperationStatus {
    MachineApiOperationStatus {
        name: name.to_owned(),
        available: blockers.is_empty(),
        blockers,
    }
}

fn shared_operation_blockers(
    state_operations_available: bool,
    shared_blockers: &[String],
) -> Vec<String> {
    if state_operations_available {
        Vec::new()
    } else {
        shared_blockers.to_vec()
    }
}

fn merge_operation_blockers(shared: &[String], mut specific: Vec<String>) -> Vec<String> {
    let mut blockers = shared.to_vec();
    blockers.append(&mut specific);
    blockers
}

fn missing_binary_blockers(
    binary_statuses: &[MachineApiBinaryStatus],
    operation_name: &str,
) -> Vec<String> {
    binary_statuses
        .iter()
        .filter(|binary| {
            !binary.present
                && binary
                    .required_for_operations
                    .iter()
                    .any(|required| required == operation_name)
        })
        .map(|binary| {
            format!(
                "missing guest binary required for {}: {}",
                operation_name, binary.name
            )
        })
        .collect()
}

fn probe_machine_port_forwarder(config: &OciMachinePortForwarderConfig) -> Result<(), String> {
    let mut addresses = (config.host.as_str(), config.port)
        .to_socket_addrs()
        .map_err(|error| {
            format!(
                "guest machine port forwarder DNS lookup failed for {}:{}: {error}",
                config.host, config.port
            )
        })?;
    let address = addresses.next().ok_or_else(|| {
        format!(
            "guest machine port forwarder {}:{} did not resolve to an address",
            config.host, config.port
        )
    })?;
    TcpStream::connect_timeout(&address, MACHINE_PORT_FORWARDER_TIMEOUT).map_err(|error| {
        format!(
            "guest machine port forwarder is not reachable at {}:{}: {error}",
            config.host, config.port
        )
    })?;
    Ok(())
}
