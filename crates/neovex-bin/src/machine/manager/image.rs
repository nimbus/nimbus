use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use flate2::read::GzDecoder;
use neovex::Error;
use oci_client::Reference;
use oci_client::client::{Client as OciClient, ClientConfig as OciClientConfig, ClientProtocol};
use oci_client::manifest::{
    IMAGE_MANIFEST_LIST_MEDIA_TYPE, IMAGE_MANIFEST_MEDIA_TYPE, OCI_IMAGE_INDEX_MEDIA_TYPE,
    OCI_IMAGE_MEDIA_TYPE, OciDescriptor,
};
use oci_client::secrets::RegistryAuth;
use reqwest::blocking::Client as BlockingClient;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;

use crate::cli_ux;

use super::super::record::{MachineImageFormat, MachineImageSource};
use super::{
    HTTP_IMAGE_TIMEOUT, MachinePaths, OCI_ANNOTATION_MACHINE_ATTESTATION_REPOSITORY,
    OCI_ANNOTATION_MACHINE_NEOVEX_VERSION, OCI_ANNOTATION_SOURCE, OCI_ANNOTATION_TITLE,
    OCI_MACHINE_OS, emit_machine_info, emit_machine_warning,
};

#[derive(Debug, Deserialize)]
struct RegistryImageIndex {
    manifests: Vec<RegistryManifestDescriptor>,
}

#[derive(Debug, Clone, Deserialize)]
struct RegistryManifestDescriptor {
    digest: String,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
    platform: Option<RegistryPlatform>,
}

#[derive(Debug, Clone, Deserialize)]
struct RegistryPlatform {
    architecture: String,
    os: String,
}

#[derive(Debug, Deserialize)]
struct RegistryImageManifest {
    layers: Vec<RegistryLayerDescriptor>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RegistryLayerDescriptor {
    digest: String,
    size: i64,
    #[serde(rename = "mediaType")]
    media_type: String,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct MachineArtifactMetadata {
    pub(super) attestation_repository: Option<String>,
    pub(super) source_repository_url: Option<String>,
    pub(super) neovex_version: Option<String>,
}

#[derive(Debug, Clone)]
struct SelectedMachineArtifact {
    child_reference: Reference,
    layer: RegistryLayerDescriptor,
    metadata: MachineArtifactMetadata,
}

pub(super) fn resolve_bootable_image_path(
    paths: &MachinePaths,
    image_source: &MachineImageSource,
    provider: super::super::MachineProvider,
) -> Result<PathBuf, Error> {
    let image_format = provider.image_format();
    ensure_image_materialization_supported(image_format)?;
    match image_source {
        MachineImageSource::LocalDisk { path } => {
            if !path.is_file() {
                return Err(Error::InvalidInput(format!(
                    "machine guest image {} does not exist",
                    path.display()
                )));
            }
            Ok(path.clone())
        }
        MachineImageSource::OciReference { reference } => {
            if paths.materialized_image_path.is_file() {
                return Ok(paths.materialized_image_path.clone());
            }
            materialize_oci_image(paths, reference, provider)
        }
        MachineImageSource::HttpUrl { url } => {
            if paths.materialized_image_path.is_file() {
                return Ok(paths.materialized_image_path.clone());
            }
            materialize_http_image(paths, url)
        }
    }
}

fn ensure_image_materialization_supported(image_format: MachineImageFormat) -> Result<(), Error> {
    match image_format {
        MachineImageFormat::Raw => Ok(()),
        MachineImageFormat::Tar => Err(Error::InvalidInput(
            "the current machine manager can only materialize raw-disk guest images".to_owned(),
        )),
    }
}

fn materialize_http_image(paths: &MachinePaths, url: &str) -> Result<PathBuf, Error> {
    fs::create_dir_all(&paths.image_cache_dir).map_err(|error| {
        Error::Internal(format!(
            "failed to create machine image cache directory {}: {error}",
            paths.image_cache_dir.display()
        ))
    })?;
    ensure_materialized_image_parent(&paths.materialized_image_path)?;

    let image_cache_dir = paths.image_cache_dir.clone();
    let url = url.to_owned();
    let download_url = url.clone();
    let download = run_blocking_in_thread("machine HTTP image download", move || {
        let download = NamedTempFile::new_in(&image_cache_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to allocate temporary download file under {}: {error}",
                image_cache_dir.display()
            ))
        })?;
        let client = BlockingClient::builder()
            .timeout(HTTP_IMAGE_TIMEOUT)
            .build()
            .map_err(|error| Error::Internal(format!("failed to build HTTP client: {error}")))?;
        let response = client
            .get(&download_url)
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|error| {
                Error::InvalidInput(format!(
                    "failed to download machine guest image from {download_url}: {error}"
                ))
            })?;
        let mut progress =
            cli_ux::ByteProgress::new("Downloading machine image", response.content_length())
                .map_err(|error| {
                    Error::Internal(format!("failed to initialize progress output: {error}"))
                })?;

