use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use deno_core::{BufMutView, BufView, ResourceHandleFd, WriteOutcome};
use deno_fs::sync::MaybeArc;
use deno_fs::{FileSystem, FsDirEntry, FsFileType, FsReadDir, FsReadDirRc, OpenOptions};
use deno_io::fs::{File, FsError, FsResult, FsStat, FsStatFs};
use deno_permissions::{CheckedPath, CheckedPathBuf};

use crate::PlatformStdio;

#[derive(Debug, Clone)]
pub struct MemFsBackend {
    state: Arc<Mutex<MemFsState>>,
    quota_bytes: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct MemFsReadDir {
    entries: Arc<Mutex<Vec<FsDirEntry>>>,
}

#[derive(Debug)]
struct MemFsState {
    nodes: BTreeMap<PathBuf, Node>,
    bytes_used: u64,
    next_ino: u64,
}

#[derive(Debug, Clone)]
enum Node {
    File(FileNode),
    Directory(DirectoryNode),
    Symlink(SymlinkNode),
}

#[derive(Debug, Clone)]
struct FileNode {
    data: Vec<u8>,
    mode: u32,
    ino: u64,
    atime: Option<u64>,
    mtime: Option<u64>,
}

#[derive(Debug, Clone)]
struct DirectoryNode {
    mode: u32,
    ino: u64,
}

#[derive(Debug, Clone)]
struct SymlinkNode {
    target: PathBuf,
    ino: u64,
}

#[derive(Debug)]
struct MemFile {
    fs: MemFsBackend,
    path: PathBuf,
    cursor: Mutex<u64>,
    readable: bool,
    writable: bool,
    append: bool,
}

impl MemFsBackend {
    pub fn new() -> Self {
        Self::with_quota_bytes(None)
    }

    pub fn with_quota_bytes(quota_bytes: Option<u64>) -> Self {
        let mut nodes = BTreeMap::new();
        nodes.insert(
            PathBuf::from("/"),
            Node::Directory(DirectoryNode {
                mode: 0o755,
                ino: 1,
            }),
        );
        Self {
            state: Arc::new(Mutex::new(MemFsState {
                nodes,
                bytes_used: 0,
                next_ino: 2,
            })),
            quota_bytes,
        }
    }

    pub fn total_bytes(&self) -> u64 {
        self.state.lock().unwrap().bytes_used
    }

