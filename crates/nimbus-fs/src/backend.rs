use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::PathBuf;

use deno_fs::FileSystemRc;
use deno_fs::sync::MaybeArc;
use deno_io::fs::FsResult;
use nimbus_runtime::NimbusFsBackend;

use crate::caps::FsMountCaps;
use crate::mount::MountTable;

#[derive(Debug, Clone)]
pub struct BackendRegistry {
    registrations: BTreeMap<String, BackendRegistration>,
}

#[derive(Clone)]
pub struct BackendRegistration {
    pub name: String,
    pub backend: FileSystemRc,
    pub caps: FsMountCaps,
    pub persistence: PersistenceMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersistenceMode {
    Ephemeral,
    Snapshot,
    DurableExternal { sync_required: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectUnsupportedOperation {
    RandomWrite,
    Hardlink,
    Symlink,
    MutableOwnership,
    DirectoryRename,
}

#[derive(Debug, Default, Clone)]
pub struct ObjectRwBackend;

impl BackendRegistry {
    pub fn new() -> Self {
        Self {
            registrations: BTreeMap::new(),
        }
    }

    pub fn register<B>(
        &mut self,
        name: impl Into<String>,
        backend: B,
        caps: FsMountCaps,
        persistence: PersistenceMode,
    ) -> FsResult<()>
    where
        B: NimbusFsBackend + 'static,
    {
        self.register_rc(name, MaybeArc::new(backend), caps, persistence)
    }

    pub fn register_rc(
        &mut self,
        name: impl Into<String>,
        backend: FileSystemRc,
        caps: FsMountCaps,
        persistence: PersistenceMode,
    ) -> FsResult<()> {
        validate_caps_contract(&caps)?;
        let name = name.into();
        if self.registrations.contains_key(&name) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("NimbusFS backend {name} already registered"),
            )
            .into());
        }
        self.registrations.insert(
            name.clone(),
            BackendRegistration {
                name,
                backend,
                caps,
                persistence,
            },
        );
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&BackendRegistration> {
        self.registrations.get(name)
    }

    pub fn mount_registered(
        &self,
        table: &mut MountTable,
        prefix: impl Into<PathBuf>,
        name: &str,
    ) -> FsResult<()> {
        let registration = self.registrations.get(name).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("NimbusFS backend {name} is not registered"),
            )
        })?;
        table
            .mount(prefix.into(), registration.backend.clone())
            .map_err(Into::into)
    }
}

impl BackendRegistration {
    pub fn requires_explicit_sync(&self) -> bool {
        matches!(
            self.persistence,
            PersistenceMode::DurableExternal {
                sync_required: true
            }
        )
    }
}

impl ObjectRwBackend {
    pub fn unsupported_operations() -> &'static [ObjectUnsupportedOperation] {
        &[
            ObjectUnsupportedOperation::RandomWrite,
            ObjectUnsupportedOperation::Hardlink,
            ObjectUnsupportedOperation::Symlink,
            ObjectUnsupportedOperation::MutableOwnership,
            ObjectUnsupportedOperation::DirectoryRename,
        ]
    }

    pub fn reject_unsupported(operation: ObjectUnsupportedOperation) -> FsResult<()> {
        let label = match operation {
            ObjectUnsupportedOperation::RandomWrite => "random write",
            ObjectUnsupportedOperation::Hardlink => "hardlink",
            ObjectUnsupportedOperation::Symlink => "symlink",
            ObjectUnsupportedOperation::MutableOwnership => "mutable ownership",
            ObjectUnsupportedOperation::DirectoryRename => "directory rename",
        };
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("object-store backend slot unsupported POSIX operation: {label}"),
        )
        .into())
    }
}

impl fmt::Debug for BackendRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BackendRegistration")
            .field("name", &self.name)
            .field("caps", &self.caps)
            .field("persistence", &self.persistence)
            .finish_non_exhaustive()
    }
}

fn validate_caps_contract(caps: &FsMountCaps) -> FsResult<()> {
    if !caps.visible {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "registered backend must be visible through FsCaps",
        )
        .into());
    }
    if caps.readonly
        && (caps.file_write || caps.directory_mutate || caps.metadata_mutate || caps.link_create)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "registered readonly backend cannot advertise write/mutate FsCaps",
        )
        .into());
    }
    Ok(())
}