        let mut writer = download.reopen().map_err(|error| {
            Error::Internal(format!(
                "failed to reopen temporary download file under {}: {error}",
                image_cache_dir.display()
            ))
        })?;
        let mut reader = progress.wrap_read(response);
        io::copy(&mut reader, &mut writer).map_err(|error| {
            Error::Internal(format!(
                "failed to write downloaded machine image from {download_url} into {}: {error}",
                image_cache_dir.display()
            ))
        })?;
        progress.finish();
        writer.flush().map_err(|error| {
            Error::Internal(format!(
                "failed to flush downloaded machine image for {download_url}: {error}"
            ))
        })?;
        drop(writer);
        Ok(download)
    })?;

    let temp_output = NamedTempFile::new_in(&paths.image_cache_dir).map_err(|error| {
        Error::Internal(format!(
            "failed to allocate temporary materialization file under {}: {error}",
            paths.image_cache_dir.display()
        ))
    })?;

    if url.ends_with(".gz") {
        let input = download.reopen().map_err(|error| {
            Error::Internal(format!(
                "failed to reopen temporary download file for gzip decode: {error}"
            ))
        })?;
        let mut progress = cli_ux::ByteProgress::new(
            "Extracting compressed machine image",
            Some(file_size(download.path()).map_err(|error| {
                Error::Internal(format!(
                    "failed to determine downloaded machine image size for gzip decode: {error}"
                ))
            })?),
        )
        .map_err(|error| {
            Error::Internal(format!("failed to initialize progress output: {error}"))
        })?;
        let reader = progress.wrap_read(BufReader::new(input));
        let mut decoder = GzDecoder::new(reader);
        let mut output = temp_output.reopen().map_err(|error| {
            Error::Internal(format!(
                "failed to reopen temporary materialization file for gzip decode: {error}"
            ))
        })?;
        io::copy(&mut decoder, &mut output).map_err(|error| {
            Error::Internal(format!(
                "failed to decompress gzip machine image from {url}: {error}"
            ))
        })?;
        progress.finish();
        output.flush().map_err(|error| {
            Error::Internal(format!(
                "failed to flush decompressed machine image for {url}: {error}"
            ))
        })?;
    } else {
        let input = fs::File::open(download.path()).map_err(|error| {
            Error::Internal(format!(
                "failed to reopen temporary download file for materialization: {error}"
            ))
        })?;
        let mut output = temp_output.reopen().map_err(|error| {
            Error::Internal(format!(
                "failed to reopen temporary materialization file for raw copy: {error}"
            ))
        })?;
        let mut progress = cli_ux::ByteProgress::new(
            "Materializing machine disk",
            Some(file_size(download.path()).map_err(|error| {
                Error::Internal(format!(
                    "failed to determine downloaded machine image size for materialization: {error}"
                ))
            })?),
        )
        .map_err(|error| {
            Error::Internal(format!("failed to initialize progress output: {error}"))
        })?;
        let mut reader = progress.wrap_read(BufReader::new(input));
        io::copy(&mut reader, &mut output).map_err(|error| {
            Error::Internal(format!(
                "failed to stage downloaded machine image from {url}: {error}"
            ))
        })?;
        progress.finish();
        output.flush().map_err(|error| {
            Error::Internal(format!(
                "failed to flush materialized machine image for {url}: {error}"
            ))
        })?;
    }

    temp_output
        .persist(&paths.materialized_image_path)
        .map_err(|error| {
            Error::Internal(format!(
                "failed to persist machine image from {url} into {}: {}",
                paths.materialized_image_path.display(),
                error.error
            ))
        })?;

    Ok(paths.materialized_image_path.clone())
}

