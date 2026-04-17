use std::fs;
use std::io::{BufReader, Read};
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;
use oci_client::Reference;
use oci_client::client::{Client as OciClient, ClientConfig as OciClientConfig, ClientProtocol};
use oci_client::manifest::{
    IMAGE_MANIFEST_LIST_MEDIA_TYPE, IMAGE_MANIFEST_MEDIA_TYPE, OCI_IMAGE_INDEX_MEDIA_TYPE,
    OCI_IMAGE_MEDIA_TYPE, OciDescriptor,
};
use oci_client::secrets::RegistryAuth;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tar::Archive;
use tokio::io::AsyncWriteExt;
use ulid::Ulid;

use super::buildah::{
    OciImageLaunchDefaults, parse_image_config_blob, resolve_image_user_from_rootfs,
};
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;
use crate::spec::SandboxImageProcessOverrides;

const OCI_IMAGE_OS: &str = "linux";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterializedImageRootfs {
    pub image_reference: String,
    pub rootfs_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedMaterializedImageLaunch {
    pub artifact: MaterializedImageRootfs,
    pub launch_defaults: OciImageLaunchDefaults,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OciImageMaterializer {
    blob_cache_dir: PathBuf,
    rootfs_root_dir: PathBuf,
}

impl OciImageMaterializer {
    pub fn under_state_root(state_root: impl Into<PathBuf>) -> Self {
        let state_root = state_root.into();
        Self {
            blob_cache_dir: state_root.join("image-cache").join("oci"),
            rootfs_root_dir: state_root.join("materialized-rootfs"),
        }
    }

    pub fn prepare_image_launch(
        &self,
        sandbox_id: &SandboxId,
        image_reference: &str,
        overrides: &SandboxImageProcessOverrides,
    ) -> Result<PreparedMaterializedImageLaunch> {
        fs::create_dir_all(&self.blob_cache_dir).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!(
                    "failed to create OCI blob cache directory {}: {error}",
                    self.blob_cache_dir.display()
                ),
            }
        })?;
        fs::create_dir_all(&self.rootfs_root_dir).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!(
                    "failed to create materialized rootfs directory {}: {error}",
                    self.rootfs_root_dir.display()
                ),
            }
        })?;

        let cached_image =
            pull_image_artifacts_to_cache(self.blob_cache_dir.clone(), image_reference.to_owned())?;
        let artifact =
            self.materialize_rootfs(sandbox_id, image_reference, &cached_image.layers)?;

        let config_bytes =
            fs::read(&cached_image.config_path).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to read cached OCI image config {}: {error}",
                    cached_image.config_path.display()
                ),
            })?;
        let image_config = parse_image_config_blob(&config_bytes)?;
        let resolved_user = resolve_image_user_from_rootfs(
            &artifact.rootfs_path,
            overrides.user.as_deref().or(image_config.user.as_deref()),
        )?;
        let mut config_with_resolved_user = image_config;
        config_with_resolved_user.user = resolved_user;
        let mut process_overrides = overrides.clone();
        process_overrides.user = None;

        Ok(PreparedMaterializedImageLaunch {
            launch_defaults: config_with_resolved_user
                .resolve_launch_defaults(&artifact.rootfs_path, &process_overrides)?,
            artifact,
        })
    }

    fn materialize_rootfs(
        &self,
        sandbox_id: &SandboxId,
        image_reference: &str,
        layers: &[PathBuf],
    ) -> Result<MaterializedImageRootfs> {
        let final_rootfs = self.rootfs_root_dir.join(sandbox_id.as_str());
        let temp_rootfs = self.rootfs_root_dir.join(format!(
            "{}.extracting-{}",
            sandbox_id.as_str(),
            Ulid::new()
        ));
        if temp_rootfs.exists() {
            fs::remove_dir_all(&temp_rootfs).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to remove stale temporary rootfs {}: {error}",
                    temp_rootfs.display()
                ),
            })?;
        }
        if final_rootfs.exists() {
            fs::remove_dir_all(&final_rootfs).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to replace stale materialized rootfs {}: {error}",
                    final_rootfs.display()
                ),
            })?;
        }

        let result = (|| -> Result<()> {
            fs::create_dir_all(&temp_rootfs).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to create temporary rootfs {}: {error}",
                    temp_rootfs.display()
                ),
            })?;
            for layer in layers {
                apply_layer_archive(layer, &temp_rootfs)?;
            }
            fs::rename(&temp_rootfs, &final_rootfs).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!(
                        "failed to persist materialized rootfs {}: {error}",
                        final_rootfs.display()
                    ),
                }
            })?;
            Ok(())
        })();

        if result.is_err() && temp_rootfs.exists() {
            let _ = fs::remove_dir_all(&temp_rootfs);
        }
        result?;

        Ok(MaterializedImageRootfs {
            image_reference: image_reference.to_owned(),
            rootfs_path: final_rootfs,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CachedImageArtifacts {
    config_path: PathBuf,
    layers: Vec<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct RegistryImageIndex {
    manifests: Vec<RegistryManifestDescriptor>,
}

#[derive(Debug, Clone, Deserialize)]
struct RegistryManifestDescriptor {
    digest: String,
    platform: Option<RegistryPlatform>,
}

#[derive(Debug, Clone, Deserialize)]
struct RegistryPlatform {
    architecture: String,
    os: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RegistryImageManifest {
    config: RegistryBlobDescriptor,
    layers: Vec<RegistryBlobDescriptor>,
}

#[derive(Debug, Clone, Deserialize)]
struct RegistryBlobDescriptor {
    digest: String,
    size: i64,
    #[serde(rename = "mediaType")]
    media_type: String,
}

#[derive(Debug, Clone)]
struct SelectedImageManifest {
    child_reference: Reference,
    manifest: RegistryImageManifest,
}

fn pull_image_artifacts_to_cache(
    blob_cache_dir: PathBuf,
    image_reference: String,
) -> Result<CachedImageArtifacts> {
    let worker = move || -> Result<CachedImageArtifacts> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!("failed to create OCI download runtime: {error}"),
            })?;
        runtime.block_on(pull_image_artifacts_to_cache_async(
            blob_cache_dir,
            image_reference,
        ))
    };

    if tokio::runtime::Handle::try_current().is_ok() {
        return std::thread::spawn(worker)
            .join()
            .map_err(|_| SandboxError::OperationFailed {
                message: "OCI image download worker panicked".to_owned(),
            })?;
    }

    worker()
}

async fn pull_image_artifacts_to_cache_async(
    blob_cache_dir: PathBuf,
    image_reference: String,
) -> Result<CachedImageArtifacts> {
    let stripped_reference = strip_docker_reference_prefix(&image_reference);
    let registry_reference = Reference::try_from(stripped_reference.as_str()).map_err(|error| {
        SandboxError::InvalidSpec {
            message: format!("failed to parse OCI image reference '{image_reference}': {error}"),
        }
    })?;
    let client = build_oci_client(&stripped_reference)?;
    let auth = RegistryAuth::Anonymous;
    let accepted_media_types = vec![
        OCI_IMAGE_INDEX_MEDIA_TYPE,
        IMAGE_MANIFEST_LIST_MEDIA_TYPE,
        OCI_IMAGE_MEDIA_TYPE,
        IMAGE_MANIFEST_MEDIA_TYPE,
    ];
    let (top_manifest_bytes, _) = client
        .pull_manifest_raw(&registry_reference, &auth, &accepted_media_types)
        .await
        .map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to resolve OCI image reference '{image_reference}': {error}"),
        })?;

    let selected_manifest =
        select_image_manifest(&image_reference, &top_manifest_bytes, &client, &auth).await?;
    let config_path = pull_blob_to_cache(
        &blob_cache_dir,
        &selected_manifest.child_reference,
        &selected_manifest.manifest.config,
        &client,
    )
    .await?;
    let mut layers = Vec::with_capacity(selected_manifest.manifest.layers.len());
    for layer in &selected_manifest.manifest.layers {
        layers.push(
            pull_blob_to_cache(
                &blob_cache_dir,
                &selected_manifest.child_reference,
                layer,
                &client,
            )
            .await?,
        );
    }
    Ok(CachedImageArtifacts {
        config_path,
        layers,
    })
}

