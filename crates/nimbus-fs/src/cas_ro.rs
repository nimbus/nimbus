use std::borrow::Cow;
use std::collections::BTreeMap;
use std::future::Future;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use deno_core::{BufMutView, BufView, ResourceHandleFd, WriteOutcome};
use deno_fs::sync::MaybeArc;
use deno_fs::{FileSystem, FsDirEntry, FsFileType, FsReadDir, FsReadDirRc, OpenOptions};
use deno_io::fs::{File, FsError, FsResult, FsStat, FsStatFs};
use deno_permissions::{CheckedPath, CheckedPathBuf};
use nimbus_blob::{BlobHash, BlobStore};
use tokio::io::AsyncReadExt;

#[derive(Clone)]
pub struct CasReadOnlyBackend {
    store: Arc<dyn BlobStore>,
    manifest: Arc<CasReadOnlyManifest>,
}

#[derive(Debug, Clone, Default)]
pub struct CasReadOnlyManifest {
    entries: BTreeMap<PathBuf, CasManifestEntry>,
}

#[derive(Debug, Clone)]
pub enum CasManifestEntry {
    Directory {
        mode: u32,
    },
    File {
        chunks: Vec<CasBlobChunk>,
        size: u64,
        mode: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CasBlobChunk {
    pub hash: BlobHash,
    pub len: u64,
}

#[derive(Debug, Clone)]
pub struct CasReadDir {
    entries: Arc<Mutex<Vec<FsDirEntry>>>,
}

#[derive(Debug)]
struct CasFile {
    backend: CasReadOnlyBackend,
    path: PathBuf,
    cursor: Mutex<u64>,
}

impl CasReadOnlyBackend {
    pub fn new(store: Arc<dyn BlobStore>, manifest: CasReadOnlyManifest) -> Self {
        Self {
            store,
            manifest: Arc::new(manifest),
        }
    }

    fn file_entry(&self, path: &Path) -> FsResult<(Vec<CasBlobChunk>, u64, u32)> {
        match self.manifest.entries.get(path) {
            Some(CasManifestEntry::File { chunks, size, mode }) => {
                Ok((chunks.clone(), *size, *mode))
            }
            Some(CasManifestEntry::Directory { .. }) => Err(io::Error::new(
                io::ErrorKind::IsADirectory,
                "CAS manifest path is a directory",
            )
            .into()),
            None => Err(io::ErrorKind::NotFound.into()),
        }
    }

    fn stat_path(&self, path: &Path) -> FsResult<FsStat> {
        let entry = self
            .manifest
            .entries
            .get(path)
            .ok_or(io::ErrorKind::NotFound)?;
        Ok(match entry {
            CasManifestEntry::Directory { mode } => stat_dir(*mode),
            CasManifestEntry::File { size, mode, .. } => stat_file(*size, *mode),
        })
    }

    fn read_range(&self, path: &Path, position: u64, buf: &mut [u8]) -> FsResult<usize> {
        let (chunks, size, _) = self.file_entry(path)?;
        if position >= size || buf.is_empty() {
            return Ok(0);
        }
        let max_read = (size - position).min(buf.len() as u64) as usize;
        let mut written = 0usize;
        let request_start = position;
        let request_end = position + max_read as u64;
        let mut chunk_start = 0u64;

        for chunk in chunks {
            let chunk_end = chunk_start + chunk.len;
            if request_start < chunk_end && request_end > chunk_start {
                let bytes = self.read_blob_chunk(&chunk.hash)?;
                let start = request_start.saturating_sub(chunk_start) as usize;
                let end = (request_end.min(chunk_end) - chunk_start) as usize;
                let slice = &bytes[start..end];
                buf[written..written + slice.len()].copy_from_slice(slice);
                written += slice.len();
                if written == max_read {
                    break;
                }
            }
            chunk_start = chunk_end;
        }

        Ok(written)
    }

    fn read_blob_chunk(&self, hash: &BlobHash) -> FsResult<Vec<u8>> {
        let store = self.store.clone();
        let hash = *hash;
        block_on_blob(async move {
            let mut stream = store.get_stream(&hash).await.map_err(blob_error)?;
            let mut bytes = Vec::new();
            stream
                .read_to_end(&mut bytes)
                .await
                .map_err(|error| io::Error::other(format!("read CAS blob stream: {error}")))?;
            Ok(bytes)
        })
    }

    fn readonly<T>(&self) -> FsResult<T> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "CAS read-only backend rejects mutation (EROFS)",
        )
        .into())
    }
}