fn materialize_oci_image(
    paths: &MachinePaths,
    reference: &str,
    provider: super::super::MachineProvider,
) -> Result<PathBuf, Error> {
    fs::create_dir_all(&paths.image_cache_dir).map_err(|error| {
        Error::Internal(format!(
            "failed to create machine image cache directory {}: {error}",
            paths.image_cache_dir.display()
        ))
    })?;
    ensure_materialized_image_parent(&paths.materialized_image_path)?;

    let cache_dir = paths.image_cache_dir.clone();
    let reference = reference.to_owned();
    let source_label = format!("published OCI artifact '{reference}'");
    let reference_for_pull = reference.clone();
    let cached_blob_path = run_async_in_thread(move || async move {
        pull_oci_artifact_to_cache(cache_dir, reference_for_pull, provider).await
    })?;

    materialize_cached_disk(
        &cached_blob_path,
        &paths.materialized_image_path,
        &source_label,
    )?;
    Ok(paths.materialized_image_path.clone())
}

async fn pull_oci_artifact_to_cache(
    image_cache_dir: PathBuf,
    reference: String,
    provider: super::super::MachineProvider,
) -> Result<PathBuf, Error> {
    let stripped_reference = strip_docker_reference_prefix(&reference);
    let registry_reference = Reference::try_from(stripped_reference.as_str()).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to parse machine guest OCI reference '{reference}': {error}"
        ))
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
        .map_err(|error| {
            Error::InvalidInput(format!(
                "failed to resolve machine guest OCI reference '{reference}': {error}"
            ))
        })?;

    let selected_artifact =
        select_oci_artifact_layer(&reference, &top_manifest_bytes, &client, &auth, provider)
            .await?;
    let cache_path = image_cache_dir.join(cached_oci_blob_file_name(&selected_artifact.layer));
    if cache_path.is_file() {
        return Ok(cache_path);
    }

    let download_path = image_cache_dir.join(format!(
        "{}.download",
        digest_hex(&selected_artifact.layer.digest)?
    ));
    if download_path.exists() {
        fs::remove_file(&download_path).map_err(|error| {
            Error::Internal(format!(
                "failed to remove stale machine image download {}: {error}",
                download_path.display()
            ))
        })?;
    }

    let output = tokio::fs::File::create(&download_path)
        .await
        .map_err(|error| {
            Error::Internal(format!(
                "failed to create temporary machine image download {}: {error}",
                download_path.display()
            ))
        })?;
    let mut progress = cli_ux::ByteProgress::new(
        "Pulling machine image",
        u64::try_from(selected_artifact.layer.size).ok(),
    )
    .map_err(|error| Error::Internal(format!("failed to initialize progress output: {error}")))?;
    let mut output = progress.wrap_async_write(output);
    let layer = to_oci_descriptor(&selected_artifact.layer);
    client
        .pull_blob(&selected_artifact.child_reference, &layer, &mut output)
        .await
        .map_err(|error| {
            Error::InvalidInput(format!(
                "failed to download machine guest OCI artifact '{}': {error}",
                reference
            ))
        })?;
    progress.finish();
    output.flush().await.map_err(|error| {
        Error::Internal(format!(
            "failed to flush downloaded machine guest OCI artifact '{}': {error}",
            reference
        ))
    })?;
    output.shutdown().await.map_err(|error| {
        Error::Internal(format!(
            "failed to close downloaded machine guest OCI artifact '{}': {error}",
            reference
        ))
    })?;
    drop(output);

    verify_downloaded_oci_blob(&download_path, &selected_artifact.layer)?;
    log_machine_artifact_metadata(&reference, &selected_artifact.metadata);
    check_build_attestation(
        &reference,
        &selected_artifact.layer.digest,
        selected_artifact.metadata.attestation_repository.as_deref(),
    );
    fs::rename(&download_path, &cache_path).map_err(|error| {
        Error::Internal(format!(
            "failed to persist machine guest OCI artifact cache {}: {error}",
            cache_path.display()
        ))
    })?;

    Ok(cache_path)
}

