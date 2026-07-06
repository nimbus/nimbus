use std::borrow::Cow;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use cap_std::ambient_authority;
use cap_std::fs::{Dir as CapDir, Metadata as CapMetadata, OpenOptions as CapOpenOptions};
#[cfg(unix)]
use cap_std::fs::{
    FileTypeExt as _, MetadataExt as _, OpenOptionsExt as _, Permissions as CapPermissions,
    PermissionsExt as _,
};
use cap_std::time::SystemTime as CapSystemTime;
use deno_fs::sync::MaybeArc;
use deno_fs::{FileSystem, FsDirEntry, FsFileType, FsReadDir, FsReadDirRc, OpenOptions};
use deno_io::{
    StdFileResourceInner,
    fs::{File, FsResult, FsStat, FsStatFs},
};
use deno_permissions::{CheckedPath, CheckedPathBuf};

/// Ambient-root carve-out: a `PassthroughBackend` rooted at `/` has no
/// boundary to protect — every real absolute path is already "inside" a root
/// of `/`, so the cap-std sandbox is vacuous there. For that case this
/// backend delegates the full operation surface to the ambient inner
/// `RealFs`, which restores exact RealFs/Node parity (including absolute
/// symlink creation and traversal). A backend rooted anywhere else keeps the
/// strict cap-std-rooted behavior unchanged: absolute symlink targets are
/// rejected at both creation and traversal, because there a real boundary
/// exists and must hold. `FsCaps`/`CappedBackend` rights gating sits above
/// this backend either way and is unaffected by which path a given root
/// takes.
///
/// `chdir` is the one op that deliberately does **not** use this macro, at
/// any root: `RealFs::chdir` calls `std::env::set_current_dir`, a
/// process-global mutation, and delegating to it would leak one isolate's
/// chdir into every other isolate sharing the process. See `chdir`'s own doc
/// comment for the validate-only replacement used at the ambient root.
macro_rules! ambient_root_delegate {
    ($self:expr, $call:expr) => {
        if $self.root.is_ambient_root() {
            return $call;
        }
    };
}

#[derive(Debug, Clone)]
pub struct PassthroughBackend {
    inner: deno_fs::RealFs,
    root: RootCapability,
}

impl PassthroughBackend {
    pub fn new() -> io::Result<Self> {
        Self::rooted(PathBuf::from("/"))
    }

    pub fn rooted(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        Ok(Self {
            inner: deno_fs::RealFs,
            root: RootCapability::open(root)?,
        })
    }

    fn relative_path(&self, path: &Path) -> FsResult<PathBuf> {
        Ok(self.root.relative_path(path)?)
    }

    fn host_path(&self, path: &Path) -> FsResult<PathBuf> {
        self.root.host_path(path)
    }

    fn cap_file(&self, path: &Path, options: OpenOptions) -> FsResult<cap_std::fs::File> {
        self.root.open_file(path, options).map_err(Into::into)
    }

    fn deno_file(&self, file: cap_std::fs::File, maybe_path: Option<PathBuf>) -> Rc<dyn File> {
        Rc::new(StdFileResourceInner::file(file.into_std(), maybe_path))
    }

    fn checked_path(&self, path: &CheckedPath<'_>) -> FsResult<CheckedPathBuf> {
        Ok(CheckedPathBuf::unsafe_new(self.host_path(path)?))
    }

    fn checked_buf(&self, path: CheckedPathBuf) -> FsResult<CheckedPathBuf> {
        Ok(CheckedPathBuf::unsafe_new(
            self.host_path(&path.into_path_buf())?,
        ))
    }

    fn copy_tree(&self, path: &CheckedPath<'_>, new_path: &CheckedPath<'_>) -> FsResult<()> {
        let source = self.relative_path(path)?;
        let destination = self.relative_path(new_path)?;
        self.copy_tree_relative(source.as_path(), destination.as_path())
    }

