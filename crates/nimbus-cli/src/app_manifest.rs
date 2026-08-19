//! Order-preserving edits to a developer app's `package.json`.
//!
//! Nimbus rewrites one thing in an app's manifest: the dependency spec for an
//! adapter's root package, pointed at the binary-provisioned copy under
//! `.nimbus/packages/` (BPD3). Rewriting it is only half a contract — undoing
//! it requires knowing what was there before, and `package.json` is strict
//! JSON, so the answer cannot live in a comment beside the line it explains.
//!
//! It lives instead under a top-level `"nimbus"` key in the same file:
//!
//! ```json
//! "nimbus": { "packages": { "convex": { "previous": "^1.17.0" } } }
//! ```
//!
//! npm ignores unknown top-level keys, so the record travels with the app
//! through git, clones, and `rm -rf .nimbus` — unlike a sidecar under the
//! disposable `.nimbus/` tree, which would strand a wired manifest with no way
//! back. `{ "detached": true }` records the opposite state: the developer ran
//! `nimbus packages uninstall`, and the automatic wiring in `provision::ensure`
//! must not silently undo that on the next `dev`, `codegen`, or `deploy`.
//!
//! Absence of a record is meaningful and is the common case. A scaffold and an
//! app where Nimbus added the dependency itself both need the same undo —
//! remove the key — so neither writes a record, and the manifests of apps that
//! never had a registry dependency stay untouched.
//!
//! Every edit preserves the source order, raw text, and indentation of the
//! manifest it edits, so a rewrite shows up as a one-line diff.

use serde::Deserialize;

/// What Nimbus recorded about an adapter root package in an app's manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WiringRecord {
    /// Wiring displaced a registry spec; `uninstall` restores exactly this.
    Restorable { previous: String },
    /// `uninstall` detached the package; automatic wiring must not re-apply.
    Detached,
}

const NIMBUS_KEY: &str = "nimbus";
const PACKAGES_KEY: &str = "packages";
const DEPENDENCIES_KEY: &str = "dependencies";

/// The current `dependencies[name]` spec, or `None` when the dependency (or
/// the `dependencies` object) is absent.
pub(crate) fn dependency_spec(text: &str, name: &str) -> Result<Option<String>, String> {
    let top = parse_object(text)?;
    let Some(raw) = entry(&top, DEPENDENCIES_KEY) else {
        return Ok(None);
    };
    let deps = parse_nested(raw.get(), DEPENDENCIES_KEY)?;
    let Some(spec) = entry(&deps, name) else {
        return Ok(None);
    };
    serde_json::from_str::<String>(spec.get())
        .map(Some)
        .map_err(|e| format!("`dependencies.{name}` is not a string: {e}"))
}

/// Whether any dependency still points into the provisioned package tree.
/// `uninstall` uses this to decide whether the staged payload is still in use.
pub(crate) fn has_dependency_with_prefix(text: &str, prefix: &str) -> Result<bool, String> {
    let top = parse_object(text)?;
    let Some(raw) = entry(&top, DEPENDENCIES_KEY) else {
        return Ok(false);
    };
    let deps = parse_nested(raw.get(), DEPENDENCIES_KEY)?;
    Ok(deps.iter().any(|(_, value)| {
        serde_json::from_str::<String>(value.get()).is_ok_and(|spec| spec.starts_with(prefix))
    }))
}