    fn with_state<T>(&self, f: impl FnOnce(&mut MemFsState) -> FsResult<T>) -> FsResult<T> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("MemFs state lock poisoned"))?;
        f(&mut state)
    }

    fn set_file_data(&self, path: &Path, data: Vec<u8>) -> FsResult<()> {
        self.with_state(|state| {
            let current = match state.nodes.get(path) {
                Some(Node::File(file)) => file.data.len() as u64,
                Some(_) => {
                    return Err(
                        io::Error::new(io::ErrorKind::InvalidInput, "path is not a file").into(),
                    );
                }
                None => return Err(io::ErrorKind::NotFound.into()),
            };
            let next_used = state.bytes_used - current + data.len() as u64;
            if let Some(quota) = self.quota_bytes
                && next_used > quota
            {
                return Err(io::Error::new(
                    io::ErrorKind::StorageFull,
                    "MemFs byte quota exceeded",
                )
                .into());
            }
            if let Some(Node::File(file)) = state.nodes.get_mut(path) {
                file.data = data;
                state.bytes_used = next_used;
            }
            Ok(())
        })
    }

    fn write_at_path(&self, path: &Path, position: u64, buf: &[u8]) -> FsResult<usize> {
        self.with_state(|state| {
            let Some(Node::File(file)) = state.nodes.get(path) else {
                return Err(io::ErrorKind::NotFound.into());
            };
            let mut data = file.data.clone();
            let start = usize::try_from(position).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "position overflows usize")
            })?;
            if start > data.len() {
                data.resize(start, 0);
            }
            let end = start.checked_add(buf.len()).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "write length overflows usize")
            })?;
            if end > data.len() {
                data.resize(end, 0);
            }
            data[start..end].copy_from_slice(buf);
            let current = file.data.len() as u64;
            let next_used = state.bytes_used - current + data.len() as u64;
            if let Some(quota) = self.quota_bytes
                && next_used > quota
            {
                return Err(io::Error::new(
                    io::ErrorKind::StorageFull,
                    "MemFs byte quota exceeded",
                )
                .into());
            }
            if let Some(Node::File(file)) = state.nodes.get_mut(path) {
                file.data = data;
                state.bytes_used = next_used;
            }
            Ok(buf.len())
        })
    }

    fn read_at_path(&self, path: &Path, position: u64, buf: &mut [u8]) -> FsResult<usize> {
        self.with_state(|state| {
            let node = state.nodes.get(path).ok_or(io::ErrorKind::NotFound)?;
            let Node::File(file) = node else {
                return Err(
                    io::Error::new(io::ErrorKind::InvalidInput, "path is not a file").into(),
                );
            };
            let start = usize::try_from(position).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "position overflows usize")
            })?;
            if start >= file.data.len() {
                return Ok(0);
            }
            let available = &file.data[start..];
            let nread = available.len().min(buf.len());
            buf[..nread].copy_from_slice(&available[..nread]);
            Ok(nread)
        })
    }

    fn read_all_path(&self, path: &Path, position: u64) -> FsResult<Cow<'static, [u8]>> {
        self.with_state(|state| {
            let node = state.nodes.get(path).ok_or(io::ErrorKind::NotFound)?;
            let Node::File(file) = node else {
                return Err(
                    io::Error::new(io::ErrorKind::InvalidInput, "path is not a file").into(),
                );
            };
            let start = usize::try_from(position).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "position overflows usize")
            })?;
            if start >= file.data.len() {
                return Ok(Cow::Owned(Vec::new()));
            }
            Ok(Cow::Owned(file.data[start..].to_vec()))
        })
    }

    fn stat_path(&self, path: &Path, follow_final_symlink: bool) -> FsResult<FsStat> {
        self.with_state(|state| {
            let resolved = if follow_final_symlink {
                state.resolve_symlinks(path, true)?
            } else {
                path.to_path_buf()
            };
            let node = state.nodes.get(&resolved).ok_or(io::ErrorKind::NotFound)?;
            Ok(stat_for_node(node))
        })
    }
}

impl Default for MemFsBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MemFsState {
    fn alloc_ino(&mut self) -> u64 {
        let ino = self.next_ino;
        self.next_ino += 1;
        ino
    }

    fn ensure_parent_dir(&self, path: &Path) -> FsResult<PathBuf> {
        let parent = path.parent().unwrap_or_else(|| Path::new("/"));
        if matches!(self.nodes.get(parent), Some(Node::Directory(_))) {
            Ok(parent.to_path_buf())
        } else {
            Err(io::Error::new(io::ErrorKind::NotFound, "parent directory not found").into())
        }
    }

    fn resolve_symlinks(&self, path: &Path, follow_final: bool) -> FsResult<PathBuf> {
        let mut current = PathBuf::from("/");
        let mut remaining: Vec<_> = path
            .components()
            .filter_map(|component| match component {
                Component::Normal(part) => Some(part.to_owned()),
                _ => None,
            })
            .collect();
        let mut seen = BTreeSet::new();

        while let Some(part) = remaining.first().cloned() {
            remaining.remove(0);
            current.push(&part);
            let is_final = remaining.is_empty();
            if is_final && !follow_final {
                break;
            }
            let Some(Node::Symlink(link)) = self.nodes.get(&current) else {
                continue;
            };
            if !seen.insert(current.clone()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "symlink loop denied on access",
                )
                .into());
            }
            if link.target.is_absolute() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "absolute symlink target denied on access",
                )
                .into());
            }
            let parent = current.parent().unwrap_or_else(|| Path::new("/"));
            let mut next = parent.to_path_buf();
            next.push(&link.target);
            for rest in &remaining {
                next.push(rest);
            }
            current = PathBuf::from("/");
            remaining = next
                .components()
                .filter_map(|component| match component {
                    Component::Normal(part) => Some(part.to_owned()),
                    Component::ParentDir => Some("..".into()),
                    _ => None,
                })
                .collect();
            if remaining.iter().any(|part| part == "..") {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "symlink target escapes MemFs root",
                )
                .into());
            }
        }
        Ok(current)
    }

    fn child_entries(&self, path: &Path) -> FsResult<Vec<FsDirEntry>> {
        if !matches!(self.nodes.get(path), Some(Node::Directory(_))) {
            return Err(
                io::Error::new(io::ErrorKind::NotADirectory, "path is not a directory").into(),
            );
        }
        let mut entries = BTreeMap::<String, FsDirEntry>::new();
        for (candidate, node) in &self.nodes {
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
            entries.insert(name.clone(), dir_entry(name, node));
        }
        Ok(entries.into_values().collect())
    }
}

