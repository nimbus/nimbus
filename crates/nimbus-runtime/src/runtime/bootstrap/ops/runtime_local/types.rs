use serde::{Deserialize, Serialize};

fn default_follow_symlink() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsReadFilePayload {
    pub(super) path: String,
    #[serde(default)]
    pub(super) encoding: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsWriteFilePayload {
    pub(super) path: String,
    #[serde(default)]
    pub(super) text: Option<String>,
    #[serde(default)]
    pub(super) bytes: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsOpenValidationPayload {
    pub(super) path: String,
    #[serde(default)]
    pub(super) write: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsCopyFilePayload {
    pub(super) from: String,
    pub(super) to: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsLinkPayload {
    pub(super) oldpath: String,
    pub(super) newpath: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsStatPayload {
    pub(super) path: String,
    #[serde(default = "default_follow_symlink")]
    pub(super) follow_symlink: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsMkdirPayload {
    pub(super) path: String,
    #[serde(default)]
    pub(super) recursive: bool,
    #[serde(default)]
    pub(super) mode: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsReadDirPayload {
    pub(super) path: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsChmodPayload {
    pub(super) path: String,
    pub(super) mode: u32,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsUtimePayload {
    pub(super) path: String,
    pub(super) atime_secs: i64,
    pub(super) atime_nanos: u32,
    pub(super) mtime_secs: i64,
    pub(super) mtime_nanos: u32,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsRemovePayload {
    pub(super) path: String,
    #[serde(default)]
    pub(super) recursive: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsSymlinkPayload {
    pub(super) oldpath: String,
    pub(super) newpath: String,
    #[serde(default)]
    pub(super) file_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsReadLinkPayload {
    pub(super) path: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeFsRenamePayload {
    pub(super) oldpath: String,
    pub(super) newpath: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeRequireResolvePayload {
    pub(super) specifier: String,
    #[serde(default)]
    pub(super) referrer: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeRequireReadFilePayload {
    pub(super) path: String,
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
    pub(super) is_file: bool,
    pub(super) is_directory: bool,
    pub(super) is_symlink: bool,
    pub(super) size: u64,
    pub(super) mtime_ms: Option<i64>,
    pub(super) atime_ms: Option<i64>,
    pub(super) birthtime_ms: Option<i64>,
    pub(super) ctime_ms: Option<i64>,
    pub(super) mode: Option<u32>,
    pub(super) dev: Option<u64>,
    pub(super) ino: Option<u64>,
    pub(super) nlink: Option<u64>,
    pub(super) uid: Option<u32>,
    pub(super) gid: Option<u32>,
    pub(super) rdev: Option<u64>,
    pub(super) blksize: Option<u64>,
    pub(super) blocks: Option<u64>,
    pub(super) is_block_device: bool,
    pub(super) is_char_device: bool,
    pub(super) is_fifo: bool,
    pub(super) is_socket: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RuntimeFsDirEntryDescriptor {
    pub(super) name: String,
    pub(super) is_file: bool,
    pub(super) is_directory: bool,
    pub(super) is_symlink: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub(super) enum RuntimeRequireResolveResponse {
    Builtin { module_name: String },
    CommonJs { path: String },
    EsModule { path: String },
    Json { path: String },
}