/// Rewrite `package.json` text so `dependencies[name] = spec`. Existing entries
/// keep their source order and raw value text; a missing dependency is inserted
/// at its npm-sorted position, and a missing `dependencies` object is appended.
/// Returns `Ok(None)` when the dependency already carries exactly `spec`.
pub(crate) fn set_dependency(text: &str, name: &str, spec: &str) -> Result<Option<String>, String> {
    let unit = indent_unit(text);
    let mut top = parse_object(text)?;
    let spec_json = raw_string(&serde_json::Value::String(spec.to_string()).to_string())?;

    if let Some(deps_value) = entry_mut(&mut top, DEPENDENCIES_KEY) {
        let mut deps = parse_nested(deps_value.get(), DEPENDENCIES_KEY)?;
        if let Some(dep) = deps.iter_mut().find(|(key, _)| key == name) {
            if dep.1.get() == spec_json.get() {
                return Ok(None);
            }
            dep.1 = spec_json;
        } else {
            insert_sorted(&mut deps, name, spec_json);
        }
        *deps_value = raw_string(&render_object(&deps, 2, unit))?;
    } else {
        let deps = vec![(name.to_string(), spec_json)];
        top.push((
            DEPENDENCIES_KEY.to_string(),
            raw_string(&render_object(&deps, 2, unit))?,
        ));
    }
    Ok(Some(render_document(&top, unit)))
}

/// Rewrite `package.json` text with `dependencies[name]` removed, pruning an
/// emptied `dependencies` object. Returns `Ok(None)` when it was already absent.
pub(crate) fn remove_dependency(text: &str, name: &str) -> Result<Option<String>, String> {
    let unit = indent_unit(text);
    let mut top = parse_object(text)?;
    let Some(deps_value) = entry_mut(&mut top, DEPENDENCIES_KEY) else {
        return Ok(None);
    };
    let mut deps = parse_nested(deps_value.get(), DEPENDENCIES_KEY)?;
    let Some(position) = deps.iter().position(|(key, _)| key == name) else {
        return Ok(None);
    };
    deps.remove(position);
    if deps.is_empty() {
        remove_entry(&mut top, DEPENDENCIES_KEY);
    } else {
        *deps_value = raw_string(&render_object(&deps, 2, unit))?;
    }
    Ok(Some(render_document(&top, unit)))
}

/// The recorded wiring state for `name`, or `None` when the manifest carries no
/// record — a scaffold, or an app whose dependency Nimbus added itself.
pub(crate) fn wiring_record(text: &str, name: &str) -> Result<Option<WiringRecord>, String> {
    let top = parse_object(text)?;
    let Some(nimbus) = entry(&top, NIMBUS_KEY) else {
        return Ok(None);
    };
    let nimbus = parse_nested(nimbus.get(), NIMBUS_KEY)?;
    let Some(packages) = entry(&nimbus, PACKAGES_KEY) else {
        return Ok(None);
    };
    let packages = parse_nested(packages.get(), PACKAGES_KEY)?;
    let Some(record) = entry(&packages, name) else {
        return Ok(None);
    };

    #[derive(Deserialize)]
    struct Entry {
        #[serde(default)]
        previous: Option<String>,
        #[serde(default)]
        detached: bool,
    }
    let entry: Entry = serde_json::from_str(record.get())
        .map_err(|e| format!("`{NIMBUS_KEY}.{PACKAGES_KEY}.{name}` is malformed: {e}"))?;
    if entry.detached {
        return Ok(Some(WiringRecord::Detached));
    }
    Ok(entry
        .previous
        .map(|previous| WiringRecord::Restorable { previous }))
}

