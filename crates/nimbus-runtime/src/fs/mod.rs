use std::fmt;
use std::io;
use std::path::PathBuf;
use std::rc::Rc;

use deno_fs::sync::MaybeArc;
use deno_fs::{FsDirEntry, FsFileType, FsReadDirRc, OpenOptions};
use deno_io::fs::{File, FsResult, FsStat, FsStatFs};
use deno_permissions::{CheckedPath, CheckedPathBuf};

/// Runtime-local filesystem backend seam for in-process V8 filesystem access.
///
/// This trait intentionally lives in `nimbus-runtime` because that crate speaks
/// Deno's `FileSystemRc` ABI. Implementations live outside `nimbus-runtime` so
/// the runtime keeps its zero-workspace-dependency invariant.
pub trait NimbusFsBackend: deno_fs::FileSystem {}

impl<T> NimbusFsBackend for T where T: deno_fs::FileSystem + ?Sized {}

#[derive(Clone)]
pub struct RuntimeFileSystem {
    inner: deno_fs::FileSystemRc,
}

impl RuntimeFileSystem {
    pub fn new(inner: deno_fs::FileSystemRc) -> Self {
        Self { inner }
    }

    pub fn clone_inner(&self) -> deno_fs::FileSystemRc {
        self.inner.clone()
    }
}

impl Default for RuntimeFileSystem {
    fn default() -> Self {
        Self::new(MaybeArc::new(DenyFileSystem))
    }
}

impl fmt::Debug for RuntimeFileSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeFileSystem").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DenyFileSystem;

fn denied<T>() -> FsResult<T> {
    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "runtime filesystem authority is not configured",
    )
    .into())
}

#[async_trait::async_trait(?Send)]
impl deno_fs::FileSystem for DenyFileSystem {
    fn cwd(&self) -> FsResult<PathBuf> {
        Ok(PathBuf::from("/"))
    }

    fn tmp_dir(&self) -> FsResult<PathBuf> {
        denied()
    }

    fn chdir(&self, _path: &CheckedPath<'_>) -> FsResult<()> {
        denied()
    }

    fn umask(&self, _mask: Option<u32>) -> FsResult<u32> {
        denied()
    }

    fn open_sync(&self, _path: &CheckedPath<'_>, _options: OpenOptions) -> FsResult<Rc<dyn File>> {
        denied()
    }

