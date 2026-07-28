//! Durable OCI annotation for authenticating creator-attempt runtime state.

use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::Path;

use serde_json::{Map, Value};

use crate::backends::conmon::lifecycle::CREATOR_ATTEMPT_ANNOTATION;
use crate::error::{Result, SandboxError};

/// Publish the exact creator attempt into the already materialized OCI bundle.
///
/// This write completes before the launch-gated wrapper is spawned. Runtime
/// `state` responses can therefore authenticate that a same-ID runtime belongs
/// to this attempt rather than a stale predecessor.
pub(crate) fn publish_creator_attempt_annotation(
    config_path: &Path,
    attempt_id: &str,
) -> Result<()> {
    if attempt_id.trim().is_empty() {
        return Err(SandboxError::OperationFailed {
            message: "creator attempt annotation must not be empty".to_owned(),
        });
    }
    let metadata =
        fs::symlink_metadata(config_path).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "cannot inspect OCI bundle config {} before creator launch: {error}",
                config_path.display()
            ),
        })?;
    if !metadata.file_type().is_file() {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "OCI bundle config {} is not a regular file",
                config_path.display()
            ),
        });
    }

    let mut bytes = Vec::new();
    File::open(config_path)
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to read OCI bundle config {}: {error}",
                config_path.display()
            ),
        })?;
    let mut document: Value =
        serde_json::from_slice(&bytes).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to parse OCI bundle config {}: {error}",
                config_path.display()
            ),
        })?;
    let object = document
        .as_object_mut()
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "OCI bundle config {} must be a JSON object",
                config_path.display()
            ),
        })?;
    let annotations = object
        .entry("annotations")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "OCI bundle config {} has non-object annotations",
                config_path.display()
            ),
        })?;
    annotations.insert(
        CREATOR_ATTEMPT_ANNOTATION.to_owned(),
        Value::String(attempt_id.to_owned()),
    );
    let rendered =
        serde_json::to_vec_pretty(&document).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to render OCI bundle config {}: {error}",
                config_path.display()
            ),
        })?;
    durable_replace(config_path, &rendered, metadata.permissions())
}

fn durable_replace(
    destination: &Path,
    contents: &[u8],
    permissions: fs::Permissions,
) -> Result<()> {
    let parent = destination
        .parent()
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "OCI bundle config {} has no parent directory",
                destination.display()
            ),
        })?;
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "OCI bundle config {} has an invalid file name",
                destination.display()
            ),
        })?;
    let staged_path = parent.join(format!(
        ".{file_name}.creator-attempt-{}.tmp",
        ulid::Ulid::new()
    ));
    let result = (|| {
        let mut staged = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged_path)?;
        staged.set_permissions(permissions)?;
        staged.write_all(contents)?;
        staged.sync_all()?;
        fs::rename(&staged_path, destination)?;
        File::open(parent)?.sync_all()
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&staged_path);
        return Err(SandboxError::OperationFailed {
            message: format!(
                "failed to durably publish creator attempt in OCI bundle config {}: {error}",
                destination.display()
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publishes_attempt_without_erasing_existing_annotations() {
        let root = tempfile::tempdir().expect("temporary directory should exist");
        let config = root.path().join("config.json");
        fs::write(
            &config,
            br#"{"ociVersion":"1.1.0","annotations":{"example":"retained"}}"#,
        )
        .expect("fixture config should write");

        publish_creator_attempt_annotation(&config, "attempt-alpha")
            .expect("creator annotation should publish");
        let document: Value =
            serde_json::from_slice(&fs::read(&config).expect("config should read"))
                .expect("config should remain JSON");
        assert_eq!(document["annotations"]["example"], "retained");
        assert_eq!(
            document["annotations"][CREATOR_ATTEMPT_ANNOTATION],
            "attempt-alpha"
        );
    }
}
