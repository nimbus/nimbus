//! Embedded, dependency-closed JS package payloads (BPD1).
//!
//! `scripts/stage-embedded-packages.mjs` stages each built `packages/<dir>/dist`
//! under `crates/nimbus-bin/embedded-packages/<dir>/` with a checksummed
//! `manifest.json`. This module embeds that tree into the binary (rust-embed)
//! and exposes version-locked, checksum-verified access. The provisioning
//! reconciler (BPD2) consumes these accessors to materialize
//! `<app>/.nimbus/packages/*`.

use std::path::{Path, PathBuf};
use std::{fs, io};

use rust_embed::Embed;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The staged package payload tree, version-locked to this binary build.
#[derive(Embed)]
#[folder = "$CARGO_MANIFEST_DIR/embedded-packages/"]
struct EmbeddedPackages;

#[derive(Debug, Deserialize)]
pub(crate) struct EmbeddedManifest {
    pub schema: u32,
    pub packages: Vec<EmbeddedPackage>,
    /// Build-time tooling closure for the in-binary V8 codegen runner (the
    /// codegen prebundle + esbuild JS + the platform `@esbuild` native binary).
    /// Not provisioned into apps; materialized to a temp run dir for codegen.
    #[serde(default)]
    pub tooling: Vec<EmbeddedTooling>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EmbeddedTooling {
    /// `codegen` (the prebundle), `esbuild`, or `@esbuild/<platform>`.
    pub name: String,
    pub files: Vec<EmbeddedFile>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EmbeddedPackage {
    /// Staging directory name (e.g. `convex`, `mongodb`, `@connectrpc/connect`).
    pub dir: String,
    /// Logical npm package name (e.g. `convex`, `@nimbus/mongodb`).
    pub name: String,
    pub version: String,
    /// True for co-provisioned third-party roots (no `packages/<dir>` source).
    #[serde(rename = "thirdParty", default)]
    pub third_party: bool,
    pub files: Vec<EmbeddedFile>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EmbeddedFile {
    pub path: String,
    pub sha256: String,
}

/// Lowercase hex SHA-256 of `bytes`.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Parse the embedded manifest. Panics only if the binary was built without a
/// staged payload, which the Makefile build graph prevents.
pub(crate) fn manifest() -> EmbeddedManifest {
    let raw = EmbeddedPackages::get("manifest.json")
        .expect("embedded-packages/manifest.json must be embedded at build time");
    serde_json::from_slice(&raw.data).expect("embedded manifest must be valid JSON")
}

/// Bytes of an embedded file at `<dir>/<rel>`.
pub(crate) fn file_bytes(dir: &str, rel: &str) -> Option<Vec<u8>> {
    EmbeddedPackages::get(&format!("{dir}/{rel}")).map(|f| f.data.into_owned())
}

/// SHA-256 of the embedded manifest — the version-lock stamp value written into
/// a provisioned app's `.nimbus/packages/.version`. Changes whenever any
/// embedded package's bytes or version change, so it detects binary upgrades.
pub(crate) fn manifest_digest() -> String {
    let raw = EmbeddedPackages::get("manifest.json")
        .expect("embedded-packages/manifest.json must be embedded at build time");
    sha256_hex(&raw.data)
}

/// Materialize the embedded codegen tooling closure into `dest`, laid out so the
/// in-binary V8 tooling runtime resolves it the same way the proof test does
/// (`tooling_node22_executes_esbuild_style_staged_binary`): the codegen
/// prebundle co-located at `<dest>/codegen.bundle.mjs`, with `esbuild` and the
/// platform `@esbuild/<platform>` package under `<dest>/node_modules/`. The
/// `@esbuild` native binary is made executable. Returns the path to the codegen
/// bundle (the runner's `codegenSpecifier`).
pub(crate) fn materialize_tooling(dest: &Path) -> io::Result<PathBuf> {
    let manifest = manifest();
    let mut codegen_bundle: Option<PathBuf> = None;
    for tool in &manifest.tooling {
        for file in &tool.files {
            let bytes = tooling_file_bytes(&tool.name, &file.path).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("missing embedded tooling file {}/{}", tool.name, file.path),
                )
            })?;
            verify_digest(
                &format!("tooling {}/{}", tool.name, file.path),
                &bytes,
                file,
            )
            .map_err(io::Error::other)?;
            // codegen bundle sits at the run-dir root; everything else is a
            // node_modules package so bare `import("esbuild")` resolves.
            let out = if tool.name == "codegen" {
                dest.join(&file.path)
            } else {
                dest.join("node_modules").join(&tool.name).join(&file.path)
            };
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&out, bytes)?;
            if tool.name == "codegen" && file.path == "codegen.bundle.mjs" {
                codegen_bundle = Some(out.clone());
            }
            // The @esbuild native binary must be executable to spawn.
            #[cfg(unix)]
            if tool.name.starts_with("@esbuild/") && file.path.starts_with("bin/") {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&out, fs::Permissions::from_mode(0o755))?;
            }
        }
    }
    codegen_bundle.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "embedded tooling closure is missing the codegen bundle",
        )
    })
}

/// Bytes of an embedded tooling file at `.tooling/<name>/<rel>`.
fn tooling_file_bytes(name: &str, rel: &str) -> Option<Vec<u8>> {
    EmbeddedPackages::get(&format!(".tooling/{name}/{rel}")).map(|f| f.data.into_owned())
}

