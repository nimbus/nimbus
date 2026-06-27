use std::borrow::Cow;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::rc::Rc;

use deno_core::{BufMutView, BufView, ResourceHandleFd, WriteOutcome};
use deno_fs::sync::MaybeArc;
use deno_fs::{FileSystem, FsDirEntry, FsFileType, FsReadDirRc, OpenOptions};
use deno_io::fs::{File, FsResult, FsStat, FsStatFs};
use deno_permissions::{CheckedPath, CheckedPathBuf};

use crate::mount::{MountEntry, MountTable, MountTarget};

#[derive(Debug, Clone, Default)]
pub struct FsCaps {
    grants: BTreeMap<PathBuf, FsMountCaps>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsMountCaps {
    pub visible: bool,
    pub file_read: bool,
    pub file_write: bool,
    pub directory_read: bool,
    pub directory_mutate: bool,
    pub metadata_mutate: bool,
    pub link_create: bool,
    pub write_size_limit: Option<u64>,
    pub masked: bool,
    pub readonly: bool,
}

#[derive(Debug, Clone)]
struct CappedBackend {
    inner: deno_fs::FileSystemRc,
    caps: FsMountCaps,
}

struct CappedFile {
    inner: Rc<dyn File>,
    caps: FsMountCaps,
    path: Option<PathBuf>,
}

impl std::fmt::Debug for CappedFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CappedFile")
            .field("path", &self.path)
            .field("caps", &self.caps)
            .finish_non_exhaustive()
    }
}

impl FsCaps {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn grant(mut self, prefix: impl Into<PathBuf>, caps: FsMountCaps) -> Self {
        self.grants.insert(normalize_prefix(prefix.into()), caps);
        self
    }

    pub fn apply_to_mount_table(&self, table: &MountTable) -> MountTable {
        let entries = table
            .entries()
            .iter()
            .map(|entry| self.gate_entry(entry))
            .collect();
        MountTable::from_entries(entries)
    }

    pub fn open_requires(options: OpenOptions) -> FsOpenRequirement {
        FsOpenRequirement {
            file_read: options.read || !(options.write || options.append),
            file_write: options.write || options.truncate || options.append,
            directory_mutate: options.create || options.create_new,
            truncate: options.truncate,
            append: options.append,
            create: options.create || options.create_new,
        }
    }

    fn gate_entry(&self, entry: &MountEntry) -> MountEntry {
        let prefix = entry.prefix().to_path_buf();
        let Some(caps) = self.best_grant(entry.prefix()) else {
            return MountEntry::masked(prefix, "ungranted NimbusFS mount");
        };
        if !caps.visible || caps.masked {
            return MountEntry::masked(prefix, "masked NimbusFS grant");
        }
        match entry.target() {
            MountTarget::Masked { message } => MountEntry::masked(prefix, message.clone()),
            MountTarget::Backend { backend, readonly } => MountEntry::backend(
                prefix,
                MaybeArc::new(CappedBackend {
                    inner: backend.clone(),
                    caps: caps.clone(),
                }),
                *readonly || caps.readonly || !caps.file_write,
            ),
        }
    }

    fn best_grant(&self, path: &Path) -> Option<&FsMountCaps> {
        self.grant_for_path(path)
    }