impl std::fmt::Debug for CasReadOnlyBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CasReadOnlyBackend")
            .field("manifest_entries", &self.manifest.entries.len())
            .finish_non_exhaustive()
    }
}

impl CasReadOnlyManifest {
    pub fn new() -> Self {
        let mut manifest = Self::default();
        manifest.entries.insert(
            PathBuf::from("/"),
            CasManifestEntry::Directory { mode: 0o555 },
        );
        manifest
    }

    pub fn add_dir(mut self, path: impl AsRef<Path>, mode: u32) -> FsResult<Self> {
        let path = normalize(path.as_ref())?;
        self.ensure_parent_dirs(&path);
        self.entries
            .insert(path, CasManifestEntry::Directory { mode });
        Ok(self)
    }

    pub fn add_file(
        mut self,
        path: impl AsRef<Path>,
        chunks: Vec<CasBlobChunk>,
        mode: u32,
    ) -> FsResult<Self> {
        let path = normalize(path.as_ref())?;
        let size = chunks.iter().map(|chunk| chunk.len).sum();
        self.ensure_parent_dirs(&path);
        self.entries
            .insert(path, CasManifestEntry::File { chunks, size, mode });
        Ok(self)
    }

    fn ensure_parent_dirs(&mut self, path: &Path) {
        let mut current = PathBuf::from("/");
        for component in path.parent().unwrap_or(Path::new("/")).components() {
            if let Component::Normal(part) = component {
                current.push(part);
                self.entries
                    .entry(current.clone())
                    .or_insert(CasManifestEntry::Directory { mode: 0o555 });
            }
        }
    }

    fn child_entries(&self, path: &Path) -> FsResult<Vec<FsDirEntry>> {
        if !matches!(
            self.entries.get(path),
            Some(CasManifestEntry::Directory { .. })
        ) {
            return Err(
                io::Error::new(io::ErrorKind::NotADirectory, "path is not a directory").into(),
            );
        }
        let mut children = BTreeMap::<String, FsDirEntry>::new();
        for (candidate, entry) in &self.entries {
            if candidate == path {
                continue;
            }
            let Ok(relative) = candidate.strip_prefix(path) else {
                continue;
            };
            let mut components = relative.components();
            let Some(Component::Normal(name)) = components.next() else {
                continue;
            };
            if components.next().is_some() {
                continue;
            }
            let name = name.to_string_lossy().into_owned();
            children.insert(name.clone(), dir_entry(name, entry));
        }
        Ok(children.into_values().collect())
    }
}

impl CasBlobChunk {
    pub fn new(hash: BlobHash, len: u64) -> Self {
        Self { hash, len }
    }
}

#[async_trait::async_trait(?Send)]
impl FileSystem for CasReadOnlyBackend {
    fn cwd(&self) -> FsResult<PathBuf> {
        Ok(PathBuf::from("/"))
    }

    fn tmp_dir(&self) -> FsResult<PathBuf> {
        Ok(PathBuf::from("/tmp"))
    }

