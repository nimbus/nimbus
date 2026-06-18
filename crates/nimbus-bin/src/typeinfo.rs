//! Client-side TypeScript type-info extraction (FSV8).
//!
//! [`extract_module_type_info`] drives the official TS Compiler API
//! LanguageService (via the embedded [`typeinfo.mjs`](./typeinfo.mjs) script and
//! `node`) to produce per-identifier hover info for a module — the same text an
//! editor shows on hover. There is no production-grade Rust type checker, so the
//! official compiler is the only correct source of types, and it runs
//! client-side at `nimbus deploy` where the toolchain + type closure exist.
//! The result is carried in the content-addressed source package and surfaced
//! by the console Source tab. Best-effort: never blocks a deploy.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

const TYPE_INFO_SCRIPT: &str = include_str!("typeinfo.mjs");

/// Extract per-identifier type hints for `target_file`, resolving `typescript`
/// from `node_cwd` (the app dir). Returns `Some(hints)` only on success with a
/// non-empty array; `None` when node/typescript is unavailable or extraction
/// fails — type info is additive and must never block a deploy.
pub(crate) fn extract_module_type_info(node_cwd: &Path, target_file: &Path) -> Option<Value> {
    let mut child = Command::new("node")
        .arg("--input-type=module")
        .env("NIMBUS_TYPEINFO_TARGET", target_file)
        .current_dir(node_cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    child
        .stdin
        .take()?
        .write_all(TYPE_INFO_SCRIPT.as_bytes())
        .ok()?;
    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    let hints: Value = serde_json::from_slice(&output.stdout).ok()?;
    match &hints {
        Value::Array(array) if !array.is_empty() => Some(hints),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn node_available() -> bool {
        Command::new("node")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn workspace_typescript() -> bool {
        repo_root().join("node_modules/typescript").is_dir()
    }

    /// Extracts real hover text for a typed binding via the workspace TypeScript
    /// compiler. Gated on node + typescript.
    #[test]
    fn extract_reports_hover_for_typed_binding() {
        if !node_available() || !workspace_typescript() {
            eprintln!("skipping: node or workspace typescript unavailable");
            return;
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("sample.ts");
        std::fs::write(&file, "export const greeting: string = \"hi\";\n").expect("write fixture");

        let hints = extract_module_type_info(&repo_root(), &file).expect("hints present");
        let array = hints.as_array().expect("hints array");
        let greeting = array
            .iter()
            .find(|h| h["name"] == "greeting")
            .expect("greeting hint");
        assert!(
            greeting["hover"]
                .as_str()
                .unwrap_or_default()
                .contains("string"),
            "expected inferred string type, got: {greeting:?}"
        );
    }

    /// A file with no resolvable TypeScript (bad path) yields None, not an error.
    #[test]
    fn missing_target_is_best_effort_none() {
        if !node_available() || !workspace_typescript() {
            return;
        }
        let missing = repo_root().join("does-not-exist-xyz.ts");
        assert!(extract_module_type_info(&repo_root(), &missing).is_none());
    }
}