async fn select_image_manifest(
    reference: &str,
    top_manifest_bytes: &[u8],
    client: &OciClient,
    auth: &RegistryAuth,
) -> Result<SelectedImageManifest> {
    if let Ok(index) = serde_json::from_slice::<RegistryImageIndex>(top_manifest_bytes) {
        let descriptor = select_image_manifest_descriptor(reference, &index.manifests)?;
        let child_reference = build_digest_reference(reference, &descriptor.digest)?;
        let (child_manifest_bytes, _) = client
            .pull_manifest_raw(
                &child_reference,
                auth,
                &[OCI_IMAGE_MEDIA_TYPE, IMAGE_MANIFEST_MEDIA_TYPE],
            )
            .await
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to pull OCI image child manifest '{}': {error}",
                    descriptor.digest
                ),
            })?;
        let manifest = serde_json::from_slice::<RegistryImageManifest>(&child_manifest_bytes)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to parse OCI image child manifest '{}': {error}",
                    descriptor.digest
                ),
            })?;
        return Ok(SelectedImageManifest {
            child_reference,
            manifest,
        });
    }

    let manifest =
        serde_json::from_slice::<RegistryImageManifest>(top_manifest_bytes).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!("failed to parse OCI image manifest '{reference}': {error}"),
            }
        })?;
    let child_reference = Reference::try_from(strip_docker_reference_prefix(reference).as_str())
        .map_err(|error| SandboxError::InvalidSpec {
            message: format!("failed to parse OCI image reference '{reference}': {error}"),
        })?;
    Ok(SelectedImageManifest {
        child_reference,
        manifest,
    })
}