async fn select_oci_artifact_layer(
    reference: &str,
    top_manifest_bytes: &[u8],
    client: &OciClient,
    auth: &RegistryAuth,
    provider: super::super::MachineProvider,
) -> Result<SelectedMachineArtifact, Error> {
    if let Ok(index) = serde_json::from_slice::<RegistryImageIndex>(top_manifest_bytes) {
        let manifest_descriptor =
            select_oci_manifest_descriptor(reference, &index.manifests, provider)?.clone();
        let child_reference = build_digest_reference(reference, &manifest_descriptor.digest)?;
        let (child_manifest_bytes, _) = client
            .pull_manifest_raw(
                &child_reference,
                auth,
                &[OCI_IMAGE_MEDIA_TYPE, IMAGE_MANIFEST_MEDIA_TYPE],
            )
            .await
            .map_err(|error| {
                Error::InvalidInput(format!(
                    "failed to pull machine guest OCI child manifest '{}': {error}",
                    manifest_descriptor.digest
                ))
            })?;
        let child_manifest = serde_json::from_slice::<RegistryImageManifest>(&child_manifest_bytes)
            .map_err(|error| {
                Error::Internal(format!(
                    "failed to parse machine guest OCI child manifest '{}': {error}",
                    manifest_descriptor.digest
                ))
            })?;
        let layer = select_machine_layer(reference, &child_manifest.layers)?;
        return Ok(SelectedMachineArtifact {
            child_reference,
            layer: layer.clone(),
            metadata: machine_artifact_metadata_from_annotations(
                Some(&manifest_descriptor.annotations),
                Some(&child_manifest.annotations),
            ),
        });
    }

    let image_manifest = serde_json::from_slice::<RegistryImageManifest>(top_manifest_bytes)
        .map_err(|error| {
            Error::Internal(format!(
                "failed to parse machine guest OCI manifest '{}': {error}",
                reference
            ))
        })?;
    let layer = select_machine_layer(reference, &image_manifest.layers)?;
    let registry_reference = Reference::try_from(strip_docker_reference_prefix(reference).as_str())
        .map_err(|error| {
            Error::InvalidInput(format!(
                "failed to parse machine guest OCI reference '{reference}': {error}"
            ))
        })?;
    Ok(SelectedMachineArtifact {
        child_reference: registry_reference,
        layer: layer.clone(),
        metadata: machine_artifact_metadata_from_annotations(
            Some(&image_manifest.annotations),
            None,
        ),
    })
}

fn build_oci_client(reference: &str) -> Result<OciClient, Error> {
    let mut config = OciClientConfig::default();
    if is_loopback_registry(reference) {
        config.protocol = ClientProtocol::Http;
    }
    OciClient::try_from(config).map_err(|error| {
        Error::Internal(format!(
            "failed to initialize OCI client for machine image '{reference}': {error}"
        ))
    })
}

fn is_loopback_registry(reference: &str) -> bool {
    let stripped_reference = strip_docker_reference_prefix(reference);
    let host = stripped_reference.split('/').next().unwrap_or_default();
    host.starts_with("localhost") || host.starts_with("127.0.0.1") || host.starts_with("[::1]")
}

fn strip_docker_reference_prefix(reference: &str) -> String {
    reference
        .strip_prefix("docker://")
        .unwrap_or(reference)
        .to_owned()
}