    fn chdir(&self, path: &CheckedPath<'_>) -> FsResult<()> {
        if self.stat_sync(path)?.is_directory {
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::NotADirectory, "path is not a directory").into())
        }
    }

    fn umask(&self, _mask: Option<u32>) -> FsResult<u32> {
        Ok(0)
    }

    fn open_sync(&self, path: &CheckedPath<'_>, options: OpenOptions) -> FsResult<Rc<dyn File>> {
        if options.write
            || options.create
            || options.truncate
            || options.append
            || options.create_new
        {
            return self.readonly();
        }
        let path = normalize(path)?;
        self.file_entry(&path)?;
        Ok(Rc::new(CasFile {
            backend: self.clone(),
            path,
            cursor: Mutex::new(0),
        }))
    }

    async fn open_async<'a>(
        &'a self,
        path: CheckedPathBuf,
        options: OpenOptions,
    ) -> FsResult<Rc<dyn File>> {
        self.open_sync(&path.as_checked_path(), options)
    }

    fn mkdir_sync(
        &self,
        _path: &CheckedPath<'_>,
        _recursive: bool,
        _mode: Option<u32>,
    ) -> FsResult<()> {
        self.readonly()
    }

    async fn mkdir_async(
        &self,
        _path: CheckedPathBuf,
        _recursive: bool,
        _mode: Option<u32>,
    ) -> FsResult<()> {
        self.readonly()
    }

    #[cfg(unix)]
    fn chmod_sync(&self, _path: &CheckedPath<'_>, _mode: u32) -> FsResult<()> {
        self.readonly()
    }

    #[cfg(not(unix))]
    fn chmod_sync(&self, _path: &CheckedPath<'_>, _mode: i32) -> FsResult<()> {
        self.readonly()
    }

    #[cfg(unix)]
    async fn chmod_async(&self, _path: CheckedPathBuf, _mode: u32) -> FsResult<()> {
        self.readonly()
    }

    #[cfg(not(unix))]
    async fn chmod_async(&self, _path: CheckedPathBuf, _mode: i32) -> FsResult<()> {
        self.readonly()
    }

    fn chown_sync(
        &self,
        _path: &CheckedPath<'_>,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        self.readonly()
    }

    async fn chown_async(
        &self,
        _path: CheckedPathBuf,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        self.readonly()
    }

    fn lchmod_sync(&self, _path: &CheckedPath<'_>, _mode: u32) -> FsResult<()> {
        self.readonly()
    }

    async fn lchmod_async(&self, _path: CheckedPathBuf, _mode: u32) -> FsResult<()> {
        self.readonly()
    }

    fn lchown_sync(
        &self,
        _path: &CheckedPath<'_>,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        self.readonly()
    }

    async fn lchown_async(
        &self,
        _path: CheckedPathBuf,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        self.readonly()
    }

    fn remove_sync(&self, _path: &CheckedPath<'_>, _recursive: bool) -> FsResult<()> {
        self.readonly()
    }

    async fn remove_async(&self, _path: CheckedPathBuf, _recursive: bool) -> FsResult<()> {
        self.readonly()
    }

    fn copy_file_sync(
        &self,
        _oldpath: &CheckedPath<'_>,
        _newpath: &CheckedPath<'_>,
    ) -> FsResult<()> {
        self.readonly()
    }

    async fn copy_file_async(
        &self,
        _oldpath: CheckedPathBuf,
        _newpath: CheckedPathBuf,
    ) -> FsResult<()> {
        self.readonly()
    }

    fn cp_sync(&self, _path: &CheckedPath<'_>, _new_path: &CheckedPath<'_>) -> FsResult<()> {
        self.readonly()
    }

    async fn cp_async(&self, _path: CheckedPathBuf, _new_path: CheckedPathBuf) -> FsResult<()> {
        self.readonly()
    }

    fn stat_sync(&self, path: &CheckedPath<'_>) -> FsResult<FsStat> {
        self.stat_path(&normalize(path)?)
    }

    async fn stat_async(&self, path: CheckedPathBuf) -> FsResult<FsStat> {
        self.stat_sync(&path.as_checked_path())
    }

    fn lstat_sync(&self, path: &CheckedPath<'_>) -> FsResult<FsStat> {
        self.stat_sync(path)
    }

    async fn lstat_async(&self, path: CheckedPathBuf) -> FsResult<FsStat> {
        self.stat_sync(&path.as_checked_path())
    }

    fn statfs_sync(&self, _path: &CheckedPath<'_>, _bigint: bool) -> FsResult<FsStatFs> {
        Ok(FsStatFs {
            typ: 0x43415352,
            bsize: 4096,
            blocks: 0,
            bfree: 0,
            bavail: 0,
            files: self.manifest.entries.len() as u64,
            ffree: 0,
        })
    }

    async fn statfs_async(&self, path: CheckedPathBuf, bigint: bool) -> FsResult<FsStatFs> {
        self.statfs_sync(&path.as_checked_path(), bigint)
    }

    fn realpath_sync(&self, path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        let path = normalize(path)?;
        if self.manifest.entries.contains_key(&path) {
            Ok(path)
        } else {
            Err(io::ErrorKind::NotFound.into())
        }
    }

    async fn realpath_async(&self, path: CheckedPathBuf) -> FsResult<PathBuf> {
        self.realpath_sync(&path.as_checked_path())
    }

    fn read_dir_sync(&self, path: &CheckedPath<'_>) -> FsResult<Vec<FsDirEntry>> {
        self.manifest.child_entries(&normalize(path)?)
    }

    async fn read_dir_async(&self, path: CheckedPathBuf) -> FsResult<FsReadDirRc> {
        let entries = self.read_dir_sync(&path.as_checked_path())?;
        Ok(MaybeArc::new(CasReadDir {
            entries: Arc::new(Mutex::new(entries)),
        }))
    }

    fn rename_sync(&self, _oldpath: &CheckedPath<'_>, _newpath: &CheckedPath<'_>) -> FsResult<()> {
        self.readonly()
    }

    async fn rename_async(
        &self,
        _oldpath: CheckedPathBuf,
        _newpath: CheckedPathBuf,
    ) -> FsResult<()> {
        self.readonly()
    }

    fn rmdir_sync(&self, _path: &CheckedPath<'_>) -> FsResult<()> {
        self.readonly()
    }

    async fn rmdir_async(&self, _path: CheckedPathBuf) -> FsResult<()> {
        self.readonly()
    }

    fn link_sync(&self, _oldpath: &CheckedPath<'_>, _newpath: &CheckedPath<'_>) -> FsResult<()> {
        self.readonly()
    }

    async fn link_async(&self, _oldpath: CheckedPathBuf, _newpath: CheckedPathBuf) -> FsResult<()> {
        self.readonly()
    }

    fn symlink_sync(
        &self,
        _oldpath: &CheckedPath<'_>,
        _newpath: &CheckedPath<'_>,
        _file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        self.readonly()
    }

    async fn symlink_async(
        &self,
        _oldpath: CheckedPathBuf,
        _newpath: CheckedPathBuf,
        _file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        self.readonly()
    }

    fn read_link_sync(&self, _path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        Err(FsError::NotSupported)
    }

    async fn read_link_async(&self, _path: CheckedPathBuf) -> FsResult<PathBuf> {
        Err(FsError::NotSupported)
    }

    fn truncate_sync(&self, _path: &CheckedPath<'_>, _len: u64) -> FsResult<()> {
        self.readonly()
    }

    async fn truncate_async(&self, _path: CheckedPathBuf, _len: u64) -> FsResult<()> {
        self.readonly()
    }

    fn utime_sync(
        &self,
        _path: &CheckedPath<'_>,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        self.readonly()
    }

    async fn utime_async(
        &self,
        _path: CheckedPathBuf,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        self.readonly()
    }

    fn lutime_sync(
        &self,
        _path: &CheckedPath<'_>,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        self.readonly()
    }

    async fn lutime_async(
        &self,
        _path: CheckedPathBuf,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        self.readonly()
    }

    fn exists_sync(&self, path: &CheckedPath<'_>) -> bool {
        normalize(path)
            .map(|path| self.manifest.entries.contains_key(&path))
            .unwrap_or(false)
    }

    async fn exists_async(&self, path: CheckedPathBuf) -> FsResult<bool> {
        Ok(self.exists_sync(&path.as_checked_path()))
    }
}