    fn copy_tree_relative(&self, source: &Path, destination: &Path) -> FsResult<()> {
        let source_cap = self.root.cap_path(source);
        let destination_cap = self.root.cap_path(destination);
        let metadata = self.root.dir.symlink_metadata(source_cap.as_ref())?;
        if metadata.is_dir() {
            self.root.dir.create_dir_all(destination_cap.as_ref())?;
            for entry in self.root.dir.read_dir(source_cap.as_ref())? {
                let entry = entry?;
                let name = entry.file_name();
                self.copy_tree_relative(&source.join(&name), &destination.join(&name))?;
            }
            return Ok(());
        }
        if metadata.is_symlink() {
            let target = self.root.dir.read_link(source_cap.as_ref())?;
            ensure_relative_symlink_target(&target)?;
            #[cfg(not(windows))]
            self.root.dir.symlink(target, destination_cap.as_ref())?;
            #[cfg(windows)]
            self.root
                .dir
                .symlink_file(target, destination_cap.as_ref())?;
            return Ok(());
        }
        self.root.dir.copy(
            source_cap.as_ref(),
            &self.root.dir,
            destination_cap.as_ref(),
        )?;
        Ok(())
    }
}

#[derive(Debug)]
struct RootCapability {
    root: PathBuf,
    dir: CapDir,
    ambient_fallback_allowed: bool,
}

impl Clone for RootCapability {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
            dir: self
                .dir
                .try_clone()
                .expect("passthrough root capability should clone"),
            ambient_fallback_allowed: self.ambient_fallback_allowed,
        }
    }
}

impl RootCapability {
    fn open(root: PathBuf) -> io::Result<Self> {
        let root = root.canonicalize()?;
        let ambient_fallback_allowed = root.parent().is_none();
        let dir = CapDir::open_ambient_dir(&root, ambient_authority())?;
        Ok(Self {
            root,
            dir,
            ambient_fallback_allowed,
        })
    }

    fn relative_path(&self, path: &Path) -> io::Result<PathBuf> {
        let mut relative = PathBuf::new();
        for component in path.components() {
            match component {
                Component::Prefix(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "passthrough backend does not accept platform path prefixes",
                    ));
                }
                Component::RootDir | Component::CurDir => {}
                Component::ParentDir => {
                    if !relative.pop() {
                        return Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "passthrough path escapes backend root",
                        ));
                    }
                }
                Component::Normal(part) => relative.push(part),
            }
        }
        Ok(relative)
    }

    fn cap_path<'a>(&self, relative: &'a Path) -> Cow<'a, Path> {
        if relative.as_os_str().is_empty() {
            Cow::Owned(PathBuf::from("."))
        } else {
            Cow::Borrowed(relative)
        }
    }

    /// Serves strict (non-ambient) roots only. Every caller of `host_path`
    /// (via `checked_path`/`checked_buf`) is now itself guarded by
    /// `ambient_root_delegate!` first, so this is only ever reached when
    /// `ambient_fallback_allowed` is false — there is no longer a live path
    /// that reaches `host_path` at the ambient root. It unconditionally
    /// rejects: a strict cap-std root has no host-path-join escape hatch.
    fn host_path(&self, path: &Path) -> FsResult<PathBuf> {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "operation does not have a capability-rooted passthrough implementation",
        )
        .into())
    }

    fn open_file(&self, path: &Path, options: OpenOptions) -> io::Result<cap_std::fs::File> {
        let relative = self.relative_path(path)?;
        let cap_path = self.cap_path(relative.as_path());
        if options.create && !options.write && !options.append {
            if let Err(error) = self.dir.metadata(cap_path.as_ref()) {
                if error.kind() != io::ErrorKind::NotFound {
                    return Err(error);
                }
                let mut create_options = options;
                create_options.read = false;
                create_options.write = true;
                create_options.create = true;
                create_options.truncate = false;
                drop(
                    self.dir
                        .open_with(cap_path.as_ref(), &cap_open_options(create_options))?,
                );
            }
            let mut read_options = options;
            read_options.create = false;
            read_options.create_new = false;
            return self
                .dir
                .open_with(cap_path.as_ref(), &cap_open_options(read_options));
        }
        self.dir
            .open_with(cap_path.as_ref(), &cap_open_options(options))
    }

    fn metadata(&self, path: &Path) -> FsResult<CapMetadata> {
        let relative = self.relative_path(path)?;
        self.dir
            .metadata(self.cap_path(relative.as_path()).as_ref())
            .map_err(Into::into)
    }

    fn symlink_metadata(&self, path: &Path) -> FsResult<CapMetadata> {
        let relative = self.relative_path(path)?;
        self.dir
            .symlink_metadata(self.cap_path(relative.as_path()).as_ref())
            .map_err(Into::into)
    }

    /// Whether this root has no boundary to protect: rooted at `/`, where
    /// every real absolute path is already inside the root, so the cap-std
    /// sandbox is vacuous and the ambient inner `RealFs` can own the full
    /// operation surface instead.
    fn is_ambient_root(&self) -> bool {
        self.ambient_fallback_allowed
    }

    fn virtual_path(&self, relative: PathBuf) -> PathBuf {
        if relative.as_os_str().is_empty() || relative == Path::new(".") {
            PathBuf::from("/")
        } else {
            Path::new("/").join(relative)
        }
    }
}

