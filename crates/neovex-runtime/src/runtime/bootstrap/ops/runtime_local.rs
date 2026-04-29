use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;
use std::time::SystemTime;

use deno_permissions::OpenAccessKind;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::backends::v8::embedder::{JsErrorBox, OpState, op2};
use crate::error::NeovexRuntimeError;
use crate::node_compat::{ResolvedNodeModuleKind, ResolvedNodeTarget, resolve_node_target};
use crate::runtime::bootstrap::payloads::RuntimeHostCallEnvelope;
use crate::runtime::bootstrap::state::InstalledRuntimeCapabilityPolicy;
use crate::runtime_capabilities::RuntimeEnvLookupDescriptor;

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsReadFilePayload {
    path: String,
    #[serde(default)]
    encoding: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsWriteFilePayload {
    path: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    bytes: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsStatPayload {
    path: String,
    #[serde(default = "default_follow_symlink")]
    follow_symlink: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsMkdirPayload {
    path: String,
    #[serde(default)]
    recursive: bool,
    #[serde(default)]
    mode: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsReadDirPayload {
    path: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeRequireResolvePayload {
    specifier: String,
    #[serde(default)]
    referrer: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeRequireReadFilePayload {
    path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub(super) enum RuntimeFsReadFileResponse {
    Text { value: String },
    Bytes { value: Vec<u8> },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RuntimeFsStatDescriptor {
    is_file: bool,
    is_directory: bool,
    is_symlink: bool,
    size: u64,
    mtime_ms: Option<i64>,
    atime_ms: Option<i64>,
    birthtime_ms: Option<i64>,
    ctime_ms: Option<i64>,
    mode: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RuntimeFsDirEntryDescriptor {
    name: String,
    is_file: bool,
    is_directory: bool,
    is_symlink: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub(super) enum RuntimeRequireResolveResponse {
    Builtin { module_name: String },
    CommonJs { path: String },
    EsModule { path: String },
    Json { path: String },
}

fn capability_denied_error(error: impl std::fmt::Display) -> JsErrorBox {
    JsErrorBox::generic(NeovexRuntimeError::CapabilityDenied(error.to_string()).to_string())
}

fn runtime_target_triple() -> String {
    let arch = std::env::consts::ARCH;
    let vendor = if cfg!(target_vendor = "apple") {
        "apple"
    } else if cfg!(target_vendor = "pc") {
        "pc"
    } else {
        "unknown"
    };
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        std::env::consts::OS
    };
    let env = if cfg!(target_env = "gnu") {
        Some("gnu")
    } else if cfg!(target_env = "musl") {
        Some("musl")
    } else if cfg!(target_env = "msvc") {
        Some("msvc")
    } else {
        None
    };
    match env {
        Some(env) => format!("{arch}-{vendor}-{os}-{env}"),
        None => format!("{arch}-{vendor}-{os}"),
    }
}

fn default_follow_symlink() -> bool {
    true
}

fn system_time_to_unix_millis(value: Option<SystemTime>) -> Option<i64> {
    value.and_then(|time| {
        time.duration_since(SystemTime::UNIX_EPOCH)
            .ok()
            .and_then(|duration| i64::try_from(duration.as_millis()).ok())
    })
}

#[cfg(unix)]
fn metadata_mode(metadata: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(metadata.permissions().mode())
}

#[cfg(not(unix))]
fn metadata_mode(_metadata: &std::fs::Metadata) -> Option<u32> {
    None
}

#[cfg(unix)]
fn metadata_ctime_ms(metadata: &std::fs::Metadata) -> Option<i64> {
    use std::os::unix::fs::MetadataExt;
    let millis = (metadata.ctime() as i128)
        .checked_mul(1_000)?
        .checked_add((metadata.ctime_nsec() as i128) / 1_000_000)?;
    i64::try_from(millis).ok()
}

#[cfg(not(unix))]
fn metadata_ctime_ms(_metadata: &std::fs::Metadata) -> Option<i64> {
    None
}

fn describe_metadata(metadata: &std::fs::Metadata) -> RuntimeFsStatDescriptor {
    RuntimeFsStatDescriptor {
        is_file: metadata.is_file(),
        is_directory: metadata.is_dir(),
        is_symlink: metadata.file_type().is_symlink(),
        size: metadata.len(),
        mtime_ms: system_time_to_unix_millis(metadata.modified().ok()),
        atime_ms: system_time_to_unix_millis(metadata.accessed().ok()),
        birthtime_ms: system_time_to_unix_millis(metadata.created().ok()),
        ctime_ms: metadata_ctime_ms(metadata),
        mode: metadata_mode(metadata),
    }
}

fn runtime_fs_error_value(path: &Path, op: &str, error: &std::io::Error) -> Value {
    let code = match error.kind() {
        std::io::ErrorKind::NotFound => "ENOENT",
        std::io::ErrorKind::AlreadyExists => "EEXIST",
        std::io::ErrorKind::PermissionDenied => "EACCES",
        std::io::ErrorKind::InvalidInput => "EINVAL",
        _ => "EIO",
    };
    json!({
        "code": code,
        "message": format!("{op} {} failed: {error}", path.display()),
    })
}

#[cfg(unix)]
fn apply_directory_mode(path: &Path, mode: Option<u32>) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(mode) = mode {
        let permissions = std::fs::Permissions::from_mode(mode);
        std::fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn apply_directory_mode(_path: &Path, _mode: Option<u32>) -> std::io::Result<()> {
    Ok(())
}

#[op2(fast)]
pub(super) fn op_bootstrap_color_depth(_state: &mut OpState) -> i32 {
    // Neovex runtimes do not own an interactive terminal surface today, so we
    // report the most conservative color capability until a profile-scoped
    // stdio contract exists.
    1
}

#[op2]
pub(super) fn op_bootstrap_unstable_args(_state: &mut OpState) -> Vec<String> {
    Vec::new()
}

#[op2]
#[string]
pub(super) fn op_neovex_runtime_exec_path() -> std::result::Result<String, JsErrorBox> {
    std::env::current_exe()
        .map_err(|error| {
            JsErrorBox::generic(format!(
                "failed to resolve current executable path: {error}"
            ))
        })
        .map(|path| path.display().to_string())
}

#[op2]
#[string]
pub(super) fn op_neovex_runtime_target_triple() -> String {
    runtime_target_triple()
}

#[op2]
#[serde]
pub(super) fn op_create_worker(
    _state: &mut OpState,
    #[serde] _args: serde_json::Value,
) -> std::result::Result<serde_json::Value, JsErrorBox> {
    Err(capability_denied_error(
        "worker_threads are not available inside the Neovex runtime",
    ))
}

#[op2]
pub(super) fn op_host_terminate_worker(
    _state: &mut OpState,
    #[serde] _id: serde_json::Value,
) -> std::result::Result<(), JsErrorBox> {
    Err(capability_denied_error(
        "worker_threads are not available inside the Neovex runtime",
    ))
}

#[op2]
pub(super) fn op_host_post_message(
    _state: &mut OpState,
    #[serde] _id: serde_json::Value,
    #[serde] _data: serde_json::Value,
) -> std::result::Result<(), JsErrorBox> {
    Err(capability_denied_error(
        "worker_threads are not available inside the Neovex runtime",
    ))
}

#[op2]
pub(super) fn op_host_post_message_raw(
    _state: &mut OpState,
    #[serde] _id: serde_json::Value,
    #[buffer] _data: &[u8],
) -> std::result::Result<(), JsErrorBox> {
    Err(capability_denied_error(
        "worker_threads are not available inside the Neovex runtime",
    ))
}

#[op2]
#[serde]
pub(super) async fn op_host_recv_ctrl(
    _state: Rc<RefCell<OpState>>,
    #[serde] _id: serde_json::Value,
) -> std::result::Result<serde_json::Value, JsErrorBox> {
    Err(capability_denied_error(
        "worker_threads are not available inside the Neovex runtime",
    ))
}

#[op2]
#[serde]
pub(super) async fn op_host_recv_message(
    _state: Rc<RefCell<OpState>>,
    #[serde] _id: serde_json::Value,
) -> std::result::Result<serde_json::Value, JsErrorBox> {
    Err(capability_denied_error(
        "worker_threads are not available inside the Neovex runtime",
    ))
}

#[op2]
pub(super) fn op_host_get_worker_cpu_usage(
    _state: &mut OpState,
    #[serde] _id: serde_json::Value,
    #[buffer] out: &mut [f64],
) {
    out[0] = 0.0;
    out[1] = 0.0;
}

#[op2]
#[serde]
pub(super) fn op_host_recv_message_sync(
    _state: &mut OpState,
    #[serde] _id: serde_json::Value,
) -> std::result::Result<serde_json::Value, JsErrorBox> {
    Err(capability_denied_error(
        "worker_threads are not available inside the Neovex runtime",
    ))
}

#[op2(fast)]
pub(super) fn op_current_thread_cpu_usage(#[buffer] out: &mut [f64]) {
    // Neovex does not currently expose per-thread CPU accounting through the
    // runtime host contract, so report the conservative empty sample shape that
    // Node polyfills can consume without inventing unsupported precision.
    out[0] = 0.0;
    out[1] = 0.0;
}

#[op2]
#[serde]
pub(super) async fn op_neovex_runtime_fs_read_file(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsReadFilePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let permissions = {
        let state = state.borrow();
        state
            .borrow::<InstalledRuntimeCapabilityPolicy>()
            .permissions
            .clone()
    };
    let path = permissions
        .check_open(
            std::borrow::Cow::Borrowed(Path::new(&payload.path)),
            OpenAccessKind::Read,
            Some("node:fs/promises.readFile"),
        )
        .map_err(capability_denied_error)?
        .into_owned_path();
    let bytes = tokio::fs::read(&path).await.map_err(|error| {
        JsErrorBox::generic(format!("failed to read {}: {error}", path.display()))
    })?;
    let response = match payload.encoding.as_deref() {
        None => RuntimeFsReadFileResponse::Bytes { value: bytes },
        Some("utf8") => {
            let value = String::from_utf8(bytes).map_err(|error| {
                JsErrorBox::generic(format!(
                    "failed to decode {} as utf8: {error}",
                    path.display()
                ))
            })?;
            RuntimeFsReadFileResponse::Text { value }
        }
        Some(other) => Err(JsErrorBox::generic(format!(
            "unsupported fs.readFile encoding `{other}`; only utf8 or no encoding is currently supported"
        )))?,
    };
    Ok(RuntimeHostCallEnvelope::Ok {
        value: serde_json::to_value(response)
            .map_err(|error| JsErrorBox::generic(error.to_string()))?,
    })
}

#[op2]
#[serde]
pub(super) async fn op_neovex_runtime_fs_write_file(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsWriteFilePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = {
        let state = state.borrow();
        state
            .borrow::<InstalledRuntimeCapabilityPolicy>()
            .paths
            .clone()
    };
    let path = path_policy
        .ensure_write_path(Path::new(&payload.path))
        .map_err(capability_denied_error)?;
    let data = match (payload.text, payload.bytes) {
        (Some(text), None) => text.into_bytes(),
        (None, Some(bytes)) => bytes,
        (Some(_), Some(_)) => {
            return Err(JsErrorBox::generic(
                "fs.writeFile payload may contain text or bytes, but not both",
            ));
        }
        (None, None) => {
            return Err(JsErrorBox::generic(
                "fs.writeFile payload must include text or bytes",
            ));
        }
    };
    let response = match tokio::fs::write(&path, data).await {
        Ok(()) => RuntimeHostCallEnvelope::Ok {
            value: serde_json::Value::Null,
        },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "writeFile", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(super) async fn op_neovex_runtime_stat(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsStatPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let permissions = {
        let state = state.borrow();
        state
            .borrow::<InstalledRuntimeCapabilityPolicy>()
            .permissions
            .clone()
    };
    let path = permissions
        .check_open(
            std::borrow::Cow::Borrowed(Path::new(&payload.path)),
            OpenAccessKind::Read,
            Some("Deno.stat"),
        )
        .map_err(capability_denied_error)?
        .into_owned_path();
    let metadata = if payload.follow_symlink {
        tokio::fs::metadata(&path).await
    } else {
        tokio::fs::symlink_metadata(&path).await
    };
    Ok(match metadata {
        Ok(metadata) => RuntimeHostCallEnvelope::Ok {
            value: serde_json::to_value(describe_metadata(&metadata))
                .map_err(|error| JsErrorBox::generic(error.to_string()))?,
        },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "stat", &error),
        },
    })
}

#[op2]
#[serde]
pub(super) fn op_neovex_runtime_stat_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsStatPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let permissions = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .permissions
        .clone();
    let path = permissions
        .check_open(
            std::borrow::Cow::Borrowed(Path::new(&payload.path)),
            OpenAccessKind::Read,
            Some("Deno.statSync"),
        )
        .map_err(capability_denied_error)?
        .into_owned_path();
    let metadata = if payload.follow_symlink {
        std::fs::metadata(&path)
    } else {
        std::fs::symlink_metadata(&path)
    };
    Ok(match metadata {
        Ok(metadata) => RuntimeHostCallEnvelope::Ok {
            value: serde_json::to_value(describe_metadata(&metadata))
                .map_err(|error| JsErrorBox::generic(error.to_string()))?,
        },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "statSync", &error),
        },
    })
}

#[op2]
#[serde]
pub(super) async fn op_neovex_runtime_mkdir(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsMkdirPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = {
        let state = state.borrow();
        state
            .borrow::<InstalledRuntimeCapabilityPolicy>()
            .paths
            .clone()
    };
    let path = path_policy
        .ensure_write_path(Path::new(&payload.path))
        .map_err(capability_denied_error)?;
    let result = if payload.recursive {
        tokio::fs::create_dir_all(&path).await
    } else {
        tokio::fs::create_dir(&path).await
    };
    let response = match result {
        Ok(()) => {
            apply_directory_mode(&path, payload.mode)
                .map_err(|error| JsErrorBox::generic(error.to_string()))?;
            RuntimeHostCallEnvelope::Ok { value: Value::Null }
        }
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "mkdir", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(super) fn op_neovex_runtime_mkdir_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsMkdirPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .paths
        .clone();
    let path = path_policy
        .ensure_write_path(Path::new(&payload.path))
        .map_err(capability_denied_error)?;
    let result = if payload.recursive {
        std::fs::create_dir_all(&path)
    } else {
        std::fs::create_dir(&path)
    };
    let response = match result {
        Ok(()) => {
            apply_directory_mode(&path, payload.mode)
                .map_err(|error| JsErrorBox::generic(error.to_string()))?;
            RuntimeHostCallEnvelope::Ok { value: Value::Null }
        }
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "mkdirSync", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(super) async fn op_neovex_runtime_read_dir(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsReadDirPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let permissions = {
        let state = state.borrow();
        state
            .borrow::<InstalledRuntimeCapabilityPolicy>()
            .permissions
            .clone()
    };
    let path = permissions
        .check_open(
            std::borrow::Cow::Borrowed(Path::new(&payload.path)),
            OpenAccessKind::Read,
            Some("Deno.readDir"),
        )
        .map_err(capability_denied_error)?
        .into_owned_path();
    let mut directory = match tokio::fs::read_dir(&path).await {
        Ok(directory) => directory,
        Err(error) => {
            return Ok(RuntimeHostCallEnvelope::Error {
                error: runtime_fs_error_value(&path, "readDir", &error),
            });
        }
    };
    let mut entries = Vec::new();
    while let Some(entry) = directory.next_entry().await.map_err(|error| {
        JsErrorBox::generic(format!(
            "failed to read directory {}: {error}",
            path.display()
        ))
    })? {
        let file_type = entry.file_type().await.map_err(|error| {
            JsErrorBox::generic(format!(
                "failed to inspect directory entry in {}: {error}",
                path.display()
            ))
        })?;
        entries.push(RuntimeFsDirEntryDescriptor {
            name: entry.file_name().to_string_lossy().into_owned(),
            is_file: file_type.is_file(),
            is_directory: file_type.is_dir(),
            is_symlink: file_type.is_symlink(),
        });
    }
    Ok(RuntimeHostCallEnvelope::Ok {
        value: serde_json::to_value(entries)
            .map_err(|error| JsErrorBox::generic(error.to_string()))?,
    })
}

#[op2]
#[serde]
pub(super) fn op_neovex_runtime_read_dir_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsReadDirPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let permissions = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .permissions
        .clone();
    let path = permissions
        .check_open(
            std::borrow::Cow::Borrowed(Path::new(&payload.path)),
            OpenAccessKind::Read,
            Some("Deno.readDirSync"),
        )
        .map_err(capability_denied_error)?
        .into_owned_path();
    let entries_iter = match std::fs::read_dir(&path) {
        Ok(entries) => entries,
        Err(error) => {
            return Ok(RuntimeHostCallEnvelope::Error {
                error: runtime_fs_error_value(&path, "readDirSync", &error),
            });
        }
    };
    let mut entries = Vec::new();
    for entry in entries_iter {
        let entry = entry.map_err(|error| {
            JsErrorBox::generic(format!(
                "failed to read directory entry in {}: {error}",
                path.display()
            ))
        })?;
        let file_type = entry.file_type().map_err(|error| {
            JsErrorBox::generic(format!(
                "failed to inspect directory entry in {}: {error}",
                path.display()
            ))
        })?;
        entries.push(RuntimeFsDirEntryDescriptor {
            name: entry.file_name().to_string_lossy().into_owned(),
            is_file: file_type.is_file(),
            is_directory: file_type.is_dir(),
            is_symlink: file_type.is_symlink(),
        });
    }
    Ok(RuntimeHostCallEnvelope::Ok {
        value: serde_json::to_value(entries)
            .map_err(|error| JsErrorBox::generic(error.to_string()))?,
    })
}

#[op2]
#[serde]
pub(super) fn op_neovex_runtime_require_resolve(
    state: &mut OpState,
    #[serde] payload: RuntimeRequireResolvePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .paths
        .clone();
    let referrer = payload
        .referrer
        .unwrap_or_else(|| path_policy.cwd().display().to_string());
    let resolved = resolve_node_target(
        &path_policy,
        &payload.specifier,
        &referrer,
        node_resolver::ResolutionMode::Require,
    )?;
    let response = match resolved {
        ResolvedNodeTarget::BuiltIn { module_name } => {
            RuntimeRequireResolveResponse::Builtin { module_name }
        }
        ResolvedNodeTarget::Module { path, kind } => {
            let path = path_policy
                .ensure_module_read_path(&path)
                .map_err(|error| JsErrorBox::generic(error.to_string()))?;
            match kind {
                ResolvedNodeModuleKind::CommonJs => RuntimeRequireResolveResponse::CommonJs {
                    path: path.display().to_string(),
                },
                ResolvedNodeModuleKind::EsModule => RuntimeRequireResolveResponse::EsModule {
                    path: path.display().to_string(),
                },
                ResolvedNodeModuleKind::Json => RuntimeRequireResolveResponse::Json {
                    path: path.display().to_string(),
                },
            }
        }
    };
    Ok(RuntimeHostCallEnvelope::Ok {
        value: serde_json::to_value(response)
            .map_err(|error| JsErrorBox::generic(error.to_string()))?,
    })
}

#[op2]
#[serde]
pub(super) fn op_neovex_runtime_require_read_file(
    state: &mut OpState,
    #[serde] payload: RuntimeRequireReadFilePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let permissions = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .permissions
        .clone();
    let path = permissions
        .check_open(
            std::borrow::Cow::Borrowed(Path::new(&payload.path)),
            OpenAccessKind::Read,
            Some("node:module.require"),
        )
        .map_err(capability_denied_error)?
        .into_owned_path();
    let value = std::fs::read_to_string(&path).map_err(|error| {
        JsErrorBox::generic(format!(
            "failed to read CommonJS runtime module {}: {error}",
            path.display()
        ))
    })?;
    Ok(RuntimeHostCallEnvelope::Ok {
        value: serde_json::Value::String(value),
    })
}

#[op2]
#[serde]
pub(super) fn op_neovex_runtime_env_get(
    state: &mut OpState,
    #[string] name: String,
) -> RuntimeEnvLookupDescriptor {
    let policy = state.borrow::<InstalledRuntimeCapabilityPolicy>();
    let permissions = policy.permissions.clone();
    match permissions.check_env(&name) {
        Ok(()) => policy.env.lookup(&name),
        Err(error) => RuntimeEnvLookupDescriptor::Denied {
            message: format!("runtime env capability denied for `{name}`: {error}"),
        },
    }
}

#[op2]
#[serde]
pub(super) fn op_neovex_runtime_env_snapshot(state: &mut OpState) -> BTreeMap<String, String> {
    let policy = state.borrow::<InstalledRuntimeCapabilityPolicy>();
    policy.env.snapshot()
}

#[op2(fast)]
pub(super) fn op_set_raw(
    _state: &mut OpState,
    _rid: u32,
    _is_raw: bool,
    _cbreak: bool,
) -> std::result::Result<(), JsErrorBox> {
    Err(capability_denied_error(
        "raw terminal mode is not available inside the Neovex runtime",
    ))
}

#[op2(fast)]
#[smi]
pub(super) fn op_http_start(
    _state: &mut OpState,
    #[smi] _conn_rid: u32,
) -> std::result::Result<u32, JsErrorBox> {
    Err(capability_denied_error(
        "http connection upgrade APIs are not available inside the Neovex runtime",
    ))
}