#[async_trait::async_trait(?Send)]
impl FsReadDir for CasReadDir {
    async fn next(&self) -> FsResult<Option<FsDirEntry>> {
        Ok(self.entries.lock().unwrap().pop())
    }
}

#[async_trait::async_trait(?Send)]
impl File for CasFile {
    fn maybe_path(&self) -> Option<&Path> {
        Some(&self.path)
    }

    fn read_sync(self: Rc<Self>, buf: &mut [u8]) -> FsResult<usize> {
        let mut cursor = self.cursor.lock().unwrap();
        let nread = self.backend.read_range(&self.path, *cursor, buf)?;
        *cursor += nread as u64;
        Ok(nread)
    }

    async fn read_byob(self: Rc<Self>, mut buf: BufMutView) -> FsResult<(usize, BufMutView)> {
        let nread = self.read_sync(&mut buf)?;
        Ok((nread, buf))
    }

    fn write_sync(self: Rc<Self>, _buf: &[u8]) -> FsResult<usize> {
        self.backend.readonly()
    }

    async fn write(self: Rc<Self>, view: BufView) -> FsResult<WriteOutcome> {
        self.write_sync(&view)
            .map(|nwritten| WriteOutcome::Partial { nwritten, view })
    }

    fn write_all_sync(self: Rc<Self>, _buf: &[u8]) -> FsResult<()> {
        self.backend.readonly()
    }

