//! Retained deploy artifacts: the content-addressed copy of every Convex
//! bundle a deploy activated, kept under the engine data directory so a
//! rollback can stage it again without the client.
//!
//! A deploy stages its artifacts in a private temporary directory that lives
//! only as long as the generation executes from it. [`DeployArtifactStore`]
//! copies those files into `<data_dir>/deploy-artifacts/<sha256>/` beside a
//! `manifest.json` that names every file with its SHA-256. [`stage`] copies
//! them back into a fresh private directory and refuses the copy when any
//! file no longer hashes to its manifest entry, or when `bundle.mjs` no
//! longer hashes to the provenance hash in `bundle.sha256`: a tampered or
//! stale bundle is rejected before it can be activated, the same rule the
//! runtime applies before every invocation.
//!
//! Two values become path components under the store root, and neither is
//! trusted from its caller. A bundle digest is checked to be 64 lowercase
//! hexadecimal characters, and every file name a manifest carries is checked
//! to be one ordinary path component. Both checks run before the value joins
//! a path, so a rollback request for a crafted bundle id, or a tampered
//! manifest that names `../`, cannot read or write outside the store.
//!
//! [`stage`]: DeployArtifactStore::stage

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use nimbus_core::Error;
use nimbus_runtime::RuntimeBundle;
use nimbus_system::source_package_digest;
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

/// The Convex artifact directory inside an app dir, as the registry loads it.
const CONVEX_ARTIFACT_DIR: &str = ".nimbus/convex";
const MANIFEST_FILE: &str = "manifest.json";
const RUNTIME_BUNDLE_FILE: &str = "bundle.mjs";
const RUNTIME_BUNDLE_SHA256_FILE: &str = "bundle.sha256";

#[derive(Debug, Clone)]
pub struct DeployArtifactStore {
    root: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct ArtifactManifest {
    version: u32,
    sha256: String,
    /// File name inside `.nimbus/convex` to the SHA-256 of its bytes.
    files: BTreeMap<String, String>,
}

impl DeployArtifactStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Whether a bundle's artifacts are on disk, so a rollback can stage them.
    ///
    /// A digest that is not a SHA-256 names nothing this store could have
    /// written, so the answer is `false` rather than an error.
    pub fn is_retained(&self, sha256: &str) -> bool {
        self.bundle_dir(sha256)
            .is_ok_and(|bundle_dir| bundle_dir.join(MANIFEST_FILE).is_file())
    }

    /// Copies the Convex artifacts under `app_dir` into the store under
    /// `sha256`, replacing an earlier copy of the same bundle. The copy is
    /// written beside its final path and renamed into place, so a crash
    /// mid-copy leaves no half bundle behind a manifest.
    pub fn retain(&self, sha256: &str, app_dir: &Path) -> Result<(), Error> {
        let bundle_dir = self.bundle_dir(sha256)?;
        let source_dir = app_dir.join(CONVEX_ARTIFACT_DIR);
        std::fs::create_dir_all(&self.root).map_err(|error| {
            Error::Internal(format!(
                "failed to create deploy artifact store {}: {error}",
                self.root.display()
            ))
        })?;
        let staging = tempfile::Builder::new()
            .prefix(&format!(".retain-{sha256}-"))
            .tempdir_in(&self.root)
            .map_err(|error| {
                Error::Internal(format!("failed to create deploy artifact copy: {error}"))
            })?;
        let mut files = BTreeMap::new();
        for entry in list_files(&source_dir)? {
            let bytes = std::fs::read(&entry.path).map_err(|error| {
                Error::Internal(format!(
                    "failed to read deploy artifact {}: {error}",
                    entry.path.display()
                ))
            })?;
            std::fs::write(staging.path().join(&entry.name), &bytes).map_err(|error| {
                Error::Internal(format!(
                    "failed to retain deploy artifact {}: {error}",
                    entry.name
                ))
            })?;
            files.insert(entry.name, source_package_digest(&bytes));
        }
        let manifest = ArtifactManifest {
            version: 1,
            sha256: sha256.to_owned(),
            files,
        };
        let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(|error| {
            Error::Internal(format!(
                "failed to serialize deploy artifact manifest: {error}"
            ))
        })?;
        std::fs::write(staging.path().join(MANIFEST_FILE), manifest_bytes).map_err(|error| {
            Error::Internal(format!("failed to write deploy artifact manifest: {error}"))
        })?;

        if bundle_dir.exists() {
            std::fs::remove_dir_all(&bundle_dir).map_err(|error| {
                Error::Internal(format!(
                    "failed to replace retained deploy artifacts {}: {error}",
                    bundle_dir.display()
                ))
            })?;
        }
        let staging_path = staging.keep();
        std::fs::rename(&staging_path, &bundle_dir).map_err(|error| {
            let _ = std::fs::remove_dir_all(&staging_path);
            Error::Internal(format!(
                "failed to move retained deploy artifacts into {}: {error}",
                bundle_dir.display()
            ))
        })?;
        Ok(())
    }

