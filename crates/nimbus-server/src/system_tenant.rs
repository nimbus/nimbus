mod identity;
mod inventory;
mod keys;
mod projection;
mod records;
mod schema;

#[cfg(test)]
mod tests;

pub(crate) use identity::{
    is_reserved_tenant_id, is_system_tenant_id, system_tenant_id, user_tenant_id,
};
#[cfg(test)]
use inventory::adapter_capability_inventory;
pub(crate) use inventory::route_inventory;
#[cfg(test)]
use keys::{
    machine_document_id, machine_listener_document_id, machine_port_document_id,
    subscription_document_id, workload_status_document_id,
};
pub(crate) use projection::install_table_projection_observer;
#[cfg(test)]
pub(crate) use records::ensure_system_tenant_async;
#[cfg(test)]
pub(crate) use records::record_tenant_workload_status_async;
pub(crate) use records::{
    RunRecord, delete_cron_job_state_async, delete_machine_state_async,
    delete_scheduled_job_state_async, delete_subscription_state_async, endpoint_protocol,
    prepare_system_tenant_async, record_convex_deployment_state_async, record_listener_state_async,
    record_machine_state_async, record_run_async, record_scheduled_job_result_state_async,
    record_service_handle_async, record_subscription_delivery_async,
    record_subscription_error_async, record_subscription_state_async, record_system_event_async,
    record_table_state_async, sandbox_backend, sandbox_status,
    sync_scheduler_state_for_tenant_async,
};
#[cfg(test)]
use schema::system_table_schemas;
