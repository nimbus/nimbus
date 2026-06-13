use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use deno_permissions::OpenAccessKind;
use serde_json::Value;

use crate::backends::v8::embedder::{JsErrorBox, OpState, op2};
use crate::runtime::bootstrap::payloads::RuntimeHostCallEnvelope;
use crate::runtime::bootstrap::state::InstalledRuntimeCapabilityPolicy;

use super::support::{
    apply_directory_mode, apply_fs_mode, apply_path_times, capability_denied_error,
    create_runtime_symlink, describe_metadata, runtime_fs_error_value,
};
use super::types::{
    RuntimeFsChmodPayload, RuntimeFsCopyFilePayload, RuntimeFsDirEntryDescriptor,
    RuntimeFsLinkPayload, RuntimeFsMkdirPayload, RuntimeFsOpenValidationPayload,
    RuntimeFsReadDirPayload, RuntimeFsReadFilePayload, RuntimeFsReadFileResponse,
    RuntimeFsReadLinkPayload, RuntimeFsRemovePayload, RuntimeFsRenamePayload, RuntimeFsStatPayload,
    RuntimeFsSymlinkPayload, RuntimeFsUtimePayload, RuntimeFsWriteFilePayload,
};

#[op2]
#[serde]
pub(in super::super) async fn op_nimbus_runtime_fs_read_file(
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
pub(in super::super) async fn op_nimbus_runtime_fs_write_file(
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
pub(in super::super) async fn op_nimbus_runtime_link(
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
pub(in super::super) async fn op_nimbus_runtime_stat(
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
    let path = if payload.follow_symlink {
        path_policy.ensure_read_metadata_target_path(Path::new(&payload.path))
    } else {
        path_policy.ensure_read_metadata_path(Path::new(&payload.path))
    }
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
pub(in super::super) fn op_nimbus_runtime_stat_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsStatPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .paths
        .clone();
    let path = if payload.follow_symlink {
        path_policy.ensure_read_metadata_target_path(Path::new(&payload.path))
    } else {
        path_policy.ensure_read_metadata_path(Path::new(&payload.path))
    }
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
pub(in super::super) async fn op_nimbus_runtime_mkdir(
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
pub(in super::super) fn op_nimbus_runtime_mkdir_sync(
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
pub(in super::super) async fn op_nimbus_runtime_chmod(
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
pub(in super::super) async fn op_nimbus_runtime_utime(
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
pub(in super::super) fn op_nimbus_runtime_utime_sync(
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
pub(in super::super) fn op_nimbus_runtime_chmod_sync(
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
pub(in super::super) async fn op_nimbus_runtime_read_dir(
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
pub(in super::super) fn op_nimbus_runtime_read_dir_sync(
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
pub(in super::super) async fn op_nimbus_runtime_remove(
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
        .ensure_write_link_path(Path::new(&payload.path))
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
pub(in super::super) fn op_nimbus_runtime_remove_sync(
    state: &mut OpState,
    #[serde] payload: RuntimeFsRemovePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let path_policy = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .paths
        .clone();
    let path = path_policy
        .ensure_write_link_path(Path::new(&payload.path))
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
pub(in super::super) async fn op_nimbus_runtime_symlink(
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
pub(in super::super) fn op_nimbus_runtime_symlink_sync(
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
pub(in super::super) async fn op_nimbus_runtime_read_link(
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
        .ensure_read_link_path(Path::new(&payload.path))
        .map_err(capability_denied_error)?;
    Ok(match tokio::fs::read_link(&path).await {
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
    })
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
    let path = path_policy
        .ensure_read_link_path(Path::new(&payload.path))
        .map_err(capability_denied_error)?;
    Ok(match std::fs::read_link(&path) {
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
pub(in super::super) fn op_nimbus_runtime_rename_sync(
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
