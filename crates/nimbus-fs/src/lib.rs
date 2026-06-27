//! NimbusFS: the in-process filesystem shell for V8 and WASI binders.
//!
//! `nimbus-runtime` owns only the Deno ABI-facing seam. This crate owns the
//! concrete shell and backends, then `nimbus-server` wires the default shell into
//! runtime construction.

use std::borrow::Cow;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;

use deno_fs::sync::MaybeArc;
use deno_fs::{FileSystem, FsDirEntry, FsFileType, FsReadDirRc, OpenOptions};
use deno_io::fs::{File, FsResult, FsStat, FsStatFs};
use deno_permissions::{CheckedPath, CheckedPathBuf};
use nimbus_runtime::NimbusFsBackend;

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub struct NimbusFs {
    backend: deno_fs::FileSystemRc,
    cwd: Mutex<PathBuf>,
}

impl NimbusFs {
    pub fn new<B>(backend: B) -> Self
    where
        B: NimbusFsBackend + 'static,
    {
        Self::with_backend_rc(MaybeArc::new(backend), configured_process_cwd())
    }

    pub fn with_cwd<B>(backend: B, cwd: impl Into<PathBuf>) -> Self
    where
        B: NimbusFsBackend + 'static,
    {
        Self::with_backend_rc(MaybeArc::new(backend), cwd)
    }

    pub fn with_backend_rc(backend: deno_fs::FileSystemRc, cwd: impl Into<PathBuf>) -> Self {
        Self {
            backend,
            cwd: Mutex::new(cwd.into()),
        }
    }

    fn cwd_path(&self) -> FsResult<PathBuf> {
        self.cwd
            .lock()
            .map(|cwd| cwd.clone())
            .map_err(|_| io::Error::other("NimbusFS cwd lock poisoned").into())
    }

    fn resolve_path(&self, path: &Path) -> FsResult<PathBuf> {
        if path.is_absolute() {
            return Ok(path.to_path_buf());
        }
        Ok(self.cwd_path()?.join(path))
    }

    fn checked(&self, path: &CheckedPath<'_>) -> FsResult<CheckedPathBuf> {
        Ok(CheckedPathBuf::unsafe_new(self.resolve_path(path)?))
    }

    fn checked_buf(&self, path: CheckedPathBuf) -> FsResult<CheckedPathBuf> {
        Ok(CheckedPathBuf::unsafe_new(
            self.resolve_path(&path.into_path_buf())?,
        ))
    }

    fn checked_pair(
        &self,
        oldpath: &CheckedPath<'_>,
        newpath: &CheckedPath<'_>,
    ) -> FsResult<(CheckedPathBuf, CheckedPathBuf)> {
        Ok((self.checked(oldpath)?, self.checked(newpath)?))
    }

    fn checked_buf_pair(
        &self,
        oldpath: CheckedPathBuf,
        newpath: CheckedPathBuf,
    ) -> FsResult<(CheckedPathBuf, CheckedPathBuf)> {
        Ok((self.checked_buf(oldpath)?, self.checked_buf(newpath)?))
    }
}

fn configured_process_cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
}

#[derive(Debug, Default, Clone)]
pub struct PassthroughBackend {
    inner: deno_fs::RealFs,
}

impl PassthroughBackend {
    pub fn new() -> Self {
        Self {
            inner: deno_fs::RealFs,
        }
    }
}

pub fn default_file_system() -> deno_fs::FileSystemRc {
    MaybeArc::new(NimbusFs::new(PassthroughBackend::new()))
}

#[async_trait::async_trait(?Send)]
impl FileSystem for NimbusFs {
    fn cwd(&self) -> FsResult<PathBuf> {
        self.cwd_path()
    }

    fn tmp_dir(&self) -> FsResult<PathBuf> {
        self.backend.tmp_dir()
    }

