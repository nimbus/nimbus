//! Crash-safe publication of one complete file through a same-directory stage.
//!
//! The protocol has one commit point. Before it, the destination holds its
//! previous bytes or does not exist. After it, the destination holds the new
//! bytes in full:
//!
//! 1. Create the stage file exclusively, write the bytes, and `fsync` it.
//! 2. Commit the stage. [`Commit::Replace`] renames it over the destination.
//!    [`Commit::CreateNew`] links it to a destination that must not exist.
//! 3. `fsync` the directory so that the commit survives a crash.
//!
//! A failed write removes its own stage and syncs the directory. Callers must
//! never promote a leftover stage after a crash. They delete it instead.

use std::convert::Infallible;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use crate::directory::sync_directory;

#[cfg(unix)]
const OWNER_FILE_MODE: u32 = 0o600;

/// How a durable stage becomes the destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Commit {
    /// Atomically replace any existing destination.
    Replace,
    /// Publish only when the destination does not exist yet. The commit step
    /// fails with [`io::ErrorKind::AlreadyExists`] otherwise.
    CreateNew,
}

/// A point in the protocol where the caller can observe or inject a fault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteCheckpoint {
    /// The stage file holds the complete bytes and is synced.
    StageDurable,
    /// The destination names the new bytes. The directory is not synced yet.
    Committed,
}

/// The protocol step that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteStep {
    /// Validate the paths or create the stage file exclusively.
    CreateStage,
    /// Write the bytes to the stage file.
    WriteStage,
    /// `fsync` the stage file.
    SyncStage,
    /// Rename or link the stage to the destination.
    Commit,
    /// `fsync` the directory after the commit. The commit is visible, so the
    /// outcome is ambiguous: a crash can still roll it back.
    SyncDirectory,
}

impl WriteStep {
    fn describe(self) -> &'static str {
        match self {
            Self::CreateStage => "create stage file",
            Self::WriteStage => "write stage file",
            Self::SyncStage => "sync stage file",
            Self::Commit => "commit stage file to",
            Self::SyncDirectory => "sync directory",
        }
    }
}

/// The step of failure cleanup that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupStep {
    /// Remove the stage file that this write created.
    RemoveStage,
    /// `fsync` the directory after the stage removal.
    SyncDirectory,
}

/// A failure while a failed write removed its own stage.
#[derive(Debug)]
pub struct CleanupError {
    pub step: CleanupStep,
    pub path: PathBuf,
    pub source: io::Error,
}

impl fmt::Display for CleanupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let step = match self.step {
            CleanupStep::RemoveStage => "remove stage file",
            CleanupStep::SyncDirectory => "sync directory after stage removal",
        };
        write!(
            formatter,
            "failed to {step} {}: {}",
            self.path.display(),
            self.source
        )
    }
}

impl std::error::Error for CleanupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// The primary failure of a staged write.
#[derive(Debug)]
pub enum WriteFailure<E> {
    /// A filesystem step failed. `path` is the stage for the stage steps, the
    /// destination for [`WriteStep::Commit`], and the directory for
    /// [`WriteStep::SyncDirectory`].
    Io {
        step: WriteStep,
        path: PathBuf,
        source: io::Error,
    },
    /// The caller's observer rejected a checkpoint.
    Observer {
        checkpoint: WriteCheckpoint,
        error: E,
    },
}

/// A failed staged write and the result of its stage cleanup.
#[derive(Debug)]
pub struct WriteError<E = Infallible> {
    pub failure: WriteFailure<E>,
    /// Set when the write created a stage and could not durably remove it.
    pub cleanup: Option<CleanupError>,
}

impl<E: fmt::Display> fmt::Display for WriteError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.failure {
            WriteFailure::Io { step, path, source } => write!(
                formatter,
                "failed to {} {}: {source}",
                step.describe(),
                path.display()
            )?,
            WriteFailure::Observer { checkpoint, error } => {
                write!(formatter, "observer rejected {checkpoint:?}: {error}")?;
            }
        }
        if let Some(cleanup) = &self.cleanup {
            write!(formatter, "; stage cleanup also failed: {cleanup}")?;
        }
        Ok(())
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for WriteError<E> {}

type DirectorySync<'a> = &'a mut dyn FnMut(&Path) -> io::Result<()>;
type Observer<'a, E> = &'a mut dyn FnMut(WriteCheckpoint) -> Result<(), E>;

