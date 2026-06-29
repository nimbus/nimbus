//! Mount resolver for NimbusFS virtual paths.
//!
//! The resolver owns typed `Path`/`PathBuf` normalization after Deno has
//! converted string, Buffer, or typed-array path inputs. It selects the
//! longest-prefix mount entry, rejects `..` traversal out of mount roots, and
//! feeds one backend-local path to `NimbusFs`. Backend-specific symlink policy
//! still runs at access time, while `NimbusFs` maps `realpath` and `readlink`
//! outputs back to virtual paths. Pair operations use the resolved mount prefix
//! to fail cross-mount rename/copy/link before any backend can fall through.

use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf};

use deno_fs::FileSystemRc;
use deno_io::fs::{FsError, FsResult};

use crate::mount::{MountTable, MountTarget, path_from_parts};

#[derive(Debug, Clone)]
pub struct MountResolver {
    table: MountTable,
}

#[derive(Clone)]
pub struct ResolvedPath {
    pub virtual_path: PathBuf,
    pub mount_prefix: PathBuf,
    pub backend_path: PathBuf,
    target: ResolvedTarget,
}

#[derive(Clone)]
enum ResolvedTarget {
    Backend {
        backend: FileSystemRc,
        readonly: bool,
    },
    Masked {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedAccess {
    ReadWrite,
    ReadOnly,
    Masked,
}

impl MountResolver {
    pub fn new(table: MountTable) -> Self {
        Self { table }
    }

    pub fn table(&self) -> &MountTable {
        &self.table
    }

    pub fn resolve(&self, cwd: &Path, path: &Path) -> FsResult<ResolvedPath> {
        let virtual_path = self.normalize_virtual_path(cwd, path)?;
        let entry = self.table.resolve_entry(&virtual_path).ok_or_else(|| {
            FsError::from(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no NimbusFS mount for {}", virtual_path.display()),
            ))
        })?;
        let backend_path = backend_path_for(entry.prefix(), &virtual_path);
        let target = match entry.target() {
            MountTarget::Backend { backend, readonly } => ResolvedTarget::Backend {
                backend: backend.clone(),
                readonly: *readonly,
            },
            MountTarget::Masked { message } => ResolvedTarget::Masked {
                message: message.clone(),
            },
        };
        Ok(ResolvedPath {
            virtual_path,
            mount_prefix: entry.prefix().to_path_buf(),
            backend_path,
            target,
        })
    }

    pub fn normalize_virtual_path(&self, cwd: &Path, path: &Path) -> FsResult<PathBuf> {
        let mut parts = if path.is_absolute() {
            Vec::new()
        } else {
            split_absolute(cwd)?
        };

        for component in path.components() {
            match component {
                Component::RootDir => {
                    parts.clear();
                }
                Component::CurDir => {}
                Component::Normal(part) => parts.push(part.to_owned()),
                Component::ParentDir => {
                    if parts.is_empty() {
                        return Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "path escapes NimbusFS root",
                        )
                        .into());
                    }
                    let current = path_from_parts(&parts);
                    if self.table.has_explicit_mount_root(&current) {
                        return Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "path escapes NimbusFS mount root",
                        )
                        .into());
                    }
                    parts.pop();
                }
                Component::Prefix(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "platform path prefixes are not valid NimbusFS paths",
                    )
                    .into());
                }
            }
        }

        Ok(path_from_parts(&parts))
    }
}

impl ResolvedPath {
    pub fn access(&self) -> ResolvedAccess {
        match self.target {
            ResolvedTarget::Backend {
                readonly: false, ..
            } => ResolvedAccess::ReadWrite,
            ResolvedTarget::Backend { readonly: true, .. } => ResolvedAccess::ReadOnly,
            ResolvedTarget::Masked { .. } => ResolvedAccess::Masked,
        }
    }

    pub fn backend(&self) -> FsResult<FileSystemRc> {
        match &self.target {
            ResolvedTarget::Backend { backend, .. } => Ok(backend.clone()),
            ResolvedTarget::Masked { message } => Err(masked_error(message)),
        }
    }

    pub fn ensure_readable(&self) -> FsResult<()> {
        match &self.target {
            ResolvedTarget::Backend { .. } => Ok(()),
            ResolvedTarget::Masked { message } => Err(masked_error(message)),
        }
    }

    pub fn ensure_writable(&self) -> FsResult<()> {
        match &self.target {
            ResolvedTarget::Backend {
                readonly: false, ..
            } => Ok(()),
            ResolvedTarget::Backend { readonly: true, .. } => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "read-only NimbusFS overlay (EROFS)",
            )
            .into()),
            ResolvedTarget::Masked { message } => Err(masked_error(message)),
        }
    }
}

impl std::fmt::Debug for ResolvedPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedPath")
            .field("virtual_path", &self.virtual_path)
            .field("mount_prefix", &self.mount_prefix)
            .field("backend_path", &self.backend_path)
            .field("access", &self.access())
            .finish()
    }
}

pub(crate) fn backend_path_for(prefix: &Path, virtual_path: &Path) -> PathBuf {
    if prefix == Path::new("/") {
        return virtual_path.to_path_buf();
    }
    let relative = virtual_path.strip_prefix(prefix).unwrap_or(Path::new(""));
    let mut path = PathBuf::from("/");
    path.push(relative);
    path
}

pub(crate) fn virtual_path_for_backend(prefix: &Path, backend_path: &Path) -> PathBuf {
    if prefix == Path::new("/") {
        return backend_path.to_path_buf();
    }
    let relative = backend_path.strip_prefix("/").unwrap_or(backend_path);
    let mut path = prefix.to_path_buf();
    path.push(relative);
    path
}

fn split_absolute(path: &Path) -> FsResult<Vec<OsString>> {
    if !path.is_absolute() {
        return Err(
            io::Error::new(io::ErrorKind::InvalidInput, "NimbusFS cwd must be absolute").into(),
        );
    }
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::CurDir => {}
            Component::Normal(part) => parts.push(part.to_owned()),
            Component::ParentDir | Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "NimbusFS cwd must be normalized",
                )
                .into());
            }
        }
    }
    Ok(parts)
}

fn masked_error(message: &str) -> FsError {
    io::Error::new(io::ErrorKind::NotFound, message).into()
}
