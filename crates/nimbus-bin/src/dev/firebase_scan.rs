//! Import-coverage scan gating the Firebase wiring path.
//!
//! The Firestore client path is the only dev flow that mutates the app
//! (`package.json` is rewired to the provisioned drop-in `firebase`
//! package), so it stays behind this fail-closed gate: the app's sources
//! are statically scanned and wiring proceeds only when every Firebase
//! import the app makes is covered by the drop-in package.
//!
//! Classification model:
//! - **Covered** — exported by the embedded drop-in package. The covered
//!   set is derived from the embedded package's own manifest at runtime,
//!   never hardcoded, so shipping a new export widens coverage
//!   automatically.
//! - **Uncovered** — a `firebase/*` or `@firebase/*` specifier the
//!   drop-in does not export (e.g. `firebase/auth`). Refuses.
//! - **Indeterminate** — a dynamic `import(...)`/`require(...)` whose
//!   non-literal argument mentions `firebase`: the target cannot be
//!   proven covered, so it refuses. Dynamic loads whose argument text
//!   does not mention `firebase` are out of scan — refusing every
//!   code-split app would make the gate unusable, and a non-Firebase
//!   dynamic load cannot route to the drop-in package.
//! - **Out of scan** — every other specifier, plus everything under the
//!   shared excluded-directory list (`node_modules/`, `.nimbus/`, build
//!   output — see [`super::watch::should_skip_watch_dir`]).
//!
//! The scan is textual and line-based. It recognizes `import ... from`,
//! bare `import "..."`, `export ... from`, `require("...")`, and
//! `import("...")` forms; line comments and block-comment body lines
//! (leading `*`) are skipped. Anything else that merely looks like an
//! import still counts: over-reporting refuses safely, under-reporting
//! would wire an app the drop-in package cannot serve.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use nimbus_assets::js_packages;

use super::watch::should_skip_watch_dir;

const SCANNED_EXTENSIONS: [&str; 6] = ["ts", "tsx", "js", "jsx", "mjs", "cjs"];

/// Import specifiers the embedded drop-in `firebase` package serves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CoveredSet {
    specifiers: BTreeSet<String>,
}

impl CoveredSet {
    /// Derive the covered set from the embedded drop-in package's own
    /// manifest: exports key `.` covers `firebase`, `./app` covers
    /// `firebase/app`, and so on.
    pub(super) fn from_embedded_manifest() -> io::Result<Self> {
        let bytes = js_packages::file_bytes("firebase", "package.json").ok_or_else(|| {
            io::Error::other("embedded firebase package is missing its package.json")
        })?;
        Self::from_manifest_bytes(&bytes)
    }

