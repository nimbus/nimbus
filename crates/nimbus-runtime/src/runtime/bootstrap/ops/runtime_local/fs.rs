use std::borrow::Cow;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use deno_fs::{FsDirEntry, FsFileType, OpenOptions};
use deno_permissions::OpenAccessKind;
use deno_permissions::{CheckedPath, CheckedPathBuf};
use serde_json::Value;

use crate::backends::v8::embedder::{JsErrorBox, OpState, op2};
use crate::runtime::bootstrap::payloads::RuntimeHostCallEnvelope;
use crate::runtime::bootstrap::state::{InstalledRuntimeCapabilityPolicy, InstalledRuntimeOwner};

use super::support::{capability_denied_error, describe_fs_stat, runtime_fs_error_value};
use super::types::{
    RuntimeFsChmodPayload, RuntimeFsChownPayload, RuntimeFsCopyFilePayload,
    RuntimeFsDirEntryDescriptor, RuntimeFsLinkPayload, RuntimeFsMkdirPayload,
    RuntimeFsOpenValidationPayload, RuntimeFsReadDirPayload, RuntimeFsReadFilePayload,
    RuntimeFsReadFileResponse, RuntimeFsReadLinkPayload, RuntimeFsRemovePayload,
    RuntimeFsRenamePayload, RuntimeFsStatPayload, RuntimeFsSymlinkPayload, RuntimeFsUtimePayload,
    RuntimeFsWriteFilePayload,
};

fn runtime_file_system(state: &OpState) -> deno_fs::FileSystemRc {
    state
        .borrow::<InstalledRuntimeOwner>()
        .runtime
        .policy()
        .file_system()
}

fn checked_path(path: &Path) -> CheckedPath<'_> {
    CheckedPath::unsafe_new(Cow::Borrowed(path))
}

fn checked_path_buf(path: PathBuf) -> CheckedPathBuf {
    CheckedPathBuf::unsafe_new(path)
}

fn write_file_options(payload: &RuntimeFsWriteFilePayload) -> OpenOptions {
    OpenOptions::write(true, payload.append, payload.create_new, payload.mode)
}

fn write_file_data(payload: RuntimeFsWriteFilePayload) -> std::result::Result<Vec<u8>, JsErrorBox> {
    match (payload.text, payload.bytes) {
        (Some(text), None) => Ok(text.into_bytes()),
        (None, Some(bytes)) => Ok(bytes),
        (Some(_), Some(_)) => Err(JsErrorBox::generic(
            "fs.writeFile payload may contain text or bytes, but not both",
        )),
        (None, None) => Err(JsErrorBox::generic(
            "fs.writeFile payload must include text or bytes",
        )),
    }
}

fn runtime_dir_entry(entry: FsDirEntry) -> RuntimeFsDirEntryDescriptor {
    RuntimeFsDirEntryDescriptor {
        name: entry.name,
        is_file: entry.is_file,
        is_directory: entry.is_directory,
        is_symlink: entry.is_symlink,
    }
}

fn symlink_file_type(value: Option<&str>) -> std::result::Result<Option<FsFileType>, JsErrorBox> {
    match value {
        None => Ok(None),
        Some("file") => Ok(Some(FsFileType::File)),
        Some("dir") => Ok(Some(FsFileType::Directory)),
        Some("junction") => Ok(Some(FsFileType::Junction)),
        Some(other) => Err(JsErrorBox::generic(format!(
            "unsupported symlink file type `{other}`"
        ))),
    }
}

fn chown_ids(uid: i64, gid: i64) -> std::result::Result<(Option<u32>, Option<u32>), JsErrorBox> {
    Ok((chown_id(uid, "uid")?, chown_id(gid, "gid")?))
}

fn chown_id(value: i64, label: &str) -> std::result::Result<Option<u32>, JsErrorBox> {
    if value == -1 {
        return Ok(None);
    }
    u32::try_from(value)
        .map(Some)
        .map_err(|_| JsErrorBox::generic(format!("{} is out of range", label)))
}