#[async_trait::async_trait(?Send)]
impl FileSystem for MemFsBackend {
    fn cwd(&self) -> FsResult<PathBuf> {
        Ok(PathBuf::from("/"))
    }

    fn tmp_dir(&self) -> FsResult<PathBuf> {
        Ok(PathBuf::from("/tmp"))
    }

    fn chdir(&self, path: &CheckedPath<'_>) -> FsResult<()> {
        let path = normalize(path)?;
        if matches!(
            self.state.lock().unwrap().nodes.get(&path),
            Some(Node::Directory(_))
        ) {
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::NotADirectory, "path is not a directory").into())
        }
    }

    fn umask(&self, _mask: Option<u32>) -> FsResult<u32> {
        Ok(0)
    }

    fn open_sync(&self, path: &CheckedPath<'_>, options: OpenOptions) -> FsResult<Rc<dyn File>> {
        let path = normalize(path)?;
        let path = self.with_state(|state| {
            let resolved = state.resolve_symlinks(&path, true)?;
            if matches!(state.nodes.get(&resolved), Some(Node::Directory(_))) {
                return Err(
                    io::Error::new(io::ErrorKind::IsADirectory, "path is a directory").into(),
                );
            }
            if options.create_new && state.nodes.contains_key(&resolved) {
                return Err(io::ErrorKind::AlreadyExists.into());
            }
            if !state.nodes.contains_key(&resolved) {
                if !(options.create || options.create_new) {
                    return Err(io::ErrorKind::NotFound.into());
                }
                state.ensure_parent_dir(&resolved)?;
                let ino = state.alloc_ino();
                state.nodes.insert(
                    resolved.clone(),
                    Node::File(FileNode {
                        data: Vec::new(),
                        mode: options.mode.unwrap_or(0o644),
                        ino,
                        atime: None,
                        mtime: None,
                    }),
                );
            }
            if options.truncate
                && let Some(Node::File(file)) = state.nodes.get_mut(&resolved)
            {
                state.bytes_used -= file.data.len() as u64;
                file.data.clear();
            }
            Ok(resolved)
        })?;
        let cursor = if options.append {
            self.stat_path(&path, true)?.size
        } else {
            0
        };
        Ok(Rc::new(MemFile {
            fs: self.clone(),
            path,
            cursor: Mutex::new(cursor),
            readable: options.read || !options.write,
            writable: options.write || options.append,
            append: options.append,
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
        path: &CheckedPath<'_>,
        recursive: bool,
        mode: Option<u32>,
    ) -> FsResult<()> {
        let path = normalize(path)?;
        self.with_state(|state| {
            if state.nodes.contains_key(&path) {
                return Err(io::ErrorKind::AlreadyExists.into());
            }
            if recursive {
                let mut current = PathBuf::from("/");
                for component in path.components() {
                    if let Component::Normal(part) = component {
                        current.push(part);
                        if !state.nodes.contains_key(&current) {
                            let ino = state.alloc_ino();
                            state.nodes.insert(
                                current.clone(),
                                Node::Directory(DirectoryNode {
                                    mode: mode.unwrap_or(0o755),
                                    ino,
                                }),
                            );
                        }
                    }
                }
            } else {
                state.ensure_parent_dir(&path)?;
                let ino = state.alloc_ino();
                state.nodes.insert(
                    path,
                    Node::Directory(DirectoryNode {
                        mode: mode.unwrap_or(0o755),
                        ino,
                    }),
                );
            }
            Ok(())
        })
    }

    async fn mkdir_async(
        &self,
        path: CheckedPathBuf,
        recursive: bool,
        mode: Option<u32>,
    ) -> FsResult<()> {
        self.mkdir_sync(&path.as_checked_path(), recursive, mode)
    }

    #[cfg(unix)]
    fn chmod_sync(&self, path: &CheckedPath<'_>, mode: u32) -> FsResult<()> {
        self.set_mode(&normalize(path)?, mode)
    }

    #[cfg(not(unix))]
    fn chmod_sync(&self, path: &CheckedPath<'_>, mode: i32) -> FsResult<()> {
        self.set_mode(&normalize(path)?, mode as u32)
    }

    #[cfg(unix)]
    async fn chmod_async(&self, path: CheckedPathBuf, mode: u32) -> FsResult<()> {
        self.set_mode(&normalize(&path.as_checked_path())?, mode)
    }

    #[cfg(not(unix))]
    async fn chmod_async(&self, path: CheckedPathBuf, mode: i32) -> FsResult<()> {
        self.set_mode(&normalize(&path.as_checked_path())?, mode as u32)
    }

    fn chown_sync(
        &self,
        _path: &CheckedPath<'_>,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        Err(FsError::NotSupported)
    }

    async fn chown_async(
        &self,
        path: CheckedPathBuf,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        self.chown_sync(&path.as_checked_path(), uid, gid)
    }

    // deno's lchmod keeps mode: u32 on every target while chmod is
    // platform-split to i32 off unix (interface.rs:186-209), so the
    // forward casts there.
    fn lchmod_sync(&self, path: &CheckedPath<'_>, mode: u32) -> FsResult<()> {
        #[cfg(unix)]
        {
            self.chmod_sync(path, mode)
        }
        #[cfg(not(unix))]
        {
            self.chmod_sync(path, mode as i32)
        }
    }

    async fn lchmod_async(&self, path: CheckedPathBuf, mode: u32) -> FsResult<()> {
        #[cfg(unix)]
        {
            self.chmod_async(path, mode).await
        }
        #[cfg(not(unix))]
        {
            self.chmod_async(path, mode as i32).await
        }
    }

    fn lchown_sync(
        &self,
        path: &CheckedPath<'_>,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        self.chown_sync(path, uid, gid)
    }

    async fn lchown_async(
        &self,
        path: CheckedPathBuf,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        self.chown_async(path, uid, gid).await
    }

    fn remove_sync(&self, path: &CheckedPath<'_>, recursive: bool) -> FsResult<()> {
        let path = normalize(path)?;
        self.with_state(|state| {
            let path = state.resolve_symlinks(&path, false)?;
            if path == Path::new("/") {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            let Some(node) = state.nodes.get(&path).cloned() else {
                return Err(io::ErrorKind::NotFound.into());
            };
            if matches!(node, Node::Directory(_)) {
                let children: Vec<_> = state
                    .nodes
                    .keys()
                    .filter(|candidate| *candidate != &path && candidate.starts_with(&path))
                    .cloned()
                    .collect();
                if !recursive && !children.is_empty() {
                    return Err(io::ErrorKind::DirectoryNotEmpty.into());
                }
                for child in children {
                    if let Some(Node::File(file)) = state.nodes.remove(&child) {
                        state.bytes_used -= file.data.len() as u64;
                    }
                }
            }
            if let Some(Node::File(file)) = state.nodes.remove(&path) {
                state.bytes_used -= file.data.len() as u64;
            }
            Ok(())
        })
    }

    async fn remove_async(&self, path: CheckedPathBuf, recursive: bool) -> FsResult<()> {
        self.remove_sync(&path.as_checked_path(), recursive)
    }

    fn copy_file_sync(&self, oldpath: &CheckedPath<'_>, newpath: &CheckedPath<'_>) -> FsResult<()> {
        let oldpath = normalize(oldpath)?;
        let newpath = normalize(newpath)?;
        self.with_state(|state| {
            let source = state.resolve_symlinks(&oldpath, true)?;
            let dest = state.resolve_symlinks(&newpath, false).unwrap_or(newpath);
            state.ensure_parent_dir(&dest)?;
            let data = match state.nodes.get(&source) {
                Some(Node::File(file)) => file.data.clone(),
                Some(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "source is not a file",
                    )
                    .into());
                }
                None => return Err(io::ErrorKind::NotFound.into()),
            };
            let current = match state.nodes.get(&dest) {
                Some(Node::File(file)) => file.data.len() as u64,
                Some(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "destination is not a file",
                    )
                    .into());
                }
                None => 0,
            };
            let next_used = state.bytes_used - current + data.len() as u64;
            if let Some(quota) = self.quota_bytes
                && next_used > quota
            {
                return Err(io::ErrorKind::StorageFull.into());
            }
            let ino = state.alloc_ino();
            state.nodes.insert(
                dest,
                Node::File(FileNode {
                    data,
                    mode: 0o644,
                    ino,
                    atime: None,
                    mtime: None,
                }),
            );
            state.bytes_used = next_used;
            Ok(())
        })
    }

    async fn copy_file_async(
        &self,
        oldpath: CheckedPathBuf,
        newpath: CheckedPathBuf,
    ) -> FsResult<()> {
        self.copy_file_sync(&oldpath.as_checked_path(), &newpath.as_checked_path())
    }

    fn cp_sync(&self, path: &CheckedPath<'_>, new_path: &CheckedPath<'_>) -> FsResult<()> {
        self.copy_file_sync(path, new_path)
    }

    async fn cp_async(&self, path: CheckedPathBuf, new_path: CheckedPathBuf) -> FsResult<()> {
        self.copy_file_sync(&path.as_checked_path(), &new_path.as_checked_path())
    }

    fn stat_sync(&self, path: &CheckedPath<'_>) -> FsResult<FsStat> {
        self.stat_path(&normalize(path)?, true)
    }

    async fn stat_async(&self, path: CheckedPathBuf) -> FsResult<FsStat> {
        self.stat_sync(&path.as_checked_path())
    }

    fn lstat_sync(&self, path: &CheckedPath<'_>) -> FsResult<FsStat> {
        self.stat_path(&normalize(path)?, false)
    }

    async fn lstat_async(&self, path: CheckedPathBuf) -> FsResult<FsStat> {
        self.lstat_sync(&path.as_checked_path())
    }

    fn statfs_sync(&self, _path: &CheckedPath<'_>, _bigint: bool) -> FsResult<FsStatFs> {
        let blocks = self
            .quota_bytes
            .unwrap_or_else(|| self.total_bytes().max(1))
            .div_ceil(4096);
        Ok(FsStatFs {
            typ: 0x4e46534d,
            bsize: 4096,
            blocks,
            bfree: 0,
            bavail: 0,
            files: self.state.lock().unwrap().nodes.len() as u64,
            ffree: 0,
        })
    }

    async fn statfs_async(&self, path: CheckedPathBuf, bigint: bool) -> FsResult<FsStatFs> {
        self.statfs_sync(&path.as_checked_path(), bigint)
    }

    fn realpath_sync(&self, path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        self.with_state(|state| state.resolve_symlinks(&normalize(path)?, true))
    }

    async fn realpath_async(&self, path: CheckedPathBuf) -> FsResult<PathBuf> {
        self.realpath_sync(&path.as_checked_path())
    }

    fn read_dir_sync(&self, path: &CheckedPath<'_>) -> FsResult<Vec<FsDirEntry>> {
        self.with_state(|state| {
            let path = state.resolve_symlinks(&normalize(path)?, true)?;
            state.child_entries(&path)
        })
    }

    async fn read_dir_async(&self, path: CheckedPathBuf) -> FsResult<FsReadDirRc> {
        let entries = self.read_dir_sync(&path.as_checked_path())?;
        Ok(MaybeArc::new(MemFsReadDir {
            entries: Arc::new(Mutex::new(entries)),
        }))
    }

    fn rename_sync(&self, oldpath: &CheckedPath<'_>, newpath: &CheckedPath<'_>) -> FsResult<()> {
        let oldpath = normalize(oldpath)?;
        let newpath = normalize(newpath)?;
        self.with_state(|state| {
            let oldpath = state.resolve_symlinks(&oldpath, false)?;
            state.ensure_parent_dir(&newpath)?;
            let moving: Vec<_> = state
                .nodes
                .keys()
                .filter(|candidate| *candidate == &oldpath || candidate.starts_with(&oldpath))
                .cloned()
                .collect();
            if moving.is_empty() {
                return Err(io::ErrorKind::NotFound.into());
            }
            for source in moving {
                let node = state.nodes.remove(&source).unwrap();
                let relative = source.strip_prefix(&oldpath).unwrap_or(Path::new(""));
                let mut dest = newpath.clone();
                dest.push(relative);
                state.nodes.insert(dest, node);
            }
            Ok(())
        })
    }

    async fn rename_async(&self, oldpath: CheckedPathBuf, newpath: CheckedPathBuf) -> FsResult<()> {
        self.rename_sync(&oldpath.as_checked_path(), &newpath.as_checked_path())
    }

    fn rmdir_sync(&self, path: &CheckedPath<'_>) -> FsResult<()> {
        self.remove_sync(path, false)
    }

    async fn rmdir_async(&self, path: CheckedPathBuf) -> FsResult<()> {
        self.rmdir_sync(&path.as_checked_path())
    }

    fn link_sync(&self, _oldpath: &CheckedPath<'_>, _newpath: &CheckedPath<'_>) -> FsResult<()> {
        Err(FsError::NotSupported)
    }

    async fn link_async(&self, oldpath: CheckedPathBuf, newpath: CheckedPathBuf) -> FsResult<()> {
        self.link_sync(&oldpath.as_checked_path(), &newpath.as_checked_path())
    }

    fn symlink_sync(
        &self,
        oldpath: &CheckedPath<'_>,
        newpath: &CheckedPath<'_>,
        _file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        let target = oldpath.to_path_buf();
        let newpath = normalize(newpath)?;
        self.with_state(|state| {
            state.ensure_parent_dir(&newpath)?;
            if state.nodes.contains_key(&newpath) {
                return Err(io::ErrorKind::AlreadyExists.into());
            }
            let ino = state.alloc_ino();
            state
                .nodes
                .insert(newpath, Node::Symlink(SymlinkNode { target, ino }));
            Ok(())
        })
    }

    async fn symlink_async(
        &self,
        oldpath: CheckedPathBuf,
        newpath: CheckedPathBuf,
        file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        self.symlink_sync(
            &oldpath.as_checked_path(),
            &newpath.as_checked_path(),
            file_type,
        )
    }

    fn read_link_sync(&self, path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        self.with_state(|state| {
            let path = normalize(path)?;
            match state.nodes.get(&path) {
                Some(Node::Symlink(link)) => Ok(link.target.clone()),
                Some(_) => Err(io::ErrorKind::InvalidInput.into()),
                None => Err(io::ErrorKind::NotFound.into()),
            }
        })
    }

    async fn read_link_async(&self, path: CheckedPathBuf) -> FsResult<PathBuf> {
        self.read_link_sync(&path.as_checked_path())
    }

    fn truncate_sync(&self, path: &CheckedPath<'_>, len: u64) -> FsResult<()> {
        let path = normalize(path)?;
        let path = self.with_state(|state| state.resolve_symlinks(&path, true))?;
        let len = usize::try_from(len)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "len overflows usize"))?;
        let mut data = self.read_all_path(&path, 0)?.into_owned();
        data.resize(len, 0);
        self.set_file_data(&path, data)
    }

    async fn truncate_async(&self, path: CheckedPathBuf, len: u64) -> FsResult<()> {
        self.truncate_sync(&path.as_checked_path(), len)
    }

    fn utime_sync(
        &self,
        path: &CheckedPath<'_>,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        let path = normalize(path)?;
        let atime = timestamp_millis(atime_secs, atime_nanos);
        let mtime = timestamp_millis(mtime_secs, mtime_nanos);
        self.with_state(|state| {
            let path = state.resolve_symlinks(&path, true)?;
            match state.nodes.get_mut(&path) {
                Some(Node::File(file)) => {
                    file.atime = Some(atime);
                    file.mtime = Some(mtime);
                    Ok(())
                }
                Some(_) => Ok(()),
                None => Err(io::ErrorKind::NotFound.into()),
            }
        })
    }

    async fn utime_async(
        &self,
        path: CheckedPathBuf,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        self.utime_sync(
            &path.as_checked_path(),
            atime_secs,
            atime_nanos,
            mtime_secs,
            mtime_nanos,
        )
    }

    fn lutime_sync(
        &self,
        path: &CheckedPath<'_>,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        self.utime_sync(path, atime_secs, atime_nanos, mtime_secs, mtime_nanos)
    }

    async fn lutime_async(
        &self,
        path: CheckedPathBuf,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        self.utime_async(path, atime_secs, atime_nanos, mtime_secs, mtime_nanos)
            .await
    }

    fn exists_sync(&self, path: &CheckedPath<'_>) -> bool {
        let Ok(path) = normalize(path) else {
            return false;
        };
        self.with_state(|state| {
            let Ok(path) = state.resolve_symlinks(&path, true) else {
                return Ok(false);
            };
            Ok(state.nodes.contains_key(&path))
        })
        .unwrap_or(false)
    }

    async fn exists_async(&self, path: CheckedPathBuf) -> FsResult<bool> {
        Ok(self.exists_sync(&path.as_checked_path()))
    }
}

