//! NimbusFS: the in-process filesystem shell for V8 and WASI binders.
//!
//! `nimbus-runtime` owns only the Deno ABI-facing seam. This crate owns the
//! concrete shell and backends, then `nimbus-server` wires the default shell into
//! runtime construction.
//!
//! The model is one mount table, two binders, and a shared `FsCaps` matrix:
//! V8 goes through `deno_fs::FileSystem`, WASI consumes preopen descriptors,
//! and both resolve the same virtual paths to the same backend authority.
//! Container and microVM substrates use sandbox bundles, not this in-process
//! filesystem seam.

use std::io;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;

use deno_fs::sync::MaybeArc;
use deno_fs::{FileSystem, FsDirEntry, FsFileType, FsReadDirRc, OpenOptions};
use deno_io::fs::{File, FsResult, FsStat, FsStatFs};
use deno_permissions::{CheckedPath, CheckedPathBuf};
use nimbus_runtime::NimbusFsBackend;

pub mod backend;
pub mod cache;
pub mod caps;
pub mod cas_ro;
pub mod memfs;
pub mod mount;
pub mod resolver;
pub mod wasi;

pub use backend::{
    BackendRegistration, BackendRegistry, ObjectRwBackend, ObjectUnsupportedOperation,
    PersistenceMode,
};
pub use cache::{CacheLookup, ChunkCache};
pub use caps::{FsCaps, FsMountCaps};
pub use cas_ro::{CasBlobChunk, CasManifestEntry, CasReadOnlyBackend, CasReadOnlyManifest};
pub use memfs::MemFsBackend;
pub use mount::MountTable;
pub use resolver::{MountResolver, ResolvedAccess, ResolvedPath};
pub use wasi::{DirPerms, FilePerms, WasiPreopenBuilder, WasiPreopenDescriptor};

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub struct NimbusFs {
    resolver: MountResolver,
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
        Self::with_mount_table(MountTable::new(backend), cwd)
    }

    pub fn with_mount_table(table: MountTable, cwd: impl Into<PathBuf>) -> Self {
        Self {
            resolver: MountResolver::new(table),
            cwd: Mutex::new(cwd.into()),
        }
    }

    pub fn mount_table(&self) -> &MountTable {
        self.resolver.table()
    }

    fn cwd_path(&self) -> FsResult<PathBuf> {
        self.cwd
            .lock()
            .map(|cwd| cwd.clone())
            .map_err(|_| io::Error::other("NimbusFS cwd lock poisoned").into())
    }

    fn resolve_path(&self, path: &Path) -> FsResult<ResolvedPath> {
        self.resolver.resolve(&self.cwd_path()?, path)
    }

    fn checked(&self, path: &CheckedPath<'_>) -> FsResult<(ResolvedPath, CheckedPathBuf)> {
        let resolved = self.resolve_path(path)?;
        Ok((
            resolved.clone(),
            CheckedPathBuf::unsafe_new(resolved.backend_path.clone()),
        ))
    }

    fn checked_buf(&self, path: CheckedPathBuf) -> FsResult<(ResolvedPath, CheckedPathBuf)> {
        let resolved = self.resolve_path(&path.into_path_buf())?;
        Ok((
            resolved.clone(),
            CheckedPathBuf::unsafe_new(resolved.backend_path.clone()),
        ))
    }

    fn checked_pair(
        &self,
        oldpath: &CheckedPath<'_>,
        newpath: &CheckedPath<'_>,
    ) -> FsResult<(ResolvedPath, CheckedPathBuf, ResolvedPath, CheckedPathBuf)> {
        let (old_resolved, oldpath) = self.checked(oldpath)?;
        let (new_resolved, newpath) = self.checked(newpath)?;
        Ok((old_resolved, oldpath, new_resolved, newpath))
    }

    fn checked_buf_pair(
        &self,
        oldpath: CheckedPathBuf,
        newpath: CheckedPathBuf,
    ) -> FsResult<(ResolvedPath, CheckedPathBuf, ResolvedPath, CheckedPathBuf)> {
        let (old_resolved, oldpath) = self.checked_buf(oldpath)?;
        let (new_resolved, newpath) = self.checked_buf(newpath)?;
        Ok((old_resolved, oldpath, new_resolved, newpath))
    }

    fn ensure_same_mount(op: &str, left: &ResolvedPath, right: &ResolvedPath) -> FsResult<()> {
        if left.mount_prefix == right.mount_prefix {
            return Ok(());
        }
        Err(io::Error::other(format!("cross-mount {op} is unsupported")).into())
    }

    fn virtualize_backend_path(&self, resolved: &ResolvedPath, backend_path: PathBuf) -> PathBuf {
        resolver::virtual_path_for_backend(&resolved.mount_prefix, &backend_path)
    }
}