    /// Copies a retained bundle into a fresh private app dir, verifying each
    /// file against the manifest and the runtime bundle against its recorded
    /// provenance hash. A mismatch is `Error::InvalidInput` and nothing is
    /// activated.
    pub fn stage(&self, sha256: &str) -> Result<TempDir, Error> {
        let bundle_dir = self.bundle_dir(sha256)?;
        let manifest_path = bundle_dir.join(MANIFEST_FILE);
        if !manifest_path.is_file() {
            return Err(Error::NotFound(format!(
                "no retained deploy artifacts for bundle {sha256}; only bundles deployed through \
                 the deploy route can be rolled back to"
            )));
        }
        let manifest_bytes = std::fs::read(&manifest_path).map_err(|error| {
            Error::Internal(format!(
                "failed to read deploy artifact manifest {}: {error}",
                manifest_path.display()
            ))
        })?;
        let manifest: ArtifactManifest =
            serde_json::from_slice(&manifest_bytes).map_err(|error| {
                Error::InvalidInput(format!(
                    "deploy artifact integrity check failed for bundle {sha256}: manifest does not \
                 parse: {error}"
                ))
            })?;
        if manifest.sha256 != sha256 {
            return Err(Error::InvalidInput(format!(
                "deploy artifact integrity check failed for bundle {sha256}: manifest names \
                 bundle {}",
                manifest.sha256
            )));
        }

        let app_dir = tempfile::Builder::new()
            .prefix("nimbus-rollback-")
            .tempdir()
            .map_err(|error| {
                Error::Internal(format!(
                    "failed to create rollback staging directory: {error}"
                ))
            })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            std::fs::set_permissions(app_dir.path(), std::fs::Permissions::from_mode(0o700))
                .map_err(|error| {
                    Error::Internal(format!(
                        "failed to make rollback staging directory private {}: {error}",
                        app_dir.path().display()
                    ))
                })?;
        }
        let convex_dir = app_dir.path().join(CONVEX_ARTIFACT_DIR);
        std::fs::create_dir_all(&convex_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to create rollback staging directory {}: {error}",
                convex_dir.display()
            ))
        })?;
        for (name, expected) in &manifest.files {
            // The manifest is bytes on disk. Check the name it carries before
            // it joins either path, so a tampered manifest cannot read from,
            // or write to, anywhere but this bundle's own directory.
            let name = checked_artifact_name(name).map_err(|reason| {
                Error::InvalidInput(format!(
                    "deploy artifact integrity check failed for bundle {sha256}: {reason}"
                ))
            })?;
            let bytes = std::fs::read(bundle_dir.join(name)).map_err(|error| {
                Error::InvalidInput(format!(
                    "deploy artifact integrity check failed for bundle {sha256}: {name} could not \
                     be read: {error}"
                ))
            })?;
            let actual = source_package_digest(&bytes);
            if &actual != expected {
                return Err(Error::InvalidInput(format!(
                    "deploy artifact integrity check failed for bundle {sha256}: {name} hashes \
                     to {actual}, manifest expects {expected}"
                )));
            }
            std::fs::write(convex_dir.join(name), &bytes).map_err(|error| {
                Error::Internal(format!("failed to stage rollback artifact {name}: {error}"))
            })?;
        }

        // The provenance gate the runtime applies before every invocation,
        // applied once more before the bundle can become the active one.
        let recorded_hash_path = convex_dir.join(RUNTIME_BUNDLE_SHA256_FILE);
        if recorded_hash_path.is_file() {
            let recorded = std::fs::read_to_string(&recorded_hash_path)
                .map_err(|error| {
                    Error::Internal(format!("failed to read rollback bundle hash: {error}"))
                })?
                .trim()
                .to_owned();
            let bundle_path = convex_dir.join(RUNTIME_BUNDLE_FILE);
            let actual = RuntimeBundle::compute_sha256_for_path(&bundle_path).map_err(|error| {
                Error::InvalidInput(format!(
                    "deploy artifact integrity check failed for bundle {sha256}: runtime bundle \
                     could not be hashed: {error}"
                ))
            })?;
            if actual != recorded {
                return Err(Error::InvalidInput(format!(
                    "deploy artifact integrity check failed for bundle {sha256}: runtime bundle \
                     hashes to {actual}, provenance record expects {recorded}"
                )));
            }
        }
        Ok(app_dir)
    }

    /// The directory holding one bundle's artifacts. The digest is checked
    /// here, the single place it becomes a path component, so every caller
    /// inherits the check.
    fn bundle_dir(&self, sha256: &str) -> Result<PathBuf, Error> {
        let digest = checked_digest(sha256).map_err(|reason| {
            Error::InvalidInput(format!(
                "deploy bundle {sha256:?} is not addressable: {reason}"
            ))
        })?;
        Ok(self.root.join(digest))
    }
}