    async fn write_all(self: Rc<Self>, _buf: BufView) -> FsResult<()> {
        self.backend.readonly()
    }

    fn read_all_sync(self: Rc<Self>) -> FsResult<Cow<'static, [u8]>> {
        let size = self.backend.stat_path(&self.path)?.size;
        let cursor = *self.cursor.lock().unwrap();
        let remaining = size.saturating_sub(cursor);
        let mut bytes = vec![0; remaining as usize];
        let nread = self.backend.read_range(&self.path, cursor, &mut bytes)?;
        bytes.truncate(nread);
        Ok(Cow::Owned(bytes))
    }

    async fn read_all_async(self: Rc<Self>) -> FsResult<Cow<'static, [u8]>> {
        self.read_all_sync()
    }

    fn chmod_sync(self: Rc<Self>, _pathmode: u32) -> FsResult<()> {
        self.backend.readonly()
    }

    async fn chmod_async(self: Rc<Self>, _mode: u32) -> FsResult<()> {
        self.backend.readonly()
    }

    fn chown_sync(self: Rc<Self>, _uid: Option<u32>, _gid: Option<u32>) -> FsResult<()> {
        self.backend.readonly()
    }

    async fn chown_async(self: Rc<Self>, _uid: Option<u32>, _gid: Option<u32>) -> FsResult<()> {
        self.backend.readonly()
    }

    fn seek_sync(self: Rc<Self>, pos: io::SeekFrom) -> FsResult<u64> {
        let size = self.backend.stat_path(&self.path)?.size;
        let next = match pos {
            io::SeekFrom::Start(pos) => pos as i128,
            io::SeekFrom::End(offset) => size as i128 + offset as i128,
            io::SeekFrom::Current(offset) => *self.cursor.lock().unwrap() as i128 + offset as i128,
        };
        if next < 0 {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let next = u64::try_from(next)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "seek overflows u64"))?;
        *self.cursor.lock().unwrap() = next;
        Ok(next)
    }

    async fn seek_async(self: Rc<Self>, pos: io::SeekFrom) -> FsResult<u64> {
        self.seek_sync(pos)
    }

    fn datasync_sync(self: Rc<Self>) -> FsResult<()> {
        Ok(())
    }

    async fn datasync_async(self: Rc<Self>) -> FsResult<()> {
        Ok(())
    }

    fn sync_sync(self: Rc<Self>) -> FsResult<()> {
        Ok(())
    }

    async fn sync_async(self: Rc<Self>) -> FsResult<()> {
        Ok(())
    }

    fn stat_sync(self: Rc<Self>) -> FsResult<FsStat> {
        self.backend.stat_path(&self.path)
    }

    async fn stat_async(self: Rc<Self>) -> FsResult<FsStat> {
        self.stat_sync()
    }

    fn lock_sync(self: Rc<Self>, _exclusive: bool) -> FsResult<()> {
        Ok(())
    }

    async fn lock_async(self: Rc<Self>, exclusive: bool) -> FsResult<()> {
        self.lock_sync(exclusive)
    }

    fn try_lock_sync(self: Rc<Self>, _exclusive: bool) -> FsResult<bool> {
        Ok(true)
    }

    async fn try_lock_async(self: Rc<Self>, exclusive: bool) -> FsResult<bool> {
        self.try_lock_sync(exclusive)
    }

    fn unlock_sync(self: Rc<Self>) -> FsResult<()> {
        Ok(())
    }

    async fn unlock_async(self: Rc<Self>) -> FsResult<()> {
        Ok(())
    }

    fn truncate_sync(self: Rc<Self>, _len: u64) -> FsResult<()> {
        self.backend.readonly()
    }

    async fn truncate_async(self: Rc<Self>, _len: u64) -> FsResult<()> {
        self.backend.readonly()
    }

    fn utime_sync(
        self: Rc<Self>,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        self.backend.readonly()
    }

    async fn utime_async(
        self: Rc<Self>,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        self.backend.readonly()
    }

    fn read_at_sync(self: Rc<Self>, buf: &mut [u8], position: u64) -> FsResult<usize> {
        self.backend.read_range(&self.path, position, buf)
    }

    async fn read_at_async(
        self: Rc<Self>,
        mut buf: BufMutView,
        position: u64,
    ) -> FsResult<(usize, BufMutView)> {
        let nread = self.read_at_sync(&mut buf, position)?;
        Ok((nread, buf))
    }

    fn write_at_sync(self: Rc<Self>, _buf: &[u8], _position: u64) -> FsResult<usize> {
        self.backend.readonly()
    }

    fn as_stdio(self: Rc<Self>) -> FsResult<Stdio> {
        Err(FsError::NotSupported)
    }

    fn backing_fd(self: Rc<Self>) -> Option<ResourceHandleFd> {
        None
    }

    fn try_clone_inner(self: Rc<Self>) -> FsResult<Rc<dyn File>> {
        Ok(Rc::new(CasFile {
            backend: self.backend.clone(),
            path: self.path.clone(),
            cursor: Mutex::new(*self.cursor.lock().unwrap()),
        }))
    }
}

