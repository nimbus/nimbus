use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::paths::LocalServerPaths;

pub const SERVER_DISCOVERY_PROTOCOL_VERSIONS: &[&str] = &["neovex.v1"];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerDiscoveryRecord {
    pub pid: u32,
    pub address: String,
    pub started_at: String,
    pub version: String,
    pub protocol_versions: Vec<String>,
}

impl ServerDiscoveryRecord {
    fn new(address: SocketAddr, started_at: String) -> Self {
        Self {
            pid: process::id(),
            address: address.to_string(),
            started_at,
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_versions: SERVER_DISCOVERY_PROTOCOL_VERSIONS
                .iter()
                .map(|version| (*version).to_string())
                .collect(),
        }
    }
}

#[derive(Debug)]
pub struct ServerDiscoveryLease {
    path: PathBuf,
    record: ServerDiscoveryRecord,
}

impl ServerDiscoveryLease {
    pub fn acquire(paths: &LocalServerPaths, address: SocketAddr) -> io::Result<Self> {
        let started_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|error| {
                io::Error::other(format!("failed to format server start time: {error}"))
            })?;
        Self::acquire_with(paths, address, started_at, pid_is_live)
    }

    fn acquire_with(
        paths: &LocalServerPaths,
        address: SocketAddr,
        started_at: String,
        pid_checker: impl Fn(u32) -> bool,
    ) -> io::Result<Self> {
        paths.ensure_run_state_parent_dir()?;
        let path = paths.server_discovery_path.clone();
        let _ = read_live_server_discovery_with(&path, &pid_checker)?;
        let record = ServerDiscoveryRecord::new(address, started_at);
        write_json_atomically(&path, &record)?;
        Ok(Self { path, record })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn record(&self) -> &ServerDiscoveryRecord {
        &self.record
    }
}

impl Drop for ServerDiscoveryLease {
    fn drop(&mut self) {
        let current_record = match read_server_discovery_record(&self.path) {
            Ok(Some(record)) => record,
            _ => return,
        };
        if current_record == self.record {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub fn read_live_server_discovery(
    paths: &LocalServerPaths,
) -> io::Result<Option<ServerDiscoveryRecord>> {
    read_live_server_discovery_with(&paths.server_discovery_path, pid_is_live)
}

fn read_live_server_discovery_with(
    path: &Path,
    pid_checker: impl Fn(u32) -> bool,
) -> io::Result<Option<ServerDiscoveryRecord>> {
    match read_server_discovery_record(path)? {
        Some(record) if pid_checker(record.pid) => Ok(Some(record)),
        Some(_) => {
            remove_file_if_exists(path)?;
            Ok(None)
        }
        None => Ok(None),
    }
}

fn read_server_discovery_record(path: &Path) -> io::Result<Option<ServerDiscoveryRecord>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    match serde_json::from_slice(&bytes) {
        Ok(record) => Ok(Some(record)),
        Err(_) => {
            remove_file_if_exists(path)?;
            Ok(None)
        }
    }
}

fn write_json_atomically<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path {} does not have a parent directory", path.display()),
        )
    })?;
    let temp_path = parent.join(format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("server-discovery"),
        process::id(),
        temp_suffix(),
    ));
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| {
        io::Error::other(format!("failed to serialize server discovery: {error}"))
    })?;
    fs::write(&temp_path, bytes)?;
    fs::rename(&temp_path, path)?;
    Ok(())
}

fn temp_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn remove_file_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn pid_is_live(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if result == 0 {
        return true;
    }
    match io::Error::last_os_error().raw_os_error() {
        Some(code) => code != libc::ESRCH,
        None => false,
    }
}

#[cfg(windows)]
fn pid_is_live(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    if pid == 0 {
        return false;
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle == 0 {
        return false;
    }
    unsafe {
        CloseHandle(handle);
    }
    true
}

#[cfg(not(any(unix, windows)))]
fn pid_is_live(_pid: u32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_paths(root: &Path) -> LocalServerPaths {
        LocalServerPaths {
            auth_token_path: root.join("auth").join("token"),
            server_discovery_path: root.join("run").join("server.json"),
            audit_log_path: root.join("logs").join("access.jsonl"),
        }
    }

    #[test]
    fn acquisition_writes_record_and_drop_removes_it() {
        let temp = tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());
        let address: SocketAddr = "127.0.0.1:3210".parse().expect("socket addr should parse");

        let lease = ServerDiscoveryLease::acquire_with(
            &paths,
            address,
            "2026-04-19T00:00:00Z".to_string(),
            |_| false,
        )
        .expect("discovery lease should write");

        let record = read_server_discovery_record(&paths.server_discovery_path)
            .expect("discovery file should read");
        assert_eq!(record, Some(lease.record().clone()));
        assert_eq!(
            lease.path(),
            paths.server_discovery_path.as_path(),
            "lease should point at the configured discovery file"
        );

        drop(lease);

        assert!(
            !paths.server_discovery_path.exists(),
            "clean shutdown should remove the discovery file"
        );
    }

    #[test]
    fn stale_record_is_replaced_when_pid_is_not_live() {
        let temp = tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());
        paths
            .ensure_run_state_parent_dir()
            .expect("run-state parent dir should build");
        let stale = ServerDiscoveryRecord {
            pid: 4242,
            address: "127.0.0.1:1111".to_string(),
            started_at: "2026-04-18T00:00:00Z".to_string(),
            version: "0.1.0".to_string(),
            protocol_versions: vec!["neovex.v1".to_string()],
        };
        write_json_atomically(&paths.server_discovery_path, &stale)
            .expect("stale discovery file should write");

        let lease = ServerDiscoveryLease::acquire_with(
            &paths,
            "127.0.0.1:3210".parse().expect("socket addr should parse"),
            "2026-04-19T00:00:00Z".to_string(),
            |pid| pid == process::id(),
        )
        .expect("new lease should replace stale file");

        let record = read_server_discovery_record(&paths.server_discovery_path)
            .expect("discovery file should read");
        assert_eq!(record, Some(lease.record().clone()));
        assert_ne!(record, Some(stale));
    }

    #[test]
    fn live_record_reader_cleans_stale_file() {
        let temp = tempdir().expect("tempdir should build");
        let paths = sample_paths(temp.path());
        paths
            .ensure_run_state_parent_dir()
            .expect("run-state parent dir should build");
        let stale = ServerDiscoveryRecord {
            pid: 999_999,
            address: "127.0.0.1:1111".to_string(),
            started_at: "2026-04-18T00:00:00Z".to_string(),
            version: "0.1.0".to_string(),
            protocol_versions: vec!["neovex.v1".to_string()],
        };
        write_json_atomically(&paths.server_discovery_path, &stale)
            .expect("stale discovery file should write");

        let live = read_live_server_discovery_with(&paths.server_discovery_path, |_| false)
            .expect("stale discovery file should be readable");

        assert!(live.is_none(), "stale discovery should not look live");
        assert!(
            !paths.server_discovery_path.exists(),
            "stale discovery file should be removed during cleanup"
        );
    }
}