    fn from_manifest_bytes(bytes: &[u8]) -> io::Result<Self> {
        let manifest: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| {
            io::Error::other(format!(
                "embedded firebase package.json is not valid JSON: {error}"
            ))
        })?;
        let package_name = manifest["name"].as_str().ok_or_else(|| {
            io::Error::other("embedded firebase package.json declares no package name")
        })?;
        let exports = manifest["exports"].as_object().ok_or_else(|| {
            io::Error::other("embedded firebase package.json declares no exports map")
        })?;
        let mut specifiers = BTreeSet::new();
        for key in exports.keys() {
            if key == "." {
                specifiers.insert(package_name.to_string());
            } else if let Some(subpath) = key.strip_prefix("./") {
                specifiers.insert(format!("{package_name}/{subpath}"));
            }
        }
        if specifiers.is_empty() {
            return Err(io::Error::other(
                "embedded firebase package.json exports map covers no specifiers",
            ));
        }
        Ok(Self { specifiers })
    }

    /// The covered specifiers in sorted order (for refusal reporting).
    pub(super) fn covered_specifiers(&self) -> impl Iterator<Item = &str> {
        self.specifiers.iter().map(String::as_str)
    }

    fn covers(&self, specifier: &str) -> bool {
        self.specifiers.contains(specifier)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SpecifierClass {
    Covered,
    Uncovered,
    Indeterminate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ScanFinding {
    /// Import specifier, or the dynamic argument expression for
    /// indeterminate findings.
    pub(super) specifier: String,
    /// Source path relative to the scanned app directory.
    pub(super) file: PathBuf,
    /// 1-based line number.
    pub(super) line: usize,
    pub(super) class: SpecifierClass,
}

impl ScanFinding {
    /// One refusal-report line: `path:line  <reason>`.
    pub(super) fn describe(&self) -> String {
        let location = format!("{}:{}", self.file.display(), self.line);
        match self.class {
            SpecifierClass::Covered => format!("{location}  covered import: {}", self.specifier),
            SpecifierClass::Uncovered => {
                format!("{location}  uncovered import: {}", self.specifier)
            }
            SpecifierClass::Indeterminate => format!(
                "{location}  dynamic import target cannot be verified: {}",
                self.specifier
            ),
        }
    }
}

/// Every Firebase-family finding in the app, in deterministic file/line
/// order. The scan passes only when nothing blocks.
#[derive(Debug, Default)]
pub(super) struct FirebaseScan {
    pub(super) findings: Vec<ScanFinding>,
}

impl FirebaseScan {
    pub(super) fn passes(&self) -> bool {
        self.blocking_findings().next().is_none()
    }

    pub(super) fn blocking_findings(&self) -> impl Iterator<Item = &ScanFinding> {
        self.findings
            .iter()
            .filter(|finding| finding.class != SpecifierClass::Covered)
    }
}

/// Statically scan the app's sources and classify every Firebase-family
/// import against the covered set. Read-only.
pub(super) fn scan_app(app_dir: &Path, covered: &CoveredSet) -> io::Result<FirebaseScan> {
    let mut findings = Vec::new();
    scan_dir(app_dir, app_dir, covered, &mut findings)?;
    Ok(FirebaseScan { findings })
}

fn scan_dir(
    app_dir: &Path,
    dir: &Path,
    covered: &CoveredSet,
    findings: &mut Vec<ScanFinding>,
) -> io::Result<()> {
    let mut entries = std::fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        // Symlinks are skipped entirely: file_type() reports the link
        // itself, which keeps the walk cycle-free.
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if should_skip_watch_dir(&path) {
                continue;
            }
            scan_dir(app_dir, &path, covered, findings)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        if !SCANNED_EXTENSIONS.contains(&extension) {
            continue;
        }
        let bytes = std::fs::read(&path)?;
        let content = String::from_utf8_lossy(&bytes);
        let relative = path.strip_prefix(app_dir).unwrap_or(&path).to_path_buf();
        for (index, line) in content.lines().enumerate() {
            for raw in scan_line(line) {
                if let Some((specifier, class)) = classify(raw, covered) {
                    findings.push(ScanFinding {
                        specifier,
                        file: relative.clone(),
                        line: index + 1,
                        class,
                    });
                }
            }
        }
    }
    Ok(())
}

enum RawSpecifier {
    /// A string-literal specifier in an import/export/require position.
    Literal(String),
    /// The argument text of a `require(...)`/`import(...)` call whose
    /// argument is not a string literal.
    Dynamic(String),
}

fn classify(raw: RawSpecifier, covered: &CoveredSet) -> Option<(String, SpecifierClass)> {
    match raw {
        RawSpecifier::Literal(specifier) => {
            if !is_firebase_family(&specifier) {
                return None;
            }
            let class = if covered.covers(&specifier) {
                SpecifierClass::Covered
            } else {
                SpecifierClass::Uncovered
            };
            Some((specifier, class))
        }
        RawSpecifier::Dynamic(expression) => expression
            .contains("firebase")
            .then_some((expression, SpecifierClass::Indeterminate)),
    }
}

fn is_firebase_family(specifier: &str) -> bool {
    specifier == "firebase"
        || specifier.starts_with("firebase/")
        || specifier.starts_with("@firebase/")
}

fn scan_line(line: &str) -> Vec<RawSpecifier> {
    let trimmed = line.trim_start();
    // A commented-out import is not a runtime dependency. Only the
    // unambiguous shapes are skipped; anything else stays in scan.
    if trimmed.starts_with("//") || trimmed.starts_with('*') || trimmed.starts_with("/*") {
        return Vec::new();
    }
    let mut found = Vec::new();
    collect_call_specifiers(line, "require", &mut found);
    collect_call_specifiers(line, "import", &mut found);
    collect_bare_import_specifiers(line, &mut found);
    collect_from_specifiers(line, &mut found);
    found
}

/// `require("x")`, `import("x")`, and their non-literal-argument forms.
fn collect_call_specifiers(line: &str, callee: &str, found: &mut Vec<RawSpecifier>) {
    let mut search = 0;
    while let Some(offset) = line[search..].find(callee) {
        let start = search + offset;
        let end = start + callee.len();
        search = end;
        if !word_starts_at(line, start) {
            continue;
        }
        let Some(argument_text) = line[end..].trim_start().strip_prefix('(') else {
            continue;
        };
        let argument_text = argument_text.trim_start();
        match parse_string_literal(argument_text) {
            Some(specifier) => found.push(RawSpecifier::Literal(specifier)),
            None => found.push(RawSpecifier::Dynamic(capture_argument(argument_text))),
        }
    }
}

/// Bare `import "x"` (side-effect import).
fn collect_bare_import_specifiers(line: &str, found: &mut Vec<RawSpecifier>) {
    let mut search = 0;
    while let Some(offset) = line[search..].find("import") {
        let start = search + offset;
        let end = start + "import".len();
        search = end;
        if !word_starts_at(line, start) || word_continues_at(line, end) {
            continue;
        }
        let after = line[end..].trim_start();
        if after.starts_with('(') {
            continue; // dynamic form, owned by the call rule
        }
        if let Some(specifier) = parse_string_literal(after) {
            found.push(RawSpecifier::Literal(specifier));
        }
    }
}

/// `import ... from "x"` and `export ... from "x"`.
fn collect_from_specifiers(line: &str, found: &mut Vec<RawSpecifier>) {
    let mut search = 0;
    while let Some(offset) = line[search..].find("from") {
        let start = search + offset;
        let end = start + "from".len();
        search = end;
        if !word_starts_at(line, start) || word_continues_at(line, end) {
            continue;
        }
        if let Some(specifier) = parse_string_literal(line[end..].trim_start()) {
            found.push(RawSpecifier::Literal(specifier));
        }
    }
}

fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '$'
}