fn select_image_manifest_descriptor<'a>(
    reference: &str,
    manifests: &'a [RegistryManifestDescriptor],
) -> Result<&'a RegistryManifestDescriptor> {
    manifests
        .iter()
        .find(|descriptor| {
            let Some(platform) = descriptor.platform.as_ref() else {
                return false;
            };
            platform.os == OCI_IMAGE_OS
                && current_oci_architectures()
                    .iter()
                    .any(|arch| platform.architecture == *arch)
        })
        .ok_or_else(|| SandboxError::InvalidSpec {
            message: format!(
                "OCI image reference '{reference}' does not contain a linux/{:?} image manifest",
                current_oci_architectures()
            ),
        })
}

async fn pull_blob_to_cache(
    blob_cache_dir: &Path,
    child_reference: &Reference,
    blob: &RegistryBlobDescriptor,
    client: &OciClient,
) -> Result<PathBuf> {
    let cache_path = blob_cache_dir.join(cached_blob_file_name(blob));
    if cache_path.is_file() {
        return Ok(cache_path);
    }

    let download_path = blob_cache_dir.join(format!("{}.download", digest_hex(&blob.digest)?));
    if download_path.exists() {
        fs::remove_file(&download_path).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to remove stale OCI blob download {}: {error}",
                download_path.display()
            ),
        })?;
    }

    let mut output = tokio::fs::File::create(&download_path)
        .await
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to create temporary OCI blob {}: {error}",
                download_path.display()
            ),
        })?;
    client
        .pull_blob(child_reference, &to_oci_descriptor(blob), &mut output)
        .await
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to download OCI blob '{}' from '{}': {error}",
                blob.digest, child_reference
            ),
        })?;
    output
        .flush()
        .await
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to flush downloaded OCI blob {}: {error}",
                download_path.display()
            ),
        })?;
    output
        .shutdown()
        .await
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to close downloaded OCI blob {}: {error}",
                download_path.display()
            ),
        })?;
    drop(output);

    verify_downloaded_blob(&download_path, blob)?;
    fs::rename(&download_path, &cache_path).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to persist OCI blob cache {}: {error}",
            cache_path.display()
        ),
    })?;
    Ok(cache_path)
}

fn build_oci_client(reference: &str) -> Result<OciClient> {
    let mut config = OciClientConfig::default();
    if is_loopback_registry(reference) {
        config.protocol = ClientProtocol::Http;
    }
    OciClient::try_from(config).map_err(|error| SandboxError::OperationFailed {
        message: format!("failed to initialize OCI client for image '{reference}': {error}"),
    })
}

fn strip_docker_reference_prefix(reference: &str) -> String {
    reference
        .strip_prefix("docker://")
        .unwrap_or(reference)
        .to_owned()
}

fn is_loopback_registry(reference: &str) -> bool {
    let stripped_reference = strip_docker_reference_prefix(reference);
    let host = stripped_reference.split('/').next().unwrap_or_default();
    host.starts_with("localhost") || host.starts_with("127.0.0.1") || host.starts_with("[::1]")
}

fn build_digest_reference(reference: &str, digest: &str) -> Result<Reference> {
    let reference = strip_docker_reference_prefix(reference);
    let repository = reference
        .split_once('@')
        .map(|(value, _)| value.to_owned())
        .unwrap_or_else(|| {
            let last_slash = reference.rfind('/');
            let last_colon = reference.rfind(':');
            match (last_slash, last_colon) {
                (_, None) => reference.clone(),
                (Some(slash), Some(colon)) if colon > slash => reference[..colon].to_owned(),
                (None, Some(_)) => reference.clone(),
                _ => reference.clone(),
            }
        });
    Reference::try_from(format!("{repository}@{digest}")).map_err(|error| {
        SandboxError::InvalidSpec {
            message: format!(
                "failed to build OCI digest reference '{repository}@{digest}': {error}"
            ),
        }
    })
}