fn select_oci_manifest_descriptor<'a>(
    reference: &str,
    manifests: &'a [RegistryManifestDescriptor],
    provider: super::super::MachineProvider,
) -> Result<&'a RegistryManifestDescriptor, Error> {
    let disk_type = provider.oci_artifact_disk_type();
    manifests
        .iter()
        .find(|descriptor| {
            let Some(platform) = descriptor.platform.as_ref() else {
                return false;
            };
            platform.os == OCI_MACHINE_OS
                && current_machine_oci_architectures()
                    .iter()
                    .any(|arch| platform.architecture == *arch)
                && descriptor
                    .annotations
                    .get("disktype")
                    .map(|value| value == disk_type)
                    .unwrap_or(false)
        })
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "machine guest OCI reference '{}' does not contain a linux/{:?} '{}' disk artifact",
                reference,
                current_machine_oci_architectures(),
                disk_type
            ))
        })
}

fn select_machine_layer<'a>(
    reference: &str,
    layers: &'a [RegistryLayerDescriptor],
) -> Result<&'a RegistryLayerDescriptor, Error> {
    match layers {
        [layer] => Ok(layer),
        [] => Err(Error::InvalidInput(format!(
            "machine guest OCI reference '{}' has no disk layers",
            reference
        ))),
        _ => Err(Error::InvalidInput(format!(
            "machine guest OCI reference '{}' has {} disk layers; expected exactly 1",
            reference,
            layers.len()
        ))),
    }
}

pub(super) fn current_machine_oci_architectures() -> &'static [&'static str] {
    #[cfg(target_arch = "aarch64")]
    {
        &["aarch64", "arm64"]
    }
    #[cfg(target_arch = "x86_64")]
    {
        &["x86_64", "amd64"]
    }
}

fn build_digest_reference(reference: &str, digest: &str) -> Result<Reference, Error> {
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
                (None, Some(_colon)) if !reference.contains('/') => reference.clone(),
                _ => reference.clone(),
            }
        });
    Reference::try_from(format!("{repository}@{digest}")).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to build machine guest OCI digest reference '{repository}@{digest}': {error}"
        ))
    })
}

fn cached_oci_blob_file_name(layer: &RegistryLayerDescriptor) -> String {
    let digest = digest_hex(&layer.digest).unwrap_or_else(|_| "machine-image".to_owned());
    let suffix = layer
        .annotations
        .get(OCI_ANNOTATION_TITLE)
        .and_then(|title| oci_artifact_suffix(title))
        .unwrap_or(".blob");
    format!("{digest}{suffix}")
}

fn oci_artifact_suffix(title: &str) -> Option<&str> {
    [
        ".raw.zst",
        ".raw.gz",
        ".raw",
        ".qcow2.xz",
        ".qcow2.gz",
        ".qcow2",
    ]
    .into_iter()
    .find(|suffix| title.ends_with(suffix))
}

fn verify_downloaded_oci_blob(path: &Path, layer: &RegistryLayerDescriptor) -> Result<(), Error> {
    let metadata = fs::metadata(path).map_err(|error| {
        Error::Internal(format!(
            "failed to stat downloaded machine guest OCI artifact {}: {error}",
            path.display()
        ))
    })?;
    if metadata.len() != layer.size as u64 {
        return Err(Error::InvalidInput(format!(
            "downloaded machine guest OCI artifact {} has size {}, expected {}",
            path.display(),
            metadata.len(),
            layer.size
        )));
    }
    let digest = compute_sha256(path)?;
    let expected = digest_hex(&layer.digest)?;
    if digest != expected {
        return Err(Error::InvalidInput(format!(
            "downloaded machine guest OCI artifact {} has sha256 {}, expected {}",
            path.display(),
            digest,
            expected
        )));
    }
    Ok(())
}

pub(super) fn machine_artifact_metadata_from_annotations(
    primary: Option<&BTreeMap<String, String>>,
    fallback: Option<&BTreeMap<String, String>>,
) -> MachineArtifactMetadata {
    MachineArtifactMetadata {
        attestation_repository: annotation_value(
            primary,
            fallback,
            OCI_ANNOTATION_MACHINE_ATTESTATION_REPOSITORY,
        ),
        source_repository_url: annotation_value(primary, fallback, OCI_ANNOTATION_SOURCE),
        neovex_version: annotation_value(primary, fallback, OCI_ANNOTATION_MACHINE_NEOVEX_VERSION),
    }
}