impl MemFsBackend {
    fn set_mode(&self, path: &Path, mode: u32) -> FsResult<()> {
        self.with_state(|state| {
            let path = state.resolve_symlinks(path, false)?;
            match state.nodes.get_mut(&path) {
                Some(Node::File(file)) => file.mode = mode,
                Some(Node::Directory(dir)) => dir.mode = mode,
                Some(Node::Symlink(_)) => {}
                None => return Err(io::ErrorKind::NotFound.into()),
            }
            Ok(())
        })
    }
}

#[async_trait::async_trait(?Send)]
impl FsReadDir for MemFsReadDir {
    async fn next(&self) -> FsResult<Option<FsDirEntry>> {
        Ok(self.entries.lock().unwrap().pop())
    }
}

#[async_trait::async_trait(?Send)]
impl File for MemFile {
    fn maybe_path(&self) -> Option<&Path> {
        Some(&self.path)
    }

    fn read_sync(self: Rc<Self>, buf: &mut [u8]) -> FsResult<usize> {
        if !self.readable {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let mut cursor = self.cursor.lock().unwrap();
        let nread = self.fs.read_at_path(&self.path, *cursor, buf)?;
        *cursor += nread as u64;
        Ok(nread)
    }

    async fn read_byob(self: Rc<Self>, mut buf: BufMutView) -> FsResult<(usize, BufMutView)> {
        let nread = self.read_sync(&mut buf)?;
        Ok((nread, buf))
    }

    fn write_sync(self: Rc<Self>, buf: &[u8]) -> FsResult<usize> {
        if !self.writable {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let mut cursor = self.cursor.lock().unwrap();
        let position = if self.append {
            self.fs.stat_path(&self.path, true)?.size
        } else {
            *cursor
        };
        let nwritten = self.fs.write_at_path(&self.path, position, buf)?;
        *cursor = position + nwritten as u64;
        Ok(nwritten)
    }

    async fn write(self: Rc<Self>, view: BufView) -> FsResult<WriteOutcome> {
        let nwritten = self.clone().write_sync(&view)?;
        Ok(WriteOutcome::Partial { nwritten, view })
    }

    fn write_all_sync(self: Rc<Self>, buf: &[u8]) -> FsResult<()> {
        self.write_sync(buf)?;
        Ok(())
    }

    async fn write_all(self: Rc<Self>, buf: BufView) -> FsResult<()> {
        self.write_all_sync(&buf)
    }

    fn read_all_sync(self: Rc<Self>) -> FsResult<Cow<'static, [u8]>> {
        if !self.readable {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let cursor = *self.cursor.lock().unwrap();
        self.fs.read_all_path(&self.path, cursor)
    }

    async fn read_all_async(self: Rc<Self>) -> FsResult<Cow<'static, [u8]>> {
        self.read_all_sync()
    }

    fn chmod_sync(self: Rc<Self>, pathmode: u32) -> FsResult<()> {
        self.fs.set_mode(&self.path, pathmode)
    }

    async fn chmod_async(self: Rc<Self>, mode: u32) -> FsResult<()> {
        self.chmod_sync(mode)
    }

    fn chown_sync(self: Rc<Self>, _uid: Option<u32>, _gid: Option<u32>) -> FsResult<()> {
        Err(FsError::NotSupported)
    }

    async fn chown_async(self: Rc<Self>, uid: Option<u32>, gid: Option<u32>) -> FsResult<()> {
        self.chown_sync(uid, gid)
    }

    fn seek_sync(self: Rc<Self>, pos: io::SeekFrom) -> FsResult<u64> {
        let size = self.fs.stat_path(&self.path, true)?.size;
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
        self.fs.stat_path(&self.path, true)
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

    fn truncate_sync(self: Rc<Self>, len: u64) -> FsResult<()> {
        let len = usize::try_from(len)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "len overflows usize"))?;
        let mut data = self.fs.read_all_path(&self.path, 0)?.into_owned();
        data.resize(len, 0);
        self.fs.set_file_data(&self.path, data)
    }