/// One staged write of a complete file.
///
/// The stage and the destination must be distinct entries of one directory.
/// The stage must not exist: the write creates it exclusively, so a caller
/// with a fixed stage name removes a stale stage under its own lock first.
pub struct StagedWrite<'a, E = Infallible> {
    stage: &'a Path,
    destination: &'a Path,
    commit: Commit,
    owner_only: bool,
    directory_sync: Option<DirectorySync<'a>>,
    observer: Option<Observer<'a, E>>,
}

impl<'a> StagedWrite<'a> {
    /// Replace `destination` through `stage`.
    pub fn new(stage: &'a Path, destination: &'a Path) -> Self {
        Self {
            stage,
            destination,
            commit: Commit::Replace,
            owner_only: false,
            directory_sync: None,
            observer: None,
        }
    }
}

impl<'a, E> StagedWrite<'a, E> {
    /// Select how the stage becomes the destination.
    pub fn commit(mut self, commit: Commit) -> Self {
        self.commit = commit;
        self
    }

    /// Create the stage readable and writable by its owner only (Unix mode
    /// `0600`, set explicitly so that the umask cannot widen it).
    pub fn owner_only(mut self) -> Self {
        self.owner_only = true;
        self
    }

    /// Use `sync` in place of [`sync_directory`]. Tests inject faults here.
    pub fn directory_sync(mut self, sync: DirectorySync<'a>) -> Self {
        self.directory_sync = Some(sync);
        self
    }