fn cap_open_options(options: OpenOptions) -> CapOpenOptions {
    let mut cap_options = CapOpenOptions::new();
    cap_options
        .read(options.read)
        .write(options.write)
        .create(options.create)
        .truncate(options.truncate && !options.create_new)
        .append(options.append)
        .create_new(options.create_new);
    #[cfg(unix)]
    {
        if let Some(mode) = options.mode {
            cap_options.mode(mode);
        }
        if let Some(custom_flags) = options.custom_flags {
            cap_options.custom_flags(custom_flags);
        }
    }
    cap_options
}

fn cap_metadata_to_fs_stat(metadata: CapMetadata) -> FsStat {
    #[inline(always)]
    fn to_msec(maybe_time: io::Result<CapSystemTime>) -> Option<u64> {
        match maybe_time {
            Ok(time) => Some(
                time.into_std()
                    .duration_since(UNIX_EPOCH)
                    .map(|time| time.as_millis() as u64)
                    .unwrap_or_else(|error| error.duration().as_millis() as u64),
            ),
            Err(_) => None,
        }
    }

    #[inline(always)]
    fn get_ctime(ctime_or_0: i64) -> Option<u64> {
        if ctime_or_0 > 0 {
            Some(ctime_or_0 as u64 * 1_000)
        } else {
            None
        }
    }

    macro_rules! unix_some_or_none {
        ($member:ident) => {{
            #[cfg(unix)]
            {
                Some(metadata.$member())
            }
            #[cfg(not(unix))]
            {
                None
            }
        }};
    }

    macro_rules! unix_or_zero {
        ($member:ident) => {{
            #[cfg(unix)]
            {
                metadata.$member()
            }
            #[cfg(not(unix))]
            {
                0
            }
        }};
    }

    macro_rules! unix_or_false {
        ($member:ident) => {{
            #[cfg(unix)]
            {
                metadata.file_type().$member()
            }
            #[cfg(not(unix))]
            {
                false
            }
        }};
    }

    FsStat {
        is_file: metadata.is_file(),
        is_directory: metadata.is_dir(),
        is_symlink: metadata.is_symlink(),
        size: metadata.len(),
        mtime: to_msec(metadata.modified()),
        atime: to_msec(metadata.accessed()),
        birthtime: to_msec(metadata.created()),
        ctime: get_ctime(unix_or_zero!(ctime)),
        dev: unix_or_zero!(dev),
        ino: unix_some_or_none!(ino),
        mode: unix_or_zero!(mode),
        nlink: unix_some_or_none!(nlink),
        uid: unix_or_zero!(uid),
        gid: unix_or_zero!(gid),
        rdev: unix_or_zero!(rdev),
        blksize: unix_or_zero!(blksize),
        blocks: unix_some_or_none!(blocks),
        is_block_device: unix_or_false!(is_block_device),
        is_char_device: unix_or_false!(is_char_device),
        is_fifo: unix_or_false!(is_fifo),
        is_socket: unix_or_false!(is_socket),
    }
}

fn cap_dir_entry_to_fs_dir_entry(entry: cap_std::fs::DirEntry) -> FsResult<FsDirEntry> {
    let name = entry.file_name().to_string_lossy().into_owned();
    let file_type = entry.file_type()?;
    Ok(FsDirEntry {
        name,
        is_file: file_type.is_file(),
        is_directory: file_type.is_dir(),
        is_symlink: file_type.is_symlink(),
    })
}

#[derive(Debug)]
struct CapReadDir(Mutex<cap_std::fs::ReadDir>);