    async fn truncate_async(self: Rc<Self>, len: u64) -> FsResult<()> {
        self.truncate_sync(len)
    }

    fn utime_sync(
        self: Rc<Self>,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        let path = CheckedPath::unsafe_new(Cow::Borrowed(self.path.as_path()));
        self.fs
            .utime_sync(&path, atime_secs, atime_nanos, mtime_secs, mtime_nanos)
    }

    async fn utime_async(
        self: Rc<Self>,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        self.utime_sync(atime_secs, atime_nanos, mtime_secs, mtime_nanos)
    }

    fn read_at_sync(self: Rc<Self>, buf: &mut [u8], position: u64) -> FsResult<usize> {
        if !self.readable {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        self.fs.read_at_path(&self.path, position, buf)
    }

    async fn read_at_async(
        self: Rc<Self>,
        mut buf: BufMutView,
        position: u64,
    ) -> FsResult<(usize, BufMutView)> {
        let nread = self.read_at_sync(&mut buf, position)?;
        Ok((nread, buf))
    }

    fn write_at_sync(self: Rc<Self>, buf: &[u8], position: u64) -> FsResult<usize> {
        if !self.writable {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        self.fs.write_at_path(&self.path, position, buf)
    }

    fn as_stdio(self: Rc<Self>) -> FsResult<PlatformStdio> {
        Err(FsError::NotSupported)
    }

    fn backing_fd(self: Rc<Self>) -> Option<ResourceHandleFd> {
        None
    }

    fn try_clone_inner(self: Rc<Self>) -> FsResult<Rc<dyn File>> {
        Ok(Rc::new(MemFile {
            fs: self.fs.clone(),
            path: self.path.clone(),
            cursor: Mutex::new(*self.cursor.lock().unwrap()),
            readable: self.readable,
            writable: self.writable,
            append: self.append,
        }))
    }
}

fn normalize(path: &CheckedPath<'_>) -> FsResult<PathBuf> {
    normalize_path(path)
}

fn normalize_path(path: &Path) -> FsResult<PathBuf> {
    let mut parts = Vec::new();
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        PathBuf::from("/").join(path)
    };
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(part) => parts.push(part.to_owned()),
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "path escapes MemFs root",
                    )
                    .into());
                }
            }
            Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "platform prefixes are unsupported in MemFs",
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

