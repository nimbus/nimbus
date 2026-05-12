use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime};

use deno_permissions::OpenAccessKind;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::backends::v8::embedder::{JsErrorBox, OpState, op2};
use crate::error::NimbusRuntimeError;
use crate::node_compat::{ResolvedNodeModuleKind, ResolvedNodeTarget, resolve_node_target};
use crate::runtime::bootstrap::payloads::RuntimeHostCallEnvelope;
use crate::runtime::bootstrap::state::InstalledRuntimeCapabilityPolicy;
use crate::runtime_capabilities::RuntimeEnvLookupDescriptor;

static NIMBUS_SHARED_WORKER_ENV: LazyLock<Mutex<BTreeMap<String, String>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

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
pub(super) struct RuntimeFsOpenValidationPayload {
    path: String,
    #[serde(default)]
    write: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsCopyFilePayload {
    from: String,
    to: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsLinkPayload {
    oldpath: String,
    newpath: String,
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
pub(super) struct RuntimeFsChmodPayload {
    path: String,
    mode: u32,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsUtimePayload {
    path: String,
    atime_secs: i64,
    atime_nanos: u32,
    mtime_secs: i64,
    mtime_nanos: u32,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsRemovePayload {
    path: String,
    #[serde(default)]
    recursive: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsSymlinkPayload {
    oldpath: String,
    newpath: String,
    #[serde(default)]
    file_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsReadLinkPayload {
    path: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsRenamePayload {
    oldpath: String,
    newpath: String,
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
    dev: Option<u64>,
    ino: Option<u64>,
    nlink: Option<u64>,
    uid: Option<u32>,
    gid: Option<u32>,
    rdev: Option<u64>,
    blksize: Option<u64>,
    blocks: Option<u64>,
    is_block_device: bool,
    is_char_device: bool,
    is_fifo: bool,
    is_socket: bool,
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
    JsErrorBox::generic(NimbusRuntimeError::CapabilityDenied(error.to_string()).to_string())
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

#[cfg(unix)]
fn create_runtime_symlink(
    oldpath: &Path,
    newpath: &Path,
    _file_type: Option<&str>,
) -> std::io::Result<()> {
    std::os::unix::fs::symlink(oldpath, newpath)
}

#[cfg(windows)]
fn create_runtime_symlink(
    oldpath: &Path,
    newpath: &Path,
    file_type: Option<&str>,
) -> std::io::Result<()> {
    match file_type {
        Some("dir") | Some("junction") => std::os::windows::fs::symlink_dir(oldpath, newpath),
        _ => std::os::windows::fs::symlink_file(oldpath, newpath),
    }
}

#[cfg(not(any(unix, windows)))]
fn create_runtime_symlink(
    _oldpath: &Path,
    _newpath: &Path,
    _file_type: Option<&str>,
) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "symlink is not supported on this platform",
    ))
}

fn system_time_to_unix_millis(value: Option<SystemTime>) -> Option<i64> {
    value.and_then(|time| {
        time.duration_since(SystemTime::UNIX_EPOCH)
            .ok()
            .and_then(|duration| i64::try_from(duration.as_millis()).ok())
    })
}

fn system_time_from_unix_parts(seconds: i64, nanos: u32) -> std::io::Result<SystemTime> {
    if seconds < 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "timestamp must be non-negative",
        ));
    }
    SystemTime::UNIX_EPOCH
        .checked_add(Duration::new(seconds as u64, nanos))
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "timestamp is out of range",
            )
        })
}

fn apply_path_times(
    path: &Path,
    atime_secs: i64,
    atime_nanos: u32,
    mtime_secs: i64,
    mtime_nanos: u32,
) -> std::io::Result<()> {
    let file = std::fs::File::open(path)?;
    let times = std::fs::FileTimes::new()
        .set_accessed(system_time_from_unix_parts(atime_secs, atime_nanos)?)
        .set_modified(system_time_from_unix_parts(mtime_secs, mtime_nanos)?);
    file.set_times(times)
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

#[cfg(unix)]
fn metadata_dev(metadata: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.dev())
}

