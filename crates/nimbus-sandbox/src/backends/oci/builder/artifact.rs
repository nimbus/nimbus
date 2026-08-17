use std::fs;
use std::io::{Read as _, Write as _};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    DockerfileInstruction, DockerfileRecipe, OciDockerfileBuilder, resolve_context_source_path,
};
use crate::backends::oci::buildah::{OciImageConfig, resolve_image_user_from_rootfs};
use crate::backends::oci::materializer::{
    MaterializedImageRootfs, PreparedMaterializedImageLaunch,
};
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;
use crate::spec::SandboxProcessSpec;

const BASE_MATERIALIZATION_RECEIPT_FILE: &str = "materialization.json";
const BUILD_RECEIPT_FILE: &str = "build.json";
const BUILD_RECEIPT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildProvenance {
    image_name: String,
    dockerfile_sha256: String,
    context_sha256: String,
    base_image: String,
    base_materialization_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildReceipt {
    version: u32,
    provenance: BuildProvenance,
    rootfs_sha256: String,
    image_config: OciImageConfig,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn finish_build(
    builder: &OciDockerfileBuilder,
    sandbox_id: &SandboxId,
    image_name: &str,
    built_image_reference: String,
    dockerfile_source: &[u8],
    dockerfile: &DockerfileRecipe,
    context_sha256: String,
    context_path: &Path,
    process: &SandboxProcessSpec,
    staging_artifact: MaterializedImageRootfs,
    mut image_config: OciImageConfig,
) -> Result<PreparedMaterializedImageLaunch> {
    let staging_dir = staging_artifact
        .rootfs_path
        .parent()
        .expect("materialized rootfs should have an artifact parent");
    let base_materialization_sha256 =
        compute_file_sha256(&staging_dir.join(BASE_MATERIALIZATION_RECEIPT_FILE))?;
    let provenance = BuildProvenance {
        image_name: image_name.to_owned(),
        dockerfile_sha256: compute_bytes_sha256(dockerfile_source),
        context_sha256,
        base_image: dockerfile.base_image.clone(),
        base_materialization_sha256,
    };
    let final_artifact = builder.materializer.final_artifact_path(sandbox_id);
    let final_rootfs = final_artifact.join("rootfs");

    if final_artifact.exists() {
        let receipt = read_build_receipt(&final_artifact)?;
        if receipt.version != BUILD_RECEIPT_VERSION || receipt.provenance != provenance {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "built rootfs artifact {} exists without the exact requested build provenance; refusing to delete, replace, or mutate ambiguous provider state",
                    final_artifact.display()
                ),
            });
        }
        if !final_rootfs.is_dir() || compute_tree_sha256(&final_rootfs)? != receipt.rootfs_sha256 {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "built rootfs artifact {} does not match its recorded result digest; refusing to mutate ambiguous provider state",
                    final_artifact.display()
                ),
            });
        }
        return finish_built_launch(
            built_image_reference,
            final_rootfs,
            receipt.image_config,
            process,
        );
    }

    dockerfile.apply(
        context_path,
        &staging_artifact.rootfs_path,
        &mut image_config,
    )?;

    let receipt = BuildReceipt {
        version: BUILD_RECEIPT_VERSION,
        provenance,
        rootfs_sha256: compute_tree_sha256(&staging_artifact.rootfs_path)?,
        image_config: image_config.clone(),
    };
    fs::remove_file(staging_dir.join(BASE_MATERIALIZATION_RECEIPT_FILE)).map_err(|error| {
        SandboxError::OperationFailed {
            message: format!(
                "failed to retire private base receipt in {}: {error}",
                staging_dir.display()
            ),
        }
    })?;
    write_build_receipt(staging_dir, &receipt)?;
    sync_directory(staging_dir)?;
    fs::rename(staging_dir, &final_artifact).map_err(|error| {
        SandboxError::OperationFailed {
            message: format!(
                "failed to atomically publish built rootfs artifact {} without replacing existing provider state: {error}",
                final_artifact.display()
            ),
        }
    })?;
    if let Some(parent) = final_artifact.parent() {
        sync_directory(parent)?;
    }

    finish_built_launch(built_image_reference, final_rootfs, image_config, process)
}