    pub fn grant_for_path(&self, path: &Path) -> Option<&FsMountCaps> {
        self.grants
            .iter()
            .filter(|(prefix, _)| path == prefix.as_path() || path.starts_with(prefix))
            .max_by_key(|(prefix, _)| prefix.components().count())
            .map(|(_, caps)| caps)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FsOpenRequirement {
    pub file_read: bool,
    pub file_write: bool,
    pub directory_mutate: bool,
    pub truncate: bool,
    pub append: bool,
    pub create: bool,
}

impl FsMountCaps {
    pub fn read_write() -> Self {
        Self {
            visible: true,
            file_read: true,
            file_write: true,
            directory_read: true,
            directory_mutate: true,
            metadata_mutate: true,
            link_create: true,
            write_size_limit: None,
            masked: false,
            readonly: false,
        }
    }

    pub fn read_only() -> Self {
        Self {
            file_write: false,
            directory_mutate: false,
            metadata_mutate: false,
            link_create: false,
            readonly: true,
            ..Self::read_write()
        }
    }

    pub fn invisible() -> Self {
        Self {
            visible: false,
            file_read: false,
            file_write: false,
            directory_read: false,
            directory_mutate: false,
            metadata_mutate: false,
            link_create: false,
            write_size_limit: None,
            masked: true,
            readonly: true,
        }
    }

    pub fn with_write_size_limit(mut self, limit: u64) -> Self {
        self.write_size_limit = Some(limit);
        self
    }

    fn require_visible_read(&self) -> FsResult<()> {
        if self.visible && (self.file_read || self.directory_read) {
            Ok(())
        } else {
            Err(permission("filesystem read denied"))
        }
    }

    fn require_file_read(&self) -> FsResult<()> {
        if self.visible && self.file_read {
            Ok(())
        } else {
            Err(permission("file-read denied"))
        }
    }

    fn require_file_write(&self) -> FsResult<()> {
        if self.visible && self.file_write && !self.readonly {
            Ok(())
        } else {
            Err(readonly())
        }
    }

    fn require_directory_read(&self) -> FsResult<()> {
        if self.visible && self.directory_read {
            Ok(())
        } else {
            Err(permission("directory-read denied"))
        }
    }

    fn require_directory_mutate(&self) -> FsResult<()> {
        if self.visible && self.directory_mutate && !self.readonly {
            Ok(())
        } else {
            Err(readonly())
        }
    }

    fn require_metadata_mutate(&self) -> FsResult<()> {
        if self.visible && self.metadata_mutate && !self.readonly {
            Ok(())
        } else {
            Err(permission("metadata-mutate denied"))
        }
    }

    fn require_link_create(&self) -> FsResult<()> {
        if self.visible && self.link_create && self.directory_mutate && !self.readonly {
            Ok(())
        } else {
            Err(permission("link-create denied"))
        }
    }

    fn check_write_size(&self, len: usize) -> FsResult<()> {
        if let Some(limit) = self.write_size_limit
            && len as u64 > limit
        {
            return Err(io::Error::new(
                io::ErrorKind::StorageFull,
                "FsCaps write-size quota exceeded",
            )
            .into());
        }
        Ok(())
    }

    fn require_open(&self, options: OpenOptions) -> FsResult<()> {
        let required = FsCaps::open_requires(options);
        if required.file_read {
            self.require_file_read()?;
        }
        if required.file_write || required.truncate || required.append {
            self.require_file_write()?;
        }
        if required.create {
            self.require_directory_mutate()?;
        }
        Ok(())
    }
}

#[async_trait::async_trait(?Send)]
impl FileSystem for CappedBackend {
    fn cwd(&self) -> FsResult<PathBuf> {
        self.inner.cwd()
    }

    fn tmp_dir(&self) -> FsResult<PathBuf> {
        self.inner.tmp_dir()
    }

    fn chdir(&self, path: &CheckedPath<'_>) -> FsResult<()> {
        self.caps.require_directory_read()?;
        self.inner.chdir(path)
    }

    fn umask(&self, mask: Option<u32>) -> FsResult<u32> {
        self.inner.umask(mask)
    }

    fn open_sync(&self, path: &CheckedPath<'_>, options: OpenOptions) -> FsResult<Rc<dyn File>> {
        self.caps.require_open(options)?;
        let inner = self.inner.open_sync(path, options)?;
        Ok(Rc::new(CappedFile {
            inner,
            caps: self.caps.clone(),
            path: Some(path.to_path_buf()),
        }))
    }

    async fn open_async<'a>(
        &'a self,
        path: CheckedPathBuf,
        options: OpenOptions,
    ) -> FsResult<Rc<dyn File>> {
        self.caps.require_open(options)?;
        let path_for_file = path.clone().into_path_buf();
        let inner = self.inner.open_async(path, options).await?;
        Ok(Rc::new(CappedFile {
            inner,
            caps: self.caps.clone(),
            path: Some(path_for_file),
        }))
    }

    fn mkdir_sync(
        &self,
        path: &CheckedPath<'_>,
        recursive: bool,
        mode: Option<u32>,
    ) -> FsResult<()> {
        self.caps.require_directory_mutate()?;
        self.inner.mkdir_sync(path, recursive, mode)
    }

    async fn mkdir_async(
        &self,
        path: CheckedPathBuf,
        recursive: bool,
        mode: Option<u32>,
    ) -> FsResult<()> {
        self.caps.require_directory_mutate()?;
        self.inner.mkdir_async(path, recursive, mode).await
    }

    #[cfg(unix)]
    fn chmod_sync(&self, path: &CheckedPath<'_>, mode: u32) -> FsResult<()> {
        self.caps.require_metadata_mutate()?;
        self.inner.chmod_sync(path, mode)
    }

    #[cfg(not(unix))]
    fn chmod_sync(&self, path: &CheckedPath<'_>, mode: i32) -> FsResult<()> {
        self.caps.require_metadata_mutate()?;
        self.inner.chmod_sync(path, mode)
    }

    #[cfg(unix)]
    async fn chmod_async(&self, path: CheckedPathBuf, mode: u32) -> FsResult<()> {
        self.caps.require_metadata_mutate()?;
        self.inner.chmod_async(path, mode).await
    }

    #[cfg(not(unix))]
    async fn chmod_async(&self, path: CheckedPathBuf, mode: i32) -> FsResult<()> {
        self.caps.require_metadata_mutate()?;
        self.inner.chmod_async(path, mode).await
    }

    fn chown_sync(
        &self,
        path: &CheckedPath<'_>,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        self.caps.require_metadata_mutate()?;
        self.inner.chown_sync(path, uid, gid)
    }

    async fn chown_async(
        &self,
        path: CheckedPathBuf,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        self.caps.require_metadata_mutate()?;
        self.inner.chown_async(path, uid, gid).await
    }

    fn lchmod_sync(&self, path: &CheckedPath<'_>, mode: u32) -> FsResult<()> {
        self.caps.require_metadata_mutate()?;
        self.inner.lchmod_sync(path, mode)
    }

    async fn lchmod_async(&self, path: CheckedPathBuf, mode: u32) -> FsResult<()> {
        self.caps.require_metadata_mutate()?;
        self.inner.lchmod_async(path, mode).await
    }

    fn lchown_sync(
        &self,
        path: &CheckedPath<'_>,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        self.caps.require_metadata_mutate()?;
        self.inner.lchown_sync(path, uid, gid)
    }

    async fn lchown_async(
        &self,
        path: CheckedPathBuf,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        self.caps.require_metadata_mutate()?;
        self.inner.lchown_async(path, uid, gid).await
    }

    fn remove_sync(&self, path: &CheckedPath<'_>, recursive: bool) -> FsResult<()> {
        self.caps.require_directory_mutate()?;
        self.inner.remove_sync(path, recursive)
    }

    async fn remove_async(&self, path: CheckedPathBuf, recursive: bool) -> FsResult<()> {
        self.caps.require_directory_mutate()?;
        self.inner.remove_async(path, recursive).await
    }

    fn copy_file_sync(&self, oldpath: &CheckedPath<'_>, newpath: &CheckedPath<'_>) -> FsResult<()> {
        self.caps.require_file_read()?;
        self.caps.require_file_write()?;
        self.caps.require_directory_mutate()?;
        self.inner.copy_file_sync(oldpath, newpath)
    }

    async fn copy_file_async(
        &self,
        oldpath: CheckedPathBuf,
        newpath: CheckedPathBuf,
    ) -> FsResult<()> {
        self.caps.require_file_read()?;
        self.caps.require_file_write()?;
        self.caps.require_directory_mutate()?;
        self.inner.copy_file_async(oldpath, newpath).await
    }

    fn cp_sync(&self, path: &CheckedPath<'_>, new_path: &CheckedPath<'_>) -> FsResult<()> {
        self.copy_file_sync(path, new_path)
    }

    async fn cp_async(&self, path: CheckedPathBuf, new_path: CheckedPathBuf) -> FsResult<()> {
        self.copy_file_async(path, new_path).await
    }

    fn stat_sync(&self, path: &CheckedPath<'_>) -> FsResult<FsStat> {
        self.caps.require_visible_read()?;
        self.inner.stat_sync(path)
    }

    async fn stat_async(&self, path: CheckedPathBuf) -> FsResult<FsStat> {
        self.caps.require_visible_read()?;
        self.inner.stat_async(path).await
    }

    fn lstat_sync(&self, path: &CheckedPath<'_>) -> FsResult<FsStat> {
        self.caps.require_visible_read()?;
        self.inner.lstat_sync(path)
    }

    async fn lstat_async(&self, path: CheckedPathBuf) -> FsResult<FsStat> {
        self.caps.require_visible_read()?;
        self.inner.lstat_async(path).await
    }

    fn statfs_sync(&self, path: &CheckedPath<'_>, bigint: bool) -> FsResult<FsStatFs> {
        self.caps.require_directory_read()?;
        self.inner.statfs_sync(path, bigint)
    }

    async fn statfs_async(&self, path: CheckedPathBuf, bigint: bool) -> FsResult<FsStatFs> {
        self.caps.require_directory_read()?;
        self.inner.statfs_async(path, bigint).await
    }

    fn realpath_sync(&self, path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        self.caps.require_visible_read()?;
        self.inner.realpath_sync(path)
    }

    async fn realpath_async(&self, path: CheckedPathBuf) -> FsResult<PathBuf> {
        self.caps.require_visible_read()?;
        self.inner.realpath_async(path).await
    }

    fn read_dir_sync(&self, path: &CheckedPath<'_>) -> FsResult<Vec<FsDirEntry>> {
        self.caps.require_directory_read()?;
        self.inner.read_dir_sync(path)
    }

    async fn read_dir_async(&self, path: CheckedPathBuf) -> FsResult<FsReadDirRc> {
        self.caps.require_directory_read()?;
        self.inner.read_dir_async(path).await
    }

    fn rename_sync(&self, oldpath: &CheckedPath<'_>, newpath: &CheckedPath<'_>) -> FsResult<()> {
        self.caps.require_directory_mutate()?;
        self.inner.rename_sync(oldpath, newpath)
    }

    async fn rename_async(&self, oldpath: CheckedPathBuf, newpath: CheckedPathBuf) -> FsResult<()> {
        self.caps.require_directory_mutate()?;
        self.inner.rename_async(oldpath, newpath).await
    }

    fn rmdir_sync(&self, path: &CheckedPath<'_>) -> FsResult<()> {
        self.caps.require_directory_mutate()?;
        self.inner.rmdir_sync(path)
    }

    async fn rmdir_async(&self, path: CheckedPathBuf) -> FsResult<()> {
        self.caps.require_directory_mutate()?;
        self.inner.rmdir_async(path).await
    }

    fn link_sync(&self, oldpath: &CheckedPath<'_>, newpath: &CheckedPath<'_>) -> FsResult<()> {
        self.caps.require_link_create()?;
        self.inner.link_sync(oldpath, newpath)
    }

    async fn link_async(&self, oldpath: CheckedPathBuf, newpath: CheckedPathBuf) -> FsResult<()> {
        self.caps.require_link_create()?;
        self.inner.link_async(oldpath, newpath).await
    }

    fn symlink_sync(
        &self,
        oldpath: &CheckedPath<'_>,
        newpath: &CheckedPath<'_>,
        file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        self.caps.require_link_create()?;
        self.inner.symlink_sync(oldpath, newpath, file_type)
    }

    async fn symlink_async(
        &self,
        oldpath: CheckedPathBuf,
        newpath: CheckedPathBuf,
        file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        self.caps.require_link_create()?;
        self.inner.symlink_async(oldpath, newpath, file_type).await
    }

    fn read_link_sync(&self, path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        self.caps.require_visible_read()?;
        self.inner.read_link_sync(path)
    }

    async fn read_link_async(&self, path: CheckedPathBuf) -> FsResult<PathBuf> {
        self.caps.require_visible_read()?;
        self.inner.read_link_async(path).await
    }

    fn truncate_sync(&self, path: &CheckedPath<'_>, len: u64) -> FsResult<()> {
        self.caps.require_file_write()?;
        if let Some(limit) = self.caps.write_size_limit
            && len > limit
        {
            return Err(io::ErrorKind::StorageFull.into());
        }
        self.inner.truncate_sync(path, len)
    }

    async fn truncate_async(&self, path: CheckedPathBuf, len: u64) -> FsResult<()> {
        self.caps.require_file_write()?;
        if let Some(limit) = self.caps.write_size_limit
            && len > limit
        {
            return Err(io::ErrorKind::StorageFull.into());
        }
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
        self.caps.require_metadata_mutate()?;
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
        self.caps.require_metadata_mutate()?;
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
        self.caps.require_metadata_mutate()?;
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
        self.caps.require_metadata_mutate()?;
        self.inner
            .lutime_async(path, atime_secs, atime_nanos, mtime_secs, mtime_nanos)
            .await
    }

    fn exists_sync(&self, path: &CheckedPath<'_>) -> bool {
        self.caps.require_visible_read().is_ok() && self.inner.exists_sync(path)
    }

    async fn exists_async(&self, path: CheckedPathBuf) -> FsResult<bool> {
        if self.caps.require_visible_read().is_err() {
            return Ok(false);
        }
        self.inner.exists_async(path).await
    }
}

#[async_trait::async_trait(?Send)]
impl File for CappedFile {
    fn maybe_path(&self) -> Option<&Path> {
        self.path.as_deref().or_else(|| self.inner.maybe_path())
    }