fn annotation_value(
    primary: Option<&BTreeMap<String, String>>,
    fallback: Option<&BTreeMap<String, String>>,
    key: &str,
) -> Option<String> {
    primary
        .and_then(|annotations| annotations.get(key))
        .or_else(|| fallback.and_then(|annotations| annotations.get(key)))
        .filter(|value| !value.is_empty())
        .cloned()
}

fn log_machine_artifact_metadata(reference: &str, metadata: &MachineArtifactMetadata) {
    if let Some(neovex_version) = metadata.neovex_version.as_deref() {
        emit_machine_info(format!(
            "machine image '{reference}' embeds neovex {neovex_version}"
        ));
    }
    if let Some(source_repository_url) = metadata.source_repository_url.as_deref() {
        emit_machine_info(format!(
            "machine image '{reference}' source={source_repository_url}"
        ));
    }
}

const NEOVEX_SOURCE_REPO: &str = "agentstation/neovex";

fn check_build_attestation(
    reference: &str,
    subject_digest: &str,
    explicit_repository: Option<&str>,
) {
    let stripped = strip_docker_reference_prefix(reference);
    let Some(image_repo) = extract_ghcr_repo_path(&stripped) else {
        return;
    };

    let subject_digest = subject_digest.to_owned();
    let explicit_repository = explicit_repository.map(ToOwned::to_owned);
    let _ = run_blocking_in_thread("machine build attestation lookup", move || {
        let repos_to_check = attestation_repositories_for_reference(
            &image_repo,
            explicit_repository
                .as_deref()
                .filter(|repo| !repo.is_empty()),
        );

        let client = match BlockingClient::builder()
            .timeout(Duration::from_secs(10))
            .build()
        {
            Ok(client) => client,
            Err(error) => {
                emit_machine_warning(format!("attestation lookup failed: {error}"));
                return Ok(());
            }
        };

        for repo in &repos_to_check {
            match query_attestations(&client, repo, &subject_digest) {
                Ok(count) if count > 0 => {
                    let _ = cli_ux::write_stderr_prefixed_line(
                        "verified:",
                        &format!(
                            "{count} build attestation(s) found for {subject_digest} in {repo}"
                        ),
                    );
                    return Ok(());
                }
                Ok(_) => {}
                Err(msg) => {
                    emit_machine_warning(format!("attestation lookup for {repo}: {msg}"));
                }
            }
        }

        emit_machine_warning(format!("no build attestations found for {subject_digest}"));
        Ok(())
    });
}

pub(super) fn run_blocking_in_thread<F, T>(label: &'static str, work: F) -> Result<T, Error>
where
    F: FnOnce() -> Result<T, Error> + Send + 'static,
    T: Send + 'static,
{
    thread::spawn(work)
        .join()
        .map_err(|_| Error::Internal(format!("{label} worker panicked")))?
}

fn query_attestations(
    client: &BlockingClient,
    repo: &str,
    subject_digest: &str,
) -> Result<usize, String> {
    let url = format!("https://api.github.com/repos/{repo}/attestations/{subject_digest}");

    let response = client
        .get(&url)
        .header("Accept", "application/json")
        .header("User-Agent", "neovex-machine-manager")
        .send()
        .map_err(|e| format!("{e}"))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let body: serde_json::Value = response.json().map_err(|e| format!("{e}"))?;

    Ok(body
        .get("attestations")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0))
}

pub(super) fn attestation_repositories_for_reference(
    image_repo: &str,
    explicit_repository: Option<&str>,
) -> Vec<String> {
    if let Some(explicit_repository) = explicit_repository {
        return vec![explicit_repository.to_owned()];
    }

    if image_repo == NEOVEX_SOURCE_REPO {
        vec![image_repo.to_owned()]
    } else {
        vec![image_repo.to_owned(), NEOVEX_SOURCE_REPO.to_owned()]
    }
}

