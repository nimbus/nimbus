mod identity;
mod inventory;
mod keys;
mod projection;
mod records;
mod schema;
mod source_package;
mod source_store;

#[cfg(test)]
#[path = "tests/connectivity.rs"]
mod connectivity_tests;
#[cfg(test)]
mod tests;

pub use identity::{is_reserved_tenant_id, is_system_tenant_id, system_tenant_id, user_tenant_id};
#[cfg(test)]
use inventory::adapter_capability_inventory;
pub use inventory::route_inventory;
#[cfg(test)]
use keys::{
    listener_document_id, machine_document_id, port_document_id, subscription_document_id,
    workload_status_document_id,
};
pub use projection::{SystemConnectivityProjectionRuntime, install_table_projection_observer};
pub use records::SystemTenantStatusEvidenceWriter;
pub use records::ensure_system_tenant_async;
pub(crate) use records::record_table_state_for_generation_async;
pub use records::{
    ERROR_GROUP_LIMIT, ERROR_SCAN_WINDOW, ErrorGroup, ErrorGroupPage, ErrorGroupQuery,
    LOG_PAGE_LIMIT, LOG_SCAN_WINDOW, LogPage, LogQuery, OpenSpan, RUN_SPAN_LIMIT, RunError,
    RunRecord, RunSpan, RunSpanRecorder, SystemConnectivityObservationError,
    SystemDeploymentActivation, SystemDeploymentActivationFunction,
    SystemDeploymentFunctionRecordInput, SystemDeploymentHttpRouteRecordInput,
    SystemDeploymentRecordInput, SystemEvent, SystemPortListenerObservation,
    SystemPublishedEndpointObservation, SystemServiceConnectivityObservation,
    SystemUnixListenerObservation, claim_server_listener_projection_async,
    delete_cron_job_state_async, delete_machine_state_async, delete_scheduled_job_state_async,
    delete_subscription_state_async, deployment_bundle_sha256, deployment_history_async,
    endpoint_protocol, error_class, error_fingerprint, normalize_error_message,
    prepare_system_tenant_async, query_error_groups_async, query_log_lines_async,
    record_deployment_state_async, record_machine_state_async,
    record_port_listener_observation_async, record_run_async,
    record_scheduled_job_result_state_async, record_service_connectivity_observation_async,
    record_source_package_state_async, record_subscription_delivery_async,
    record_subscription_error_async, record_subscription_state_async, record_system_event_async,
    record_table_state_async, record_unix_listener_observation_async, sandbox_backend,
    sandbox_status, span_kind_for_operation, sync_scheduler_state_for_tenant_async,
};
pub use records::{
    ModuleSource, SystemModuleRecordInput, SystemSourcePackageRecordInput,
    read_active_source_package_modules_async, read_module_source_async,
    read_source_package_modules_async,
};
#[cfg(test)]
use schema::{SystemTable, system_table_schemas};
pub use source_package::{
    ModuleInput, ParsedModule, ParsedSourcePackage, SOURCE_PACKAGE_VERSION, build_source_package,
    parse_source_package,
};
pub use source_store::{
    DiskSourcePackageStore, SourcePackageStore, StoredSourcePackage, source_package_digest,
};