    fn chdir(&self, path: &CheckedPath<'_>) -> FsResult<()> {
        let resolved = self.resolve_path(path)?;
        let stat = self
            .backend
            .stat_sync(&CheckedPath::unsafe_new(Cow::Borrowed(resolved.as_path())))?;
        if !stat.is_directory {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                format!("{} is not a directory", resolved.display()),
            )
            .into());
        }
        *self
            .cwd
            .lock()
            .map_err(|_| io::Error::other("NimbusFS cwd lock poisoned"))? = resolved;
        Ok(())
    }

    fn umask(&self, mask: Option<u32>) -> FsResult<u32> {
        self.backend.umask(mask)
    }

    fn open_sync(&self, path: &CheckedPath<'_>, options: OpenOptions) -> FsResult<Rc<dyn File>> {
        let path = self.checked(path)?;
        self.backend.open_sync(&path.as_checked_path(), options)
    }

    async fn open_async<'a>(
        &'a self,
        path: CheckedPathBuf,
        options: OpenOptions,
    ) -> FsResult<Rc<dyn File>> {
        self.backend
            .open_async(self.checked_buf(path)?, options)
            .await
    }

    fn mkdir_sync(
        &self,
        path: &CheckedPath<'_>,
        recursive: bool,
        mode: Option<u32>,
    ) -> FsResult<()> {
        let path = self.checked(path)?;
        self.backend
            .mkdir_sync(&path.as_checked_path(), recursive, mode)
    }

    async fn mkdir_async(
        &self,
        path: CheckedPathBuf,
        recursive: bool,
        mode: Option<u32>,
    ) -> FsResult<()> {
        self.backend
            .mkdir_async(self.checked_buf(path)?, recursive, mode)
            .await
    }

    #[cfg(unix)]
    fn chmod_sync(&self, path: &CheckedPath<'_>, mode: u32) -> FsResult<()> {
        let path = self.checked(path)?;
        self.backend.chmod_sync(&path.as_checked_path(), mode)
    }

    #[cfg(not(unix))]
    fn chmod_sync(&self, path: &CheckedPath<'_>, mode: i32) -> FsResult<()> {
        let path = self.checked(path)?;
        self.backend.chmod_sync(&path.as_checked_path(), mode)
    }

    #[cfg(unix)]
    async fn chmod_async(&self, path: CheckedPathBuf, mode: u32) -> FsResult<()> {
        self.backend
            .chmod_async(self.checked_buf(path)?, mode)
            .await
    }

    #[cfg(not(unix))]
    async fn chmod_async(&self, path: CheckedPathBuf, mode: i32) -> FsResult<()> {
        self.backend
            .chmod_async(self.checked_buf(path)?, mode)
            .await
    }

    fn chown_sync(
        &self,
        path: &CheckedPath<'_>,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        let path = self.checked(path)?;
        self.backend.chown_sync(&path.as_checked_path(), uid, gid)
    }

    async fn chown_async(
        &self,
        path: CheckedPathBuf,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        self.backend
            .chown_async(self.checked_buf(path)?, uid, gid)
            .await
    }

    fn lchmod_sync(&self, path: &CheckedPath<'_>, mode: u32) -> FsResult<()> {
        let path = self.checked(path)?;
        self.backend.lchmod_sync(&path.as_checked_path(), mode)
    }

    async fn lchmod_async(&self, path: CheckedPathBuf, mode: u32) -> FsResult<()> {
        self.backend
            .lchmod_async(self.checked_buf(path)?, mode)
            .await
    }

    fn lchown_sync(
        &self,
        path: &CheckedPath<'_>,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        let path = self.checked(path)?;
        self.backend.lchown_sync(&path.as_checked_path(), uid, gid)
    }

    async fn lchown_async(
        &self,
        path: CheckedPathBuf,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        self.backend
            .lchown_async(self.checked_buf(path)?, uid, gid)
            .await
    }

    fn remove_sync(&self, path: &CheckedPath<'_>, recursive: bool) -> FsResult<()> {
        let path = self.checked(path)?;
        self.backend.remove_sync(&path.as_checked_path(), recursive)
    }

    async fn remove_async(&self, path: CheckedPathBuf, recursive: bool) -> FsResult<()> {
        self.backend
            .remove_async(self.checked_buf(path)?, recursive)
            .await
    }

    fn copy_file_sync(&self, oldpath: &CheckedPath<'_>, newpath: &CheckedPath<'_>) -> FsResult<()> {
        let (oldpath, newpath) = self.checked_pair(oldpath, newpath)?;
        self.backend
            .copy_file_sync(&oldpath.as_checked_path(), &newpath.as_checked_path())
    }

    async fn copy_file_async(
        &self,
        oldpath: CheckedPathBuf,
        newpath: CheckedPathBuf,
    ) -> FsResult<()> {
        let (oldpath, newpath) = self.checked_buf_pair(oldpath, newpath)?;
        self.backend.copy_file_async(oldpath, newpath).await
    }

    fn cp_sync(&self, path: &CheckedPath<'_>, new_path: &CheckedPath<'_>) -> FsResult<()> {
        let (path, new_path) = self.checked_pair(path, new_path)?;
        self.backend
            .cp_sync(&path.as_checked_path(), &new_path.as_checked_path())
    }

    async fn cp_async(&self, path: CheckedPathBuf, new_path: CheckedPathBuf) -> FsResult<()> {
        let (path, new_path) = self.checked_buf_pair(path, new_path)?;
        self.backend.cp_async(path, new_path).await
    }

    fn stat_sync(&self, path: &CheckedPath<'_>) -> FsResult<FsStat> {
        let path = self.checked(path)?;
        self.backend.stat_sync(&path.as_checked_path())
    }

    async fn stat_async(&self, path: CheckedPathBuf) -> FsResult<FsStat> {
        self.backend.stat_async(self.checked_buf(path)?).await
    }

    fn lstat_sync(&self, path: &CheckedPath<'_>) -> FsResult<FsStat> {
        let path = self.checked(path)?;
        self.backend.lstat_sync(&path.as_checked_path())
    }

    async fn lstat_async(&self, path: CheckedPathBuf) -> FsResult<FsStat> {
        self.backend.lstat_async(self.checked_buf(path)?).await
    }

    fn statfs_sync(&self, path: &CheckedPath<'_>, bigint: bool) -> FsResult<FsStatFs> {
        let path = self.checked(path)?;
        self.backend.statfs_sync(&path.as_checked_path(), bigint)
    }

    async fn statfs_async(&self, path: CheckedPathBuf, bigint: bool) -> FsResult<FsStatFs> {
        self.backend
            .statfs_async(self.checked_buf(path)?, bigint)
            .await
    }

    fn realpath_sync(&self, path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        let path = self.checked(path)?;
        self.backend.realpath_sync(&path.as_checked_path())
    }

    async fn realpath_async(&self, path: CheckedPathBuf) -> FsResult<PathBuf> {
        self.backend.realpath_async(self.checked_buf(path)?).await
    }

    fn read_dir_sync(&self, path: &CheckedPath<'_>) -> FsResult<Vec<FsDirEntry>> {
        let path = self.checked(path)?;
        self.backend.read_dir_sync(&path.as_checked_path())
    }

    async fn read_dir_async(&self, path: CheckedPathBuf) -> FsResult<FsReadDirRc> {
        self.backend.read_dir_async(self.checked_buf(path)?).await
    }

    fn rename_sync(&self, oldpath: &CheckedPath<'_>, newpath: &CheckedPath<'_>) -> FsResult<()> {
        let (oldpath, newpath) = self.checked_pair(oldpath, newpath)?;
        self.backend
            .rename_sync(&oldpath.as_checked_path(), &newpath.as_checked_path())
    }

    async fn rename_async(&self, oldpath: CheckedPathBuf, newpath: CheckedPathBuf) -> FsResult<()> {
        let (oldpath, newpath) = self.checked_buf_pair(oldpath, newpath)?;
        self.backend.rename_async(oldpath, newpath).await
    }

    fn rmdir_sync(&self, path: &CheckedPath<'_>) -> FsResult<()> {
        let path = self.checked(path)?;
        self.backend.rmdir_sync(&path.as_checked_path())
    }

    async fn rmdir_async(&self, path: CheckedPathBuf) -> FsResult<()> {
        self.backend.rmdir_async(self.checked_buf(path)?).await
    }

    fn link_sync(&self, oldpath: &CheckedPath<'_>, newpath: &CheckedPath<'_>) -> FsResult<()> {
        let (oldpath, newpath) = self.checked_pair(oldpath, newpath)?;
        self.backend
            .link_sync(&oldpath.as_checked_path(), &newpath.as_checked_path())
    }

    async fn link_async(&self, oldpath: CheckedPathBuf, newpath: CheckedPathBuf) -> FsResult<()> {
        let (oldpath, newpath) = self.checked_buf_pair(oldpath, newpath)?;
        self.backend.link_async(oldpath, newpath).await
    }

    fn symlink_sync(
        &self,
        oldpath: &CheckedPath<'_>,
        newpath: &CheckedPath<'_>,
        file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        let newpath = self.checked(newpath)?;
        self.backend
            .symlink_sync(oldpath, &newpath.as_checked_path(), file_type)
    }

    async fn symlink_async(
        &self,
        oldpath: CheckedPathBuf,
        newpath: CheckedPathBuf,
        file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        self.backend
            .symlink_async(oldpath, self.checked_buf(newpath)?, file_type)
            .await
    }

    fn read_link_sync(&self, path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        let path = self.checked(path)?;
        self.backend.read_link_sync(&path.as_checked_path())
    }

    async fn read_link_async(&self, path: CheckedPathBuf) -> FsResult<PathBuf> {
        self.backend.read_link_async(self.checked_buf(path)?).await
    }

    fn truncate_sync(&self, path: &CheckedPath<'_>, len: u64) -> FsResult<()> {
        let path = self.checked(path)?;
        self.backend.truncate_sync(&path.as_checked_path(), len)
    }

    async fn truncate_async(&self, path: CheckedPathBuf, len: u64) -> FsResult<()> {
        self.backend
            .truncate_async(self.checked_buf(path)?, len)
            .await
    }

    fn utime_sync(
        &self,
        path: &CheckedPath<'_>,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        let path = self.checked(path)?;
        self.backend.utime_sync(
            &path.as_checked_path(),
            atime_secs,
            atime_nanos,
            mtime_secs,
            mtime_nanos,
        )
    }

    async fn utime_async(
        &self,
        path: CheckedPathBuf,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        self.backend
            .utime_async(
                self.checked_buf(path)?,
                atime_secs,
                atime_nanos,
                mtime_secs,
                mtime_nanos,
            )
            .await
    }

    fn lutime_sync(
        &self,
        path: &CheckedPath<'_>,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        let path = self.checked(path)?;
        self.backend.lutime_sync(
            &path.as_checked_path(),
            atime_secs,
            atime_nanos,
            mtime_secs,
            mtime_nanos,
        )
    }

    async fn lutime_async(
        &self,
        path: CheckedPathBuf,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        self.backend
            .lutime_async(
                self.checked_buf(path)?,
                atime_secs,
                atime_nanos,
                mtime_secs,
                mtime_nanos,
            )
            .await
    }

    fn exists_sync(&self, path: &CheckedPath<'_>) -> bool {
        let Ok(path) = self.checked(path) else {
            return false;
        };
        self.backend.exists_sync(&path.as_checked_path())
    }

    async fn exists_async(&self, path: CheckedPathBuf) -> FsResult<bool> {
        self.backend.exists_async(self.checked_buf(path)?).await
    }
}

