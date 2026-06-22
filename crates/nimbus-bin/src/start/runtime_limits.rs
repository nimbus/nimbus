use std::num::{NonZeroU32, NonZeroUsize};
use std::time::Duration;

use nimbus::{
    RuntimeAdaptiveControllerSettings, RuntimeControllerReplayConfig, RuntimeHostResourceBudget,
    RuntimeLimits,
};

use super::{CliRuntimeAdaptiveMode, StartCommand};

pub(crate) fn default_runtime_heap_mb() -> usize {
    RuntimeLimits::default().max_heap_mb
}

pub(crate) fn default_runtime_initial_heap_mb() -> usize {
    RuntimeLimits::default().initial_heap_mb
}

pub(crate) fn default_runtime_timeout_secs() -> u64 {
    RuntimeLimits::default().execution_timeout.as_secs()
}

pub(crate) fn default_runtime_max_instances() -> usize {
    RuntimeLimits::default().max_concurrent_runtime_instances
}

pub(crate) fn default_runtime_max_active_per_tenant() -> usize {
    RuntimeLimits::default().max_active_top_level_invocations_per_tenant
}

pub(crate) fn default_runtime_max_in_flight_per_tenant() -> usize {
    RuntimeLimits::default().max_in_flight_top_level_invocations_per_tenant
}

pub(crate) fn default_runtime_max_queued_per_tenant() -> usize {
    RuntimeLimits::default().max_queued_top_level_invocations_per_tenant
}

pub(crate) fn default_runtime_worker_threads() -> usize {
    RuntimeLimits::default().worker_threads
}

pub(crate) fn default_runtime_max_nested_calls() -> usize {
    RuntimeLimits::default().max_nested_runtime_invocations
}

pub(crate) fn default_runtime_host_millicpus() -> u32 {
    default_runtime_host_resource_budget().host_millicpus
}

pub(crate) fn default_runtime_system_reserve_millicpus() -> u32 {
    default_runtime_host_resource_budget().system_reserved_millicpus
}

pub(crate) fn default_runtime_control_plane_reserve_millicpus() -> u32 {
    default_runtime_host_resource_budget().nimbus_control_plane_reserved_millicpus
}

pub(crate) fn default_runtime_seat_millicpus() -> u32 {
    default_runtime_host_resource_budget()
        .runtime_seat_millicpus
        .get()
}

fn default_runtime_host_resource_budget() -> RuntimeHostResourceBudget {
    let fallback_cpus = NonZeroUsize::new(default_runtime_worker_threads().max(1))
        .expect("max(1) keeps runtime worker thread fallback nonzero");
    let host_logical_cpus = std::thread::available_parallelism().unwrap_or(fallback_cpus);
    RuntimeHostResourceBudget::conservative_for_logical_cpus(host_logical_cpus)
}

pub(crate) fn runtime_limits_from_command(command: &StartCommand) -> RuntimeLimits {
    RuntimeLimits {
        max_heap_mb: command.runtime_heap_mb,
        initial_heap_mb: command.runtime_initial_heap_mb,
        execution_timeout: Duration::from_secs(command.runtime_timeout_secs),
        max_concurrent_runtime_instances: command.runtime_max_instances,
        max_active_top_level_invocations_per_tenant: command.runtime_max_active_per_tenant,
        max_in_flight_top_level_invocations_per_tenant: command.runtime_max_in_flight_per_tenant,
        max_queued_top_level_invocations_per_tenant: command.runtime_max_queued_per_tenant,
        worker_threads: command.runtime_worker_threads,
        max_nested_runtime_invocations: command.runtime_max_nested_calls,
        ..RuntimeLimits::default()
    }
}

pub(crate) fn runtime_host_resource_budget_from_command(
    command: &StartCommand,
) -> RuntimeHostResourceBudget {
    RuntimeHostResourceBudget {
        host_millicpus: command.runtime_host_millicpus,
        system_reserved_millicpus: command.runtime_system_reserve_millicpus,
        nimbus_control_plane_reserved_millicpus: command.runtime_control_plane_reserve_millicpus,
        runtime_hard_ceiling_millicpus: command.runtime_hard_ceiling_millicpus,
        runtime_seat_millicpus: NonZeroU32::new(command.runtime_seat_millicpus)
            .expect("clap/defaults keep runtime seat millicpus nonzero"),
    }
}

pub(crate) fn runtime_adaptive_controller_settings_from_command(
    command: &StartCommand,
) -> RuntimeAdaptiveControllerSettings {
    let replay_config = RuntimeControllerReplayConfig::default();
    let settings = match command.runtime_adaptive_mode {
        CliRuntimeAdaptiveMode::Disabled => RuntimeAdaptiveControllerSettings::disabled(),
        CliRuntimeAdaptiveMode::Shadow => RuntimeAdaptiveControllerSettings::shadow(replay_config),
        CliRuntimeAdaptiveMode::Canary => RuntimeAdaptiveControllerSettings::canary(
            replay_config,
            command.runtime_adaptive_canary_percent,
        ),
        CliRuntimeAdaptiveMode::Live => RuntimeAdaptiveControllerSettings::live(replay_config),
    };
    settings.with_rollback_to_static_defaults(command.runtime_adaptive_rollback)
}