/// Verify every manifest-listed file is present and matches its checksum.
pub(crate) fn verify_integrity() -> Result<(), String> {
    let manifest = manifest();
    if manifest.schema != 1 {
        return Err(format!(
            "unsupported embedded package manifest schema {}",
            manifest.schema
        ));
    }
    for pkg in &manifest.packages {
        if pkg.version.trim().is_empty() {
            return Err(format!(
                "embedded package {} has an empty version",
                pkg.name
            ));
        }
        if pkg.third_party && pkg.dir != pkg.name {
            return Err(format!(
                "third-party embedded package {} must use its package name as the staging dir",
                pkg.name
            ));
        }
        for file in &pkg.files {
            let bytes = file_bytes(&pkg.dir, &file.path)
                .ok_or_else(|| format!("missing embedded file {}/{}", pkg.dir, file.path))?;
            verify_digest(&format!("{}/{}", pkg.dir, file.path), &bytes, file)?;
        }
    }
    for tool in &manifest.tooling {
        for file in &tool.files {
            let bytes = tooling_file_bytes(&tool.name, &file.path).ok_or_else(|| {
                format!("missing embedded tooling file {}/{}", tool.name, file.path)
            })?;
            verify_digest(
                &format!("tooling {}/{}", tool.name, file.path),
                &bytes,
                file,
            )?;
        }
    }
    Ok(())
}

fn verify_digest(label: &str, bytes: &[u8], file: &EmbeddedFile) -> Result<(), String> {
    let digest = sha256_hex(bytes);
    if digest != file.sha256 {
        return Err(format!(
            "checksum mismatch for {label}: expected {}, got {}",
            file.sha256, digest
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materialize_tooling_lays_out_codegen_bundle_and_esbuild_closure() {
        let temp = tempfile::tempdir().unwrap();
        let bundle = materialize_tooling(temp.path()).expect("tooling materializes");
        // codegen prebundle is co-located at the run-dir root.
        assert_eq!(bundle, temp.path().join("codegen.bundle.mjs"));
        assert!(bundle.is_file(), "codegen bundle must be written");
        // esbuild JS wrapper resolves as a node_modules package.
        assert!(
            temp.path()
                .join("node_modules/esbuild/package.json")
                .is_file(),
            "esbuild must be staged under node_modules"
        );
        // the platform @esbuild native binary is present and executable.
        let manifest = manifest();
        let platform = manifest
            .tooling
            .iter()
            .find(|t| t.name.starts_with("@esbuild/"))
            .expect("a platform @esbuild tooling root is staged");
        let binary_rel = platform
            .files
            .iter()
            .find_map(|file| match file.path.as_str() {
                "bin/esbuild" | "esbuild.exe" => Some(file.path.as_str()),
                _ => None,
            })
            .expect("platform @esbuild tooling root must list a native binary");
        let bin = temp
            .path()
            .join("node_modules")
            .join(&platform.name)
            .join(binary_rel);
        assert!(
            bin.is_file(),
            "platform @esbuild binary must be staged at {}",
            bin.display()
        );
        #[cfg(unix)]
        {
            if binary_rel == "bin/esbuild" {
                use std::os::unix::fs::PermissionsExt;
                let mode = fs::metadata(&bin).unwrap().permissions().mode();
                assert!(
                    mode & 0o111 != 0,
                    "the @esbuild binary must be executable (mode {mode:o})"
                );
            }
        }
    }

    const EXPECTED: &[&str] = &[
        "convex",
        "nimbus",
        "@nimbus/firebase",
        "@nimbus/mongodb",
        "@nimbus/dynamodb",
    ];

    #[test]
    fn manifest_lists_all_provisioned_packages() {
        let manifest = manifest();
        assert_eq!(manifest.schema, 1);
        let names: Vec<&str> = manifest.packages.iter().map(|p| p.name.as_str()).collect();
        for expected in EXPECTED {
            assert!(
                names.contains(expected),
                "embedded manifest missing {expected}"
            );
        }
    }

    #[test]
    fn embedded_versions_match_source_package_json() {
        let manifest = manifest();
        for pkg in &manifest.packages {
            // Third-party roots have no `packages/<dir>` source; their version is
            // pinned by node_modules at stage time, not a Nimbus source manifest.
            if pkg.third_party {
                continue;
            }
            let src_path = format!(
                "{}/../../packages/{}/package.json",
                env!("CARGO_MANIFEST_DIR"),
                pkg.dir
            );
            let src = std::fs::read_to_string(&src_path)
                .unwrap_or_else(|e| panic!("read {src_path}: {e}"));
            let json: serde_json::Value = serde_json::from_str(&src).unwrap();
            assert_eq!(
                json["version"].as_str().unwrap(),
                pkg.version,
                "embedded {} version is not locked to source",
                pkg.dir
            );
        }
    }

    #[test]
    fn embedded_bytes_match_manifest_checksums() {
        verify_integrity().expect("embedded package integrity");
    }

    #[test]
    fn tamper_detection_rejects_modified_bytes() {
        let manifest = manifest();
        let mongodb = manifest
            .packages
            .iter()
            .find(|p| p.dir == "mongodb")
            .expect("mongodb staged");
        let uri = mongodb
            .files
            .iter()
            .find(|f| f.path == "uri.js")
            .expect("uri.js listed");
        let bytes = file_bytes("mongodb", "uri.js").expect("uri.js present");
        // Positive: real bytes match the recorded checksum.
        assert_eq!(sha256_hex(&bytes), uri.sha256);
        // Negative: any modification is detected.
        let mut tampered = bytes.clone();
        tampered.push(b'X');
        assert_ne!(
            sha256_hex(&tampered),
            uri.sha256,
            "tamper must change the digest"
        );
    }
}