    async fn open_async<'a>(
        &'a self,
        _path: CheckedPathBuf,
        _options: OpenOptions,
    ) -> FsResult<Rc<dyn File>> {
        denied()
    }

    fn mkdir_sync(
        &self,
        _path: &CheckedPath<'_>,
        _recursive: bool,
        _mode: Option<u32>,
    ) -> FsResult<()> {
        denied()
    }

    async fn mkdir_async(
        &self,
        _path: CheckedPathBuf,
        _recursive: bool,
        _mode: Option<u32>,
    ) -> FsResult<()> {
        denied()
    }

    #[cfg(unix)]
    fn chmod_sync(&self, _path: &CheckedPath<'_>, _mode: u32) -> FsResult<()> {
        denied()
    }

    #[cfg(not(unix))]
    fn chmod_sync(&self, _path: &CheckedPath<'_>, _mode: i32) -> FsResult<()> {
        denied()
    }

    #[cfg(unix)]
    async fn chmod_async(&self, _path: CheckedPathBuf, _mode: u32) -> FsResult<()> {
        denied()
    }

    #[cfg(not(unix))]
    async fn chmod_async(&self, _path: CheckedPathBuf, _mode: i32) -> FsResult<()> {
        denied()
    }

    fn chown_sync(
        &self,
        _path: &CheckedPath<'_>,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        denied()
    }

    async fn chown_async(
        &self,
        _path: CheckedPathBuf,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        denied()
    }

    fn lchmod_sync(&self, _path: &CheckedPath<'_>, _mode: u32) -> FsResult<()> {
        denied()
    }

    async fn lchmod_async(&self, _path: CheckedPathBuf, _mode: u32) -> FsResult<()> {
        denied()
    }

    fn lchown_sync(
        &self,
        _path: &CheckedPath<'_>,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        denied()
    }

    async fn lchown_async(
        &self,
        _path: CheckedPathBuf,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        denied()
    }

    fn remove_sync(&self, _path: &CheckedPath<'_>, _recursive: bool) -> FsResult<()> {
        denied()
    }

    async fn remove_async(&self, _path: CheckedPathBuf, _recursive: bool) -> FsResult<()> {
        denied()
    }

    fn copy_file_sync(
        &self,
        _oldpath: &CheckedPath<'_>,
        _newpath: &CheckedPath<'_>,
    ) -> FsResult<()> {
        denied()
    }

    async fn copy_file_async(
        &self,
        _oldpath: CheckedPathBuf,
        _newpath: CheckedPathBuf,
    ) -> FsResult<()> {
        denied()
    }

    fn cp_sync(&self, _path: &CheckedPath<'_>, _new_path: &CheckedPath<'_>) -> FsResult<()> {
        denied()
    }

    async fn cp_async(&self, _path: CheckedPathBuf, _new_path: CheckedPathBuf) -> FsResult<()> {
        denied()
    }

    fn stat_sync(&self, _path: &CheckedPath<'_>) -> FsResult<FsStat> {
        denied()
    }

    async fn stat_async(&self, _path: CheckedPathBuf) -> FsResult<FsStat> {
        denied()
    }

    fn lstat_sync(&self, _path: &CheckedPath<'_>) -> FsResult<FsStat> {
        denied()
    }

    async fn lstat_async(&self, _path: CheckedPathBuf) -> FsResult<FsStat> {
        denied()
    }

    fn statfs_sync(&self, _path: &CheckedPath<'_>, _bigint: bool) -> FsResult<FsStatFs> {
        denied()
    }

    async fn statfs_async(&self, _path: CheckedPathBuf, _bigint: bool) -> FsResult<FsStatFs> {
        denied()
    }

    fn realpath_sync(&self, _path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        denied()
    }

    async fn realpath_async(&self, _path: CheckedPathBuf) -> FsResult<PathBuf> {
        denied()
    }

    fn read_dir_sync(&self, _path: &CheckedPath<'_>) -> FsResult<Vec<FsDirEntry>> {
        denied()
    }

    async fn read_dir_async(&self, _path: CheckedPathBuf) -> FsResult<FsReadDirRc> {
        denied()
    }

    fn rename_sync(&self, _oldpath: &CheckedPath<'_>, _newpath: &CheckedPath<'_>) -> FsResult<()> {
        denied()
    }

    async fn rename_async(
        &self,
        _oldpath: CheckedPathBuf,
        _newpath: CheckedPathBuf,
    ) -> FsResult<()> {
        denied()
    }

    fn rmdir_sync(&self, _path: &CheckedPath<'_>) -> FsResult<()> {
        denied()
    }

    async fn rmdir_async(&self, _path: CheckedPathBuf) -> FsResult<()> {
        denied()
    }

    fn link_sync(&self, _oldpath: &CheckedPath<'_>, _newpath: &CheckedPath<'_>) -> FsResult<()> {
        denied()
    }

    async fn link_async(&self, _oldpath: CheckedPathBuf, _newpath: CheckedPathBuf) -> FsResult<()> {
        denied()
    }

    fn symlink_sync(
        &self,
        _oldpath: &CheckedPath<'_>,
        _newpath: &CheckedPath<'_>,
        _file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        denied()
    }

    async fn symlink_async(
        &self,
        _oldpath: CheckedPathBuf,
        _newpath: CheckedPathBuf,
        _file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        denied()
    }

    fn read_link_sync(&self, _path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        denied()
    }

    async fn read_link_async(&self, _path: CheckedPathBuf) -> FsResult<PathBuf> {
        denied()
    }

    fn truncate_sync(&self, _path: &CheckedPath<'_>, _len: u64) -> FsResult<()> {
        denied()
    }

    async fn truncate_async(&self, _path: CheckedPathBuf, _len: u64) -> FsResult<()> {
        denied()
    }

    fn utime_sync(
        &self,
        _path: &CheckedPath<'_>,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        denied()
    }

    async fn utime_async(
        &self,
        _path: CheckedPathBuf,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        denied()
    }

    fn lutime_sync(
        &self,
        _path: &CheckedPath<'_>,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        denied()
    }

    async fn lutime_async(
        &self,
        _path: CheckedPathBuf,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        denied()
    }

    fn exists_sync(&self, _path: &CheckedPath<'_>) -> bool {
        false
    }

    async fn exists_async(&self, _path: CheckedPathBuf) -> FsResult<bool> {
        Ok(false)
    }
}
