use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};

use deno_fs::FileSystemRc;

#[derive(Clone)]
pub struct MountTable {
    entries: Vec<MountEntry>,
}

#[derive(Clone)]
pub struct MountEntry {
    prefix: PathBuf,
    target: MountTarget,
}

#[derive(Clone)]
pub enum MountTarget {
    Backend {
        backend: FileSystemRc,
        readonly: bool,
    },
    Masked {
        message: String,
    },
}

impl MountTable {
    pub fn new(fallback: FileSystemRc) -> Self {
        Self {
            entries: vec![MountEntry {
                prefix: PathBuf::from("/"),
                target: MountTarget::Backend {
                    backend: fallback,
                    readonly: false,
                },
            }],
        }
    }

    pub fn mount(&mut self, prefix: impl AsRef<Path>, backend: FileSystemRc) -> io::Result<()> {
        self.push_entry(
            prefix,
            MountTarget::Backend {
                backend,
                readonly: false,
            },
        )
    }

    pub fn mount_readonly(
        &mut self,
        prefix: impl AsRef<Path>,
        backend: FileSystemRc,
    ) -> io::Result<()> {
        self.push_entry(
            prefix,
            MountTarget::Backend {
                backend,
                readonly: true,
            },
        )
    }

    pub fn mount_masked(&mut self, prefix: impl AsRef<Path>) -> io::Result<()> {
        self.push_entry(
            prefix,
            MountTarget::Masked {
                message: "masked NimbusFS path".to_string(),
            },
        )
    }

    pub fn entries(&self) -> &[MountEntry] {
        &self.entries
    }

    pub(crate) fn from_entries(entries: Vec<MountEntry>) -> Self {
        Self { entries }
    }

    pub fn resolve_entry(&self, virtual_path: &Path) -> Option<&MountEntry> {
        // Longest-prefix matching is the authority boundary: overlays and
        // concrete backends are both represented as mount-table entries.
        self.entries
            .iter()
            .filter(|entry| path_has_prefix(virtual_path, entry.prefix()))
            .max_by_key(|entry| entry.prefix().components().count())
    }

    pub(crate) fn has_explicit_mount_root(&self, path: &Path) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.prefix() != Path::new("/") && entry.prefix() == path)
    }

    fn push_entry(&mut self, prefix: impl AsRef<Path>, target: MountTarget) -> io::Result<()> {
        let prefix = normalize_mount_prefix(prefix.as_ref())?;
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|existing| existing.prefix == prefix)
        {
            existing.target = target;
            return Ok(());
        }
        self.entries.push(MountEntry { prefix, target });
        Ok(())
    }
}

impl MountEntry {
    pub(crate) fn backend(prefix: PathBuf, backend: FileSystemRc, readonly: bool) -> Self {
        Self {
            prefix,
            target: MountTarget::Backend { backend, readonly },
        }
    }

    pub(crate) fn masked(prefix: PathBuf, message: impl Into<String>) -> Self {
        Self {
            prefix,
            target: MountTarget::Masked {
                message: message.into(),
            },
        }
    }

    pub fn prefix(&self) -> &Path {
        &self.prefix
    }

    pub fn target(&self) -> &MountTarget {
        &self.target
    }
}

impl MountTarget {
    pub fn access_label(&self) -> &'static str {
        match self {
            Self::Backend {
                readonly: false, ..
            } => "read-write",
            Self::Backend { readonly: true, .. } => "readonly",
            Self::Masked { .. } => "masked",
        }
    }
}

impl fmt::Debug for MountTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MountTable")
            .field("entries", &self.entries)
            .finish()
    }
}

impl fmt::Debug for MountEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MountEntry")
            .field("prefix", &self.prefix)
            .field("target", &self.target)
            .finish()
    }
}

impl fmt::Debug for MountTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Backend { readonly, .. } => f
                .debug_struct("Backend")
                .field("readonly", readonly)
                .finish_non_exhaustive(),
            Self::Masked { message } => f.debug_struct("Masked").field("message", message).finish(),
        }
    }
}

fn normalize_mount_prefix(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "mount prefix must be absolute",
        ));
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
                    "mount prefix cannot contain parent or platform prefix components",
                ));
            }
        }
    }
    Ok(path_from_parts(&parts))
}

pub(crate) fn path_from_parts(parts: &[std::ffi::OsString]) -> PathBuf {
    let mut path = PathBuf::from("/");
    for part in parts {
        path.push(part);
    }
    path
}

pub(crate) fn path_has_prefix(path: &Path, prefix: &Path) -> bool {
    if prefix == Path::new("/") {
        return path.is_absolute();
    }
    path == prefix || path.starts_with(prefix)
}