fn stat_for_node(node: &Node) -> FsStat {
    match node {
        Node::File(file) => FsStat {
            is_file: true,
            is_directory: false,
            is_symlink: false,
            size: file.data.len() as u64,
            mtime: file.mtime,
            atime: file.atime,
            birthtime: None,
            ctime: None,
            dev: 0,
            ino: Some(file.ino),
            mode: file.mode,
            nlink: Some(1),
            uid: 0,
            gid: 0,
            rdev: 0,
            blksize: 4096,
            blocks: Some((file.data.len() as u64).div_ceil(512)),
            is_block_device: false,
            is_char_device: false,
            is_fifo: false,
            is_socket: false,
        },
        Node::Directory(dir) => FsStat {
            is_file: false,
            is_directory: true,
            is_symlink: false,
            size: 0,
            mtime: None,
            atime: None,
            birthtime: None,
            ctime: None,
            dev: 0,
            ino: Some(dir.ino),
            mode: dir.mode,
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
        },
        Node::Symlink(link) => FsStat {
            is_file: false,
            is_directory: false,
            is_symlink: true,
            size: link.target.as_os_str().len() as u64,
            mtime: None,
            atime: None,
            birthtime: None,
            ctime: None,
            dev: 0,
            ino: Some(link.ino),
            mode: 0o777,
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
        },
    }
}

fn dir_entry(name: String, node: &Node) -> FsDirEntry {
    FsDirEntry {
        name,
        is_file: matches!(node, Node::File(_)),
        is_directory: matches!(node, Node::Directory(_)),
        is_symlink: matches!(node, Node::Symlink(_)),
    }
}

fn timestamp_millis(secs: i64, nanos: u32) -> u64 {
    let secs = secs.max(0) as u64;
    secs.saturating_mul(1000)
        .saturating_add((nanos / 1_000_000) as u64)
}