/// Accepts a bundle digest only in the form this store writes: exactly 64
/// lowercase hexadecimal characters. That form cannot hold a path separator,
/// `.`, or `..`, so a checked digest is always one directory under the root.
fn checked_digest(sha256: &str) -> Result<&str, String> {
    if sha256.len() == 64
        && sha256
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Ok(sha256);
    }
    Err("expected 64 lowercase hexadecimal characters".to_owned())
}

/// Accepts an artifact file name only when it is one ordinary path component.
/// This rejects an empty name, `.`, `..`, an absolute path, and any name
/// carrying a separator, on every platform the store runs on.
fn checked_artifact_name(name: &str) -> Result<&str, String> {
    let mut components = Path::new(name).components();
    let first = components.next();
    let rest = components.next();
    match (first, rest) {
        (Some(Component::Normal(component)), None) if component == OsStr::new(name) => Ok(name),
        _ => Err(format!(
            "artifact name {name:?} must be one ordinary path component"
        )),
    }
}

struct ArtifactFile {
    name: String,
    path: PathBuf,
}

fn list_files(dir: &Path) -> Result<Vec<ArtifactFile>, Error> {
    let entries = std::fs::read_dir(dir).map_err(|error| {
        Error::Internal(format!(
            "failed to list deploy artifacts {}: {error}",
            dir.display()
        ))
    })?;
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            Error::Internal(format!(
                "failed to list deploy artifacts {}: {error}",
                dir.display()
            ))
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let name = checked_artifact_name(name).map_err(|reason| {
            Error::Internal(format!(
                "refusing to retain deploy artifact {}: {reason}",
                path.display()
            ))
        })?;
        files.push(ArtifactFile {
            name: name.to_owned(),
            path: path.clone(),
        });
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store key is a SHA-256, so the tests use real digests rather than a
    /// short stand-in that the path check would rightly refuse.
    const BUNDLE: &str = "b5bb9d8014a0f9b1d61e21e796d78dccdf1352f23cd32812f4850b878ae4944c";
    const UNRETAINED_BUNDLE: &str =
        "7d865e959b2466918c9863afca942d0fb89d7c9ac0c99bafc3749504ded97730";

    fn write_app_dir(root: &Path, bundle: &str) -> PathBuf {
        let app_dir = root.join("app");
        let convex_dir = app_dir.join(CONVEX_ARTIFACT_DIR);
        std::fs::create_dir_all(&convex_dir).expect("convex dir should create");
        std::fs::write(convex_dir.join("functions.json"), b"{\"functions\":[]}")
            .expect("functions should write");
        std::fs::write(convex_dir.join(RUNTIME_BUNDLE_FILE), bundle).expect("bundle should write");
        let sha = RuntimeBundle::compute_sha256_for_path(convex_dir.join(RUNTIME_BUNDLE_FILE))
            .expect("bundle should hash");
        std::fs::write(convex_dir.join(RUNTIME_BUNDLE_SHA256_FILE), sha)
            .expect("hash should write");
        app_dir
    }

    #[test]
    fn retain_then_stage_round_trips_the_convex_artifacts() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let app_dir = write_app_dir(temp.path(), "export const value = 1;\n");
        let store = DeployArtifactStore::new(temp.path().join("store"));
        assert!(!store.is_retained(BUNDLE));
        store
            .retain(BUNDLE, &app_dir)
            .expect("artifacts should retain");
        assert!(store.is_retained(BUNDLE));

        let staged = store.stage(BUNDLE).expect("artifacts should stage");
        let convex_dir = staged.path().join(CONVEX_ARTIFACT_DIR);
        assert_eq!(
            std::fs::read_to_string(convex_dir.join(RUNTIME_BUNDLE_FILE)).expect("bundle reads"),
            "export const value = 1;\n"
        );
        assert!(convex_dir.join("functions.json").is_file());
        assert!(!convex_dir.join(MANIFEST_FILE).exists());
    }

    #[test]
    fn stage_rejects_a_tampered_runtime_bundle() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let app_dir = write_app_dir(temp.path(), "export const value = 1;\n");
        let store = DeployArtifactStore::new(temp.path().join("store"));
        store
            .retain(BUNDLE, &app_dir)
            .expect("artifacts should retain");
        std::fs::write(
            temp.path()
                .join("store")
                .join(BUNDLE)
                .join(RUNTIME_BUNDLE_FILE),
            "export const value = 2;\n",
        )
        .expect("tamper should write");

        let error = store
            .stage(BUNDLE)
            .expect_err("tampered bundle must not stage");
        assert!(
            matches!(&error, Error::InvalidInput(message) if message.contains("integrity check failed")),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn stage_rejects_a_bundle_whose_provenance_record_disagrees() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let app_dir = write_app_dir(temp.path(), "export const value = 1;\n");
        // Retain with a wrong provenance hash: the manifest is consistent, the
        // recorded provenance is not.
        std::fs::write(
            app_dir
                .join(CONVEX_ARTIFACT_DIR)
                .join(RUNTIME_BUNDLE_SHA256_FILE),
            "0000",
        )
        .expect("hash should write");
        let store = DeployArtifactStore::new(temp.path().join("store"));
        store
            .retain(BUNDLE, &app_dir)
            .expect("artifacts should retain");

        let error = store
            .stage(BUNDLE)
            .expect_err("stale provenance must not stage");
        assert!(
            matches!(&error, Error::InvalidInput(message) if message.contains("provenance record expects 0000")),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn is_retained_refuses_a_key_that_is_not_a_sha256() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let store = DeployArtifactStore::new(temp.path().join("store"));
        assert!(!store.is_retained("../../etc"));
        assert!(!store.is_retained(""));
        assert!(!store.is_retained("ABC"));
    }

    #[test]
    fn retain_refuses_a_key_that_is_not_a_sha256() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let app_dir = write_app_dir(temp.path(), "export const value = 1;\n");
        let store = DeployArtifactStore::new(temp.path().join("store"));

        let error = store
            .retain("../escape", &app_dir)
            .expect_err("a traversing key must not retain");
        assert!(
            matches!(&error, Error::InvalidInput(message) if message.contains("not addressable")),
            "unexpected error: {error:?}"
        );
        assert!(
            !temp.path().join("escape").exists(),
            "nothing may be written outside the store root"
        );
    }

    #[test]
    fn stage_refuses_a_key_that_escapes_the_store_root() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let store = DeployArtifactStore::new(temp.path().join("store"));

        let error = store
            .stage("../../etc")
            .expect_err("a traversing key must not stage");
        assert!(
            matches!(&error, Error::InvalidInput(message) if message.contains("not addressable")),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn stage_refuses_a_manifest_name_that_leaves_the_bundle_directory() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let app_dir = write_app_dir(temp.path(), "export const value = 1;\n");
        let store_root = temp.path().join("store");
        let store = DeployArtifactStore::new(store_root.clone());
        store
            .retain(BUNDLE, &app_dir)
            .expect("artifacts should retain");

        // The payload exists and hashes to what the tampered manifest claims,
        // so the digest check cannot refuse it. Only the name check can.
        let payload = b"export const owned = true;\n";
        std::fs::write(store_root.join("escape.mjs"), payload).expect("payload should write");
        std::fs::write(
            store_root.join(BUNDLE).join(MANIFEST_FILE),
            format!(
                "{{\"version\":1,\"sha256\":\"{BUNDLE}\",\"files\":{{\"../escape.mjs\":\"{}\"}}}}",
                source_package_digest(payload)
            ),
        )
        .expect("tampered manifest should write");

        let error = store
            .stage(BUNDLE)
            .expect_err("a traversing artifact name must not stage");
        assert!(
            matches!(&error, Error::InvalidInput(message)
                if message.contains("integrity check failed")
                    && message.contains("../escape.mjs")),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn stage_reports_a_bundle_that_was_never_retained() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let store = DeployArtifactStore::new(temp.path().join("store"));
        let error = store
            .stage(UNRETAINED_BUNDLE)
            .expect_err("missing bundle must not stage");
        assert!(
            matches!(error, Error::NotFound(_)),
            "unexpected error: {error:?}"
        );
    }
}