#[async_trait::async_trait(?Send)]
impl FileSystem for PassthroughBackend {
    fn cwd(&self) -> FsResult<PathBuf> {
        self.inner.cwd()
    }

    fn tmp_dir(&self) -> FsResult<PathBuf> {
        self.inner.tmp_dir()
    }

    fn chdir(&self, path: &CheckedPath<'_>) -> FsResult<()> {
        self.inner.chdir(path)
    }

    fn umask(&self, mask: Option<u32>) -> FsResult<u32> {
        self.inner.umask(mask)
    }

    fn open_sync(&self, path: &CheckedPath<'_>, options: OpenOptions) -> FsResult<Rc<dyn File>> {
        self.inner.open_sync(path, options)
    }

    async fn open_async<'a>(
        &'a self,
        path: CheckedPathBuf,
        options: OpenOptions,
    ) -> FsResult<Rc<dyn File>> {
        self.inner.open_async(path, options).await
    }

    fn mkdir_sync(
        &self,
        path: &CheckedPath<'_>,
        recursive: bool,
        mode: Option<u32>,
    ) -> FsResult<()> {
        self.inner.mkdir_sync(path, recursive, mode)
    }

    async fn mkdir_async(
        &self,
        path: CheckedPathBuf,
        recursive: bool,
        mode: Option<u32>,
    ) -> FsResult<()> {
        self.inner.mkdir_async(path, recursive, mode).await
    }

    #[cfg(unix)]
    fn chmod_sync(&self, path: &CheckedPath<'_>, mode: u32) -> FsResult<()> {
        self.inner.chmod_sync(path, mode)
    }

    #[cfg(not(unix))]
    fn chmod_sync(&self, path: &CheckedPath<'_>, mode: i32) -> FsResult<()> {
        self.inner.chmod_sync(path, mode)
    }

    #[cfg(unix)]
    async fn chmod_async(&self, path: CheckedPathBuf, mode: u32) -> FsResult<()> {
        self.inner.chmod_async(path, mode).await
    }

    #[cfg(not(unix))]
    async fn chmod_async(&self, path: CheckedPathBuf, mode: i32) -> FsResult<()> {
        self.inner.chmod_async(path, mode).await
    }

    fn chown_sync(
        &self,
        path: &CheckedPath<'_>,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        self.inner.chown_sync(path, uid, gid)
    }

    async fn chown_async(
        &self,
        path: CheckedPathBuf,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        self.inner.chown_async(path, uid, gid).await
    }

    fn lchmod_sync(&self, path: &CheckedPath<'_>, mode: u32) -> FsResult<()> {
        self.inner.lchmod_sync(path, mode)
    }

    async fn lchmod_async(&self, path: CheckedPathBuf, mode: u32) -> FsResult<()> {
        self.inner.lchmod_async(path, mode).await
    }

    fn lchown_sync(
        &self,
        path: &CheckedPath<'_>,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        self.inner.lchown_sync(path, uid, gid)
    }

    async fn lchown_async(
        &self,
        path: CheckedPathBuf,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        self.inner.lchown_async(path, uid, gid).await
    }

    fn remove_sync(&self, path: &CheckedPath<'_>, recursive: bool) -> FsResult<()> {
        self.inner.remove_sync(path, recursive)
    }

    async fn remove_async(&self, path: CheckedPathBuf, recursive: bool) -> FsResult<()> {
        self.inner.remove_async(path, recursive).await
    }

    fn copy_file_sync(&self, oldpath: &CheckedPath<'_>, newpath: &CheckedPath<'_>) -> FsResult<()> {
        self.inner.copy_file_sync(oldpath, newpath)
    }

    async fn copy_file_async(
        &self,
        oldpath: CheckedPathBuf,
        newpath: CheckedPathBuf,
    ) -> FsResult<()> {
        self.inner.copy_file_async(oldpath, newpath).await
    }

    fn cp_sync(&self, path: &CheckedPath<'_>, new_path: &CheckedPath<'_>) -> FsResult<()> {
        self.inner.cp_sync(path, new_path)
    }

    async fn cp_async(&self, path: CheckedPathBuf, new_path: CheckedPathBuf) -> FsResult<()> {
        self.inner.cp_async(path, new_path).await
    }

    fn stat_sync(&self, path: &CheckedPath<'_>) -> FsResult<FsStat> {
        self.inner.stat_sync(path)
    }

    async fn stat_async(&self, path: CheckedPathBuf) -> FsResult<FsStat> {
        self.inner.stat_async(path).await
    }

    fn lstat_sync(&self, path: &CheckedPath<'_>) -> FsResult<FsStat> {
        self.inner.lstat_sync(path)
    }

    async fn lstat_async(&self, path: CheckedPathBuf) -> FsResult<FsStat> {
        self.inner.lstat_async(path).await
    }

    fn statfs_sync(&self, path: &CheckedPath<'_>, bigint: bool) -> FsResult<FsStatFs> {
        self.inner.statfs_sync(path, bigint)
    }

    async fn statfs_async(&self, path: CheckedPathBuf, bigint: bool) -> FsResult<FsStatFs> {
        self.inner.statfs_async(path, bigint).await
    }

    fn realpath_sync(&self, path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        self.inner.realpath_sync(path)
    }

    async fn realpath_async(&self, path: CheckedPathBuf) -> FsResult<PathBuf> {
        self.inner.realpath_async(path).await
    }

    fn read_dir_sync(&self, path: &CheckedPath<'_>) -> FsResult<Vec<FsDirEntry>> {
        self.inner.read_dir_sync(path)
    }

    async fn read_dir_async(&self, path: CheckedPathBuf) -> FsResult<FsReadDirRc> {
        self.inner.read_dir_async(path).await
    }

    fn rename_sync(&self, oldpath: &CheckedPath<'_>, newpath: &CheckedPath<'_>) -> FsResult<()> {
        self.inner.rename_sync(oldpath, newpath)
    }

    async fn rename_async(&self, oldpath: CheckedPathBuf, newpath: CheckedPathBuf) -> FsResult<()> {
        self.inner.rename_async(oldpath, newpath).await
    }

    fn rmdir_sync(&self, path: &CheckedPath<'_>) -> FsResult<()> {
        self.inner.rmdir_sync(path)
    }

    async fn rmdir_async(&self, path: CheckedPathBuf) -> FsResult<()> {
        self.inner.rmdir_async(path).await
    }

    fn link_sync(&self, oldpath: &CheckedPath<'_>, newpath: &CheckedPath<'_>) -> FsResult<()> {
        self.inner.link_sync(oldpath, newpath)
    }

    async fn link_async(&self, oldpath: CheckedPathBuf, newpath: CheckedPathBuf) -> FsResult<()> {
        self.inner.link_async(oldpath, newpath).await
    }

    fn symlink_sync(
        &self,
        oldpath: &CheckedPath<'_>,
        newpath: &CheckedPath<'_>,
        file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        self.inner.symlink_sync(oldpath, newpath, file_type)
    }

    async fn symlink_async(
        &self,
        oldpath: CheckedPathBuf,
        newpath: CheckedPathBuf,
        file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        self.inner.symlink_async(oldpath, newpath, file_type).await
    }

    fn read_link_sync(&self, path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        self.inner.read_link_sync(path)
    }

    async fn read_link_async(&self, path: CheckedPathBuf) -> FsResult<PathBuf> {
        self.inner.read_link_async(path).await
    }

    fn truncate_sync(&self, path: &CheckedPath<'_>, len: u64) -> FsResult<()> {
        self.inner.truncate_sync(path, len)
    }

    async fn truncate_async(&self, path: CheckedPathBuf, len: u64) -> FsResult<()> {
        self.inner.truncate_async(path, len).await
    }

    fn utime_sync(
        &self,
        path: &CheckedPath<'_>,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        self.inner
            .utime_sync(path, atime_secs, atime_nanos, mtime_secs, mtime_nanos)
    }

    async fn utime_async(
        &self,
        path: CheckedPathBuf,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        self.inner
            .utime_async(path, atime_secs, atime_nanos, mtime_secs, mtime_nanos)
            .await
    }

    fn lutime_sync(
        &self,
        path: &CheckedPath<'_>,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        self.inner
            .lutime_sync(path, atime_secs, atime_nanos, mtime_secs, mtime_nanos)
    }

    async fn lutime_async(
        &self,
        path: CheckedPathBuf,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        self.inner
            .lutime_async(path, atime_secs, atime_nanos, mtime_secs, mtime_nanos)
            .await
    }

    fn exists_sync(&self, path: &CheckedPath<'_>) -> bool {
        self.inner.exists_sync(path)
    }

    async fn exists_async(&self, path: CheckedPathBuf) -> FsResult<bool> {
        self.inner.exists_async(path).await
    }
}