fn configured_process_cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
}

#[derive(Debug, Clone)]
pub struct PassthroughBackend {
    inner: deno_fs::RealFs,
    root: PathBuf,
}

impl PassthroughBackend {
    pub fn new() -> Self {
        Self::rooted(PathBuf::from("/"))
    }

    pub fn rooted(root: impl Into<PathBuf>) -> Self {
        Self {
            inner: deno_fs::RealFs,
            root: root.into(),
        }
    }

    fn host_path(&self, path: &Path) -> FsResult<PathBuf> {
        let mut relative = PathBuf::new();
        for component in path.components() {
            match component {
                Component::Prefix(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "passthrough backend does not accept platform path prefixes",
                    )
                    .into());
                }
                Component::RootDir | Component::CurDir => {}
                Component::ParentDir => {
                    if !relative.pop() {
                        return Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "passthrough path escapes backend root",
                        )
                        .into());
                    }
                }
                Component::Normal(part) => relative.push(part),
            }
        }
        let path = self.root.join(relative);
        self.ensure_existing_ancestor_inside_root(&path)?;
        Ok(path)
    }

    fn checked_path(&self, path: &CheckedPath<'_>) -> FsResult<CheckedPathBuf> {
        Ok(CheckedPathBuf::unsafe_new(self.host_path(path)?))
    }

    fn checked_buf(&self, path: CheckedPathBuf) -> FsResult<CheckedPathBuf> {
        Ok(CheckedPathBuf::unsafe_new(
            self.host_path(&path.into_path_buf())?,
        ))
    }

    fn symlink_target(&self, path: &CheckedPath<'_>) -> FsResult<CheckedPathBuf> {
        if path.is_absolute() {
            self.checked_path(path)
        } else {
            Ok(CheckedPathBuf::unsafe_new(path.to_path_buf()))
        }
    }

    fn symlink_target_buf(&self, path: CheckedPathBuf) -> FsResult<CheckedPathBuf> {
        if path.is_absolute() {
            self.checked_buf(path)
        } else {
            Ok(path)
        }
    }

    fn ensure_existing_ancestor_inside_root(&self, path: &Path) -> io::Result<()> {
        let root = self.root.canonicalize()?;
        let mut ancestor = path;
        while !ancestor.exists() {
            ancestor = ancestor.parent().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "passthrough path has no ancestor inside backend root",
                )
            })?;
        }
        let ancestor = ancestor.canonicalize()?;
        if ancestor.starts_with(&root) {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "passthrough path resolves outside backend root",
            ))
        }
    }

    fn backend_path_for_host(&self, path: PathBuf) -> FsResult<PathBuf> {
        let root = self.root.canonicalize()?;
        let path = path.canonicalize()?;
        let relative = path.strip_prefix(&root).map_err(|_| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "passthrough path resolves outside backend root",
            )
        })?;
        Ok(Path::new("/").join(relative))
    }
}

impl Default for PassthroughBackend {
    fn default() -> Self {
        Self::new()
    }
}

pub fn default_file_system() -> deno_fs::FileSystemRc {
    MaybeArc::new(NimbusFs::new(PassthroughBackend::new()))
}

fn open_options_require_write(options: OpenOptions) -> bool {
    options.write || options.create || options.truncate || options.append || options.create_new
}

#[async_trait::async_trait(?Send)]
impl FileSystem for NimbusFs {
    fn cwd(&self) -> FsResult<PathBuf> {
        self.cwd_path()
    }

    fn tmp_dir(&self) -> FsResult<PathBuf> {
        let resolved = self.resolve_path(Path::new("/"))?;
        resolved.backend()?.tmp_dir()
    }