pub(super) fn context_sha256(dockerfile: &DockerfileRecipe, context_path: &Path) -> Result<String> {
    let canonical_context =
        fs::canonicalize(context_path).map_err(|error| SandboxError::InvalidSpec {
            message: format!(
                "failed to resolve build context {}: {error}",
                context_path.display()
            ),
        })?;
    let mut hasher = Sha256::new();
    update_digest_field(&mut hasher, b"nimbus-build-context-v1");
    for (instruction_index, instruction) in dockerfile.instructions.iter().enumerate() {
        let DockerfileInstruction::Copy(copy) = instruction else {
            continue;
        };
        update_digest_field(&mut hasher, &instruction_index.to_le_bytes());
        for source in &copy.sources {
            update_digest_field(&mut hasher, source.as_bytes());
            let source_path = resolve_context_source_path(context_path, source)?;
            let canonical_source =
                fs::canonicalize(&source_path).map_err(|error| SandboxError::InvalidSpec {
                    message: format!(
                        "failed to resolve build context source {}: {error}",
                        source_path.display()
                    ),
                })?;
            if !canonical_source.starts_with(&canonical_context) {
                return Err(SandboxError::InvalidSpec {
                    message: format!(
                        "build context source {} resolves outside declared context {}; refusing an unbound source",
                        source_path.display(),
                        context_path.display()
                    ),
                });
            }
            hash_tree_entry(&source_path, Path::new(source), &mut hasher, false)?;
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn finish_built_launch(
    image_reference: String,
    rootfs_path: PathBuf,
    mut image_config: OciImageConfig,
    process: &SandboxProcessSpec,
) -> Result<PreparedMaterializedImageLaunch> {
    let resolved_user = resolve_image_user_from_rootfs(
        &rootfs_path,
        process.user.as_deref().or(image_config.user.as_deref()),
    )?;
    image_config.user = resolved_user;

    Ok(PreparedMaterializedImageLaunch {
        launch_defaults: image_config.resolve_launch_defaults(&rootfs_path, process)?,
        artifact: MaterializedImageRootfs {
            image_reference,
            rootfs_path,
        },
    })
}

fn read_build_receipt(artifact_dir: &Path) -> Result<BuildReceipt> {
    let path = artifact_dir.join(BUILD_RECEIPT_FILE);
    let bytes = fs::read(&path).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "built rootfs artifact {} lacks a readable exact build receipt: {error}",
            artifact_dir.display()
        ),
    })?;
    serde_json::from_slice(&bytes).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "built rootfs receipt {} is invalid: {error}",
            path.display()
        ),
    })
}

fn write_build_receipt(artifact_dir: &Path, receipt: &BuildReceipt) -> Result<()> {
    let path = artifact_dir.join(BUILD_RECEIPT_FILE);
    let bytes =
        serde_json::to_vec_pretty(receipt).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to serialize exact build receipt: {error}"),
        })?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to create exact build receipt {}: {error}",
                path.display()
            ),
        })?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to persist exact build receipt {}: {error}",
                path.display()
            ),
        })
}

fn sync_directory(path: &Path) -> Result<()> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to durably sync directory {}: {error}",
                path.display()
            ),
        })
}

fn compute_file_sha256(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to open provenance input {}: {error}",
            path.display()
        ),
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to hash provenance input {}: {error}",
                    path.display()
                ),
            })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn compute_bytes_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn compute_tree_sha256(root: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    hash_tree_entry(root, Path::new("."), &mut hasher, true)?;
    Ok(format!("{:x}", hasher.finalize()))
}

fn hash_tree_entry(
    path: &Path,
    relative: &Path,
    hasher: &mut Sha256,
    preserve_symlink: bool,
) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to inspect provenance path {}: {error}",
            path.display()
        ),
    })?;
    let relative = relative.to_str().ok_or_else(|| SandboxError::InvalidSpec {
        message: format!(
            "provenance path {} is not valid UTF-8 and cannot be bound deterministically",
            path.display()
        ),
    })?;
    update_digest_field(hasher, relative.as_bytes());
    update_permissions_identity(hasher, &metadata);

    if metadata.file_type().is_symlink() {
        if !preserve_symlink {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "build context source {} contains a symbolic link; exact context binding requires regular files and directories",
                    path.display()
                ),
            });
        }
        update_digest_field(hasher, b"symlink");
        let target = fs::read_link(path).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to read rootfs symlink {}: {error}", path.display()),
        })?;
        let target = target
            .to_str()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "rootfs symlink target for {} is not valid UTF-8",
                    path.display()
                ),
            })?;
        update_digest_field(hasher, target.as_bytes());
        return Ok(());
    }
    if metadata.is_file() {
        update_digest_field(hasher, b"file");
        let bytes = fs::read(path).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to hash provenance file {}: {error}", path.display()),
        })?;
        update_digest_field(hasher, &bytes);
        return Ok(());
    }
    if metadata.is_dir() {
        update_digest_field(hasher, b"directory");
        let mut entries = fs::read_dir(path)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to read provenance directory {}: {error}",
                    path.display()
                ),
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to enumerate provenance directory {}: {error}",
                    path.display()
                ),
            })?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            hash_tree_entry(
                &entry.path(),
                &Path::new(relative).join(entry.file_name()),
                hasher,
                preserve_symlink,
            )?;
        }
        return Ok(());
    }

    Err(SandboxError::OperationFailed {
        message: format!(
            "provenance path {} has an unsupported provider file type",
            path.display()
        ),
    })
}

#[cfg(unix)]
fn update_permissions_identity(hasher: &mut Sha256, metadata: &fs::Metadata) {
    update_digest_field(hasher, &metadata.permissions().mode().to_le_bytes());
}

#[cfg(windows)]
fn update_permissions_identity(hasher: &mut Sha256, metadata: &fs::Metadata) {
    use std::os::windows::fs::MetadataExt as _;

    update_digest_field(hasher, &metadata.file_attributes().to_le_bytes());
}

#[cfg(not(any(unix, windows)))]
fn update_permissions_identity(hasher: &mut Sha256, _metadata: &fs::Metadata) {
    let _ = hasher;
}

fn update_digest_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(value.len().to_le_bytes());
    hasher.update(value);
}
