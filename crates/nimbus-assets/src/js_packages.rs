//! Embedded, dependency-closed JS package payloads (BPD).
//!
//! `scripts/stage-embedded-packages.mjs` stages each built `packages/<dir>/dist`
//! under `crates/nimbus-assets/embedded/packages/<dir>/` with a checksummed
//! `manifest.json`. This module embeds that tree into Nimbus binaries and
//! exposes version-locked, checksum-verified package bytes. Consumer crates own
//! provisioning decisions, filesystem writes, CLI output, and app reconciliation.

use std::fs;
use std::path::{Path, PathBuf};

use rust_embed::Embed;
use serde::Deserialize;

use crate::integrity::sha256_hex;

/// The staged package payload tree, version-locked to this binary build.
#[derive(Embed)]
#[folder = "$CARGO_MANIFEST_DIR/embedded/packages/"]
struct EmbeddedPackages;

#[derive(Debug, Deserialize)]
pub struct EmbeddedPackageManifest {
    pub schema: u32,
    pub packages: Vec<EmbeddedPackage>,
    /// Build-time tooling closure for the in-binary V8 codegen runner (the
    /// codegen prebundle + esbuild JS + the platform `@esbuild` native binary).
    /// Not provisioned into apps; materialized to a temp run dir for codegen.
    #[serde(default)]
    pub tooling: Vec<EmbeddedTooling>,
}

#[derive(Debug, Deserialize)]
pub struct EmbeddedTooling {
    /// `codegen` (the prebundle), `esbuild`, or `@esbuild/<platform>`.
    pub name: String,
    pub files: Vec<EmbeddedFile>,
}

#[derive(Debug, Deserialize)]
pub struct EmbeddedPackage {
    /// Staging directory name (e.g. `convex`, `mongodb`, `@connectrpc/connect`).
    pub dir: String,
    /// Source workspace directory for Nimbus-owned packages when it differs from
    /// the npm staging directory (for example `packages/nimbus` ->
    /// `@nimbus/nimbus`).
    #[serde(rename = "sourceDir", default)]
    pub source_dir: Option<String>,
    /// Logical npm package name (e.g. `convex`, `@nimbus/mongodb`).
    pub name: String,
    pub version: String,
    /// True for co-provisioned third-party roots (no `packages/<dir>` source).
    #[serde(rename = "thirdParty", default)]
    pub third_party: bool,
    pub files: Vec<EmbeddedFile>,
}

#[derive(Debug, Deserialize)]
pub struct EmbeddedFile {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug)]
pub struct PackageAsset {
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub struct EmbeddedPackageFile {
    pub path: String,
    pub bytes: Vec<u8>,
    pub sha256: String,
}

/// Parse the embedded manifest. Panics only if the binary was built without a
/// staged payload, which the Makefile build graph prevents.
pub fn manifest() -> EmbeddedPackageManifest {
    let raw = EmbeddedPackages::get("manifest.json")
        .expect("embedded/packages/manifest.json must be embedded at build time");
    serde_json::from_slice(&raw.data).expect("embedded manifest must be valid JSON")
}

/// SHA-256 of the embedded manifest. This is the version-lock stamp value
/// written into a provisioned app's `.nimbus/packages/.version`.
pub fn manifest_digest() -> String {
    let raw = EmbeddedPackages::get("manifest.json")
        .expect("embedded/packages/manifest.json must be embedded at build time");
    sha256_hex(&raw.data)
}

pub fn package_names() -> Vec<String> {
    manifest()
        .packages
        .into_iter()
        .map(|package| package.name)
        .collect()
}

pub fn file(path: &str) -> Option<PackageAsset> {
    EmbeddedPackages::get(path).map(|embedded| PackageAsset {
        data: embedded.data.into_owned(),
    })
}

/// Bytes of an embedded package file at `<dir>/<rel>`.
pub fn file_bytes(dir: &str, rel: &str) -> Option<Vec<u8>> {
    file(&format!("{dir}/{rel}")).map(|asset| asset.data)
}

pub fn package_files(dir: &str) -> Result<Vec<EmbeddedPackageFile>, String> {
    let manifest = manifest();
    let package = manifest
        .packages
        .iter()
        .find(|package| package.dir == dir)
        .ok_or_else(|| format!("no embedded package {dir}"))?;
    package
        .files
        .iter()
        .map(|file| {
            let bytes = file_bytes(dir, &file.path)
                .ok_or_else(|| format!("missing embedded file {dir}/{}", file.path))?;
            Ok(EmbeddedPackageFile {
                path: file.path.clone(),
                bytes,
                sha256: file.sha256.clone(),
            })
        })
        .collect()
}

/// Materialize the embedded codegen tooling closure into `dest`, laid out so the
/// in-binary V8 tooling runtime resolves it the same way the proof test does
/// (`tooling_node22_executes_esbuild_style_staged_binary`): the codegen
/// prebundle co-located at `<dest>/codegen.bundle.mjs`, with `esbuild` and the
/// platform `@esbuild/<platform>` package under `<dest>/node_modules/`. The
/// `@esbuild` native binary is made executable. Returns the path to the codegen
/// bundle (the runner's `codegenSpecifier`).
pub fn materialize_tooling(dest: &Path) -> std::io::Result<PathBuf> {
    let manifest = manifest();
    let mut codegen_bundle: Option<PathBuf> = None;
    for tool in &manifest.tooling {
        for file in &tool.files {
            let bytes = tooling_file_bytes(&tool.name, &file.path).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("missing embedded tooling file {}/{}", tool.name, file.path),
                )
            })?;
            verify_digest(
                &format!("tooling {}/{}", tool.name, file.path),
                &bytes,
                file,
            )
            .map_err(std::io::Error::other)?;
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
            #[cfg(unix)]
            if tool.name.starts_with("@esbuild/") && file.path.starts_with("bin/") {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&out, fs::Permissions::from_mode(0o755))?;
            }
        }
    }
    codegen_bundle.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "embedded tooling closure is missing the codegen bundle",
        )
    })
}

