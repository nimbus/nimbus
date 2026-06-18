//! Client-side TypeScript type-info extraction (FSV8 foundation).
//!
//! The extraction script [`typeinfo.mjs`](./typeinfo.mjs) drives the official
//! TS Compiler API LanguageService to produce per-identifier hover info for a
//! module — the same text an editor shows on hover (`getQuickInfoAtPosition`).
//! This is the type tier of code navigation; there is no production-grade Rust
//! type checker, so the official compiler is the only correct source of types,
//! and it runs client-side at `nimbus deploy` where the toolchain + type
//! closure already exist (the research-chosen path).
//!
//! Wiring the extraction into the deploy artifact + the console hover overlay
//! is FSV8b. This module ships the validated extraction script; the test proves
//! it produces real hover text against the workspace TypeScript.

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    const TYPE_INFO_SCRIPT: &str = include_str!("typeinfo.mjs");

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

    /// The extraction script returns real hover text for a typed binding,
    /// driven by the workspace TypeScript compiler. Gated on node + typescript.
    #[test]
    fn type_info_script_reports_hover_for_typed_binding() {
        if !node_available() || !workspace_typescript() {
            eprintln!("skipping: node or workspace typescript unavailable");
            return;
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("sample.ts");
        std::fs::write(
            &file,
            "export const greeting: string = \"hi\";\nexport const size = greeting.length;\n",
        )
        .expect("write fixture");

        let mut child = Command::new("node")
            .arg("--input-type=module")
            .env("NIMBUS_TYPEINFO_TARGET", &file)
            .current_dir(repo_root())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn node");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(TYPE_INFO_SCRIPT.as_bytes())
            .expect("write script");
        let output = child.wait_with_output().expect("node output");
        assert!(
            output.status.success(),
            "type-info script failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let hints: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("parse hints json");
        let hints = hints.as_array().expect("hints array");
        let greeting = hints
            .iter()
            .find(|h| h["name"] == "greeting")
            .expect("greeting hint present");
        let hover = greeting["hover"].as_str().expect("hover string");
        assert!(
            hover.contains("string"),
            "expected an inferred string type, got: {hover}"
        );
    }
}