#[async_trait::async_trait(?Send)]
impl FsReadDir for CapReadDir {
    async fn next(&self) -> FsResult<Option<FsDirEntry>> {
        let mut read_dir = self
            .0
            .lock()
            .map_err(|_| io::Error::other("passthrough read_dir lock poisoned"))?;
        read_dir
            .next()
            .map(|entry| cap_dir_entry_to_fs_dir_entry(entry?))
            .transpose()
    }
}

#[async_trait::async_trait(?Send)]
impl FileSystem for PassthroughBackend {
    fn cwd(&self) -> FsResult<PathBuf> {
        Ok(self.root.root.clone())
    }

    fn tmp_dir(&self) -> FsResult<PathBuf> {
        Ok(PathBuf::from("/tmp"))
    }

    /// `chdir` is validate-only at every root shape — it is the one op the
    /// `ambient_root_delegate!` macro deliberately does not touch. The
    /// `NimbusFs` shell owns per-instance cwd precisely so isolates never
    /// observe each other's chdir; a backend that delegated to
    /// `RealFs::chdir` would call `std::env::set_current_dir`, a
    /// process-global mutation that leaks across every isolate sharing the
    /// process. At the ambient root, validation uses `RealFs::stat_sync`
    /// (follows absolute symlinks, matching what a real chdir would resolve)
    /// instead of the strict cap-std metadata check, so an ambient-rooted
    /// backend admits a directory reachable only through an absolute
    /// symlink — but it still only *validates*, and never mutates cwd. A
    /// strict (non-ambient) root keeps the pre-existing cap-std metadata
    /// check, byte-for-byte unchanged.
    fn chdir(&self, path: &CheckedPath<'_>) -> FsResult<()> {
        let stat = if self.root.is_ambient_root() {
            self.inner.stat_sync(path)?
        } else {
            cap_metadata_to_fs_stat(self.root.metadata(path)?)
        };
        if stat.is_directory {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                format!("{} is not a directory", path.display()),
            )
            .into())
        }
    }

    fn umask(&self, mask: Option<u32>) -> FsResult<u32> {
        self.inner.umask(mask)
    }

    fn open_sync(&self, path: &CheckedPath<'_>, options: OpenOptions) -> FsResult<Rc<dyn File>> {
        ambient_root_delegate!(self, self.inner.open_sync(path, options));
        let file = self.cap_file(path, options)?;
        Ok(self.deno_file(file, None))
    }

    async fn open_async<'a>(
        &'a self,
        path: CheckedPathBuf,
        options: OpenOptions,
    ) -> FsResult<Rc<dyn File>> {
        ambient_root_delegate!(self, self.inner.open_async(path, options).await);
        let file = self.cap_file(&path.into_path_buf(), options)?;
        Ok(self.deno_file(file, None))
    }

    fn mkdir_sync(
        &self,
        path: &CheckedPath<'_>,
        recursive: bool,
        mode: Option<u32>,
    ) -> FsResult<()> {
        ambient_root_delegate!(self, self.inner.mkdir_sync(path, recursive, mode));
        let relative = self.relative_path(path)?;
        let cap_path = self.root.cap_path(relative.as_path());
        if recursive {
            self.root.dir.create_dir_all(cap_path.as_ref())?;
        } else {
            self.root.dir.create_dir(cap_path.as_ref())?;
        }
        #[cfg(unix)]
        if let Some(mode) = mode {
            self.root
                .dir
                .set_permissions(cap_path.as_ref(), CapPermissions::from_mode(mode))?;
        }
        #[cfg(not(unix))]
        let _ = mode;
        Ok(())
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
        ambient_root_delegate!(self, self.inner.chmod_sync(path, mode));
        let relative = self.relative_path(path)?;
        self.root
            .dir
            .set_permissions(
                self.root.cap_path(relative.as_path()).as_ref(),
                CapPermissions::from_mode(mode),
            )
            .map_err(Into::into)
    }

    #[cfg(not(unix))]
    fn chmod_sync(&self, path: &CheckedPath<'_>, mode: i32) -> FsResult<()> {
        ambient_root_delegate!(self, self.inner.chmod_sync(path, mode));
        let path = self.checked_path(path)?;
        self.inner.chmod_sync(&path.as_checked_path(), mode)
    }

    #[cfg(unix)]
    async fn chmod_async(&self, path: CheckedPathBuf, mode: u32) -> FsResult<()> {
        self.chmod_sync(&path.as_checked_path(), mode)
    }

    #[cfg(not(unix))]
    async fn chmod_async(&self, path: CheckedPathBuf, mode: i32) -> FsResult<()> {
        ambient_root_delegate!(self, self.inner.chmod_async(path, mode).await);
        let path = self.checked_buf(path)?;
        self.inner.chmod_async(path, mode).await
    }

    fn chown_sync(
        &self,
        path: &CheckedPath<'_>,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        ambient_root_delegate!(self, self.inner.chown_sync(path, uid, gid));
        let path = self.checked_path(path)?;
        self.inner.chown_sync(&path.as_checked_path(), uid, gid)
    }

    async fn chown_async(
        &self,
        path: CheckedPathBuf,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        ambient_root_delegate!(self, self.inner.chown_async(path, uid, gid).await);
        let path = self.checked_buf(path)?;
        self.inner.chown_async(path, uid, gid).await
    }

    fn lchmod_sync(&self, path: &CheckedPath<'_>, mode: u32) -> FsResult<()> {
        ambient_root_delegate!(self, self.inner.lchmod_sync(path, mode));
        let path = self.checked_path(path)?;
        self.inner.lchmod_sync(&path.as_checked_path(), mode)
    }

    async fn lchmod_async(&self, path: CheckedPathBuf, mode: u32) -> FsResult<()> {
        ambient_root_delegate!(self, self.inner.lchmod_async(path, mode).await);
        let path = self.checked_buf(path)?;
        self.inner.lchmod_async(path, mode).await
    }

    fn lchown_sync(
        &self,
        path: &CheckedPath<'_>,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        ambient_root_delegate!(self, self.inner.lchown_sync(path, uid, gid));
        let path = self.checked_path(path)?;
        self.inner.lchown_sync(&path.as_checked_path(), uid, gid)
    }

    async fn lchown_async(
        &self,
        path: CheckedPathBuf,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        ambient_root_delegate!(self, self.inner.lchown_async(path, uid, gid).await);
        let path = self.checked_buf(path)?;
        self.inner.lchown_async(path, uid, gid).await
    }

    fn remove_sync(&self, path: &CheckedPath<'_>, recursive: bool) -> FsResult<()> {
        ambient_root_delegate!(self, self.inner.remove_sync(path, recursive));
        let relative = self.relative_path(path)?;
        let cap_path = self.root.cap_path(relative.as_path());
        if recursive {
            return self
                .root
                .dir
                .remove_dir_all(cap_path.as_ref())
                .map_err(Into::into);
        }
        match self.root.dir.remove_file(cap_path.as_ref()) {
            Ok(()) => Ok(()),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::IsADirectory | io::ErrorKind::PermissionDenied
                ) =>
            {
                self.root
                    .dir
                    .remove_dir(cap_path.as_ref())
                    .map_err(Into::into)
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn remove_async(&self, path: CheckedPathBuf, recursive: bool) -> FsResult<()> {
        self.remove_sync(&path.as_checked_path(), recursive)
    }

    fn copy_file_sync(&self, oldpath: &CheckedPath<'_>, newpath: &CheckedPath<'_>) -> FsResult<()> {
        ambient_root_delegate!(self, self.inner.copy_file_sync(oldpath, newpath));
        let oldpath = self.relative_path(oldpath)?;
        let newpath = self.relative_path(newpath)?;
        self.root
            .dir
            .copy(
                self.root.cap_path(oldpath.as_path()).as_ref(),
                &self.root.dir,
                self.root.cap_path(newpath.as_path()).as_ref(),
            )
            .map(|_| ())
            .map_err(Into::into)
    }

    async fn copy_file_async(
        &self,
        oldpath: CheckedPathBuf,
        newpath: CheckedPathBuf,
    ) -> FsResult<()> {
        self.copy_file_sync(&oldpath.as_checked_path(), &newpath.as_checked_path())
    }

    fn cp_sync(&self, path: &CheckedPath<'_>, new_path: &CheckedPath<'_>) -> FsResult<()> {
        ambient_root_delegate!(self, self.inner.cp_sync(path, new_path));
        self.copy_tree(path, new_path)
    }

    async fn cp_async(&self, path: CheckedPathBuf, new_path: CheckedPathBuf) -> FsResult<()> {
        self.cp_sync(&path.as_checked_path(), &new_path.as_checked_path())
    }

    fn stat_sync(&self, path: &CheckedPath<'_>) -> FsResult<FsStat> {
        ambient_root_delegate!(self, self.inner.stat_sync(path));
        self.root.metadata(path).map(cap_metadata_to_fs_stat)
    }

    async fn stat_async(&self, path: CheckedPathBuf) -> FsResult<FsStat> {
        self.stat_sync(&path.as_checked_path())
    }

    fn lstat_sync(&self, path: &CheckedPath<'_>) -> FsResult<FsStat> {
        ambient_root_delegate!(self, self.inner.lstat_sync(path));
        self.root
            .symlink_metadata(path)
            .map(cap_metadata_to_fs_stat)
    }

    async fn lstat_async(&self, path: CheckedPathBuf) -> FsResult<FsStat> {
        self.lstat_sync(&path.as_checked_path())
    }

    fn statfs_sync(&self, path: &CheckedPath<'_>, bigint: bool) -> FsResult<FsStatFs> {
        ambient_root_delegate!(self, self.inner.statfs_sync(path, bigint));
        let path = self.checked_path(path)?;
        self.inner.statfs_sync(&path.as_checked_path(), bigint)
    }

    async fn statfs_async(&self, path: CheckedPathBuf, bigint: bool) -> FsResult<FsStatFs> {
        ambient_root_delegate!(self, self.inner.statfs_async(path, bigint).await);
        let path = self.checked_buf(path)?;
        self.inner.statfs_async(path, bigint).await
    }

    fn realpath_sync(&self, path: &CheckedPath<'_>) -> FsResult<PathBuf> {
        ambient_root_delegate!(self, self.inner.realpath_sync(path));
        let relative = self.relative_path(path)?;
        self.root
            .dir
            .canonicalize(self.root.cap_path(relative.as_path()).as_ref())
            .map(|path| self.root.virtual_path(path))
            .map_err(Into::into)
    }

    async fn realpath_async(&self, path: CheckedPathBuf) -> FsResult<PathBuf> {
        self.realpath_sync(&path.as_checked_path())
    }

    fn read_dir_sync(&self, path: &CheckedPath<'_>) -> FsResult<Vec<FsDirEntry>> {
        ambient_root_delegate!(self, self.inner.read_dir_sync(path));
        let relative = self.relative_path(path)?;
        self.root
            .dir
            .read_dir(self.root.cap_path(relative.as_path()).as_ref())?
            .map(|entry| cap_dir_entry_to_fs_dir_entry(entry?))
            .collect()
    }

    async fn read_dir_async(&self, path: CheckedPathBuf) -> FsResult<FsReadDirRc> {
        ambient_root_delegate!(self, self.inner.read_dir_async(path).await);
        let relative = self.relative_path(&path.into_path_buf())?;
        let read_dir = self
            .root
            .dir
            .read_dir(self.root.cap_path(relative.as_path()).as_ref())?;
        Ok(MaybeArc::new(CapReadDir(Mutex::new(read_dir))))
    }

    fn rename_sync(&self, oldpath: &CheckedPath<'_>, newpath: &CheckedPath<'_>) -> FsResult<()> {
        ambient_root_delegate!(self, self.inner.rename_sync(oldpath, newpath));
        let oldpath = self.relative_path(oldpath)?;
        let newpath = self.relative_path(newpath)?;
        self.root
            .dir
            .rename(
                self.root.cap_path(oldpath.as_path()).as_ref(),
                &self.root.dir,
                self.root.cap_path(newpath.as_path()).as_ref(),
            )
            .map_err(Into::into)
    }

    async fn rename_async(&self, oldpath: CheckedPathBuf, newpath: CheckedPathBuf) -> FsResult<()> {
        self.rename_sync(&oldpath.as_checked_path(), &newpath.as_checked_path())
    }

    fn rmdir_sync(&self, path: &CheckedPath<'_>) -> FsResult<()> {
        ambient_root_delegate!(self, self.inner.rmdir_sync(path));
        let relative = self.relative_path(path)?;
        self.root
            .dir
            .remove_dir(self.root.cap_path(relative.as_path()).as_ref())
            .map_err(Into::into)
    }

    async fn rmdir_async(&self, path: CheckedPathBuf) -> FsResult<()> {
        self.rmdir_sync(&path.as_checked_path())
    }

    fn link_sync(&self, oldpath: &CheckedPath<'_>, newpath: &CheckedPath<'_>) -> FsResult<()> {
        ambient_root_delegate!(self, self.inner.link_sync(oldpath, newpath));
        let oldpath = self.relative_path(oldpath)?;
        let newpath = self.relative_path(newpath)?;
        self.root
            .dir
            .hard_link(
                self.root.cap_path(oldpath.as_path()).as_ref(),
                &self.root.dir,
                self.root.cap_path(newpath.as_path()).as_ref(),
            )
            .map_err(Into::into)
    }

    async fn link_async(&self, oldpath: CheckedPathBuf, newpath: CheckedPathBuf) -> FsResult<()> {
        self.link_sync(&oldpath.as_checked_path(), &newpath.as_checked_path())
    }

    fn symlink_sync(
        &self,
        oldpath: &CheckedPath<'_>,
        newpath: &CheckedPath<'_>,
        file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        ambient_root_delegate!(self, self.inner.symlink_sync(oldpath, newpath, file_type));
        ensure_relative_symlink_target(oldpath)?;
        let newpath = self.relative_path(newpath)?;
        #[cfg(not(windows))]
        {
            let _ = file_type;
            self.root
                .dir
                .symlink(oldpath, self.root.cap_path(newpath.as_path()).as_ref())
                .map_err(Into::into)
        }
        #[cfg(windows)]
        {
            match file_type {
                Some(FsFileType::Directory) => self
                    .root
                    .dir
                    .symlink_dir(oldpath, self.root.cap_path(newpath.as_path()).as_ref()),
                _ => self
                    .root
                    .dir
                    .symlink_file(oldpath, self.root.cap_path(newpath.as_path()).as_ref()),
            }
            .map_err(Into::into)
        }
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
        ambient_root_delegate!(self, self.inner.read_link_sync(path));
        let relative = self.relative_path(path)?;
        self.root
            .dir
            .read_link(self.root.cap_path(relative.as_path()).as_ref())
            .map_err(Into::into)
    }

    async fn read_link_async(&self, path: CheckedPathBuf) -> FsResult<PathBuf> {
        self.read_link_sync(&path.as_checked_path())
    }

    fn truncate_sync(&self, path: &CheckedPath<'_>, len: u64) -> FsResult<()> {
        ambient_root_delegate!(self, self.inner.truncate_sync(path, len));
        let relative = self.relative_path(path)?;
        let mut options = CapOpenOptions::new();
        options.write(true);
        let file = self
            .root
            .dir
            .open_with(self.root.cap_path(relative.as_path()).as_ref(), &options)?;
        file.set_len(len).map_err(Into::into)
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
        ambient_root_delegate!(
            self,
            self.inner
                .utime_sync(path, atime_secs, atime_nanos, mtime_secs, mtime_nanos)
        );
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
        ambient_root_delegate!(
            self,
            self.inner
                .utime_async(path, atime_secs, atime_nanos, mtime_secs, mtime_nanos)
                .await
        );
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
        ambient_root_delegate!(
            self,
            self.inner
                .lutime_sync(path, atime_secs, atime_nanos, mtime_secs, mtime_nanos)
        );
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
        ambient_root_delegate!(
            self,
            self.inner
                .lutime_async(path, atime_secs, atime_nanos, mtime_secs, mtime_nanos)
                .await
        );
        let path = self.checked_buf(path)?;
        self.inner
            .lutime_async(path, atime_secs, atime_nanos, mtime_secs, mtime_nanos)
            .await
    }

    fn exists_sync(&self, path: &CheckedPath<'_>) -> bool {
        ambient_root_delegate!(self, self.inner.exists_sync(path));
        self.root.symlink_metadata(path).is_ok()
    }

    async fn exists_async(&self, path: CheckedPathBuf) -> FsResult<bool> {
        Ok(self.exists_sync(&path.as_checked_path()))
    }
}

fn ensure_relative_symlink_target(path: &Path) -> io::Result<()> {
    if path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "passthrough symlink target must be relative to the link location",
        ));
    }
    Ok(())
}
