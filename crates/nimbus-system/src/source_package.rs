//! The source-package wire format — the deploy-captured archive of original
//! module source (+ source maps) that backs the console Source view. The client
//! (`nimbus deploy`) builds it from the app's `convex/` source; the server
//! parses it to project `modules` rows and to serve module source. See the
//! Function Source Visibility plan (FSV3).
//!
//! Format (canonical JSON, sorted module keys so identical source produces
//! identical bytes → a stable content digest → store-level dedup):
//!
//! ```json
//! {
//!   "version": 1,
//!   "modules": {
//!     "<module path>": {
//!       "source": "<text>",
//!       "sourceMap": "<text>" | null,
//!       "typeInfo": [ { "name", "line", "col", "hover" }, ... ]   // FSV8, optional
//!     }
//!   }
//! }
//! ```
//!
//! A module path is the function-path prefix (`messages` for `messages:list`,
//! `admin/users` for `admin/users:create`), so a function resolves to its module
//! here and the module to its source package.

use std::collections::BTreeMap;

use nimbus_core::{Error, Result};
use serde_json::{Map, Value, json};

use crate::source_store::source_package_digest;

/// Current source-package format version.
pub const SOURCE_PACKAGE_VERSION: u64 = 1;

/// A module to pack into a source package: original source, optional source map,
/// and optional client-extracted type info (FSV8) — the JSON hint array from the
/// TS compiler, computed at deploy where the toolchain exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleInput {
    pub source: String,
    pub source_map: Option<String>,
    pub type_info: Option<Value>,
}

/// A module parsed out of a source package: its path, original source, optional
/// source map, optional type info, and the content hash of the source text
/// (change detector, recorded on the `modules` row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedModule {
    pub path: String,
    pub source: String,
    pub source_map: Option<String>,
    pub type_info: Option<Value>,
    pub sha256: String,
}

/// A parsed source package: its modules (sorted by path) and the total
/// uncompressed source byte count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSourcePackage {
    pub modules: Vec<ParsedModule>,
    pub unpacked_bytes: u64,
}

/// Build canonical source-package bytes from `path -> ModuleInput`.
/// Deterministic: the `BTreeMap` input fixes module order, so identical content
/// yields identical bytes and therefore a stable digest.
pub fn build_source_package(modules: &BTreeMap<String, ModuleInput>) -> Vec<u8> {
    let mut module_map = Map::new();
    for (path, module) in modules {
        let mut entry = Map::new();
        entry.insert("source".to_owned(), Value::String(module.source.clone()));
        entry.insert(
            "sourceMap".to_owned(),
            module
                .source_map
                .as_ref()
                .map_or(Value::Null, |map| Value::String(map.clone())),
        );
        if let Some(type_info) = &module.type_info {
            entry.insert("typeInfo".to_owned(), type_info.clone());
        }
        module_map.insert(path.clone(), Value::Object(entry));
    }
    let document = json!({
        "version": SOURCE_PACKAGE_VERSION,
        "modules": Value::Object(module_map),
    });
    serde_json::to_vec(&document).expect("source package document serializes")
}

/// Parse and validate source-package bytes, computing each module's source hash
/// and the total unpacked size. Fails closed on malformed input.
pub fn parse_source_package(bytes: &[u8]) -> Result<ParsedSourcePackage> {
    let document: Value = serde_json::from_slice(bytes).map_err(|error| {
        Error::InvalidInput(format!("source package is not valid JSON: {error}"))
    })?;
    let modules_obj = document
        .get("modules")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::InvalidInput("source package missing 'modules' object".to_owned()))?;

    let mut modules = Vec::with_capacity(modules_obj.len());
    let mut unpacked_bytes: u64 = 0;
    for (path, entry) in modules_obj {
        let source = entry.get("source").and_then(Value::as_str).ok_or_else(|| {
            Error::InvalidInput(format!("source package module '{path}' missing 'source'"))
        })?;
        let source_map = entry
            .get("sourceMap")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let type_info = entry
            .get("typeInfo")
            .cloned()
            .filter(|value| !value.is_null());
        unpacked_bytes = unpacked_bytes.saturating_add(source.len() as u64);
        modules.push(ParsedModule {
            path: path.clone(),
            sha256: source_package_digest(source.as_bytes()),
            source: source.to_owned(),
            source_map,
            type_info,
        });
    }
    modules.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ParsedSourcePackage {
        modules,
        unpacked_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> BTreeMap<String, ModuleInput> {
        BTreeMap::from([
            (
                "messages".to_owned(),
                ModuleInput {
                    source: "export const list = query({});\n".to_owned(),
                    source_map: None,
                    type_info: Some(
                        json!([{"name": "list", "line": 1, "hover": "const list: Query"}]),
                    ),
                },
            ),
            (
                "admin/users".to_owned(),
                ModuleInput {
                    source: "export const create = mutation({});\n".to_owned(),
                    source_map: Some("{\"version\":3}".to_owned()),
                    type_info: None,
                },
            ),
        ])
    }

    #[test]
    fn build_then_parse_round_trips_modules_with_hashes_and_size() {
        let modules = sample();
        let bytes = build_source_package(&modules);
        let parsed = parse_source_package(&bytes).expect("parse should succeed");

        assert_eq!(parsed.modules.len(), 2);
        // Sorted by path.
        assert_eq!(parsed.modules[0].path, "admin/users");
        assert_eq!(parsed.modules[1].path, "messages");

        let messages = &parsed.modules[1];
        assert_eq!(messages.source, "export const list = query({});\n");
        assert_eq!(messages.source_map, None);
        assert_eq!(
            messages.sha256,
            source_package_digest(messages.source.as_bytes())
        );
        // type info round-trips on the module that carried it.
        let hints = messages.type_info.as_ref().expect("messages type info");
        assert_eq!(hints[0]["name"], "list");
        assert_eq!(hints[0]["hover"], "const list: Query");

        let admin = &parsed.modules[0];
        assert_eq!(admin.source_map.as_deref(), Some("{\"version\":3}"));
        assert_eq!(admin.type_info, None);

        let expected_unpacked: u64 = modules
            .values()
            .map(|module| module.source.len() as u64)
            .sum();
        assert_eq!(parsed.unpacked_bytes, expected_unpacked);
    }

    #[test]
    fn build_is_deterministic_for_dedup() {
        let modules = sample();
        assert_eq!(
            build_source_package(&modules),
            build_source_package(&modules),
            "identical source must build identical bytes for content-addressed dedup"
        );
        // A different ordering of the same entries (BTreeMap normalizes) is equal.
        let mut reordered = BTreeMap::new();
        for (k, v) in modules.iter().rev() {
            reordered.insert(k.clone(), v.clone());
        }
        assert_eq!(
            build_source_package(&modules),
            build_source_package(&reordered)
        );
    }

    #[test]
    fn parse_rejects_non_json() {
        assert!(parse_source_package(b"not json at all").is_err());
    }

    #[test]
    fn parse_rejects_missing_modules() {
        assert!(parse_source_package(br#"{"version":1}"#).is_err());
    }

    #[test]
    fn parse_rejects_module_without_source() {
        let bytes = br#"{"version":1,"modules":{"messages":{"sourceMap":null}}}"#;
        let error = parse_source_package(bytes).expect_err("missing source must fail");
        assert!(
            format!("{error}").contains("missing 'source'"),
            "got: {error}"
        );
    }
}