fn word_starts_at(line: &str, index: usize) -> bool {
    line[..index]
        .chars()
        .next_back()
        .is_none_or(|ch| !is_identifier_char(ch))
}

fn word_continues_at(line: &str, index: usize) -> bool {
    line[index..].chars().next().is_some_and(is_identifier_char)
}

/// Parse a leading string literal. Template literals count only without
/// interpolation; escapes are treated as non-literal so they fall through
/// to the conservative dynamic path.
fn parse_string_literal(text: &str) -> Option<String> {
    let mut chars = text.chars();
    let quote = chars.next()?;
    if quote != '"' && quote != '\'' && quote != '`' {
        return None;
    }
    let rest = chars.as_str();
    let literal = &rest[..rest.find(quote)?];
    if literal.contains('\\') || (quote == '`' && literal.contains("${")) {
        return None;
    }
    Some(literal.to_string())
}

/// The argument text of a call, up to its matching close paren (or end of
/// line for multi-line arguments).
fn capture_argument(text: &str) -> String {
    let mut depth = 1_usize;
    let mut argument = String::new();
    for ch in text.chars() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        argument.push(ch);
    }
    argument.trim().to_string()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::*;

    fn embedded_covered_set() -> CoveredSet {
        CoveredSet::from_embedded_manifest().expect("embedded covered set should derive")
    }

    fn write_source(app_dir: &Path, relative: &str, contents: &str) {
        let path = app_dir.join(relative);
        fs::create_dir_all(path.parent().expect("source path should have a parent"))
            .expect("source dirs should create");
        fs::write(path, contents).expect("source file should write");
    }

    #[test]
    fn covered_set_derives_from_embedded_package_manifest() {
        let covered = embedded_covered_set();

        // Independently derive the expectation from the same embedded
        // manifest the production path reads: `.` maps to the package
        // name, `./<subpath>` maps to `<name>/<subpath>`.
        let bytes = js_packages::file_bytes("firebase", "package.json")
            .expect("embedded firebase package.json should exist");
        let manifest: serde_json::Value =
            serde_json::from_slice(&bytes).expect("embedded manifest should parse");
        let name = manifest["name"].as_str().expect("manifest should be named");
        let expected: BTreeSet<String> = manifest["exports"]
            .as_object()
            .expect("manifest should declare exports")
            .keys()
            .map(|key| match key.strip_prefix("./") {
                Some(subpath) => format!("{name}/{subpath}"),
                None => name.to_string(),
            })
            .collect();

        let derived: BTreeSet<String> = covered.covered_specifiers().map(str::to_string).collect();
        assert_eq!(derived, expected);
        assert!(
            covered.covers("firebase") && covered.covers("firebase/firestore"),
            "drop-in package must cover its core entry points: {derived:?}"
        );
    }

    #[test]
    fn manifest_without_exports_is_an_error() {
        let error = CoveredSet::from_manifest_bytes(br#"{"name": "firebase"}"#)
            .expect_err("manifest without exports must fail closed");
        assert!(
            error.to_string().contains("exports"),
            "error should name the missing exports map: {error}"
        );
    }

    #[test]
    fn covered_only_app_passes_scan() {
        let temp = tempdir().expect("tempdir should build");
        write_source(
            temp.path(),
            "src/main.ts",
            concat!(
                "import { initializeApp } from 'firebase/app';\n",
                "import { getFirestore, addDoc } from \"firebase/firestore\";\n",
                "import 'firebase/firestore';\n",
                "import firebase from 'firebase';\n",
                "import * as React from 'react';\n",
                "// import { getAuth } from 'firebase/auth';\n",
            ),
        );

        let scan = scan_app(temp.path(), &embedded_covered_set()).expect("scan should run");

        assert!(scan.passes(), "covered-only app must pass: {scan:?}");
        assert_eq!(
            scan.findings.len(),
            4,
            "every covered import is recorded, the react import and the \
             commented-out import are not: {scan:?}"
        );
        assert!(
            scan.findings
                .iter()
                .all(|finding| finding.class == SpecifierClass::Covered)
        );
    }

    #[test]
    fn app_without_firebase_imports_passes_scan() {
        let temp = tempdir().expect("tempdir should build");
        write_source(
            temp.path(),
            "src/app.tsx",
            "import * as React from 'react';\nconst page = await import(routePath);\n",
        );

        let scan = scan_app(temp.path(), &embedded_covered_set()).expect("scan should run");

        assert!(scan.passes());
        assert!(
            scan.findings.is_empty(),
            "nothing is Firebase-family here: {scan:?}"
        );
    }

    #[test]
    fn uncovered_auth_import_refuses_with_file_line() {
        let temp = tempdir().expect("tempdir should build");
        write_source(
            temp.path(),
            "src/auth.ts",
            "import { initializeApp } from 'firebase/app';\nimport { getAuth } from 'firebase/auth';\n",
        );

        let scan = scan_app(temp.path(), &embedded_covered_set()).expect("scan should run");

        assert!(!scan.passes(), "uncovered import must refuse");
        let blocking: Vec<_> = scan.blocking_findings().collect();
        assert_eq!(blocking.len(), 1, "only firebase/auth blocks: {blocking:?}");
        let finding = blocking[0];
        assert_eq!(finding.specifier, "firebase/auth");
        assert_eq!(finding.file, Path::new("src/auth.ts"));
        assert_eq!(finding.line, 2);
        assert_eq!(finding.class, SpecifierClass::Uncovered);
        assert!(finding.describe().contains("src/auth.ts:2"));
    }

    #[test]
    fn require_and_export_from_forms_are_scanned() {
        let temp = tempdir().expect("tempdir should build");
        write_source(
            temp.path(),
            "lib/compat.cjs",
            "const { getAuth } = require('firebase/auth');\n",
        );
        write_source(
            temp.path(),
            "lib/reexport.js",
            "export { onAuthStateChanged } from 'firebase/auth';\n",
        );

        let scan = scan_app(temp.path(), &embedded_covered_set()).expect("scan should run");

        let blocking: Vec<_> = scan.blocking_findings().collect();
        assert_eq!(
            blocking.len(),
            2,
            "require and export-from forms must both be found: {blocking:?}"
        );
        assert!(
            blocking
                .iter()
                .all(|finding| finding.specifier == "firebase/auth"
                    && finding.class == SpecifierClass::Uncovered)
        );
    }

    #[test]
    fn scoped_firebase_packages_are_uncovered() {
        let temp = tempdir().expect("tempdir should build");
        write_source(
            temp.path(),
            "src/internal.ts",
            "import { FirebaseApp } from '@firebase/app';\n",
        );

        let scan = scan_app(temp.path(), &embedded_covered_set()).expect("scan should run");

        assert!(!scan.passes());
        let blocking: Vec<_> = scan.blocking_findings().collect();
        assert_eq!(blocking.len(), 1);
        assert_eq!(blocking[0].specifier, "@firebase/app");
        assert_eq!(blocking[0].class, SpecifierClass::Uncovered);
    }

    #[test]
    fn dynamic_specifier_is_indeterminate_and_refuses() {
        let temp = tempdir().expect("tempdir should build");
        write_source(
            temp.path(),
            "src/lazy.ts",
            concat!(
                "const flavor = 'app';\n",
                "const mod = await import(`firebase/${flavor}`);\n",
                "const page = await import(routePath);\n",
            ),
        );

        let scan = scan_app(temp.path(), &embedded_covered_set()).expect("scan should run");

        assert!(!scan.passes(), "dynamic firebase specifier must refuse");
        let blocking: Vec<_> = scan.blocking_findings().collect();
        assert_eq!(
            blocking.len(),
            1,
            "the non-Firebase dynamic import stays out of scan: {blocking:?}"
        );
        let finding = blocking[0];
        assert_eq!(finding.class, SpecifierClass::Indeterminate);
        assert_eq!(finding.file, Path::new("src/lazy.ts"));
        assert_eq!(finding.line, 2);
        assert!(
            finding.specifier.contains("firebase/${flavor}"),
            "finding should carry the dynamic expression: {finding:?}"
        );
    }

    #[test]
    fn node_modules_specifiers_are_out_of_scan() {
        let temp = tempdir().expect("tempdir should build");
        write_source(
            temp.path(),
            "node_modules/some-lib/index.js",
            "const { getAuth } = require('firebase/auth');\n",
        );
        write_source(
            temp.path(),
            ".nimbus/packages/firebase/index.js",
            "import '@firebase/app';\n",
        );
        write_source(
            temp.path(),
            "dist/bundle.js",
            "import { getAuth } from 'firebase/auth';\n",
        );
        write_source(temp.path(), "src/main.ts", "import 'firebase/firestore';\n");

        let scan = scan_app(temp.path(), &embedded_covered_set()).expect("scan should run");

        assert!(scan.passes());
        assert_eq!(
            scan.findings.len(),
            1,
            "only the app source counts; excluded directories are out of scan: {scan:?}"
        );
        assert_eq!(scan.findings[0].file, Path::new("src/main.ts"));
    }

    #[test]
    fn minified_single_line_imports_are_scanned() {
        let temp = tempdir().expect("tempdir should build");
        write_source(
            temp.path(),
            "src/minified.js",
            "import{getAuth}from\"firebase/auth\";import{initializeApp}from\"firebase/app\";\n",
        );

        let scan = scan_app(temp.path(), &embedded_covered_set()).expect("scan should run");

        assert!(!scan.passes());
        let specifiers: Vec<&str> = scan
            .findings
            .iter()
            .map(|finding| finding.specifier.as_str())
            .collect();
        assert_eq!(specifiers, ["firebase/auth", "firebase/app"]);
    }
}