fn extract_ghcr_repo_path(reference: &str) -> Option<String> {
    let without_host = reference.strip_prefix("ghcr.io/")?;
    let without_tag = without_host
        .split_once('@')
        .map(|(r, _)| r)
        .unwrap_or(without_host);
    let without_tag = without_tag
        .split_once(':')
        .map(|(r, _)| r)
        .unwrap_or(without_tag);
    let parts: Vec<&str> = without_tag.splitn(3, '/').collect();
    if parts.len() >= 2 {
        Some(format!("{}/{}", parts[0], parts[1]))
    } else {
        None
    }
}

pub(super) fn compute_sha256(path: &Path) -> Result<String, Error> {
    let mut reader = BufReader::new(fs::File::open(path).map_err(|error| {
        Error::Internal(format!(
            "failed to open {} for sha256 verification: {error}",
            path.display()
        ))
    })?);
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer).map_err(|error| {
            Error::Internal(format!(
                "failed to read {} for sha256 verification: {error}",
                path.display()
            ))
        })?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn digest_hex(digest: &str) -> Result<String, Error> {
    let (algorithm, hex) = digest.split_once(':').ok_or_else(|| {
        Error::InvalidInput(format!(
            "invalid OCI digest '{}': missing algorithm prefix",
            digest
        ))
    })?;
    if algorithm != "sha256" {
        return Err(Error::InvalidInput(format!(
            "unsupported OCI digest algorithm '{}'; expected sha256",
            algorithm
        )));
    }
    Ok(hex.to_owned())
}

pub(super) fn materialize_cached_disk(
    source_path: &Path,
    output_path: &Path,
    source_label: &str,
) -> Result<(), Error> {
    ensure_materialized_image_parent(output_path)?;
    let temp_output = NamedTempFile::new_in(output_path.parent().ok_or_else(|| {
        Error::Internal(format!("{} has no parent directory", output_path.display()))
    })?)
    .map_err(|error| {
        Error::Internal(format!(
            "failed to allocate temporary materialization file for {}: {error}",
            source_label
        ))
    })?;

    let compression = detect_disk_compression(source_path)?;
    match compression {
        DiskCompression::None => {
            let input = fs::File::open(source_path).map_err(|error| {
                Error::Internal(format!(
                    "failed to open {} for materialization: {error}",
                    source_path.display()
                ))
            })?;
            let mut output = temp_output.reopen().map_err(|error| {
                Error::Internal(format!(
                    "failed to reopen {} for materialization: {error}",
                    temp_output.path().display()
                ))
            })?;
            let mut progress = cli_ux::ByteProgress::new(
                "Materializing machine disk",
                Some(file_size(source_path).map_err(|error| {
                    Error::Internal(format!(
                        "failed to determine {} size for materialization: {error}",
                        source_path.display()
                    ))
                })?),
            )
            .map_err(|error| {
                Error::Internal(format!("failed to initialize progress output: {error}"))
            })?;
            let mut reader = progress.wrap_read(BufReader::new(input));
            io::copy(&mut reader, &mut output).map_err(|error| {
                Error::Internal(format!(
                    "failed to stage {} into {}: {error}",
                    source_label,
                    temp_output.path().display()
                ))
            })?;
            progress.finish();
            output.flush().map_err(|error| {
                Error::Internal(format!(
                    "failed to flush materialized {}: {error}",
                    source_label
                ))
            })?;
        }
        DiskCompression::Gzip => {
            let input = fs::File::open(source_path).map_err(|error| {
                Error::Internal(format!(
                    "failed to open {} for gzip decode: {error}",
                    source_path.display()
                ))
            })?;
            let mut progress = cli_ux::ByteProgress::new(
                "Extracting compressed machine image",
                Some(file_size(source_path).map_err(|error| {
                    Error::Internal(format!(
                        "failed to determine {} size for gzip decode: {error}",
                        source_path.display()
                    ))
                })?),
            )
            .map_err(|error| {
                Error::Internal(format!("failed to initialize progress output: {error}"))
            })?;
            let reader = progress.wrap_read(BufReader::new(input));
            let mut decoder = GzDecoder::new(reader);
            let mut output = temp_output.reopen().map_err(|error| {
                Error::Internal(format!(
                    "failed to reopen {} for gzip decode: {error}",
                    temp_output.path().display()
                ))
            })?;
            io::copy(&mut decoder, &mut output).map_err(|error| {
                Error::Internal(format!(
                    "failed to decompress gzip {}: {error}",
                    source_label
                ))
            })?;
            progress.finish();
            output.flush().map_err(|error| {
                Error::Internal(format!(
                    "failed to flush decompressed {}: {error}",
                    source_label
                ))
            })?;
        }
        DiskCompression::Zstd => {
            let input = fs::File::open(source_path).map_err(|error| {
                Error::Internal(format!(
                    "failed to open {} for zstd decode: {error}",
                    source_path.display()
                ))
            })?;
            let mut progress = cli_ux::ByteProgress::new(
                "Extracting compressed machine image",
                Some(file_size(source_path).map_err(|error| {
                    Error::Internal(format!(
                        "failed to determine {} size for zstd decode: {error}",
                        source_path.display()
                    ))
                })?),
            )
            .map_err(|error| {
                Error::Internal(format!("failed to initialize progress output: {error}"))
            })?;
            let reader = progress.wrap_read(BufReader::new(input));
            let mut output = temp_output.reopen().map_err(|error| {
                Error::Internal(format!(
                    "failed to reopen {} for zstd decode: {error}",
                    temp_output.path().display()
                ))
            })?;
            zstd::stream::copy_decode(reader, &mut output).map_err(|error| {
                Error::Internal(format!(
                    "failed to decompress zstd {}: {error}",
                    source_label
                ))
            })?;
            progress.finish();
            output.flush().map_err(|error| {
                Error::Internal(format!(
                    "failed to flush decompressed {}: {error}",
                    source_label
                ))
            })?;
        }
    }

    temp_output.persist(output_path).map_err(|error| {
        Error::Internal(format!(
            "failed to persist materialized machine image {}: {}",
            output_path.display(),
            error.error
        ))
    })?;
    Ok(())
}