/// Verify every manifest-listed file is present and matches its checksum.
pub fn verify_manifest_integrity() -> Result<(), String> {
    let manifest = manifest();
    if manifest.schema != 1 {
        return Err(format!(
            "unsupported embedded package manifest schema {}",
            manifest.schema
        ));
    }
    for package in &manifest.packages {
        if package.version.trim().is_empty() {
            return Err(format!(
                "embedded package {} has an empty version",
                package.name
            ));
        }
        if package.third_party && package.dir != package.name {
            return Err(format!(
                "third-party embedded package {} must use its package name as the staging dir",
                package.name
            ));
        }
        for file in &package.files {
            let bytes = file_bytes(&package.dir, &file.path)
                .ok_or_else(|| format!("missing embedded file {}/{}", package.dir, file.path))?;
            verify_digest(&format!("{}/{}", package.dir, file.path), &bytes, file)?;
        }
    }
    for tool in &manifest.tooling {
        verify_tooling_entry(tool)?;
    }
    Ok(())
}

/// Verify the embedded build-time tooling closure without also checking
/// app-provisioned packages.
pub fn verify_tooling_integrity() -> Result<(), String> {
    let manifest = manifest();
    if manifest.tooling.is_empty() {
        return Err("embedded tooling manifest is empty".to_string());
    }
    for tool in &manifest.tooling {
        verify_tooling_entry(tool)?;
    }
    Ok(())
}

pub fn tooling_available() -> bool {
    verify_tooling_integrity().is_ok()
}

/// Backwards-compatible name for callers that only need the manifest checksum
/// gate. New code should prefer `verify_manifest_integrity`.
pub fn verify_integrity() -> Result<(), String> {
    verify_manifest_integrity()
}

fn verify_tooling_entry(tool: &EmbeddedTooling) -> Result<(), String> {
    for file in &tool.files {
        let bytes = tooling_file_bytes(&tool.name, &file.path)
            .ok_or_else(|| format!("missing embedded tooling file {}/{}", tool.name, file.path))?;
        verify_digest(
            &format!("tooling {}/{}", tool.name, file.path),
            &bytes,
            file,
        )?;
    }
    Ok(())
}

/// Bytes of an embedded tooling file at `tooling/<name>/<rel>`.
fn tooling_file_bytes(name: &str, rel: &str) -> Option<Vec<u8>> {
    EmbeddedPackages::get(&format!("tooling/{name}/{rel}")).map(|f| f.data.into_owned())
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

    const EXPECTED: &[&str] = &[
        "convex",
        "@nimbus/nimbus",
        "firebase",
        "@nimbus/mongodb",
        "@nimbus/dynamodb",
    ];

    #[test]
    fn materialize_tooling_lays_out_codegen_bundle_and_esbuild_closure() {
        let temp = tempfile::tempdir().unwrap();
        let bundle = materialize_tooling(temp.path()).expect("tooling materializes");
        assert_eq!(bundle, temp.path().join("codegen.bundle.mjs"));
        assert!(bundle.is_file(), "codegen bundle must be written");
        assert!(
            temp.path()
                .join("node_modules/esbuild/package.json")
                .is_file(),
            "esbuild must be staged under node_modules"
        );
        let manifest = manifest();
        let platform = manifest
            .tooling
            .iter()
            .find(|tool| tool.name.starts_with("@esbuild/"))
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

    #[test]
    fn manifest_lists_all_provisioned_packages() {
        let manifest = manifest();
        assert_eq!(manifest.schema, 1);
        let names: Vec<&str> = manifest
            .packages
            .iter()
            .map(|package| package.name.as_str())
            .collect();
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
        for package in &manifest.packages {
            if package.third_party {
                continue;
            }
            let source_dir = package.source_dir.as_deref().unwrap_or(&package.dir);
            let src_path = format!(
                "{}/../../packages/{}/package.json",
                env!("CARGO_MANIFEST_DIR"),
                source_dir
            );
            let src = std::fs::read_to_string(&src_path)
                .unwrap_or_else(|error| panic!("read {src_path}: {error}"));
            let json: serde_json::Value = serde_json::from_str(&src).unwrap();
            assert_eq!(
                json["version"].as_str().unwrap(),
                package.version,
                "embedded {} version is not locked to source",
                package.dir
            );
        }
    }

    #[test]
    fn embedded_bytes_match_manifest_checksums() {
        verify_manifest_integrity().expect("embedded package integrity");
    }

    #[test]
    fn tamper_detection_rejects_modified_bytes() {
        let manifest = manifest();
        let mongodb = manifest
            .packages
            .iter()
            .find(|package| package.dir == "@nimbus/mongodb")
            .expect("mongodb staged");
        let uri = mongodb
            .files
            .iter()
            .find(|file| file.path == "uri.js")
            .expect("uri.js listed");
        let bytes = file_bytes("@nimbus/mongodb", "uri.js").expect("uri.js present");
        assert_eq!(sha256_hex(&bytes), uri.sha256);
        let mut tampered = bytes.clone();
        tampered.push(b'X');
        assert_ne!(
            sha256_hex(&tampered),
            uri.sha256,
            "tamper must change the digest"
        );
    }
}