fn current_oci_architectures() -> &'static [&'static str] {
    #[cfg(target_arch = "aarch64")]
    {
        &["aarch64", "arm64"]
    }
    #[cfg(target_arch = "x86_64")]
    {
        &["x86_64", "amd64"]
    }
}

fn cached_blob_file_name(blob: &RegistryBlobDescriptor) -> String {
    format!(
        "{}.blob",
        digest_hex(&blob.digest).unwrap_or_else(|_| "oci".to_owned())
    )
}

fn verify_downloaded_blob(path: &Path, blob: &RegistryBlobDescriptor) -> Result<()> {
    let metadata = fs::metadata(path).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to stat downloaded OCI blob {}: {error}",
            path.display()
        ),
    })?;
    if metadata.len() != blob.size as u64 {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "downloaded OCI blob {} has size {}, expected {}",
                path.display(),
                metadata.len(),
                blob.size
            ),
        });
    }
    let digest = compute_sha256(path)?;
    let expected = digest_hex(&blob.digest)?;
    if digest != expected {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "downloaded OCI blob {} has sha256 {}, expected {}",
                path.display(),
                digest,
                expected
            ),
        });
    }
    Ok(())
}

fn compute_sha256(path: &Path) -> Result<String> {
    let mut reader =
        BufReader::new(
            fs::File::open(path).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to open {} for sha256 verification: {error}",
                    path.display()
                ),
            })?,
        );
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to read {} for sha256 verification: {error}",
                    path.display()
                ),
            })?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn digest_hex(digest: &str) -> Result<String> {
    let (algorithm, hex) = digest
        .split_once(':')
        .ok_or_else(|| SandboxError::InvalidSpec {
            message: format!("invalid OCI digest '{digest}': missing algorithm prefix"),
        })?;
    if algorithm != "sha256" {
        return Err(SandboxError::InvalidSpec {
            message: format!("unsupported OCI digest algorithm '{algorithm}'; expected sha256"),
        });
    }
    Ok(hex.to_owned())
}

fn to_oci_descriptor(blob: &RegistryBlobDescriptor) -> OciDescriptor {
    OciDescriptor {
        digest: blob.digest.clone(),
        media_type: blob.media_type.clone(),
        size: blob.size,
        ..Default::default()
    }
}

fn apply_layer_archive(layer_path: &Path, rootfs_path: &Path) -> Result<()> {
    let reader = open_layer_reader(layer_path)?;
    let mut archive = Archive::new(reader);
    let entries = archive
        .entries()
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to read OCI layer archive {}: {error}",
                layer_path.display()
            ),
        })?;
    for entry in entries {
        let mut entry = entry.map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to iterate OCI layer archive {}: {error}",
                layer_path.display()
            ),
        })?;
        let relative_path = sanitize_archive_path(
            entry
                .path()
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to read OCI layer path from {}: {error}",
                        layer_path.display()
                    ),
                })?
                .as_ref(),
        )?;
        if relative_path.as_os_str().is_empty() {
            continue;
        }
        if handle_whiteout(rootfs_path, &relative_path)? {
            continue;
        }
        entry
            .unpack_in(rootfs_path)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to unpack OCI layer entry {:?} from {} into {}: {error}",
                    relative_path,
                    layer_path.display(),
                    rootfs_path.display()
                ),
            })?;
    }
    Ok(())
}

fn open_layer_reader(layer_path: &Path) -> Result<Box<dyn Read>> {
    let file = fs::File::open(layer_path).map_err(|error| SandboxError::OperationFailed {
        message: format!("failed to open OCI layer {}: {error}", layer_path.display()),
    })?;
    let reader = BufReader::new(file);
    match detect_layer_compression(layer_path)? {
        LayerCompression::None => Ok(Box::new(reader)),
        LayerCompression::Gzip => Ok(Box::new(GzDecoder::new(reader))),
        LayerCompression::Zstd => {
            let decoder = zstd::stream::read::Decoder::with_buffer(reader).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!(
                        "failed to initialize zstd decoder for {}: {error}",
                        layer_path.display()
                    ),
                }
            })?;
            Ok(Box::new(decoder))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LayerCompression {
    None,
    Gzip,
    Zstd,
}

