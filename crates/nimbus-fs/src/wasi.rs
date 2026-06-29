use std::path::{Path, PathBuf};

use crate::caps::{FsCaps, FsMountCaps};
use crate::mount::{MountTable, MountTarget};
use crate::resolver::{MountResolver, ResolvedAccess};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirPerms(u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilePerms(u8);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasiPreopenDescriptor {
    pub path: PathBuf,
    pub dir_perms: DirPerms,
    pub file_perms: FilePerms,
    pub rights: FsMountCaps,
}

#[derive(Debug, Clone)]
pub struct WasiPreopenBuilder {
    descriptors: Vec<WasiPreopenDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossBinderResolution {
    pub v8_mount_prefix: PathBuf,
    pub wasi_preopen_path: PathBuf,
    pub v8_access: ResolvedAccess,
    pub wasi_rights: FsMountCaps,
}

impl DirPerms {
    pub const READ: Self = Self(0b01);
    pub const MUTATE: Self = Self(0b10);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl FilePerms {
    pub const READ: Self = Self(0b01);
    pub const WRITE: Self = Self(0b10);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for DirPerms {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for DirPerms {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitOr for FilePerms {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for FilePerms {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl WasiPreopenBuilder {
    pub fn from_caps(table: &MountTable, caps: &FsCaps) -> Self {
        let descriptors = table
            .entries()
            .iter()
            .filter_map(|entry| {
                let rights = caps.grant_for_path(entry.prefix())?.clone();
                if !rights.visible || rights.masked {
                    return None;
                }
                let MountTarget::Backend { readonly, .. } = entry.target() else {
                    return None;
                };
                Some(WasiPreopenDescriptor {
                    path: entry.prefix().to_path_buf(),
                    dir_perms: dir_perms(&rights, *readonly),
                    file_perms: file_perms(&rights, *readonly),
                    rights: if *readonly {
                        FsMountCaps {
                            readonly: true,
                            file_write: false,
                            directory_mutate: false,
                            metadata_mutate: false,
                            link_create: false,
                            ..rights
                        }
                    } else {
                        rights
                    },
                })
            })
            .collect();
        Self { descriptors }
    }

    pub fn descriptors(&self) -> &[WasiPreopenDescriptor] {
        &self.descriptors
    }

    pub fn descriptor_for_path(&self, path: &Path) -> Option<&WasiPreopenDescriptor> {
        self.descriptors
            .iter()
            .filter(|descriptor| path == descriptor.path || path.starts_with(&descriptor.path))
            .max_by_key(|descriptor| descriptor.path.components().count())
    }

    pub fn cross_binder_resolution(
        &self,
        resolver: &MountResolver,
        cwd: &Path,
        path: &Path,
    ) -> Option<CrossBinderResolution> {
        let resolved = resolver.resolve(cwd, path).ok()?;
        let descriptor = self.descriptor_for_path(&resolved.virtual_path)?;
        Some(CrossBinderResolution {
            v8_mount_prefix: resolved.mount_prefix.clone(),
            wasi_preopen_path: descriptor.path.clone(),
            v8_access: resolved.access(),
            wasi_rights: descriptor.rights.clone(),
        })
    }
}

fn dir_perms(rights: &FsMountCaps, readonly_overlay: bool) -> DirPerms {
    let mut perms = DirPerms::empty();
    if rights.directory_read {
        perms |= DirPerms::READ;
    }
    if rights.directory_mutate && !rights.readonly && !readonly_overlay {
        perms |= DirPerms::MUTATE;
    }
    perms
}

fn file_perms(rights: &FsMountCaps, readonly_overlay: bool) -> FilePerms {
    let mut perms = FilePerms::empty();
    if rights.file_read {
        perms |= FilePerms::READ;
    }
    if rights.file_write && !rights.readonly && !readonly_overlay {
        perms |= FilePerms::WRITE;
    }
    perms
}