#[cfg(not(unix))]
fn metadata_dev(_metadata: &std::fs::Metadata) -> Option<u64> {
    None
}

#[cfg(unix)]
fn metadata_ino(metadata: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.ino())
}

#[cfg(not(unix))]
fn metadata_ino(_metadata: &std::fs::Metadata) -> Option<u64> {
    None
}

#[cfg(unix)]
fn metadata_nlink(metadata: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.nlink())
}

#[cfg(not(unix))]
fn metadata_nlink(_metadata: &std::fs::Metadata) -> Option<u64> {
    None
}

#[cfg(unix)]
fn metadata_uid(metadata: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.uid())
}

#[cfg(not(unix))]
fn metadata_uid(_metadata: &std::fs::Metadata) -> Option<u32> {
    None
}

#[cfg(unix)]
fn metadata_gid(metadata: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.gid())
}

#[cfg(not(unix))]
fn metadata_gid(_metadata: &std::fs::Metadata) -> Option<u32> {
    None
}

#[cfg(unix)]
fn metadata_rdev(metadata: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.rdev())
}

#[cfg(not(unix))]
fn metadata_rdev(_metadata: &std::fs::Metadata) -> Option<u64> {
    None
}

#[cfg(unix)]
fn metadata_blksize(metadata: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.blksize())
}

#[cfg(not(unix))]
fn metadata_blksize(_metadata: &std::fs::Metadata) -> Option<u64> {
    None
}

#[cfg(unix)]
fn metadata_blocks(metadata: &std::fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(metadata.blocks())
}

#[cfg(not(unix))]
fn metadata_blocks(_metadata: &std::fs::Metadata) -> Option<u64> {
    None
}

#[cfg(unix)]
fn metadata_is_block_device(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::FileTypeExt;
    metadata.file_type().is_block_device()
}

#[cfg(not(unix))]
fn metadata_is_block_device(_metadata: &std::fs::Metadata) -> bool {
    false
}

#[cfg(unix)]
fn metadata_is_char_device(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::FileTypeExt;
    metadata.file_type().is_char_device()
}

#[cfg(not(unix))]
fn metadata_is_char_device(_metadata: &std::fs::Metadata) -> bool {
    false
}

#[cfg(unix)]
fn metadata_is_fifo(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::FileTypeExt;
    metadata.file_type().is_fifo()
}

#[cfg(not(unix))]
fn metadata_is_fifo(_metadata: &std::fs::Metadata) -> bool {
    false
}

#[cfg(unix)]
fn metadata_is_socket(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::FileTypeExt;
    metadata.file_type().is_socket()
}

#[cfg(not(unix))]
fn metadata_is_socket(_metadata: &std::fs::Metadata) -> bool {
    false
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
        dev: metadata_dev(metadata),
        ino: metadata_ino(metadata),
        nlink: metadata_nlink(metadata),
        uid: metadata_uid(metadata),
        gid: metadata_gid(metadata),
        rdev: metadata_rdev(metadata),
        blksize: metadata_blksize(metadata),
        blocks: metadata_blocks(metadata),
        is_block_device: metadata_is_block_device(metadata),
        is_char_device: metadata_is_char_device(metadata),
        is_fifo: metadata_is_fifo(metadata),
        is_socket: metadata_is_socket(metadata),
    }
}

