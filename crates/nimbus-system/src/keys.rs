use nimbus_core::{DocumentId, TableName, TenantId};

pub(super) fn stable_key_segment(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() {
            encoded.push(byte as char);
        } else {
            encoded.push('~');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    encoded
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

pub(super) fn source_package_document_id(digest: &str) -> String {
    format!("source-package:{}", stable_key_segment(digest))
}

pub(super) fn module_document_id(source_package_digest: &str, module_path: &str) -> String {
    format!(
        "module:{}:{}",
        stable_key_segment(source_package_digest),
        stable_key_segment(module_path)
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

pub(super) fn workload_status_document_id(tenant_id: &TenantId, workload_uid: &str) -> String {
    format!(
        "workload-status:{}:{}",
        stable_key_segment(tenant_id.as_str()),
        stable_key_segment(workload_uid)
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn stable_key_segment_is_injective_for_separator_and_case_variants() {
        let inputs = [
            "foo.bar",
            "foo bar",
            "foo-bar",
            "foo/bar",
            "Foo-Bar",
            "foo~2dbar",
            "東京",
            "",
        ];
        let segments = inputs
            .into_iter()
            .map(stable_key_segment)
            .collect::<BTreeSet<_>>();

        assert_eq!(segments.len(), inputs.len());
        assert_eq!(stable_key_segment("foo.bar"), "foo~2ebar");
        assert_eq!(stable_key_segment("foo bar"), "foo~20bar");
        assert_eq!(stable_key_segment("foo-bar"), "foo~2dbar");
        assert_eq!(stable_key_segment("foo/bar"), "foo~2fbar");
        assert_eq!(stable_key_segment("Foo-Bar"), "Foo~2dBar");
        assert_eq!(stable_key_segment("東京"), "~e6~9d~b1~e4~ba~ac");
    }

    #[test]
    fn system_document_ids_do_not_collide_for_distinct_projected_names() {
        let tenant_id = TenantId::new("demo").expect("tenant should parse");
        let service_ids = ["foo.bar", "foo bar", "foo-bar", "foo/bar"]
            .into_iter()
            .map(|name| service_document_id(&tenant_id, name))
            .collect::<BTreeSet<_>>();

        assert_eq!(service_ids.len(), 4);
        for document_id in service_ids {
            DocumentId::from_key(document_id).expect("system document id should parse");
        }

        let function_ids = ["messages:send", "messages/send", "messages send"]
            .into_iter()
            .map(|name| function_document_id("bundle:sha", name))
            .collect::<BTreeSet<_>>();

        assert_eq!(function_ids.len(), 3);
        for document_id in function_ids {
            DocumentId::from_key(document_id).expect("system document id should parse");
        }
    }
}