fn detect_layer_compression(layer_path: &Path) -> Result<LayerCompression> {
    let mut file = fs::File::open(layer_path).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to open OCI layer {} for compression detection: {error}",
            layer_path.display()
        ),
    })?;
    let mut header = [0_u8; 4];
    let read = file
        .read(&mut header)
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to read OCI layer {} for compression detection: {error}",
                layer_path.display()
            ),
        })?;
    if read >= 2 && header[..2] == [0x1f, 0x8b] {
        return Ok(LayerCompression::Gzip);
    }
    if read >= 4 && header == [0x28, 0xb5, 0x2f, 0xfd] {
        return Ok(LayerCompression::Zstd);
    }
    Ok(LayerCompression::None)
}

fn sanitize_archive_path(path: &Path) -> Result<PathBuf> {
    let mut sanitized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => sanitized.push(part),
            Component::RootDir | Component::ParentDir | Component::Prefix(_) => {
                return Err(SandboxError::OperationFailed {
                    message: format!("OCI layer path {:?} escapes the target rootfs", path),
                });
            }
        }
    }
    Ok(sanitized)
}

fn handle_whiteout(rootfs_path: &Path, relative_path: &Path) -> Result<bool> {
    let Some(file_name) = relative_path.file_name().and_then(|name| name.to_str()) else {
        return Ok(false);
    };
    if file_name == ".wh..wh..opq" {
        let parent = relative_path.parent().map(|path| rootfs_path.join(path));
        if let Some(parent) = parent {
            clear_directory_contents(&parent)?;
        }
        return Ok(true);
    }
    let Some(target_name) = file_name.strip_prefix(".wh.") else {
        return Ok(false);
    };
    let parent = relative_path.parent().unwrap_or_else(|| Path::new(""));
    remove_path_if_exists(&rootfs_path.join(parent).join(target_name))?;
    Ok(true)
}

fn clear_directory_contents(path: &Path) -> Result<()> {
    if !path.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(path).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to read opaque directory {}: {error}",
            path.display()
        ),
    })? {
        let entry = entry.map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to iterate opaque directory {}: {error}",
                path.display()
            ),
        })?;
        remove_path_if_exists(&entry.path())?;
    }
    Ok(())
}