fn runtime_fs_error_value(path: &Path, op: &str, error: &std::io::Error) -> Value {
    let code = match error.kind() {
        std::io::ErrorKind::NotFound => "ENOENT",
        std::io::ErrorKind::AlreadyExists => "EEXIST",
        std::io::ErrorKind::PermissionDenied => "EACCES",
        std::io::ErrorKind::InvalidInput => "EINVAL",
        _ => match error.raw_os_error() {
            Some(20) => "ENOTDIR",
            Some(21) => "EISDIR",
            Some(1) => "EPERM",
            Some(39) => "ENOTEMPTY",
            #[cfg(windows)]
            Some(267) => "ENOTDIR",
            #[cfg(windows)]
            Some(145) => "ENOTEMPTY",
            _ => "EIO",
        },
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

#[cfg(unix)]
fn apply_fs_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(path, permissions)
}

#[cfg(not(unix))]
fn apply_fs_mode(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

#[op2(fast)]
pub(super) fn op_bootstrap_color_depth(_state: &mut OpState) -> i32 {
    // Nimbus runtimes do not own an interactive terminal surface today, so we
    // report the most conservative color capability until a grant-scoped
    // stdio contract exists.
    1
}

#[op2]
pub(super) fn op_bootstrap_unstable_args(_state: &mut OpState) -> Vec<String> {
    Vec::new()
}

#[op2]
#[string]
pub(super) fn op_nimbus_runtime_exec_path() -> std::result::Result<String, JsErrorBox> {
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
pub(super) fn op_nimbus_runtime_target_triple() -> String {
    runtime_target_triple()
}

#[op2]
#[serde]
pub(super) async fn op_nimbus_runtime_fs_read_file(
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
pub(super) async fn op_nimbus_runtime_fs_write_file(
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
pub(super) fn op_nimbus_runtime_validate_open_path(
    state: &mut OpState,
    #[serde] payload: RuntimeFsOpenValidationPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let permissions = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .permissions
        .clone();
    let access = if payload.write {
        OpenAccessKind::Write
    } else {
        OpenAccessKind::Read
    };
    let checked = permissions
        .check_open(
            std::borrow::Cow::Borrowed(Path::new(&payload.path)),
            access,
            Some("node:fs/promises.open"),
        )
        .map_err(capability_denied_error)?;
    Ok(RuntimeHostCallEnvelope::Ok {
        value: Value::String(checked.into_owned_path().display().to_string()),
    })
}

#[op2]
#[serde]
pub(super) async fn op_nimbus_runtime_copy_file(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsCopyFilePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let (permissions, path_policy) = {
        let state = state.borrow();
        (
            state
                .borrow::<InstalledRuntimeCapabilityPolicy>()
                .permissions
                .clone(),
            state
                .borrow::<InstalledRuntimeCapabilityPolicy>()
                .paths
                .clone(),
        )
    };
    let from = permissions
        .check_open(
            std::borrow::Cow::Borrowed(Path::new(&payload.from)),
            OpenAccessKind::Read,
            Some("Deno.copyFile"),
        )
        .map_err(capability_denied_error)?
        .into_owned_path();
    let to = path_policy
        .ensure_write_path(Path::new(&payload.to))
        .map_err(capability_denied_error)?;
    let response = match tokio::fs::copy(&from, &to).await {
        Ok(_) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&to, "copyFile", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(super) fn op_nimbus_runtime_copy_file_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsCopyFilePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let permissions = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .permissions
        .clone();
    let path_policy = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .paths
        .clone();
    let from = permissions
        .check_open(
            std::borrow::Cow::Borrowed(Path::new(&payload.from)),
            OpenAccessKind::Read,
            Some("Deno.copyFileSync"),
        )
        .map_err(capability_denied_error)?
        .into_owned_path();
    let to = path_policy
        .ensure_write_path(Path::new(&payload.to))
        .map_err(capability_denied_error)?;
    let response = match std::fs::copy(&from, &to) {
        Ok(_) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&to, "copyFileSync", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(super) async fn op_nimbus_runtime_link(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsLinkPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let (permissions, path_policy) = {
        let state = state.borrow();
        (
            state
                .borrow::<InstalledRuntimeCapabilityPolicy>()
                .permissions
                .clone(),
            state
                .borrow::<InstalledRuntimeCapabilityPolicy>()
                .paths
                .clone(),
        )
    };
    let oldpath = permissions
        .check_open(
            std::borrow::Cow::Borrowed(Path::new(&payload.oldpath)),
            OpenAccessKind::Read,
            Some("Deno.link"),
        )
        .map_err(capability_denied_error)?
        .into_owned_path();
    let newpath = path_policy
        .ensure_write_path(Path::new(&payload.newpath))
        .map_err(capability_denied_error)?;
    let response = match tokio::fs::hard_link(&oldpath, &newpath).await {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&newpath, "link", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(super) fn op_nimbus_runtime_link_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsLinkPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let permissions = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .permissions
        .clone();
    let path_policy = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .paths
        .clone();
    let oldpath = permissions
        .check_open(
            std::borrow::Cow::Borrowed(Path::new(&payload.oldpath)),
            OpenAccessKind::Read,
            Some("Deno.linkSync"),
        )
        .map_err(capability_denied_error)?
        .into_owned_path();
    let newpath = path_policy
        .ensure_write_path(Path::new(&payload.newpath))
        .map_err(capability_denied_error)?;
    let response = match std::fs::hard_link(&oldpath, &newpath) {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&newpath, "linkSync", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(super) async fn op_nimbus_runtime_stat(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsStatPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = {
        let state = state.borrow();
        state
            .borrow::<InstalledRuntimeCapabilityPolicy>()
            .paths
            .clone()
    };
    let path = path_policy
        .ensure_read_metadata_path(Path::new(&payload.path))
        .map_err(capability_denied_error)?;
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
pub(super) fn op_nimbus_runtime_stat_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsStatPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .paths
        .clone();
    let path = path_policy
        .ensure_read_metadata_path(Path::new(&payload.path))
        .map_err(capability_denied_error)?;
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
pub(super) async fn op_nimbus_runtime_mkdir(
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
pub(super) fn op_nimbus_runtime_mkdir_sync(
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
pub(super) async fn op_nimbus_runtime_chmod(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsChmodPayload,
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
            OpenAccessKind::Write,
            Some("Deno.chmod"),
        )
        .map_err(capability_denied_error)?
        .into_owned_path();
    let mode = payload.mode;
    let response = match tokio::task::spawn_blocking({
        let path = path.clone();
        move || apply_fs_mode(&path, mode)
    })
    .await
    .map_err(|error| JsErrorBox::generic(error.to_string()))?
    {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "chmod", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(super) async fn op_nimbus_runtime_utime(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsUtimePayload,
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
    let atime_secs = payload.atime_secs;
    let atime_nanos = payload.atime_nanos;
    let mtime_secs = payload.mtime_secs;
    let mtime_nanos = payload.mtime_nanos;
    let response = match tokio::task::spawn_blocking({
        let path = path.clone();
        move || apply_path_times(&path, atime_secs, atime_nanos, mtime_secs, mtime_nanos)
    })
    .await
    .map_err(|error| JsErrorBox::generic(error.to_string()))?
    {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "utime", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(super) fn op_nimbus_runtime_utime_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsUtimePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .paths
        .clone();
    let path = path_policy
        .ensure_write_path(Path::new(&payload.path))
        .map_err(capability_denied_error)?;
    let response = match apply_path_times(
        &path,
        payload.atime_secs,
        payload.atime_nanos,
        payload.mtime_secs,
        payload.mtime_nanos,
    ) {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "utimeSync", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(super) fn op_nimbus_runtime_chmod_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsChmodPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let permissions = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .permissions
        .clone();
    let path = permissions
        .check_open(
            std::borrow::Cow::Borrowed(Path::new(&payload.path)),
            OpenAccessKind::Write,
            Some("Deno.chmodSync"),
        )
        .map_err(capability_denied_error)?
        .into_owned_path();
    let response = match apply_fs_mode(&path, payload.mode) {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "chmodSync", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(super) async fn op_nimbus_runtime_read_dir(
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
pub(super) fn op_nimbus_runtime_read_dir_sync(
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

fn remove_path(path: &Path, recursive: bool) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        std::fs::remove_file(path)
    } else if recursive {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_dir(path)
    }
}

#[op2]
#[serde]
pub(super) async fn op_nimbus_runtime_remove(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsRemovePayload,
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
    let recursive = payload.recursive;
    let path_for_task = path.clone();
    let result = tokio::task::spawn_blocking(move || remove_path(&path_for_task, recursive))
        .await
        .map_err(|error| JsErrorBox::generic(error.to_string()))?;
    Ok(match result {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "remove", &error),
        },
    })
}

#[op2]
#[serde]
pub(super) fn op_nimbus_runtime_remove_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsRemovePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .paths
        .clone();
    let path = path_policy
        .ensure_write_path(Path::new(&payload.path))
        .map_err(capability_denied_error)?;
    let result = remove_path(&path, payload.recursive);
    Ok(match result {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "removeSync", &error),
        },
    })
}

#[op2]
#[serde]
pub(super) async fn op_nimbus_runtime_symlink(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsSymlinkPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = {
        let state = state.borrow();
        state
            .borrow::<InstalledRuntimeCapabilityPolicy>()
            .paths
            .clone()
    };
    let newpath = path_policy
        .ensure_write_path(Path::new(&payload.newpath))
        .map_err(capability_denied_error)?;
    let oldpath = path_policy
        .ensure_symlink_target_path(Path::new(&payload.oldpath), &newpath)
        .map_err(capability_denied_error)?;
    let file_type = payload.file_type;
    let result = tokio::task::spawn_blocking({
        let oldpath = oldpath.clone();
        let newpath = newpath.clone();
        move || create_runtime_symlink(&oldpath, &newpath, file_type.as_deref())
    })
    .await
    .map_err(|error| JsErrorBox::generic(error.to_string()))?;
    Ok(match result {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&newpath, "symlink", &error),
        },
    })
}

#[op2]
#[serde]
pub(super) fn op_nimbus_runtime_symlink_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsSymlinkPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .paths
        .clone();
    let newpath = path_policy
        .ensure_write_path(Path::new(&payload.newpath))
        .map_err(capability_denied_error)?;
    let oldpath = path_policy
        .ensure_symlink_target_path(Path::new(&payload.oldpath), &newpath)
        .map_err(capability_denied_error)?;
    Ok(
        match create_runtime_symlink(&oldpath, &newpath, payload.file_type.as_deref()) {
            Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
            Err(error) => RuntimeHostCallEnvelope::Error {
                error: runtime_fs_error_value(&newpath, "symlinkSync", &error),
            },
        },
    )
}

#[op2]
#[serde]
pub(super) async fn op_nimbus_runtime_read_link(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsReadLinkPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = {
        let state = state.borrow();
        state
            .borrow::<InstalledRuntimeCapabilityPolicy>()
            .paths
            .clone()
    };
    let path = path_policy
        .ensure_read_path_lexical(Path::new(&payload.path))
        .map_err(capability_denied_error)?
        .to_path_buf();
    Ok(match tokio::fs::read_link(&path).await {
        Ok(target) => RuntimeHostCallEnvelope::Ok {
            value: Value::String(target.to_string_lossy().into_owned()),
        },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "readLink", &error),
        },
    })
}

#[op2]
#[serde]
pub(super) fn op_nimbus_runtime_read_link_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsReadLinkPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .paths
        .clone();
    let path = path_policy
        .ensure_read_path_lexical(Path::new(&payload.path))
        .map_err(capability_denied_error)?
        .to_path_buf();
    Ok(match std::fs::read_link(&path) {
        Ok(target) => RuntimeHostCallEnvelope::Ok {
            value: Value::String(target.to_string_lossy().into_owned()),
        },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "readLinkSync", &error),
        },
    })
}

#[op2]
#[serde]
pub(super) async fn op_nimbus_runtime_rename(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsRenamePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = {
        let state = state.borrow();
        state
            .borrow::<InstalledRuntimeCapabilityPolicy>()
            .paths
            .clone()
    };
    let oldpath = path_policy
        .ensure_write_path(Path::new(&payload.oldpath))
        .map_err(capability_denied_error)?;
    let newpath = path_policy
        .ensure_write_path(Path::new(&payload.newpath))
        .map_err(capability_denied_error)?;
    let response = match tokio::fs::rename(&oldpath, &newpath).await {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&oldpath, "rename", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(super) fn op_nimbus_runtime_rename_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsRenamePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .paths
        .clone();
    let oldpath = path_policy
        .ensure_write_path(Path::new(&payload.oldpath))
        .map_err(capability_denied_error)?;
    let newpath = path_policy
        .ensure_write_path(Path::new(&payload.newpath))
        .map_err(capability_denied_error)?;
    Ok(match std::fs::rename(&oldpath, &newpath) {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&oldpath, "renameSync", &error),
        },
    })
}

#[op2]
#[serde]
pub(super) fn op_nimbus_runtime_require_resolve(
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
pub(super) fn op_nimbus_runtime_require_read_file(
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
pub(super) fn op_nimbus_runtime_env_get(
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
pub(super) fn op_nimbus_runtime_env_snapshot(state: &mut OpState) -> BTreeMap<String, String> {
    let policy = state.borrow::<InstalledRuntimeCapabilityPolicy>();
    policy.env.snapshot()
}

fn is_valid_shared_env_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

#[op2]
#[serde]
pub(super) fn op_nimbus_runtime_shared_env_seed(
    #[serde] snapshot: BTreeMap<String, String>,
) -> std::result::Result<(), JsErrorBox> {
    for name in snapshot.keys() {
        if !is_valid_shared_env_name(name) {
            return Err(JsErrorBox::generic(format!(
                "invalid shared worker env variable name `{name}`"
            )));
        }
    }
    *NIMBUS_SHARED_WORKER_ENV
        .lock()
        .expect("shared worker env lock should not be poisoned") = snapshot;
    Ok(())
}

#[op2]
#[string]
pub(super) fn op_nimbus_runtime_shared_env_get(
    #[string] name: String,
) -> std::result::Result<Option<String>, JsErrorBox> {
    if !is_valid_shared_env_name(&name) {
        return Err(JsErrorBox::generic(format!(
            "invalid shared worker env variable name `{name}`"
        )));
    }
    Ok(NIMBUS_SHARED_WORKER_ENV
        .lock()
        .expect("shared worker env lock should not be poisoned")
        .get(&name)
        .cloned())
}

#[op2]
#[serde]
pub(super) fn op_nimbus_runtime_shared_env_snapshot() -> BTreeMap<String, String> {
    NIMBUS_SHARED_WORKER_ENV
        .lock()
        .expect("shared worker env lock should not be poisoned")
        .clone()
}

#[op2(fast)]
pub(super) fn op_nimbus_runtime_shared_env_set(
    #[string] name: String,
    #[string] value: String,
) -> std::result::Result<(), JsErrorBox> {
    if !is_valid_shared_env_name(&name) {
        return Err(JsErrorBox::generic(format!(
            "invalid shared worker env variable name `{name}`"
        )));
    }
    NIMBUS_SHARED_WORKER_ENV
        .lock()
        .expect("shared worker env lock should not be poisoned")
        .insert(name, value);
    Ok(())
}

#[op2(fast)]
pub(super) fn op_nimbus_runtime_shared_env_delete(
    #[string] name: String,
) -> std::result::Result<(), JsErrorBox> {
    if !is_valid_shared_env_name(&name) {
        return Err(JsErrorBox::generic(format!(
            "invalid shared worker env variable name `{name}`"
        )));
    }
    NIMBUS_SHARED_WORKER_ENV
        .lock()
        .expect("shared worker env lock should not be poisoned")
        .remove(&name);
    Ok(())
}

#[op2(fast)]
pub(super) fn op_set_raw(
    _state: &mut OpState,
    _rid: u32,
    _is_raw: bool,
    _cbreak: bool,
) -> std::result::Result<(), JsErrorBox> {
    Err(capability_denied_error(
        "raw terminal mode is not available inside the Nimbus runtime",
    ))
}

#[op2(fast)]
#[smi]
pub(super) fn op_http_start(
    _state: &mut OpState,
    #[smi] _conn_rid: u32,
) -> std::result::Result<u32, JsErrorBox> {
    Err(capability_denied_error(
        "http connection upgrade APIs are not available inside the Nimbus runtime",
    ))
}