    /// Call `observer` at each [`WriteCheckpoint`]. An error aborts the write
    /// and cleans up as any other failure does.
    pub fn observer<F>(self, observer: Observer<'a, F>) -> StagedWrite<'a, F> {
        StagedWrite {
            stage: self.stage,
            destination: self.destination,
            commit: self.commit,
            owner_only: self.owner_only,
            directory_sync: self.directory_sync,
            observer: Some(observer),
        }
    }

    /// Publish `bytes` at the destination.
    pub fn write(mut self, bytes: &[u8]) -> Result<(), WriteError<E>> {
        let directory = self.directory().map_err(|failure| WriteError {
            failure,
            cleanup: None,
        })?;
        let mut stage_created = false;
        match self.publish(&directory, bytes, &mut stage_created) {
            Ok(()) => Ok(()),
            Err(failure) => {
                let cleanup = if stage_created {
                    self.remove_stage(&directory).err()
                } else {
                    None
                };
                Err(WriteError { failure, cleanup })
            }
        }
    }

    fn directory(&self) -> Result<PathBuf, WriteFailure<E>> {
        let invalid = |reason: &str| WriteFailure::Io {
            step: WriteStep::CreateStage,
            path: self.stage.to_path_buf(),
            source: io::Error::new(io::ErrorKind::InvalidInput, reason.to_owned()),
        };
        if self.stage.file_name().is_none() || self.destination.file_name().is_none() {
            return Err(invalid("stage and destination must name files"));
        }
        if self.stage == self.destination {
            return Err(invalid("stage and destination must differ"));
        }
        let parent = self.destination.parent();
        if self.stage.parent() != parent {
            return Err(invalid("stage and destination must share one directory"));
        }
        Ok(match parent {
            Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
            _ => PathBuf::from("."),
        })
    }

    fn publish(
        &mut self,
        directory: &Path,
        bytes: &[u8],
        stage_created: &mut bool,
    ) -> Result<(), WriteFailure<E>> {
        let stage = self.stage;
        let io_failure = |step: WriteStep, path: &Path| {
            let path = path.to_path_buf();
            move |source| WriteFailure::Io { step, path, source }
        };

        let mut file = create_stage(stage, self.owner_only, stage_created)
            .map_err(io_failure(WriteStep::CreateStage, stage))?;
        file.write_all(bytes)
            .map_err(io_failure(WriteStep::WriteStage, stage))?;
        file.sync_all()
            .map_err(io_failure(WriteStep::SyncStage, stage))?;
        drop(file);
        self.checkpoint(WriteCheckpoint::StageDurable)?;

        match self.commit {
            Commit::Replace => replace_file(stage, self.destination),
            Commit::CreateNew => fs::hard_link(stage, self.destination),
        }
        .map_err(io_failure(WriteStep::Commit, self.destination))?;
        self.checkpoint(WriteCheckpoint::Committed)?;

        self.sync(directory)
            .map_err(io_failure(WriteStep::SyncDirectory, directory))?;
        if self.commit == Commit::CreateNew {
            // The destination link is durable. The stage name carries no
            // authority, so a failed removal leaves only an inert file.
            let _ = fs::remove_file(stage);
        }
        Ok(())
    }

    fn checkpoint(&mut self, checkpoint: WriteCheckpoint) -> Result<(), WriteFailure<E>> {
        match self.observer.as_mut() {
            Some(observer) => {
                observer(checkpoint).map_err(|error| WriteFailure::Observer { checkpoint, error })
            }
            None => Ok(()),
        }
    }

    fn sync(&mut self, directory: &Path) -> io::Result<()> {
        match self.directory_sync.as_mut() {
            Some(sync) => sync(directory),
            None => sync_directory(directory),
        }
    }

    fn remove_stage(&mut self, directory: &Path) -> Result<(), CleanupError> {
        match fs::remove_file(self.stage) {
            Ok(()) => self.sync(directory).map_err(|source| CleanupError {
                step: CleanupStep::SyncDirectory,
                path: directory.to_path_buf(),
                source,
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(CleanupError {
                step: CleanupStep::RemoveStage,
                path: self.stage.to_path_buf(),
                source,
            }),
        }
    }
}

fn create_stage(path: &Path, owner_only: bool, created: &mut bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if owner_only {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(OWNER_FILE_MODE);
    }
    let file = options.open(path)?;
    *created = true;
    #[cfg(unix)]
    if owner_only {
        use std::os::unix::fs::PermissionsExt as _;
        file.set_permissions(fs::Permissions::from_mode(OWNER_FILE_MODE))?;
    }
    #[cfg(not(unix))]
    let _ = owner_only;
    Ok(file)
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let mut source_wide: Vec<u16> = source.as_os_str().encode_wide().collect();
    source_wide.push(0);
    let mut destination_wide: Vec<u16> = destination.as_os_str().encode_wide().collect();
    destination_wide.push(0);
    // SAFETY: both pointers address null-terminated buffers that remain alive
    // for the duration of the synchronous call.
    let result = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(directory)
            .expect("directory should list")
            .map(|entry| {
                entry
                    .expect("entry should read")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn replace_publishes_bytes_and_syncs_the_directory_after_the_commit() {
        let root = tempfile::tempdir().expect("temporary directory should exist");
        let stage = root.path().join("record.stage");
        let destination = root.path().join("record.json");
        fs::write(&destination, b"old").expect("previous record should write");
        let mut events = Vec::new();
        let mut synced = Vec::new();
        let mut sync = |path: &Path| {
            synced.push(path.to_path_buf());
            Ok(())
        };
        let mut observe = |checkpoint: WriteCheckpoint| {
            events.push(checkpoint);
            Ok::<(), Infallible>(())
        };

        StagedWrite::new(&stage, &destination)
            .directory_sync(&mut sync)
            .observer(&mut observe)
            .write(b"new")
            .expect("staged write should publish");

        assert_eq!(fs::read(&destination).expect("record should read"), b"new");
        assert_eq!(entries(root.path()), ["record.json"]);
        assert_eq!(
            events,
            [WriteCheckpoint::StageDurable, WriteCheckpoint::Committed]
        );
        assert_eq!(synced, [root.path().to_path_buf()]);
    }

    #[test]
    fn create_new_refuses_an_existing_destination_and_removes_its_stage() {
        let root = tempfile::tempdir().expect("temporary directory should exist");
        let stage = root.path().join("record.stage");
        let destination = root.path().join("record.json");
        fs::write(&destination, b"owner").expect("existing owner should write");

        let error = StagedWrite::new(&stage, &destination)
            .commit(Commit::CreateNew)
            .write(b"intruder")
            .expect_err("an existing destination must not be replaced");

        assert!(matches!(
            &error.failure,
            WriteFailure::Io { step: WriteStep::Commit, source, .. }
                if source.kind() == io::ErrorKind::AlreadyExists
        ));
        assert!(error.cleanup.is_none());
        assert_eq!(fs::read(&destination).expect("owner should read"), b"owner");
        assert_eq!(entries(root.path()), ["record.json"]);
    }

    #[test]
    fn create_new_publishes_and_removes_its_stage_after_the_directory_sync() {
        let root = tempfile::tempdir().expect("temporary directory should exist");
        let stage = root.path().join("record.stage");
        let destination = root.path().join("record.json");
        let mut stage_present_at_sync = Vec::new();
        let mut sync = |_: &Path| {
            stage_present_at_sync.push(stage.exists());
            Ok(())
        };

        StagedWrite::new(&stage, &destination)
            .commit(Commit::CreateNew)
            .directory_sync(&mut sync)
            .write(b"first")
            .expect("first publication should succeed");

        assert_eq!(stage_present_at_sync, [true]);
        assert_eq!(
            fs::read(&destination).expect("record should read"),
            b"first"
        );
        assert_eq!(entries(root.path()), ["record.json"]);
    }

    #[test]
    fn observer_failure_before_commit_cleans_the_stage_and_keeps_the_old_record() {
        let root = tempfile::tempdir().expect("temporary directory should exist");
        let stage = root.path().join("record.stage");
        let destination = root.path().join("record.json");
        fs::write(&destination, b"old").expect("previous record should write");
        let mut synced = 0;
        let mut sync = |_: &Path| {
            synced += 1;
            Ok(())
        };
        let mut observe = |checkpoint: WriteCheckpoint| match checkpoint {
            WriteCheckpoint::StageDurable => Err("lost acknowledgement"),
            WriteCheckpoint::Committed => Ok(()),
        };

        let error = StagedWrite::new(&stage, &destination)
            .directory_sync(&mut sync)
            .observer(&mut observe)
            .write(b"new")
            .expect_err("observer failure should abort");

        assert!(matches!(
            error.failure,
            WriteFailure::Observer {
                checkpoint: WriteCheckpoint::StageDurable,
                error: "lost acknowledgement"
            }
        ));
        assert!(error.cleanup.is_none());
        assert_eq!(synced, 1, "stage removal must be made durable");
        assert_eq!(fs::read(&destination).expect("record should read"), b"old");
        assert_eq!(entries(root.path()), ["record.json"]);
    }

    #[test]
    fn directory_sync_failure_after_commit_reports_the_ambiguous_step() {
        let root = tempfile::tempdir().expect("temporary directory should exist");
        let stage = root.path().join("record.stage");
        let destination = root.path().join("record.json");
        let mut sync = |_: &Path| Err(io::Error::other("injected directory sync failure"));

        let error = StagedWrite::new(&stage, &destination)
            .directory_sync(&mut sync)
            .write(b"new")
            .expect_err("directory sync failure should surface");

        assert!(matches!(
            error.failure,
            WriteFailure::Io {
                step: WriteStep::SyncDirectory,
                ..
            }
        ));
        assert!(
            error.cleanup.is_none(),
            "the renamed stage needs no cleanup"
        );
        assert_eq!(fs::read(&destination).expect("record should read"), b"new");
        assert!(error.to_string().contains("failed to sync directory"));
    }

    #[test]
    fn existing_stage_fails_closed_without_touching_either_file() {
        let root = tempfile::tempdir().expect("temporary directory should exist");
        let stage = root.path().join("record.stage");
        let destination = root.path().join("record.json");
        fs::write(&stage, b"stale").expect("stale stage should write");

        let error = StagedWrite::new(&stage, &destination)
            .write(b"new")
            .expect_err("an existing stage must not be reused");

        assert!(matches!(
            error.failure,
            WriteFailure::Io {
                step: WriteStep::CreateStage,
                ..
            }
        ));
        assert_eq!(fs::read(&stage).expect("stage should read"), b"stale");
        assert!(!destination.exists());
    }

    #[test]
    fn stage_outside_the_destination_directory_is_rejected() {
        let root = tempfile::tempdir().expect("temporary directory should exist");
        let nested = root.path().join("nested");
        fs::create_dir(&nested).expect("nested directory should create");
        let stage = nested.join("record.stage");
        let destination = root.path().join("record.json");

        let error = StagedWrite::new(&stage, &destination)
            .write(b"new")
            .expect_err("a cross-directory stage must be rejected");

        assert!(matches!(
            &error.failure,
            WriteFailure::Io { step: WriteStep::CreateStage, source, .. }
                if source.kind() == io::ErrorKind::InvalidInput
        ));
        assert!(!stage.exists() && !destination.exists());
    }

    #[cfg(unix)]
    #[test]
    fn owner_only_write_publishes_a_private_record() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = tempfile::tempdir().expect("temporary directory should exist");
        let stage = root.path().join("record.stage");
        let destination = root.path().join("record.json");

        StagedWrite::new(&stage, &destination)
            .owner_only()
            .write(b"secret")
            .expect("owner-only write should publish");

        let mode = fs::metadata(&destination)
            .expect("record metadata should read")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, OWNER_FILE_MODE);
    }
}