#[op2]
#[serde]
pub(in super::super) async fn op_nimbus_runtime_fs_read_file(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsReadFilePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let (permissions, fs) = {
        let state = state.borrow();
        (
            state
                .borrow::<InstalledRuntimeCapabilityPolicy>()
                .permissions
                .clone(),
            runtime_file_system(&state),
        )
    };
    let path = permissions
        .check_open(
            std::borrow::Cow::Borrowed(Path::new(&payload.path)),
            OpenAccessKind::Read,
            Some("node:fs/promises.readFile"),
        )
        .map_err(capability_denied_error)?
        .into_owned_path();
    let bytes = fs
        .read_file_async(checked_path_buf(path.clone()), OpenOptions::read())
        .await
        .map(|bytes| bytes.into_owned())
        .map_err(|error| {
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
pub(in super::super) async fn op_nimbus_runtime_fs_write_file(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsWriteFilePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let (path_policy, fs) = {
        let state = state.borrow();
        (
            state
                .borrow::<InstalledRuntimeCapabilityPolicy>()
                .paths
                .clone(),
            runtime_file_system(&state),
        )
    };
    let path = path_policy
        .ensure_write_path(Path::new(&payload.path))
        .map_err(capability_denied_error)?;
    let options = write_file_options(&payload);
    let data = write_file_data(payload)?;
    let response = match fs
        .write_file_async(
            checked_path_buf(path.clone()),
            options,
            data.into_boxed_slice(),
        )
        .await
    {
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
pub(in super::super) fn op_nimbus_runtime_fs_write_file_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsWriteFilePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let (path_policy, fs) = (
        state
            .borrow::<InstalledRuntimeCapabilityPolicy>()
            .paths
            .clone(),
        runtime_file_system(state),
    );
    let path = path_policy
        .ensure_write_path(Path::new(&payload.path))
        .map_err(capability_denied_error)?;
    let options = write_file_options(&payload);
    let data = write_file_data(payload)?;
    let response = match fs.write_file_sync(&checked_path(&path), options, &data) {
        Ok(()) => RuntimeHostCallEnvelope::Ok {
            value: serde_json::Value::Null,
        },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "writeFileSync", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_validate_open_path(
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
pub(in super::super) async fn op_nimbus_runtime_copy_file(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsCopyFilePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let (permissions, path_policy, fs) = {
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
            runtime_file_system(&state),
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
    let response = match fs
        .copy_file_async(checked_path_buf(from), checked_path_buf(to.clone()))
        .await
    {
        Ok(_) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&to, "copyFile", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_copy_file_sync(
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
    let fs = runtime_file_system(state);
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
    let response = match fs.copy_file_sync(&checked_path(&from), &checked_path(&to)) {
        Ok(_) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&to, "copyFileSync", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(in super::super) async fn op_nimbus_runtime_link(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsLinkPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let (permissions, path_policy, fs) = {
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
            runtime_file_system(&state),
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
    let response = match fs
        .link_async(checked_path_buf(oldpath), checked_path_buf(newpath.clone()))
        .await
    {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&newpath, "link", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_link_sync(
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
    let fs = runtime_file_system(state);
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
    let response = match fs.link_sync(&checked_path(&oldpath), &checked_path(&newpath)) {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&newpath, "linkSync", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(in super::super) async fn op_nimbus_runtime_stat(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsStatPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let (path_policy, fs) = {
        let state = state.borrow();
        (
            state
                .borrow::<InstalledRuntimeCapabilityPolicy>()
                .paths
                .clone(),
            runtime_file_system(&state),
        )
    };
    let path = if payload.follow_symlink {
        path_policy.ensure_read_metadata_target_path(Path::new(&payload.path))
    } else {
        path_policy.ensure_read_metadata_path(Path::new(&payload.path))
    }
    .map_err(capability_denied_error)?;
    let metadata = if payload.follow_symlink {
        fs.stat_async(checked_path_buf(path.clone())).await
    } else {
        fs.lstat_async(checked_path_buf(path.clone())).await
    };
    Ok(match metadata {
        Ok(metadata) => RuntimeHostCallEnvelope::Ok {
            value: serde_json::to_value(describe_fs_stat(&metadata))
                .map_err(|error| JsErrorBox::generic(error.to_string()))?,
        },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "stat", &error),
        },
    })
}

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_stat_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsStatPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .paths
        .clone();
    let fs = runtime_file_system(state);
    let path = if payload.follow_symlink {
        path_policy.ensure_read_metadata_target_path(Path::new(&payload.path))
    } else {
        path_policy.ensure_read_metadata_path(Path::new(&payload.path))
    }
    .map_err(capability_denied_error)?;
    let metadata = if payload.follow_symlink {
        fs.stat_sync(&checked_path(&path))
    } else {
        fs.lstat_sync(&checked_path(&path))
    };
    Ok(match metadata {
        Ok(metadata) => RuntimeHostCallEnvelope::Ok {
            value: serde_json::to_value(describe_fs_stat(&metadata))
                .map_err(|error| JsErrorBox::generic(error.to_string()))?,
        },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "statSync", &error),
        },
    })
}

#[op2]
#[serde]
pub(in super::super) async fn op_nimbus_runtime_mkdir(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsMkdirPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let (path_policy, fs) = {
        let state = state.borrow();
        (
            state
                .borrow::<InstalledRuntimeCapabilityPolicy>()
                .paths
                .clone(),
            runtime_file_system(&state),
        )
    };
    let path = path_policy
        .ensure_write_path(Path::new(&payload.path))
        .map_err(capability_denied_error)?;
    let result = fs
        .mkdir_async(
            checked_path_buf(path.clone()),
            payload.recursive,
            payload.mode,
        )
        .await;
    let response = match result {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "mkdir", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_mkdir_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsMkdirPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .paths
        .clone();
    let fs = runtime_file_system(state);
    let path = path_policy
        .ensure_write_path(Path::new(&payload.path))
        .map_err(capability_denied_error)?;
    let result = fs.mkdir_sync(&checked_path(&path), payload.recursive, payload.mode);
    let response = match result {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "mkdirSync", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(in super::super) async fn op_nimbus_runtime_chmod(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsChmodPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let (permissions, fs) = {
        let state = state.borrow();
        (
            state
                .borrow::<InstalledRuntimeCapabilityPolicy>()
                .permissions
                .clone(),
            runtime_file_system(&state),
        )
    };
    let path = permissions
        .check_open(
            std::borrow::Cow::Borrowed(Path::new(&payload.path)),
            OpenAccessKind::Write,
            Some("Deno.chmod"),
        )
        .map_err(capability_denied_error)?
        .into_owned_path();
    let response = match fs
        .chmod_async(checked_path_buf(path.clone()), payload.mode)
        .await
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
pub(in super::super) async fn op_nimbus_runtime_lchmod(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsChmodPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let (path_policy, fs) = {
        let state = state.borrow();
        (
            state
                .borrow::<InstalledRuntimeCapabilityPolicy>()
                .paths
                .clone(),
            runtime_file_system(&state),
        )
    };
    let path = path_policy
        .ensure_write_link_path(Path::new(&payload.path))
        .map_err(capability_denied_error)?;
    let response = match fs
        .lchmod_async(checked_path_buf(path.clone()), payload.mode)
        .await
    {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "lchmod", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(in super::super) async fn op_nimbus_runtime_chown(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsChownPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let (permissions, fs) = {
        let state = state.borrow();
        (
            state
                .borrow::<InstalledRuntimeCapabilityPolicy>()
                .permissions
                .clone(),
            runtime_file_system(&state),
        )
    };
    let path = permissions
        .check_open(
            std::borrow::Cow::Borrowed(Path::new(&payload.path)),
            OpenAccessKind::Write,
            Some("Deno.chown"),
        )
        .map_err(capability_denied_error)?
        .into_owned_path();
    let (uid, gid) = chown_ids(payload.uid, payload.gid)?;
    let response = match fs
        .chown_async(checked_path_buf(path.clone()), uid, gid)
        .await
    {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "chown", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_chown_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsChownPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let permissions = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .permissions
        .clone();
    let fs = runtime_file_system(state);
    let path = permissions
        .check_open(
            std::borrow::Cow::Borrowed(Path::new(&payload.path)),
            OpenAccessKind::Write,
            Some("Deno.chownSync"),
        )
        .map_err(capability_denied_error)?
        .into_owned_path();
    let (uid, gid) = chown_ids(payload.uid, payload.gid)?;
    Ok(match fs.chown_sync(&checked_path(&path), uid, gid) {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "chownSync", &error),
        },
    })
}

#[op2]
#[serde]
pub(in super::super) async fn op_nimbus_runtime_utime(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsUtimePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let (path_policy, fs) = {
        let state = state.borrow();
        (
            state
                .borrow::<InstalledRuntimeCapabilityPolicy>()
                .paths
                .clone(),
            runtime_file_system(&state),
        )
    };
    let path = path_policy
        .ensure_write_path(Path::new(&payload.path))
        .map_err(capability_denied_error)?;
    let response = match fs
        .utime_async(
            checked_path_buf(path.clone()),
            payload.atime_secs,
            payload.atime_nanos,
            payload.mtime_secs,
            payload.mtime_nanos,
        )
        .await
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
pub(in super::super) fn op_nimbus_runtime_utime_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsUtimePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .paths
        .clone();
    let fs = runtime_file_system(state);
    let path = path_policy
        .ensure_write_path(Path::new(&payload.path))
        .map_err(capability_denied_error)?;
    let response = match fs.utime_sync(
        &checked_path(&path),
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
pub(in super::super) fn op_nimbus_runtime_chmod_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsChmodPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let permissions = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .permissions
        .clone();
    let fs = runtime_file_system(state);
    let path = permissions
        .check_open(
            std::borrow::Cow::Borrowed(Path::new(&payload.path)),
            OpenAccessKind::Write,
            Some("Deno.chmodSync"),
        )
        .map_err(capability_denied_error)?
        .into_owned_path();
    let response = match fs.chmod_sync(&checked_path(&path), payload.mode) {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "chmodSync", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_lchmod_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsChmodPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .paths
        .clone();
    let fs = runtime_file_system(state);
    let path = path_policy
        .ensure_write_link_path(Path::new(&payload.path))
        .map_err(capability_denied_error)?;
    let response = match fs.lchmod_sync(&checked_path(&path), payload.mode) {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "lchmodSync", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(in super::super) async fn op_nimbus_runtime_read_dir(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsReadDirPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let (permissions, fs) = {
        let state = state.borrow();
        (
            state
                .borrow::<InstalledRuntimeCapabilityPolicy>()
                .permissions
                .clone(),
            runtime_file_system(&state),
        )
    };
    let path = permissions
        .check_open(
            std::borrow::Cow::Borrowed(Path::new(&payload.path)),
            OpenAccessKind::Read,
            Some("Deno.readDir"),
        )
        .map_err(capability_denied_error)?
        .into_owned_path();
    let directory = match fs.read_dir_async(checked_path_buf(path.clone())).await {
        Ok(directory) => directory,
        Err(error) => {
            return Ok(RuntimeHostCallEnvelope::Error {
                error: runtime_fs_error_value(&path, "readDir", &error),
            });
        }
    };
    let mut entries = Vec::new();
    while let Some(entry) = directory.next().await.map_err(|error| {
        JsErrorBox::generic(format!(
            "failed to read directory {}: {error}",
            path.display()
        ))
    })? {
        entries.push(runtime_dir_entry(entry));
    }
    Ok(RuntimeHostCallEnvelope::Ok {
        value: serde_json::to_value(entries)
            .map_err(|error| JsErrorBox::generic(error.to_string()))?,
    })
}

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_read_dir_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsReadDirPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let permissions = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .permissions
        .clone();
    let fs = runtime_file_system(state);
    let path = permissions
        .check_open(
            std::borrow::Cow::Borrowed(Path::new(&payload.path)),
            OpenAccessKind::Read,
            Some("Deno.readDirSync"),
        )
        .map_err(capability_denied_error)?
        .into_owned_path();
    let entries_iter = match fs.read_dir_sync(&checked_path(&path)) {
        Ok(entries) => entries,
        Err(error) => {
            return Ok(RuntimeHostCallEnvelope::Error {
                error: runtime_fs_error_value(&path, "readDirSync", &error),
            });
        }
    };
    let mut entries = Vec::new();
    for entry in entries_iter {
        entries.push(runtime_dir_entry(entry));
    }
    Ok(RuntimeHostCallEnvelope::Ok {
        value: serde_json::to_value(entries)
            .map_err(|error| JsErrorBox::generic(error.to_string()))?,
    })
}

#[op2]
#[serde]
pub(in super::super) async fn op_nimbus_runtime_remove(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsRemovePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let (path_policy, fs) = {
        let state = state.borrow();
        (
            state
                .borrow::<InstalledRuntimeCapabilityPolicy>()
                .paths
                .clone(),
            runtime_file_system(&state),
        )
    };
    let path = path_policy
        .ensure_write_link_path(Path::new(&payload.path))
        .map_err(capability_denied_error)?;
    let result = if payload.directory_only {
        fs.rmdir_async(checked_path_buf(path.clone())).await
    } else {
        fs.remove_async(checked_path_buf(path.clone()), payload.recursive)
            .await
    };
    Ok(match result {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "remove", &error),
        },
    })
}

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_remove_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsRemovePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .paths
        .clone();
    let fs = runtime_file_system(state);
    let path = path_policy
        .ensure_write_link_path(Path::new(&payload.path))
        .map_err(capability_denied_error)?;
    let result = if payload.directory_only {
        fs.rmdir_sync(&checked_path(&path))
    } else {
        fs.remove_sync(&checked_path(&path), payload.recursive)
    };
    Ok(match result {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "removeSync", &error),
        },
    })
}

#[op2]
#[serde]
pub(in super::super) async fn op_nimbus_runtime_symlink(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsSymlinkPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let (path_policy, fs) = {
        let state = state.borrow();
        (
            state
                .borrow::<InstalledRuntimeCapabilityPolicy>()
                .paths
                .clone(),
            runtime_file_system(&state),
        )
    };
    let newpath = path_policy
        .ensure_write_path(Path::new(&payload.newpath))
        .map_err(capability_denied_error)?;
    let oldpath = path_policy
        .ensure_symlink_target_path(Path::new(&payload.oldpath), &newpath)
        .map_err(capability_denied_error)?;
    let file_type = symlink_file_type(payload.file_type.as_deref())?;
    let result = fs
        .symlink_async(
            checked_path_buf(oldpath),
            checked_path_buf(newpath.clone()),
            file_type,
        )
        .await;
    Ok(match result {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&newpath, "symlink", &error),
        },
    })
}

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_symlink_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsSymlinkPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .paths
        .clone();
    let fs = runtime_file_system(state);
    let newpath = path_policy
        .ensure_write_path(Path::new(&payload.newpath))
        .map_err(capability_denied_error)?;
    let oldpath = path_policy
        .ensure_symlink_target_path(Path::new(&payload.oldpath), &newpath)
        .map_err(capability_denied_error)?;
    let file_type = symlink_file_type(payload.file_type.as_deref())?;
    Ok(
        match fs.symlink_sync(&checked_path(&oldpath), &checked_path(&newpath), file_type) {
            Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
            Err(error) => RuntimeHostCallEnvelope::Error {
                error: runtime_fs_error_value(&newpath, "symlinkSync", &error),
            },
        },
    )
}

#[op2]
#[serde]
pub(in super::super) async fn op_nimbus_runtime_read_link(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsReadLinkPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let (path_policy, fs) = {
        let state = state.borrow();
        (
            state
                .borrow::<InstalledRuntimeCapabilityPolicy>()
                .paths
                .clone(),
            runtime_file_system(&state),
        )
    };
    let path = path_policy
        .ensure_read_link_path(Path::new(&payload.path))
        .map_err(capability_denied_error)?;
    Ok(
        match fs.read_link_async(checked_path_buf(path.clone())).await {
            Ok(target) => {
                path_policy
                    .ensure_read_link_target_path(&target, &path)
                    .map_err(capability_denied_error)?;
                RuntimeHostCallEnvelope::Ok {
                    value: Value::String(target.to_string_lossy().into_owned()),
                }
            }
            Err(error) => RuntimeHostCallEnvelope::Error {
                error: runtime_fs_error_value(&path, "readLink", &error),
            },
        },
    )
}

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_read_link_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsReadLinkPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .paths
        .clone();
    let fs = runtime_file_system(state);
    let path = path_policy
        .ensure_read_link_path(Path::new(&payload.path))
        .map_err(capability_denied_error)?;
    Ok(match fs.read_link_sync(&checked_path(&path)) {
        Ok(target) => {
            path_policy
                .ensure_read_link_target_path(&target, &path)
                .map_err(capability_denied_error)?;
            RuntimeHostCallEnvelope::Ok {
                value: Value::String(target.to_string_lossy().into_owned()),
            }
        }
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&path, "readLinkSync", &error),
        },
    })
}

#[op2]
#[serde]
pub(in super::super) async fn op_nimbus_runtime_rename(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeFsRenamePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let (path_policy, fs) = {
        let state = state.borrow();
        (
            state
                .borrow::<InstalledRuntimeCapabilityPolicy>()
                .paths
                .clone(),
            runtime_file_system(&state),
        )
    };
    let oldpath = path_policy
        .ensure_write_path(Path::new(&payload.oldpath))
        .map_err(capability_denied_error)?;
    let newpath = path_policy
        .ensure_write_path(Path::new(&payload.newpath))
        .map_err(capability_denied_error)?;
    let response = match fs
        .rename_async(checked_path_buf(oldpath.clone()), checked_path_buf(newpath))
        .await
    {
        Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
        Err(error) => RuntimeHostCallEnvelope::Error {
            error: runtime_fs_error_value(&oldpath, "rename", &error),
        },
    };
    Ok(response)
}

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_rename_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsRenamePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .paths
        .clone();
    let fs = runtime_file_system(state);
    let oldpath = path_policy
        .ensure_write_path(Path::new(&payload.oldpath))
        .map_err(capability_denied_error)?;
    let newpath = path_policy
        .ensure_write_path(Path::new(&payload.newpath))
        .map_err(capability_denied_error)?;
    Ok(
        match fs.rename_sync(&checked_path(&oldpath), &checked_path(&newpath)) {
            Ok(()) => RuntimeHostCallEnvelope::Ok { value: Value::Null },
            Err(error) => RuntimeHostCallEnvelope::Error {
                error: runtime_fs_error_value(&oldpath, "renameSync", &error),
            },
        },
    )
}