/// Rewrite `package.json` text so the wiring record for `name` is `record`, or
/// removed when `record` is `None`. Prunes the `packages` and `nimbus` objects
/// once empty, so an app that returns to its original state also returns to its
/// original manifest shape. Returns `Ok(None)` when nothing changes.
pub(crate) fn set_wiring_record(
    text: &str,
    name: &str,
    record: Option<&WiringRecord>,
) -> Result<Option<String>, String> {
    let unit = indent_unit(text);
    let mut top = parse_object(text)?;
    let mut nimbus = match entry(&top, NIMBUS_KEY) {
        Some(raw) => parse_nested(raw.get(), NIMBUS_KEY)?,
        None => Vec::new(),
    };
    let mut packages = match entry(&nimbus, PACKAGES_KEY) {
        Some(raw) => parse_nested(raw.get(), PACKAGES_KEY)?,
        None => Vec::new(),
    };

    let wanted = match record {
        Some(WiringRecord::Restorable { previous }) => Some(format!(
            "{{ \"previous\": {} }}",
            serde_json::Value::String(previous.clone())
        )),
        Some(WiringRecord::Detached) => Some("{ \"detached\": true }".to_string()),
        None => None,
    };

    match wanted {
        Some(body) => {
            let value = raw_string(&body)?;
            if let Some(existing) = packages.iter_mut().find(|(key, _)| key == name) {
                if existing.1.get() == value.get() {
                    return Ok(None);
                }
                existing.1 = value;
            } else {
                insert_sorted(&mut packages, name, value);
            }
        }
        None => {
            let Some(position) = packages.iter().position(|(key, _)| key == name) else {
                return Ok(None);
            };
            packages.remove(position);
        }
    }

    if packages.is_empty() {
        remove_entry(&mut nimbus, PACKAGES_KEY);
    } else {
        upsert(
            &mut nimbus,
            PACKAGES_KEY,
            raw_string(&render_object(&packages, 3, unit))?,
        );
    }
    if nimbus.is_empty() {
        remove_entry(&mut top, NIMBUS_KEY);
    } else {
        upsert(
            &mut top,
            NIMBUS_KEY,
            raw_string(&render_object(&nimbus, 2, unit))?,
        );
    }
    Ok(Some(render_document(&top, unit)))
}

/// A JSON object's entries in source order; values keep their original raw
/// text so one entry can be rewritten without reformatting the rest. (The
/// workspace `serde_json` deliberately does not enable `preserve_order`, so
/// `serde_json::Map` cannot do this.)
struct ObjectEntries(Vec<(String, Box<serde_json::value::RawValue>)>);

type Entries = Vec<(String, Box<serde_json::value::RawValue>)>;

impl<'de> serde::Deserialize<'de> for ObjectEntries {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct EntriesVisitor;
        impl<'de> serde::de::Visitor<'de> for EntriesVisitor {
            type Value = ObjectEntries;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a JSON object")
            }

            fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut entries = Vec::new();
                while let Some(entry) =
                    access.next_entry::<String, Box<serde_json::value::RawValue>>()?
                {
                    entries.push(entry);
                }
                Ok(ObjectEntries(entries))
            }
        }
        deserializer.deserialize_map(EntriesVisitor)
    }
}

fn parse_object(text: &str) -> Result<Entries, String> {
    serde_json::from_str::<ObjectEntries>(text)
        .map(|ObjectEntries(entries)| entries)
        .map_err(|e| format!("invalid JSON: {e}"))
}

fn parse_nested(text: &str, key: &str) -> Result<Entries, String> {
    serde_json::from_str::<ObjectEntries>(text)
        .map(|ObjectEntries(entries)| entries)
        .map_err(|e| format!("invalid `{key}` object: {e}"))
}

fn raw_string(value: &str) -> Result<Box<serde_json::value::RawValue>, String> {
    serde_json::value::RawValue::from_string(value.to_string()).map_err(|e| e.to_string())
}

fn entry<'a>(entries: &'a Entries, key: &str) -> Option<&'a serde_json::value::RawValue> {
    entries
        .iter()
        .find_map(|(entry_key, value)| (entry_key == key).then_some(&**value))
}

fn entry_mut<'a>(
    entries: &'a mut Entries,
    key: &str,
) -> Option<&'a mut Box<serde_json::value::RawValue>> {
    entries
        .iter_mut()
        .find_map(|(entry_key, value)| (entry_key == key).then_some(value))
}

/// Replace `key`'s value, appending the entry when it is absent.
fn upsert(entries: &mut Entries, key: &str, value: Box<serde_json::value::RawValue>) {
    match entry_mut(entries, key) {
        Some(existing) => *existing = value,
        None => entries.push((key.to_string(), value)),
    }
}

/// Insert at the npm-sorted position, matching how npm orders a manifest's
/// dependency keys.
fn insert_sorted(entries: &mut Entries, key: &str, value: Box<serde_json::value::RawValue>) {
    let position = entries
        .iter()
        .position(|(entry_key, _)| entry_key.as_str() > key)
        .unwrap_or(entries.len());
    entries.insert(position, (key.to_string(), value));
}

