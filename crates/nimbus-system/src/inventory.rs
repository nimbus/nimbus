use super::keys::stable_key_segment;

#[derive(Debug, Clone, Copy)]
pub struct RouteInventoryEntry {
    pub method: &'static str,
    pub path: &'static str,
    pub adapter: &'static str,
    pub handler: &'static str,
    pub auth_required: bool,
}

impl RouteInventoryEntry {
    pub fn document_id(self) -> String {
        format!(
            "route:{}:{}",
            self.method.to_ascii_lowercase(),
            stable_key_segment(self.path)
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct AdapterCapabilityEntry {
    pub(super) adapter: &'static str,
    pub(super) feature: &'static str,
    pub(super) status: &'static str,
    pub(super) caveat: &'static str,
    pub(super) evidence: &'static str,
}

impl AdapterCapabilityEntry {
    pub(super) fn document_id(self) -> String {
        format!(
            "capability:{}:{}",
            stable_key_segment(self.adapter),
            stable_key_segment(self.feature)
        )
    }
}

pub fn route_inventory() -> Vec<RouteInventoryEntry> {
    vec![
        route("GET", "/health", "native", "health", false),
        route("GET", "/ui", "ui", "ui_root", false),
        route("GET", "/ui/auth", "ui", "ui_auth", false),
        route("GET", "/ui/auth.js", "ui", "ui_auth_script", false),
        route("POST", "/ui/auth/session", "ui", "create_ui_session", false),
        route(
            "GET",
            "/debug/license/status",
            "native",
            "license_status",
            true,
        ),
        route(
            "GET",
            "/debug/encryption/status",
            "native",
            "encryption_status",
            true,
        ),
        route(
            "POST",
            "/api/system/token/rotate",
            "native",
            "rotate_local_admin_token",
            true,
        ),
        route(
            "POST",
            "/api/system/shutdown",
            "native",
            "shutdown_system",
            true,
        ),
        route(
            "GET",
            "/debug/runtime/metrics",
            "native",
            "runtime_diagnostics",
            true,
        ),
        route(
            "GET",
            "/debug/tenants/{tenant_id}/consistency",
            "native",
            "tenant_consistency_report",
            true,
        ),
        route(
            "GET",
            "/debug/tenants/{tenant_id}/engine/metrics",
            "native",
            "tenant_engine_diagnostics",
            true,
        ),
        route("GET", "/api/tenants", "native", "list_tenants", true),
        route("POST", "/api/tenants", "native", "create_tenant", true),
        route(
            "DELETE",
            "/api/tenants/{tenant_id}",
            "native",
            "delete_tenant",
            true,
        ),
        route(
            "POST",
            "/api/machines/{name}/create",
            "native",
            "create_machine",
            true,
        ),
        route(
            "PATCH",
            "/api/machines/{name}",
            "native",
            "update_machine",
            true,
        ),
        route(
            "DELETE",
            "/api/machines/{name}",
            "native",
            "delete_machine",
            true,
        ),
        route(
            "POST",
            "/api/machines/{name}/start",
            "native",
            "start_machine",
            true,
        ),
        route(
            "POST",
            "/api/machines/{name}/stop",
            "native",
            "stop_machine",
            true,
        ),
        route(
            "POST",
            "/api/machines/{name}/restart",
            "native",
            "restart_machine",
            true,
        ),
        route(
            "POST",
            "/api/tenants/{tenant_id}/services/{service_name}/start",
            "native",
            "start_service",
            true,
        ),
        route(
            "POST",
            "/api/tenants/{tenant_id}/services/{service_name}/stop",
            "native",
            "stop_service",
            true,
        ),
        route(
            "GET",
            "/api/tenants/{tenant_id}/documents/{table}",
            "native",
            "list_documents",
            true,
        ),
        route(
            "POST",
            "/api/tenants/{tenant_id}/documents",
            "native",
            "insert_document",
            true,
        ),
        route(
            "POST",
            "/api/tenants/{tenant_id}/query",
            "native",
            "query_documents",
            true,
        ),
        route(
            "POST",
            "/api/tenants/{tenant_id}/query/paginated",
            "native",
            "query_documents_paginated",
            true,
        ),
        route(
            "GET",
            "/api/tenants/{tenant_id}/journal",
            "native",
            "read_journal",
            true,
        ),
        route(
            "GET",
            "/api/tenants/{tenant_id}/journal/bootstrap",
            "native",
            "bootstrap_journal",
            true,
        ),
        route(
            "GET",
            "/api/tenants/{tenant_id}/schema",
            "native",
            "get_schema",
            true,
        ),
        route(
            "PUT",
            "/api/tenants/{tenant_id}/schema/{table}",
            "native",
            "set_table_schema",
            true,
        ),
        route(
            "POST",
            "/convex/{tenant_id}/query",
            "convex",
            "query",
            false,
        ),
        route(
            "POST",
            "/convex/{tenant_id}/query/paginated",
            "convex",
            "paginated_query",
            false,
        ),
        route(
            "POST",
            "/convex/{tenant_id}/mutation",
            "convex",
            "mutation",
            false,
        ),
        route(
            "POST",
            "/convex/{tenant_id}/action",
            "convex",
            "action",
            false,
        ),
        route("GET", "/convex/{tenant_id}/ws", "convex", "ws", false),
        route(
            "POST",
            "/v1/projects/{project_id}/databases/{database_id}/documents:commit",
            "firebase",
            "commit",
            false,
        ),
        route(
            "POST",
            "/v1/projects/{project_id}/databases/{database_id}/documents:runQuery",
            "firebase",
            "run_query",
            false,
        ),
        route(
            "GET",
            "/google.firestore.v1.Firestore/Listen",
            "firebase",
            "listen_websocket",
            false,
        ),
    ]
}

fn route(
    method: &'static str,
    path: &'static str,
    adapter: &'static str,
    handler: &'static str,
    auth_required: bool,
) -> RouteInventoryEntry {
    RouteInventoryEntry {
        method,
        path,
        adapter,
        handler,
        auth_required,
    }
}

pub(super) fn adapter_capability_inventory() -> Vec<AdapterCapabilityEntry> {
    vec![
        capability(
            "convex",
            "reactive-functions",
            "supported",
            "",
            "crates/nimbus-convex/ with nimbus-server route, WebSocket, runtime, and system-evidence shells.",
        ),
        capability(
            "convex",
            "system-tenant-ui-functions",
            "supported-with-caveats",
            "The system table contract exists; the packaged function bundle is still tracked by ST4.",
            "docs/private/plans/system-tenant-api-plan.md",
        ),
        capability(
            "mongodb",
            "wire-protocol-crud",
            "supported-with-caveats",
            "Nimbus implements the local compatibility surface, not Atlas administration.",
            "crates/nimbus-mongodb/ with TCP listener lifecycle retained in nimbus-server.",
        ),
        capability(
            "firebase",
            "firestore-rest-grpc",
            "supported-with-caveats",
            "Nimbus implements the Firestore-compatible local data path; Firebase project administration and hosted rules are not claimed.",
            "crates/nimbus-firebase/ with REST/gRPC transport and auth shells retained in nimbus-server.",
        ),
        capability(
            "native",
            "local-admin-rest",
            "supported",
            "",
            "crates/nimbus-server/src/router.rs",
        ),
        capability(
            "machine",
            "bootc-macos-machine",
            "supported-with-caveats",
            "Published bootc image is the current macOS default; live machine state persistence into _nimbus is still tracked by ST2.",
            "docs/private/architecture/sandbox/macos-machine-flow.md",
        ),
    ]
}

fn capability(
    adapter: &'static str,
    feature: &'static str,
    status: &'static str,
    caveat: &'static str,
    evidence: &'static str,
) -> AdapterCapabilityEntry {
    AdapterCapabilityEntry {
        adapter,
        feature,
        status,
        caveat,
        evidence,
    }
}