fn remove_path_if_exists(path: &Path) -> Result<()> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to remove directory {}: {error}", path.display()),
        })
    } else {
        fs::remove_file(path).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to remove file {}: {error}", path.display()),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use flate2::Compression;
    use flate2::write::GzEncoder;
    use sha2::{Digest, Sha256};
    use tempfile::TempDir;

    use super::{OciImageMaterializer, current_oci_architectures};
    use crate::backends::oci::buildah::OciExposedPortProtocol;
    use crate::instance::SandboxId;
    use crate::spec::SandboxImageProcessOverrides;

    #[test]
    fn materializer_pulls_and_extracts_image_rootfs_from_registry() {
        let temp_dir = TempDir::new().expect("tempdir should build");
        let layer_body = build_layer_archive();
        let registry = serve_fake_oci_registry(layer_body);
        let materializer = OciImageMaterializer::under_state_root(temp_dir.path());

        let prepared = materializer
            .prepare_image_launch(
                &SandboxId::new("db-01"),
                &registry,
                &SandboxImageProcessOverrides::default(),
            )
            .expect("image should materialize");

        assert_eq!(
            prepared.launch_defaults.process.args,
            vec!["/usr/bin/demo".to_owned(), "--serve".to_owned()]
        );
        assert_eq!(
            prepared.launch_defaults.process.env,
            vec!["PATH=/usr/bin".to_owned(), "PORT=8080".to_owned()]
        );
        assert_eq!(prepared.launch_defaults.user.as_deref(), Some("1000:1000"));
        assert_eq!(prepared.launch_defaults.exposed_ports.len(), 1);
        assert_eq!(
            prepared.launch_defaults.exposed_ports[0].protocol,
            OciExposedPortProtocol::Tcp
        );
        assert!(
            prepared.artifact.rootfs_path.join("usr/bin/demo").is_file(),
            "expected extracted image payload in the materialized rootfs"
        );
    }

    #[test]
    fn materializer_can_run_inside_an_existing_tokio_runtime() {
        let temp_dir = TempDir::new().expect("tempdir should build");
        let layer_body = build_layer_archive();
        let registry = serve_fake_oci_registry(layer_body);
        let materializer = OciImageMaterializer::under_state_root(temp_dir.path());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime should build");

        let prepared = runtime
            .block_on(async {
                materializer.prepare_image_launch(
                    &SandboxId::new("db-02"),
                    &registry,
                    &SandboxImageProcessOverrides::default(),
                )
            })
            .expect("image should materialize from within an existing runtime");

        assert!(
            prepared.artifact.rootfs_path.join("usr/bin/demo").is_file(),
            "runtime-safe materialization should still extract the image payload"
        );
    }

    fn build_layer_archive() -> Vec<u8> {
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut builder = tar::Builder::new(encoder);

        write_tar_file(
            &mut builder,
            "etc/passwd",
            b"demo:x:1000:1000:demo:/home/demo:/bin/sh\n",
            0o644,
        );
        write_tar_file(&mut builder, "etc/group", b"demo:x:1000:\n", 0o644);
        write_tar_file(
            &mut builder,
            "usr/bin/demo",
            b"#!/bin/sh\nexec sleep 60\n",
            0o755,
        );

        let encoder = builder.into_inner().expect("tar encoder should finish");
        encoder.finish().expect("gzip layer should finish")
    }

    fn write_tar_file(
        builder: &mut tar::Builder<GzEncoder<Vec<u8>>>,
        path: &str,
        body: &[u8],
        mode: u32,
    ) {
        let mut header = tar::Header::new_gnu();
        header.set_mode(mode);
        header.set_size(body.len() as u64);
        header.set_cksum();
        builder
            .append_data(&mut header, path, Cursor::new(body))
            .expect("layer entry should append");
    }

    fn serve_fake_oci_registry(layer_body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("registry listener should bind");
        let address = listener
            .local_addr()
            .expect("registry listener should report local addr");

        let config = serde_json::json!({
            "config": {
                "Entrypoint": ["/usr/bin/demo"],
                "Cmd": ["--serve"],
                "Env": ["PATH=/usr/bin", "PORT=8080"],
                "User": "demo",
                "WorkingDir": "/workspace",
                "ExposedPorts": {
                    "8080/tcp": {}
                },
                "Labels": {
                    "app": "demo"
                }
            }
        });
        let config_bytes = serde_json::to_vec(&config).expect("config should serialize");
        let config_digest = format!("sha256:{:x}", Sha256::digest(&config_bytes));
        let layer_digest = format!("sha256:{:x}", Sha256::digest(&layer_body));
        let child_manifest = serde_json::json!({
            "schemaVersion": 2,
            "config": {
                "mediaType": "application/vnd.oci.image.config.v1+json",
                "size": config_bytes.len(),
                "digest": config_digest
            },
            "layers": [{
                "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
                "size": layer_body.len(),
                "digest": layer_digest
            }]
        });
        let child_manifest_bytes =
            serde_json::to_vec(&child_manifest).expect("child manifest should serialize");
        let child_manifest_digest = format!("sha256:{:x}", Sha256::digest(&child_manifest_bytes));
        let index_manifest = serde_json::json!({
            "schemaVersion": 2,
            "manifests": [{
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "size": child_manifest_bytes.len(),
                "digest": child_manifest_digest,
                "platform": {
                    "architecture": current_oci_architectures()[0],
                    "os": "linux"
                }
            }]
        });
        let index_manifest_bytes =
            serde_json::to_vec(&index_manifest).expect("index manifest should serialize");

        thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.expect("registry connection should accept");
                let mut buffer = [0_u8; 4096];
                let read = stream
                    .read(&mut buffer)
                    .expect("registry request should read");
                let request = String::from_utf8_lossy(&buffer[..read]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/");

                let (status, body) = match path {
                    "/v2/" => (200, Vec::new()),
                    "/v2/library/demo/manifests/latest" => (200, index_manifest_bytes.clone()),
                    _ if path == format!("/v2/library/demo/manifests/{child_manifest_digest}") => {
                        (200, child_manifest_bytes.clone())
                    }
                    _ if path == format!("/v2/library/demo/blobs/{config_digest}") => {
                        (200, config_bytes.clone())
                    }
                    _ if path == format!("/v2/library/demo/blobs/{layer_digest}") => {
                        (200, layer_body.clone())
                    }
                    _ => (404, Vec::new()),
                };

                let response = format!(
                    "HTTP/1.1 {status} {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    if status == 200 { "OK" } else { "Not Found" },
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("registry response head should write");
                stream
                    .write_all(&body)
                    .expect("registry response body should write");
            }
        });

        format!("localhost:{}/library/demo:latest", address.port())
    }
}