    fn read_sync(self: Rc<Self>, buf: &mut [u8]) -> FsResult<usize> {
        self.caps.require_file_read()?;
        self.inner.clone().read_sync(buf)
    }

    async fn read_byob(self: Rc<Self>, buf: BufMutView) -> FsResult<(usize, BufMutView)> {
        self.caps.require_file_read()?;
        self.inner.clone().read_byob(buf).await
    }

    fn write_sync(self: Rc<Self>, buf: &[u8]) -> FsResult<usize> {
        self.caps.require_file_write()?;
        self.caps.check_write_size(buf.len())?;
        self.inner.clone().write_sync(buf)
    }

    async fn write(self: Rc<Self>, buf: BufView) -> FsResult<WriteOutcome> {
        self.caps.require_file_write()?;
        self.caps.check_write_size(buf.len())?;
        self.inner.clone().write(buf).await
    }

    fn write_all_sync(self: Rc<Self>, buf: &[u8]) -> FsResult<()> {
        self.caps.require_file_write()?;
        self.caps.check_write_size(buf.len())?;
        self.inner.clone().write_all_sync(buf)
    }

    async fn write_all(self: Rc<Self>, buf: BufView) -> FsResult<()> {
        self.caps.require_file_write()?;
        self.caps.check_write_size(buf.len())?;
        self.inner.clone().write_all(buf).await
    }