fn remove_entry(entries: &mut Entries, key: &str) {
    entries.retain(|(entry_key, _)| entry_key != key);
}

/// The indentation one level of this manifest already uses. A rewrite has to
/// match the file it edits: re-indenting to a fixed unit turns a one-line
/// change into a whole-file diff, and leaves untouched nested objects -- which
/// keep their raw source text -- indented differently from the keys around
/// them. Falls back to two spaces for a manifest with no indented line at all.
fn indent_unit(text: &str) -> &str {
    text.lines()
        .find_map(|line| {
            let indent_len = line
                .find(|c: char| c != ' ' && c != '\t')
                .filter(|len| *len > 0)?;
            Some(&line[..indent_len])
        })
        .unwrap_or("  ")
}

fn render_document(top: &Entries, unit: &str) -> String {
    format!("{}\n", render_object(top, 1, unit))
}

/// Render object entries indented with `unit`: entries at `depth` levels, the
/// closing brace one level out. Raw multi-line values embed as-is.
fn render_object(entries: &Entries, depth: usize, unit: &str) -> String {
    if entries.is_empty() {
        return "{}".to_string();
    }
    let inner = unit.repeat(depth);
    let outer = unit.repeat(depth - 1);
    let body = entries
        .iter()
        .map(|(key, value)| {
            format!(
                "{inner}{}: {}",
                serde_json::Value::String(key.clone()),
                value.get()
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    format!("{{\n{body}\n{outer}}}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: &str = concat!(
        "{\n",
        "  \"name\": \"my-app\",\n",
        "  \"dependencies\": {\n",
        "    \"convex\": \"^1.17.0\",\n",
        "    \"react\": \"^19.0.0\"\n",
        "  }\n",
        "}\n",
    );

    #[test]
    fn recording_then_clearing_returns_the_original_manifest_shape() {
        let recorded = set_wiring_record(
            APP,
            "convex",
            Some(&WiringRecord::Restorable {
                previous: "^1.17.0".to_string(),
            }),
        )
        .unwrap()
        .expect("a new record rewrites the manifest");
        assert_eq!(
            recorded,
            concat!(
                "{\n",
                "  \"name\": \"my-app\",\n",
                "  \"dependencies\": {\n",
                "    \"convex\": \"^1.17.0\",\n",
                "    \"react\": \"^19.0.0\"\n",
                "  },\n",
                "  \"nimbus\": {\n",
                "    \"packages\": {\n",
                "      \"convex\": { \"previous\": \"^1.17.0\" }\n",
                "    }\n",
                "  }\n",
                "}\n",
            ),
        );
        assert_eq!(
            wiring_record(&recorded, "convex").unwrap(),
            Some(WiringRecord::Restorable {
                previous: "^1.17.0".to_string()
            })
        );

        let cleared = set_wiring_record(&recorded, "convex", None)
            .unwrap()
            .expect("clearing the last record rewrites the manifest");
        assert_eq!(
            cleared, APP,
            "emptied `packages` and `nimbus` objects must be pruned, not left behind"
        );
    }

    #[test]
    fn recording_is_idempotent() {
        let record = WiringRecord::Detached;
        let first = set_wiring_record(APP, "convex", Some(&record))
            .unwrap()
            .unwrap();
        assert_eq!(
            set_wiring_record(&first, "convex", Some(&record)).unwrap(),
            None,
            "an unchanged record must not rewrite the file"
        );
        assert_eq!(
            wiring_record(&first, "convex").unwrap(),
            Some(WiringRecord::Detached)
        );
    }

    /// A detached entry answers "do not wire" even though it carries no
    /// `previous`, so the two states must not be confused for one another.
    #[test]
    fn detached_and_restorable_are_distinct_states() {
        let detached =
            "{\n  \"nimbus\": { \"packages\": { \"convex\": { \"detached\": true } } }\n}\n";
        assert_eq!(
            wiring_record(detached, "convex").unwrap(),
            Some(WiringRecord::Detached)
        );
        assert_eq!(
            wiring_record("{}\n", "convex").unwrap(),
            None,
            "no record is its own state: wire freely, remove on uninstall"
        );
    }

    #[test]
    fn removing_the_last_dependency_prunes_the_dependencies_object() {
        let one =
            "{\n  \"name\": \"x\",\n  \"dependencies\": {\n    \"convex\": \"^1.17.0\"\n  }\n}\n";
        assert_eq!(
            remove_dependency(one, "convex").unwrap().unwrap(),
            "{\n  \"name\": \"x\"\n}\n"
        );
        assert_eq!(
            remove_dependency(APP, "absent").unwrap(),
            None,
            "removing what is not there must not rewrite the file"
        );
        assert_eq!(
            remove_dependency(APP, "convex").unwrap().unwrap(),
            "{\n  \"name\": \"my-app\",\n  \"dependencies\": {\n    \"react\": \"^19.0.0\"\n  }\n}\n",
        );
    }

    #[test]
    fn provisioned_dependencies_are_detected_by_prefix() {
        let wired = set_dependency(APP, "convex", "file:./.nimbus/packages/convex")
            .unwrap()
            .unwrap();
        assert!(has_dependency_with_prefix(&wired, "file:./.nimbus/packages/").unwrap());
        assert!(!has_dependency_with_prefix(APP, "file:./.nimbus/packages/").unwrap());
        assert!(
            !has_dependency_with_prefix("{}\n", "file:./.nimbus/packages/").unwrap(),
            "a manifest with no dependencies holds nothing in use"
        );
    }

    /// A manifest that does not use two spaces must come back in its own
    /// indentation. Re-indenting the keys around an untouched nested object --
    /// which keeps its raw source text -- would both mix the two styles in one
    /// file and turn every one-line edit into a whole-file diff.
    #[test]
    fn edits_keep_the_manifest_indentation() {
        let four = concat!(
            "{\n",
            "    \"name\": \"x\",\n",
            "    \"scripts\": {\n",
            "        \"build\": \"tsc\"\n",
            "    },\n",
            "    \"dependencies\": {\n",
            "        \"convex\": \"^1.17.0\"\n",
            "    }\n",
            "}\n",
        );
        assert_eq!(
            set_dependency(four, "convex", "file:./.nimbus/packages/convex")
                .unwrap()
                .unwrap(),
            concat!(
                "{\n",
                "    \"name\": \"x\",\n",
                "    \"scripts\": {\n",
                "        \"build\": \"tsc\"\n",
                "    },\n",
                "    \"dependencies\": {\n",
                "        \"convex\": \"file:./.nimbus/packages/convex\"\n",
                "    }\n",
                "}\n",
            ),
        );

        let tabbed = "{\n\t\"dependencies\": {\n\t\t\"convex\": \"^1.17.0\"\n\t}\n}\n";
        assert_eq!(
            set_wiring_record(
                tabbed,
                "convex",
                Some(&WiringRecord::Restorable {
                    previous: "^1.17.0".to_string()
                }),
            )
            .unwrap()
            .unwrap(),
            concat!(
                "{\n",
                "\t\"dependencies\": {\n",
                "\t\t\"convex\": \"^1.17.0\"\n",
                "\t},\n",
                "\t\"nimbus\": {\n",
                "\t\t\"packages\": {\n",
                "\t\t\t\"convex\": { \"previous\": \"^1.17.0\" }\n",
                "\t\t}\n",
                "\t}\n",
                "}\n",
            ),
        );
    }

    #[test]
    fn malformed_records_are_reported_not_ignored() {
        let bad = "{\n  \"nimbus\": { \"packages\": { \"convex\": 7 } }\n}\n";
        let error = wiring_record(bad, "convex").expect_err("a non-object record must be an error");
        assert!(
            error.contains("nimbus.packages.convex"),
            "the error must name the offending key: {error}"
        );
    }
}
