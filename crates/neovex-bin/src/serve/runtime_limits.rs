use std::time::Duration;

use neovex::RuntimeLimits;

use super::ServeCommand;

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

pub(crate) fn default_runtime_worker_threads() -> usize {
    RuntimeLimits::default().worker_threads
}

pub(crate) fn default_runtime_max_nested_calls() -> usize {
    RuntimeLimits::default().max_nested_runtime_invocations
}

pub(crate) fn runtime_limits_from_command(command: &ServeCommand) -> RuntimeLimits {
    RuntimeLimits {
        max_heap_mb: command.runtime_heap_mb,
        initial_heap_mb: command.runtime_initial_heap_mb,
        execution_timeout: Duration::from_secs(command.runtime_timeout_secs),
        max_concurrent_runtime_instances: command.runtime_max_instances,
        worker_threads: command.runtime_worker_threads,
        max_nested_runtime_invocations: command.runtime_max_nested_calls,
        ..RuntimeLimits::default()
    }
}