    fn read_all_sync(self: Rc<Self>) -> FsResult<Cow<'static, [u8]>> {
        self.caps.require_file_read()?;
        self.inner.clone().read_all_sync()
    }

    async fn read_all_async(self: Rc<Self>) -> FsResult<Cow<'static, [u8]>> {
        self.caps.require_file_read()?;
        self.inner.clone().read_all_async().await
    }

    fn chmod_sync(self: Rc<Self>, pathmode: u32) -> FsResult<()> {
        self.caps.require_metadata_mutate()?;
        self.inner.clone().chmod_sync(pathmode)
    }

    async fn chmod_async(self: Rc<Self>, mode: u32) -> FsResult<()> {
        self.caps.require_metadata_mutate()?;
        self.inner.clone().chmod_async(mode).await
    }

    fn chown_sync(self: Rc<Self>, uid: Option<u32>, gid: Option<u32>) -> FsResult<()> {
        self.caps.require_metadata_mutate()?;
        self.inner.clone().chown_sync(uid, gid)
    }

    async fn chown_async(self: Rc<Self>, uid: Option<u32>, gid: Option<u32>) -> FsResult<()> {
        self.caps.require_metadata_mutate()?;
        self.inner.clone().chown_async(uid, gid).await
    }

    fn seek_sync(self: Rc<Self>, pos: io::SeekFrom) -> FsResult<u64> {
        self.inner.clone().seek_sync(pos)
    }

    async fn seek_async(self: Rc<Self>, pos: io::SeekFrom) -> FsResult<u64> {
        self.inner.clone().seek_async(pos).await
    }

    fn datasync_sync(self: Rc<Self>) -> FsResult<()> {
        self.inner.clone().datasync_sync()
    }

    async fn datasync_async(self: Rc<Self>) -> FsResult<()> {
        self.inner.clone().datasync_async().await
    }

    fn sync_sync(self: Rc<Self>) -> FsResult<()> {
        self.inner.clone().sync_sync()
    }

    async fn sync_async(self: Rc<Self>) -> FsResult<()> {
        self.inner.clone().sync_async().await
    }

    fn stat_sync(self: Rc<Self>) -> FsResult<FsStat> {
        self.caps.require_visible_read()?;
        self.inner.clone().stat_sync()
    }

    async fn stat_async(self: Rc<Self>) -> FsResult<FsStat> {
        self.caps.require_visible_read()?;
        self.inner.clone().stat_async().await
    }

    fn lock_sync(self: Rc<Self>, exclusive: bool) -> FsResult<()> {
        self.inner.clone().lock_sync(exclusive)
    }

    async fn lock_async(self: Rc<Self>, exclusive: bool) -> FsResult<()> {
        self.inner.clone().lock_async(exclusive).await
    }

    fn try_lock_sync(self: Rc<Self>, exclusive: bool) -> FsResult<bool> {
        self.inner.clone().try_lock_sync(exclusive)
    }

    async fn try_lock_async(self: Rc<Self>, exclusive: bool) -> FsResult<bool> {
        self.inner.clone().try_lock_async(exclusive).await
    }

    fn unlock_sync(self: Rc<Self>) -> FsResult<()> {
        self.inner.clone().unlock_sync()
    }

    async fn unlock_async(self: Rc<Self>) -> FsResult<()> {
        self.inner.clone().unlock_async().await
    }

    fn truncate_sync(self: Rc<Self>, len: u64) -> FsResult<()> {
        self.caps.require_file_write()?;
        if let Some(limit) = self.caps.write_size_limit
            && len > limit
        {
            return Err(io::ErrorKind::StorageFull.into());
        }
        self.inner.clone().truncate_sync(len)
    }

    async fn truncate_async(self: Rc<Self>, len: u64) -> FsResult<()> {
        self.caps.require_file_write()?;
        if let Some(limit) = self.caps.write_size_limit
            && len > limit
        {
            return Err(io::ErrorKind::StorageFull.into());
        }
        self.inner.clone().truncate_async(len).await
    }

    fn utime_sync(
        self: Rc<Self>,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        self.caps.require_metadata_mutate()?;
        self.inner
            .clone()
            .utime_sync(atime_secs, atime_nanos, mtime_secs, mtime_nanos)
    }

    async fn utime_async(
        self: Rc<Self>,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        self.caps.require_metadata_mutate()?;
        self.inner
            .clone()
            .utime_async(atime_secs, atime_nanos, mtime_secs, mtime_nanos)
            .await
    }

    fn read_at_sync(self: Rc<Self>, buf: &mut [u8], position: u64) -> FsResult<usize> {
        self.caps.require_file_read()?;
        self.inner.clone().read_at_sync(buf, position)
    }

    async fn read_at_async(
        self: Rc<Self>,
        buf: BufMutView,
        position: u64,
    ) -> FsResult<(usize, BufMutView)> {
        self.caps.require_file_read()?;
        self.inner.clone().read_at_async(buf, position).await
    }

    fn write_at_sync(self: Rc<Self>, buf: &[u8], position: u64) -> FsResult<usize> {
        self.caps.require_file_write()?;
        self.caps.check_write_size(buf.len())?;
        self.inner.clone().write_at_sync(buf, position)
    }

    fn as_stdio(self: Rc<Self>) -> FsResult<Stdio> {
        self.inner.clone().as_stdio()
    }

    fn backing_fd(self: Rc<Self>) -> Option<ResourceHandleFd> {
        self.inner.clone().backing_fd()
    }

    fn try_clone_inner(self: Rc<Self>) -> FsResult<Rc<dyn File>> {
        Ok(Rc::new(Self {
            inner: self.inner.clone().try_clone_inner()?,
            caps: self.caps.clone(),
            path: self.path.clone(),
        }))
    }
}

fn normalize_prefix(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        PathBuf::from("/").join(path)
    }
}

fn readonly() -> deno_io::fs::FsError {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "FsCaps readonly overlay denied mutation (EROFS)",
    )
    .into()
}

fn permission(message: &'static str) -> deno_io::fs::FsError {
    io::Error::new(io::ErrorKind::PermissionDenied, message).into()
}
