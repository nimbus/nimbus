use nimbus_core::{DocumentId, TableName, TenantId};

pub(super) fn stable_key_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}

pub(super) fn service_document_id(tenant_id: &TenantId, service_name: &str) -> String {
    format!(
        "service:{}:{}",
        stable_key_segment(tenant_id.as_str()),
        stable_key_segment(service_name)
    )
}

pub(super) fn machine_document_id(machine_name: &str) -> String {
    format!("machine:{}", stable_key_segment(machine_name))
}

pub(super) fn table_document_id(tenant_id: &TenantId, table: &TableName) -> String {
    format!(
        "table:{}:{}",
        stable_key_segment(tenant_id.as_str()),
        stable_key_segment(table.as_str())
    )
}

pub(super) fn bundle_document_id(sha256: &str) -> String {
    format!("bundle:{}", stable_key_segment(sha256))
}

pub(super) fn function_document_id(bundle_sha256: &str, function_name: &str) -> String {
    format!(
        "function:{}:{}",
        stable_key_segment(bundle_sha256),
        stable_key_segment(function_name)
    )
}

pub(super) fn scheduled_job_document_id(tenant_id: &TenantId, job_id: &DocumentId) -> String {
    format!(
        "scheduled-job:{}:{}",
        stable_key_segment(tenant_id.as_str()),
        stable_key_segment(&job_id.to_string())
    )
}

pub(super) fn cron_job_document_id(tenant_id: &TenantId, name: &str) -> String {
    format!(
        "cron-job:{}:{}",
        stable_key_segment(tenant_id.as_str()),
        stable_key_segment(name)
    )
}

pub(super) fn machine_listener_document_id(machine_name: &str) -> String {
    format!("listener:machine-api:{}", stable_key_segment(machine_name))
}

pub(super) fn listener_document_id(adapter: &str, protocol: &str) -> String {
    format!(
        "listener:{}:{}",
        stable_key_segment(adapter),
        stable_key_segment(protocol)
    )
}

pub(super) fn machine_port_document_id(machine_name: &str, port_name: &str) -> String {
    format!(
        "port:machine:{}:{}",
        stable_key_segment(machine_name),
        stable_key_segment(port_name)
    )
}

pub(super) fn service_port_document_id(
    tenant_id: &TenantId,
    service_name: &str,
    endpoint_name: &str,
) -> String {
    format!(
        "port:service:{}:{}:{}",
        stable_key_segment(tenant_id.as_str()),
        stable_key_segment(service_name),
        stable_key_segment(endpoint_name)
    )
}

pub(super) fn subscription_document_id(
    adapter: &str,
    tenant_id: &TenantId,
    subscription_id: u64,
) -> String {
    format!(
        "subscription:{}:{}:{}",
        stable_key_segment(adapter),
        stable_key_segment(tenant_id.as_str()),
        subscription_id
    )
}

#[cfg(test)]
pub(super) fn workload_status_document_id(tenant_id: &TenantId, workload_uid: &str) -> String {
    format!(
        "workload-status:{}:{}",
        stable_key_segment(tenant_id.as_str()),
        stable_key_segment(workload_uid)
    )
}