pub(super) fn file_size(path: &Path) -> io::Result<u64> {
    fs::metadata(path).map(|metadata| metadata.len())
}

fn ensure_materialized_image_parent(output_path: &Path) -> Result<(), Error> {
    let parent = output_path.parent().ok_or_else(|| {
        Error::Internal(format!("{} has no parent directory", output_path.display()))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        Error::Internal(format!(
            "failed to create machine image data directory {}: {error}",
            parent.display()
        ))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiskCompression {
    None,
    Gzip,
    Zstd,
}

fn detect_disk_compression(path: &Path) -> Result<DiskCompression, Error> {
    let mut file = fs::File::open(path).map_err(|error| {
        Error::Internal(format!(
            "failed to open machine image {} for compression detection: {error}",
            path.display()
        ))
    })?;
    let mut header = [0_u8; 4];
    let read = file.read(&mut header).map_err(|error| {
        Error::Internal(format!(
            "failed to read machine image {} for compression detection: {error}",
            path.display()
        ))
    })?;
    if read >= 2 && header[..2] == [0x1f, 0x8b] {
        return Ok(DiskCompression::Gzip);
    }
    if read >= 4 && header == [0x28, 0xb5, 0x2f, 0xfd] {
        return Ok(DiskCompression::Zstd);
    }
    Ok(DiskCompression::None)
}

fn to_oci_descriptor(layer: &RegistryLayerDescriptor) -> OciDescriptor {
    OciDescriptor {
        digest: layer.digest.clone(),
        media_type: layer.media_type.clone(),
        size: layer.size,
        annotations: if layer.annotations.is_empty() {
            None
        } else {
            Some(layer.annotations.clone())
        },
        ..Default::default()
    }
}

fn run_async_in_thread<F, Fut, T>(build: F) -> Result<T, Error>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, Error>> + Send + 'static,
    T: Send + 'static,
{
    thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| {
                Error::Internal(format!("failed to build machine async runtime: {error}"))
            })?
            .block_on(build())
    })
    .join()
    .map_err(|_| Error::Internal("machine async worker panicked".to_owned()))?
}