fn normalize(path: &Path) -> FsResult<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        PathBuf::from("/").join(path)
    };
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(part) => parts.push(part.to_owned()),
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "path escapes CAS manifest root",
                    )
                    .into());
                }
            }
            Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "platform prefixes are unsupported in CAS manifest paths",
                )
                .into());
            }
        }
    }
    let mut normalized = PathBuf::from("/");
    for part in parts {
        normalized.push(part);
    }
    Ok(normalized)
}

fn stat_file(size: u64, mode: u32) -> FsStat {
    FsStat {
        is_file: true,
        is_directory: false,
        is_symlink: false,
        size,
        mtime: None,
        atime: None,
        birthtime: None,
        ctime: None,
        dev: 0,
        ino: None,
        mode,
        nlink: Some(1),
        uid: 0,
        gid: 0,
        rdev: 0,
        blksize: 4096,
        blocks: Some(size.div_ceil(512)),
        is_block_device: false,
        is_char_device: false,
        is_fifo: false,
        is_socket: false,
    }
}

fn stat_dir(mode: u32) -> FsStat {
    FsStat {
        is_file: false,
        is_directory: true,
        is_symlink: false,
        size: 0,
        mtime: None,
        atime: None,
        birthtime: None,
        ctime: None,
        dev: 0,
        ino: None,
        mode,
        nlink: Some(1),
        uid: 0,
        gid: 0,
        rdev: 0,
        blksize: 4096,
        blocks: Some(0),
        is_block_device: false,
        is_char_device: false,
        is_fifo: false,
        is_socket: false,
    }
}

fn dir_entry(name: String, entry: &CasManifestEntry) -> FsDirEntry {
    FsDirEntry {
        name,
        is_file: matches!(entry, CasManifestEntry::File { .. }),
        is_directory: matches!(entry, CasManifestEntry::Directory { .. }),
        is_symlink: false,
    }
}

fn blob_error(error: nimbus_core::Error) -> io::Error {
    match error {
        nimbus_core::Error::NotFound(message) => io::Error::new(io::ErrorKind::NotFound, message),
        other => io::Error::other(other.to_string()),
    }
}

fn block_on_blob<T, F>(future: F) -> FsResult<T>
where
    T: Send + 'static,
    F: Future<Output = FsResult<T>> + Send + 'static,
{
    let runner = move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| io::Error::other(format!("build CAS runtime: {error}")))?
            .block_on(future)
    };
    if tokio::runtime::Handle::try_current().is_ok() {
        std::thread::spawn(runner)
            .join()
            .map_err(|_| io::Error::other("CAS stream thread panicked"))?
    } else {
        runner()
    }
}
