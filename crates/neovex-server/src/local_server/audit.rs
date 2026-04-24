use std::fs::{File, OpenOptions};
#[cfg(any(test, unix))]
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use axum::http::{HeaderMap, header};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::paths::LocalServerPaths;
use super::policy::LocalServerRouteFamily;

#[derive(Debug)]
pub(crate) struct LocalServerAuditLog {
    path: PathBuf,
    write_lock: Mutex<()>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocalServerAuditEvent {
    pub(crate) route_family: LocalServerRouteFamily,
    pub(crate) tenant_id: Option<String>,
    pub(crate) auth_scope: &'static str,
    pub(crate) auth_method: Option<&'static str>,
    pub(crate) success: bool,
    pub(crate) origin: Option<String>,
    pub(crate) reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalServerAuditRecord {
    pub(crate) ts: String,
    pub(crate) route_family: String,
    pub(crate) tenant_id: Option<String>,
    pub(crate) auth_scope: String,
    pub(crate) auth_method: Option<String>,
    pub(crate) success: bool,
    pub(crate) origin: Option<String>,
    pub(crate) reason: String,
}

impl LocalServerAuditLog {
    pub(crate) fn new(paths: &LocalServerPaths) -> Self {
        Self {
            path: paths.audit_log_path.clone(),
            write_lock: Mutex::new(()),
        }
    }

    pub(crate) fn append(
        &self,
        paths: &LocalServerPaths,
        event: LocalServerAuditEvent,
    ) -> io::Result<()> {
        paths.ensure_audit_parent_dir()?;
        let _guard = self
            .write_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let file_missing = !self.path.exists();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        if file_missing {
            set_secure_file_permissions(&file)?;
            set_secure_path_permissions(&self.path)?;
        }
        let record = LocalServerAuditRecord {
            ts: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .map_err(|error| {
                    io::Error::other(format!("failed to format local server audit time: {error}"))
                })?,
            route_family: event.route_family.as_str().to_string(),
            tenant_id: event.tenant_id,
            auth_scope: event.auth_scope.to_string(),
            auth_method: event.auth_method.map(str::to_string),
            success: event.success,
            origin: event.origin,
            reason: event.reason,
        };
        serde_json::to_writer(&mut file, &record).map_err(|error| {
            io::Error::other(format!(
                "failed to serialize local server audit record: {error}"
            ))
        })?;
        file.write_all(b"\n")?;
        file.flush()?;
        Ok(())
    }
}

pub(crate) fn origin_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

pub(crate) fn tenant_id_from_path(path: &str) -> Option<String> {
    let tenant_segment = if let Some(rest) = path.strip_prefix("/api/tenants/") {
        Some(rest)
    } else if let Some(rest) = path.strip_prefix("/debug/tenants/") {
        Some(rest)
    } else {
        path.strip_prefix("/convex/")
    }?;
    tenant_segment
        .split('/')
        .next()
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
}

#[cfg(unix)]
fn set_secure_file_permissions(file: &File) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    file.set_permissions(fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_secure_file_permissions(_file: &File) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_secure_path_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_secure_path_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_paths(root: &Path) -> LocalServerPaths {
        LocalServerPaths {
            auth_token_path: root.join("auth").join("token"),
            server_discovery_path: root.join("run").join("server.json"),
            audit_log_path: root.join("logs").join("access.jsonl"),
        }
    }

    #[test]
    fn append_writes_jsonl_record() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());
        let audit = LocalServerAuditLog::new(&paths);

        audit
            .append(
                &paths,
                LocalServerAuditEvent {
                    route_family: LocalServerRouteFamily::NativeApi,
                    tenant_id: Some("demo".to_string()),
                    auth_scope: "server_access",
                    auth_method: Some("local_admin_bearer"),
                    success: true,
                    origin: Some("http://localhost:3210".to_string()),
                    reason: "authorized".to_string(),
                },
            )
            .expect("audit append should succeed");

        let records = fs::read_to_string(&paths.audit_log_path)
            .expect("audit log should be readable")
            .lines()
            .map(|line| {
                serde_json::from_str::<LocalServerAuditRecord>(line)
                    .expect("audit log line should parse")
            })
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].route_family, "native_api");
        assert_eq!(records[0].tenant_id.as_deref(), Some("demo"));
        assert_eq!(records[0].auth_scope, "server_access");
        assert_eq!(
            records[0].auth_method.as_deref(),
            Some("local_admin_bearer")
        );
        assert!(records[0].success);
        assert_eq!(records[0].origin.as_deref(), Some("http://localhost:3210"));
        assert_eq!(records[0].reason, "authorized");
    }

    #[test]
    fn tenant_id_from_path_extracts_native_debug_and_convex_routes() {
        assert_eq!(
            tenant_id_from_path("/api/tenants/demo/documents").as_deref(),
            Some("demo")
        );
        assert_eq!(
            tenant_id_from_path("/debug/tenants/demo/engine/metrics").as_deref(),
            Some("demo")
        );
        assert_eq!(
            tenant_id_from_path("/convex/demo/query").as_deref(),
            Some("demo")
        );
        assert_eq!(tenant_id_from_path("/health"), None);
    }

    #[cfg(unix)]
    #[test]
    fn audit_log_is_written_with_user_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());
        let audit = LocalServerAuditLog::new(&paths);

        audit
            .append(
                &paths,
                LocalServerAuditEvent {
                    route_family: LocalServerRouteFamily::NativeApi,
                    tenant_id: None,
                    auth_scope: "origin",
                    auth_method: None,
                    success: false,
                    origin: Some("http://example.com".to_string()),
                    reason: "origin rejected".to_string(),
                },
            )
            .expect("audit append should succeed");

        let mode = fs::metadata(&paths.audit_log_path)
            .expect("audit log metadata should load")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}