    fn chdir(&self, path: &CheckedPath<'_>) -> FsResult<()> {
        let (resolved, backend_path) = self.checked(path)?;
        resolved.ensure_readable()?;
        let stat = resolved
            .backend()?
            .stat_sync(&backend_path.as_checked_path())?;
        if !stat.is_directory {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                format!("{} is not a directory", resolved.virtual_path.display()),
            )
            .into());
        }
        *self
            .cwd
            .lock()
            .map_err(|_| io::Error::other("NimbusFS cwd lock poisoned"))? = resolved.virtual_path;
        Ok(())
    }

    fn umask(&self, mask: Option<u32>) -> FsResult<u32> {
        let resolved = self.resolve_path(Path::new("/"))?;
        resolved.backend()?.umask(mask)
    }

    fn open_sync(&self, path: &CheckedPath<'_>, options: OpenOptions) -> FsResult<Rc<dyn File>> {
        let (resolved, path) = self.checked(path)?;
        if open_options_require_write(options) {
            resolved.ensure_writable()?;
        } else {
            resolved.ensure_readable()?;
        }
        resolved
            .backend()?
            .open_sync(&path.as_checked_path(), options)
    }

    async fn open_async<'a>(
        &'a self,
        path: CheckedPathBuf,
        options: OpenOptions,
    ) -> FsResult<Rc<dyn File>> {
        let (resolved, path) = self.checked_buf(path)?;
        if open_options_require_write(options) {
            resolved.ensure_writable()?;
        } else {
            resolved.ensure_readable()?;
        }
        resolved.backend()?.open_async(path, options).await
    }

    fn mkdir_sync(
        &self,
        path: &CheckedPath<'_>,
        recursive: bool,
        mode: Option<u32>,
    ) -> FsResult<()> {
        let (resolved, path) = self.checked(path)?;
        resolved.ensure_writable()?;
        resolved
            .backend()?
            .mkdir_sync(&path.as_checked_path(), recursive, mode)
    }

    async fn mkdir_async(
        &self,
        path: CheckedPathBuf,
        recursive: bool,
        mode: Option<u32>,
    ) -> FsResult<()> {
        let (resolved, path) = self.checked_buf(path)?;
        resolved.ensure_writable()?;
        resolved.backend()?.mkdir_async(path, recursive, mode).await
    }

    #[cfg(unix)]
    fn chmod_sync(&self, path: &CheckedPath<'_>, mode: u32) -> FsResult<()> {
        let (resolved, path) = self.checked(path)?;
        resolved.ensure_writable()?;
        resolved
            .backend()?
            .chmod_sync(&path.as_checked_path(), mode)
    }

    #[cfg(not(unix))]
    fn chmod_sync(&self, path: &CheckedPath<'_>, mode: i32) -> FsResult<()> {
        let (resolved, path) = self.checked(path)?;
        resolved.ensure_writable()?;
        resolved
            .backend()?
            .chmod_sync(&path.as_checked_path(), mode)
    }

    #[cfg(unix)]
    async fn chmod_async(&self, path: CheckedPathBuf, mode: u32) -> FsResult<()> {
        let (resolved, path) = self.checked_buf(path)?;
        resolved.ensure_writable()?;
        resolved.backend()?.chmod_async(path, mode).await
    }

    #[cfg(not(unix))]
    async fn chmod_async(&self, path: CheckedPathBuf, mode: i32) -> FsResult<()> {
        let (resolved, path) = self.checked_buf(path)?;
        resolved.ensure_writable()?;
        resolved.backend()?.chmod_async(path, mode).await
    }

    fn chown_sync(
        &self,
        path: &CheckedPath<'_>,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        let (resolved, path) = self.checked(path)?;
        resolved.ensure_writable()?;
        resolved
            .backend()?
            .chown_sync(&path.as_checked_path(), uid, gid)
    }

    async fn chown_async(
        &self,
        path: CheckedPathBuf,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        let (resolved, path) = self.checked_buf(path)?;
        resolved.ensure_writable()?;
        resolved.backend()?.chown_async(path, uid, gid).await
    }

    fn lchmod_sync(&self, path: &CheckedPath<'_>, mode: u32) -> FsResult<()> {
        let (resolved, path) = self.checked(path)?;
        resolved.ensure_writable()?;
        resolved
            .backend()?
            .lchmod_sync(&path.as_checked_path(), mode)
    }

    async fn lchmod_async(&self, path: CheckedPathBuf, mode: u32) -> FsResult<()> {
        let (resolved, path) = self.checked_buf(path)?;
        resolved.ensure_writable()?;
        resolved.backend()?.lchmod_async(path, mode).await
    }

    fn lchown_sync(
        &self,
        path: &CheckedPath<'_>,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        let (resolved, path) = self.checked(path)?;
        resolved.ensure_writable()?;
        resolved
            .backend()?
            .lchown_sync(&path.as_checked_path(), uid, gid)
    }

    async fn lchown_async(
        &self,
        path: CheckedPathBuf,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        let (resolved, path) = self.checked_buf(path)?;
        resolved.ensure_writable()?;
        resolved.backend()?.lchown_async(path, uid, gid).await
    }

    fn remove_sync(&self, path: &CheckedPath<'_>, recursive: bool) -> FsResult<()> {
        let (resolved, path) = self.checked(path)?;
        resolved.ensure_writable()?;
        resolved
            .backend()?
            .remove_sync(&path.as_checked_path(), recursive)
    }

    async fn remove_async(&self, path: CheckedPathBuf, recursive: bool) -> FsResult<()> {
        let (resolved, path) = self.checked_buf(path)?;
        resolved.ensure_writable()?;
        resolved.backend()?.remove_async(path, recursive).await
    }

    fn copy_file_sync(&self, oldpath: &CheckedPath<'_>, newpath: &CheckedPath<'_>) -> FsResult<()> {
        let (old_resolved, oldpath, new_resolved, newpath) = self.checked_pair(oldpath, newpath)?;
        old_resolved.ensure_readable()?;
        new_resolved.ensure_writable()?;
        Self::ensure_same_mount("copy", &old_resolved, &new_resolved)?;
        old_resolved
            .backend()?
            .copy_file_sync(&oldpath.as_checked_path(), &newpath.as_checked_path())
    }

    async fn copy_file_async(
        &self,
        oldpath: CheckedPathBuf,
        newpath: CheckedPathBuf,
    ) -> FsResult<()> {
        let (old_resolved, oldpath, new_resolved, newpath) =
            self.checked_buf_pair(oldpath, newpath)?;
        old_resolved.ensure_readable()?;
        new_resolved.ensure_writable()?;
        Self::ensure_same_mount("copy", &old_resolved, &new_resolved)?;
        old_resolved
            .backend()?
            .copy_file_async(oldpath, newpath)
            .await
    }

    fn cp_sync(&self, path: &CheckedPath<'_>, new_path: &CheckedPath<'_>) -> FsResult<()> {
        let (resolved, path, new_resolved, new_path) = self.checked_pair(path, new_path)?;
        resolved.ensure_readable()?;
        new_resolved.ensure_writable()?;
        Self::ensure_same_mount("copy", &resolved, &new_resolved)?;
        resolved
            .backend()?
            .cp_sync(&path.as_checked_path(), &new_path.as_checked_path())
    }

    async fn cp_async(&self, path: CheckedPathBuf, new_path: CheckedPathBuf) -> FsResult<()> {
        let (resolved, path, new_resolved, new_path) = self.checked_buf_pair(path, new_path)?;
        resolved.ensure_readable()?;
        new_resolved.ensure_writable()?;
        Self::ensure_same_mount("copy", &resolved, &new_resolved)?;
        resolved.backend()?.cp_async(path, new_path).await
    }

    fn stat_sync(&self, path: &CheckedPath<'_>) -> FsResult<FsStat> {
        let (resolved, path) = self.checked(path)?;
        resolved.ensure_readable()?;
        resolved.backend()?.stat_sync(&path.as_checked_path())
    }

    async fn stat_async(&self, path: CheckedPathBuf) -> FsResult<FsStat> {
        let (resolved, path) = self.checked_buf(path)?;
        resolved.ensure_readable()?;
        resolved.backend()?.stat_async(path).await
    }

    fn lstat_sync(&self, path: &CheckedPath<'_>) -> FsResult<FsStat> {
        let (resolved, path) = self.checked(path)?;
        resolved.ensure_readable()?;
        resolved.backend()?.lstat_sync(&path.as_checked_path())
    }

    async fn lstat_async(&self, path: CheckedPathBuf) -> FsResult<FsStat> {
        let (resolved, path) = self.checked_buf(path)?;
        resolved.ensure_readable()?;
        resolved.backend()?.lstat_async(path).await
    }

    fn statfs_sync(&self, path: &CheckedPath<'_>, bigint: bool) -> FsResult<FsStatFs> {
        let (resolved, path) = self.checked(path)?;
        resolved.ensure_readable()?;
        resolved
            .backend()?
            .statfs_sync(&path.as_checked_path(), bigint)
    }

    async fn statfs_async(&self, path: CheckedPathBuf, bigint: bool) -> FsResult<FsStatFs> {
        let (resolved, path) = self.checked_buf(path)?;
        resolved.ensure_readable()?;
        resolved.backend()?.statfs_async(path, bigint).await
    }

    fn realpath_sync(&self, path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        let (resolved, path) = self.checked(path)?;
        resolved.ensure_readable()?;
        let backend_path = resolved.backend()?.realpath_sync(&path.as_checked_path())?;
        Ok(self.virtualize_backend_path(&resolved, backend_path))
    }

    async fn realpath_async(&self, path: CheckedPathBuf) -> FsResult<PathBuf> {
        let (resolved, path) = self.checked_buf(path)?;
        resolved.ensure_readable()?;
        let backend_path = resolved.backend()?.realpath_async(path).await?;
        Ok(self.virtualize_backend_path(&resolved, backend_path))
    }

    fn read_dir_sync(&self, path: &CheckedPath<'_>) -> FsResult<Vec<FsDirEntry>> {
        let (resolved, path) = self.checked(path)?;
        resolved.ensure_readable()?;
        resolved.backend()?.read_dir_sync(&path.as_checked_path())
    }

    async fn read_dir_async(&self, path: CheckedPathBuf) -> FsResult<FsReadDirRc> {
        let (resolved, path) = self.checked_buf(path)?;
        resolved.ensure_readable()?;
        resolved.backend()?.read_dir_async(path).await
    }

    fn rename_sync(&self, oldpath: &CheckedPath<'_>, newpath: &CheckedPath<'_>) -> FsResult<()> {
        let (old_resolved, oldpath, new_resolved, newpath) = self.checked_pair(oldpath, newpath)?;
        old_resolved.ensure_writable()?;
        new_resolved.ensure_writable()?;
        Self::ensure_same_mount("rename", &old_resolved, &new_resolved)?;
        old_resolved
            .backend()?
            .rename_sync(&oldpath.as_checked_path(), &newpath.as_checked_path())
    }

    async fn rename_async(&self, oldpath: CheckedPathBuf, newpath: CheckedPathBuf) -> FsResult<()> {
        let (old_resolved, oldpath, new_resolved, newpath) =
            self.checked_buf_pair(oldpath, newpath)?;
        old_resolved.ensure_writable()?;
        new_resolved.ensure_writable()?;
        Self::ensure_same_mount("rename", &old_resolved, &new_resolved)?;
        old_resolved.backend()?.rename_async(oldpath, newpath).await
    }

    fn rmdir_sync(&self, path: &CheckedPath<'_>) -> FsResult<()> {
        let (resolved, path) = self.checked(path)?;
        resolved.ensure_writable()?;
        resolved.backend()?.rmdir_sync(&path.as_checked_path())
    }

    async fn rmdir_async(&self, path: CheckedPathBuf) -> FsResult<()> {
        let (resolved, path) = self.checked_buf(path)?;
        resolved.ensure_writable()?;
        resolved.backend()?.rmdir_async(path).await
    }

    fn link_sync(&self, oldpath: &CheckedPath<'_>, newpath: &CheckedPath<'_>) -> FsResult<()> {
        let (old_resolved, oldpath, new_resolved, newpath) = self.checked_pair(oldpath, newpath)?;
        old_resolved.ensure_readable()?;
        new_resolved.ensure_writable()?;
        Self::ensure_same_mount("link", &old_resolved, &new_resolved)?;
        old_resolved
            .backend()?
            .link_sync(&oldpath.as_checked_path(), &newpath.as_checked_path())
    }

    async fn link_async(&self, oldpath: CheckedPathBuf, newpath: CheckedPathBuf) -> FsResult<()> {
        let (old_resolved, oldpath, new_resolved, newpath) =
            self.checked_buf_pair(oldpath, newpath)?;
        old_resolved.ensure_readable()?;
        new_resolved.ensure_writable()?;
        Self::ensure_same_mount("link", &old_resolved, &new_resolved)?;
        old_resolved.backend()?.link_async(oldpath, newpath).await
    }

    fn symlink_sync(
        &self,
        oldpath: &CheckedPath<'_>,
        newpath: &CheckedPath<'_>,
        file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        let (resolved, newpath) = self.checked(newpath)?;
        resolved.ensure_writable()?;
        resolved
            .backend()?
            .symlink_sync(oldpath, &newpath.as_checked_path(), file_type)
    }

    async fn symlink_async(
        &self,
        oldpath: CheckedPathBuf,
        newpath: CheckedPathBuf,
        file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        let (resolved, newpath) = self.checked_buf(newpath)?;
        resolved.ensure_writable()?;
        resolved
            .backend()?
            .symlink_async(oldpath, newpath, file_type)
            .await
    }

    fn read_link_sync(&self, path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        let (resolved, path) = self.checked(path)?;
        resolved.ensure_readable()?;
        let target = resolved
            .backend()?
            .read_link_sync(&path.as_checked_path())?;
        if target.is_absolute() {
            Ok(self.virtualize_backend_path(&resolved, target))
        } else {
            Ok(target)
        }
    }

    async fn read_link_async(&self, path: CheckedPathBuf) -> FsResult<PathBuf> {
        let (resolved, path) = self.checked_buf(path)?;
        resolved.ensure_readable()?;
        let target = resolved.backend()?.read_link_async(path).await?;
        if target.is_absolute() {
            Ok(self.virtualize_backend_path(&resolved, target))
        } else {
            Ok(target)
        }
    }

    fn truncate_sync(&self, path: &CheckedPath<'_>, len: u64) -> FsResult<()> {
        let (resolved, path) = self.checked(path)?;
        resolved.ensure_writable()?;
        resolved
            .backend()?
            .truncate_sync(&path.as_checked_path(), len)
    }

    async fn truncate_async(&self, path: CheckedPathBuf, len: u64) -> FsResult<()> {
        let (resolved, path) = self.checked_buf(path)?;
        resolved.ensure_writable()?;
        resolved.backend()?.truncate_async(path, len).await
    }

    fn utime_sync(
        &self,
        path: &CheckedPath<'_>,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        let (resolved, path) = self.checked(path)?;
        resolved.ensure_writable()?;
        resolved.backend()?.utime_sync(
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
        let (resolved, path) = self.checked_buf(path)?;
        resolved.ensure_writable()?;
        resolved
            .backend()?
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
        let (resolved, path) = self.checked(path)?;
        resolved.ensure_writable()?;
        resolved.backend()?.lutime_sync(
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
        let (resolved, path) = self.checked_buf(path)?;
        resolved.ensure_writable()?;
        resolved
            .backend()?
            .lutime_async(path, atime_secs, atime_nanos, mtime_secs, mtime_nanos)
            .await
    }

    fn exists_sync(&self, path: &CheckedPath<'_>) -> bool {
        let Ok((resolved, path)) = self.checked(path) else {
            return false;
        };
        if resolved.ensure_readable().is_err() {
            return false;
        }
        resolved
            .backend()
            .map(|backend| backend.exists_sync(&path.as_checked_path()))
            .unwrap_or(false)
    }

    async fn exists_async(&self, path: CheckedPathBuf) -> FsResult<bool> {
        let (resolved, path) = self.checked_buf(path)?;
        if resolved.ensure_readable().is_err() {
            return Ok(false);
        }
        resolved.backend()?.exists_async(path).await
    }
}

#[async_trait::async_trait(?Send)]
impl FileSystem for PassthroughBackend {
    fn cwd(&self) -> FsResult<PathBuf> {
        Ok(self.root.clone())
    }

    fn tmp_dir(&self) -> FsResult<PathBuf> {
        self.host_path(Path::new("/tmp"))
    }

    fn chdir(&self, path: &CheckedPath<'_>) -> FsResult<()> {
        let path = self.checked_path(path)?;
        self.inner.chdir(&path.as_checked_path())
    }

    fn umask(&self, mask: Option<u32>) -> FsResult<u32> {
        self.inner.umask(mask)
    }

    fn open_sync(&self, path: &CheckedPath<'_>, options: OpenOptions) -> FsResult<Rc<dyn File>> {
        let path = self.checked_path(path)?;
        self.inner.open_sync(&path.as_checked_path(), options)
    }

    async fn open_async<'a>(
        &'a self,
        path: CheckedPathBuf,
        options: OpenOptions,
    ) -> FsResult<Rc<dyn File>> {
        let path = self.checked_buf(path)?;
        self.inner.open_async(path, options).await
    }

    fn mkdir_sync(
        &self,
        path: &CheckedPath<'_>,
        recursive: bool,
        mode: Option<u32>,
    ) -> FsResult<()> {
        let path = self.checked_path(path)?;
        self.inner
            .mkdir_sync(&path.as_checked_path(), recursive, mode)
    }

    async fn mkdir_async(
        &self,
        path: CheckedPathBuf,
        recursive: bool,
        mode: Option<u32>,
    ) -> FsResult<()> {
        let path = self.checked_buf(path)?;
        self.inner.mkdir_async(path, recursive, mode).await
    }

    #[cfg(unix)]
    fn chmod_sync(&self, path: &CheckedPath<'_>, mode: u32) -> FsResult<()> {
        let path = self.checked_path(path)?;
        self.inner.chmod_sync(&path.as_checked_path(), mode)
    }

    #[cfg(not(unix))]
    fn chmod_sync(&self, path: &CheckedPath<'_>, mode: i32) -> FsResult<()> {
        let path = self.checked_path(path)?;
        self.inner.chmod_sync(&path.as_checked_path(), mode)
    }

    #[cfg(unix)]
    async fn chmod_async(&self, path: CheckedPathBuf, mode: u32) -> FsResult<()> {
        let path = self.checked_buf(path)?;
        self.inner.chmod_async(path, mode).await
    }

    #[cfg(not(unix))]
    async fn chmod_async(&self, path: CheckedPathBuf, mode: i32) -> FsResult<()> {
        let path = self.checked_buf(path)?;
        self.inner.chmod_async(path, mode).await
    }

    fn chown_sync(
        &self,
        path: &CheckedPath<'_>,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        let path = self.checked_path(path)?;
        self.inner.chown_sync(&path.as_checked_path(), uid, gid)
    }

    async fn chown_async(
        &self,
        path: CheckedPathBuf,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        let path = self.checked_buf(path)?;
        self.inner.chown_async(path, uid, gid).await
    }

    fn lchmod_sync(&self, path: &CheckedPath<'_>, mode: u32) -> FsResult<()> {
        let path = self.checked_path(path)?;
        self.inner.lchmod_sync(&path.as_checked_path(), mode)
    }

    async fn lchmod_async(&self, path: CheckedPathBuf, mode: u32) -> FsResult<()> {
        let path = self.checked_buf(path)?;
        self.inner.lchmod_async(path, mode).await
    }

    fn lchown_sync(
        &self,
        path: &CheckedPath<'_>,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        let path = self.checked_path(path)?;
        self.inner.lchown_sync(&path.as_checked_path(), uid, gid)
    }

    async fn lchown_async(
        &self,
        path: CheckedPathBuf,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        let path = self.checked_buf(path)?;
        self.inner.lchown_async(path, uid, gid).await
    }

    fn remove_sync(&self, path: &CheckedPath<'_>, recursive: bool) -> FsResult<()> {
        let path = self.checked_path(path)?;
        self.inner.remove_sync(&path.as_checked_path(), recursive)
    }

    async fn remove_async(&self, path: CheckedPathBuf, recursive: bool) -> FsResult<()> {
        let path = self.checked_buf(path)?;
        self.inner.remove_async(path, recursive).await
    }

    fn copy_file_sync(&self, oldpath: &CheckedPath<'_>, newpath: &CheckedPath<'_>) -> FsResult<()> {
        let oldpath = self.checked_path(oldpath)?;
        let newpath = self.checked_path(newpath)?;
        self.inner
            .copy_file_sync(&oldpath.as_checked_path(), &newpath.as_checked_path())
    }

    async fn copy_file_async(
        &self,
        oldpath: CheckedPathBuf,
        newpath: CheckedPathBuf,
    ) -> FsResult<()> {
        let oldpath = self.checked_buf(oldpath)?;
        let newpath = self.checked_buf(newpath)?;
        self.inner.copy_file_async(oldpath, newpath).await
    }

    fn cp_sync(&self, path: &CheckedPath<'_>, new_path: &CheckedPath<'_>) -> FsResult<()> {
        let path = self.checked_path(path)?;
        let new_path = self.checked_path(new_path)?;
        self.inner
            .cp_sync(&path.as_checked_path(), &new_path.as_checked_path())
    }

    async fn cp_async(&self, path: CheckedPathBuf, new_path: CheckedPathBuf) -> FsResult<()> {
        let path = self.checked_buf(path)?;
        let new_path = self.checked_buf(new_path)?;
        self.inner.cp_async(path, new_path).await
    }

    fn stat_sync(&self, path: &CheckedPath<'_>) -> FsResult<FsStat> {
        let path = self.checked_path(path)?;
        self.inner.stat_sync(&path.as_checked_path())
    }

    async fn stat_async(&self, path: CheckedPathBuf) -> FsResult<FsStat> {
        let path = self.checked_buf(path)?;
        self.inner.stat_async(path).await
    }

    fn lstat_sync(&self, path: &CheckedPath<'_>) -> FsResult<FsStat> {
        let path = self.checked_path(path)?;
        self.inner.lstat_sync(&path.as_checked_path())
    }

    async fn lstat_async(&self, path: CheckedPathBuf) -> FsResult<FsStat> {
        let path = self.checked_buf(path)?;
        self.inner.lstat_async(path).await
    }

    fn statfs_sync(&self, path: &CheckedPath<'_>, bigint: bool) -> FsResult<FsStatFs> {
        let path = self.checked_path(path)?;
        self.inner.statfs_sync(&path.as_checked_path(), bigint)
    }

    async fn statfs_async(&self, path: CheckedPathBuf, bigint: bool) -> FsResult<FsStatFs> {
        let path = self.checked_buf(path)?;
        self.inner.statfs_async(path, bigint).await
    }

    fn realpath_sync(&self, path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        let path = self.checked_path(path)?;
        let realpath = self.inner.realpath_sync(&path.as_checked_path())?;
        self.backend_path_for_host(realpath)
    }

    async fn realpath_async(&self, path: CheckedPathBuf) -> FsResult<PathBuf> {
        let path = self.checked_buf(path)?;
        let realpath = self.inner.realpath_async(path).await?;
        self.backend_path_for_host(realpath)
    }

    fn read_dir_sync(&self, path: &CheckedPath<'_>) -> FsResult<Vec<FsDirEntry>> {
        let path = self.checked_path(path)?;
        self.inner.read_dir_sync(&path.as_checked_path())
    }

    async fn read_dir_async(&self, path: CheckedPathBuf) -> FsResult<FsReadDirRc> {
        let path = self.checked_buf(path)?;
        self.inner.read_dir_async(path).await
    }

    fn rename_sync(&self, oldpath: &CheckedPath<'_>, newpath: &CheckedPath<'_>) -> FsResult<()> {
        let oldpath = self.checked_path(oldpath)?;
        let newpath = self.checked_path(newpath)?;
        self.inner
            .rename_sync(&oldpath.as_checked_path(), &newpath.as_checked_path())
    }

    async fn rename_async(&self, oldpath: CheckedPathBuf, newpath: CheckedPathBuf) -> FsResult<()> {
        let oldpath = self.checked_buf(oldpath)?;
        let newpath = self.checked_buf(newpath)?;
        self.inner.rename_async(oldpath, newpath).await
    }

    fn rmdir_sync(&self, path: &CheckedPath<'_>) -> FsResult<()> {
        let path = self.checked_path(path)?;
        self.inner.rmdir_sync(&path.as_checked_path())
    }

    async fn rmdir_async(&self, path: CheckedPathBuf) -> FsResult<()> {
        let path = self.checked_buf(path)?;
        self.inner.rmdir_async(path).await
    }

    fn link_sync(&self, oldpath: &CheckedPath<'_>, newpath: &CheckedPath<'_>) -> FsResult<()> {
        let oldpath = self.checked_path(oldpath)?;
        let newpath = self.checked_path(newpath)?;
        self.inner
            .link_sync(&oldpath.as_checked_path(), &newpath.as_checked_path())
    }

    async fn link_async(&self, oldpath: CheckedPathBuf, newpath: CheckedPathBuf) -> FsResult<()> {
        let oldpath = self.checked_buf(oldpath)?;
        let newpath = self.checked_buf(newpath)?;
        self.inner.link_async(oldpath, newpath).await
    }

    fn symlink_sync(
        &self,
        oldpath: &CheckedPath<'_>,
        newpath: &CheckedPath<'_>,
        file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        let oldpath = self.symlink_target(oldpath)?;
        let newpath = self.checked_path(newpath)?;
        self.inner.symlink_sync(
            &oldpath.as_checked_path(),
            &newpath.as_checked_path(),
            file_type,
        )
    }

    async fn symlink_async(
        &self,
        oldpath: CheckedPathBuf,
        newpath: CheckedPathBuf,
        file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        let oldpath = self.symlink_target_buf(oldpath)?;
        let newpath = self.checked_buf(newpath)?;
        self.inner.symlink_async(oldpath, newpath, file_type).await
    }

    fn read_link_sync(&self, path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        let path = self.checked_path(path)?;
        let target = self.inner.read_link_sync(&path.as_checked_path())?;
        if target.is_absolute() {
            self.backend_path_for_host(target)
        } else {
            Ok(target)
        }
    }

    async fn read_link_async(&self, path: CheckedPathBuf) -> FsResult<PathBuf> {
        let path = self.checked_buf(path)?;
        let target = self.inner.read_link_async(path).await?;
        if target.is_absolute() {
            self.backend_path_for_host(target)
        } else {
            Ok(target)
        }
    }

    fn truncate_sync(&self, path: &CheckedPath<'_>, len: u64) -> FsResult<()> {
        let path = self.checked_path(path)?;
        self.inner.truncate_sync(&path.as_checked_path(), len)
    }

    async fn truncate_async(&self, path: CheckedPathBuf, len: u64) -> FsResult<()> {
        let path = self.checked_buf(path)?;
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
        let path = self.checked_path(path)?;
        self.inner.utime_sync(
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
        let path = self.checked_buf(path)?;
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
        let path = self.checked_path(path)?;
        self.inner.lutime_sync(
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
        let path = self.checked_buf(path)?;
        self.inner
            .lutime_async(path, atime_secs, atime_nanos, mtime_secs, mtime_nanos)
            .await
    }

    fn exists_sync(&self, path: &CheckedPath<'_>) -> bool {
        let Ok(path) = self.checked_path(path) else {
            return false;
        };
        self.inner.exists_sync(&path.as_checked_path())
    }

    async fn exists_async(&self, path: CheckedPathBuf) -> FsResult<bool> {
        let path = self.checked_buf(path)?;
        self.inner.exists_async(path).await
    }
}
