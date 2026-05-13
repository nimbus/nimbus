#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import Any

from schema import default_schema_path, validate_payload_against_schema


DEFAULT_CATALOG_PATH = Path("tests/runtime/node/expectations/rust-watchpoints.json")
CATALOG_SCHEMA_PATH = default_schema_path("rust-watchpoints.schema.json")
RUST_NODE_COMPAT_ROOT = Path("crates/nimbus-runtime/src/runtime/tests/node")
RUST_WATCHPOINT_SOURCE_GLOBS = (
    "cases/watchpoints_core.rs",
    "cases/watchpoints_loader_and_tools.rs",
    "cases/watchpoints_extended.rs",
)
PASS_OUTCOMES = {"green", "ok", "pass", "passed", "success"}


def repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def source_paths() -> list[Path]:
    root = repo_root() / RUST_NODE_COMPAT_ROOT
    return [root / relative_path for relative_path in RUST_WATCHPOINT_SOURCE_GLOBS]


def default_catalog_path() -> Path:
    return repo_root() / DEFAULT_CATALOG_PATH


def load_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def lane_for_test_name(test_name: str | None) -> str | None:
    if test_name is None:
        return None
    match = re.match(r"^(node(?:20|22|24))_", test_name)
    if match is None:
        return None
    return match.group(1)


def expectation_for_entry(test_name: str | None, reason: str) -> tuple[str, str]:
    if "diagnostic batch" in reason:
        return ("diagnostic_expected_failure", "local_patch_regression")
    return ("expected_failure", "watchpoint")


def rust_ignore_inventory() -> list[dict[str, Any]]:
    entries: list[dict[str, Any]] = []
    for path in source_paths():
        relative_path = path.relative_to(repo_root())
        lines = path.read_text(encoding="utf-8").splitlines()
        for index, line in enumerate(lines):
            match = re.search(r'#\[ignore = "(?P<reason>.*)"\]', line)
            if match is None:
                continue
            test_name = None
            for probe_line in lines[index + 1 : index + 8]:
                function_match = re.search(r"\bfn\s+([A-Za-z0-9_]+)\s*\(", probe_line)
                if function_match is not None:
                    test_name = function_match.group(1)
                    break
            entries.append(
                {
                    "test_name": test_name,
                    "reason": match.group("reason"),
                    "source_path": str(relative_path),
                    "source_line": index + 1,
                }
            )
    return entries


def catalog_entry_from_inventory(entry: dict[str, Any]) -> dict[str, Any]:
    expectation, classification = expectation_for_entry(
        entry["test_name"], entry["reason"]
    )
    return {
        "id": entry["test_name"],
        "test_name": entry["test_name"],
        "lane": lane_for_test_name(entry["test_name"]),
        "expectation": expectation,
        "classification": classification,
        "reason": entry["reason"],
        "source_path": entry["source_path"],
        "source_line": entry["source_line"],
        "unexpected_pass_action": "remove_ignore_and_promote_or_reclassify_expectation",
    }


def build_catalog() -> dict[str, Any]:
    entries = [catalog_entry_from_inventory(entry) for entry in rust_ignore_inventory()]
    return {
        "schema_version": 1,
        "catalog_kind": "node_compat_rust_watchpoint_expectations",
        "generated_from": "rust_ignore_attributes",
        "source_path": str(RUST_NODE_COMPAT_ROOT),
        "contract": (
            "Each entry mirrors a Rust #[ignore] watchpoint in the Node compatibility "
            "harness. A passing observed result for any cataloged entry is treated as "
            "an unexpected pass and should remove the ignore or reclassify the entry."
        ),
        "entries": entries,
    }


def load_catalog(path: Path) -> dict[str, Any]:
    payload = load_json(path)
    if not isinstance(payload, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return payload


def summarize_catalog(catalog: dict[str, Any]) -> dict[str, Any]:
    entries = catalog_entries(catalog)
    by_expectation: dict[str, int] = {}
    by_classification: dict[str, int] = {}
    for entry in entries:
        by_expectation[entry["expectation"]] = (
            by_expectation.get(entry["expectation"], 0) + 1
        )
        by_classification[entry["classification"]] = (
            by_classification.get(entry["classification"], 0) + 1
        )
    return {
        "entry_count": len(entries),
        "by_expectation": dict(sorted(by_expectation.items())),
        "by_classification": dict(sorted(by_classification.items())),
    }


def catalog_entries(catalog: dict[str, Any]) -> list[dict[str, Any]]:
    entries = catalog.get("entries")
    if not isinstance(entries, list):
        return []
    return [entry for entry in entries if isinstance(entry, dict)]


def validate_catalog(catalog: dict[str, Any]) -> list[dict[str, Any]]:
    errors: list[dict[str, Any]] = validate_payload_against_schema(
        catalog, CATALOG_SCHEMA_PATH
    )
    if catalog.get("schema_version") != 1:
        errors.append(
            {
                "kind": "invalid_schema_version",
                "expected": 1,
                "actual": catalog.get("schema_version"),
            }
        )
    if catalog.get("catalog_kind") != "node_compat_rust_watchpoint_expectations":
        errors.append(
            {
                "kind": "invalid_catalog_kind",
                "actual": catalog.get("catalog_kind"),
            }
        )

    ids: set[str] = set()
    test_names: set[str] = set()
    required_fields = {
        "id",
        "test_name",
        "lane",
        "expectation",
        "classification",
        "reason",
        "source_path",
        "source_line",
        "unexpected_pass_action",
    }
    valid_expectations = {
        "diagnostic_expected_failure",
        "expected_failure",
        "expected_skip",
    }
    for index, entry in enumerate(catalog_entries(catalog)):
        missing_fields = sorted(required_fields - set(entry))
        if missing_fields:
            errors.append(
                {"kind": "entry_missing_fields", "index": index, "fields": missing_fields}
            )
        entry_id = entry.get("id")
        if not isinstance(entry_id, str) or not entry_id:
            errors.append({"kind": "entry_invalid_id", "index": index})
        elif entry_id in ids:
            errors.append({"kind": "duplicate_entry_id", "id": entry_id})
        else:
            ids.add(entry_id)
        test_name = entry.get("test_name")
        if not isinstance(test_name, str) or not test_name:
            errors.append({"kind": "entry_invalid_test_name", "index": index})
        elif test_name in test_names:
            errors.append({"kind": "duplicate_test_name", "test_name": test_name})
        else:
            test_names.add(test_name)
        if entry.get("expectation") not in valid_expectations:
            errors.append(
                {
                    "kind": "entry_invalid_expectation",
                    "index": index,
                    "expectation": entry.get("expectation"),
                }
            )
        source_path = entry.get("source_path")
        if not isinstance(source_path, str) or not source_path:
            errors.append(
                {
                    "kind": "entry_invalid_source_path",
                    "index": index,
                    "source_path": source_path,
                }
            )
        if not isinstance(entry.get("source_line"), int) or entry["source_line"] < 1:
            errors.append({"kind": "entry_invalid_source_line", "index": index})
    return errors


def validate_catalog_against_inventory(
    catalog: dict[str, Any],
    inventory: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    errors = validate_catalog(catalog)
    catalog_by_test = {entry.get("test_name"): entry for entry in catalog_entries(catalog)}
    inventory_by_test = {entry.get("test_name"): entry for entry in inventory}

    for test_name in sorted(set(inventory_by_test) - set(catalog_by_test)):
        errors.append({"kind": "catalog_missing_rust_ignore", "test_name": test_name})
    for test_name in sorted(set(catalog_by_test) - set(inventory_by_test)):
        errors.append({"kind": "catalog_stale_rust_ignore", "test_name": test_name})

    for test_name in sorted(set(inventory_by_test) & set(catalog_by_test)):
        inventory_entry = inventory_by_test[test_name]
        catalog_entry = catalog_by_test[test_name]
        for field in ("reason", "source_path", "source_line"):
            if catalog_entry.get(field) != inventory_entry.get(field):
                errors.append(
                    {
                        "kind": f"catalog_{field}_mismatch",
                        "test_name": test_name,
                        "expected": inventory_entry.get(field),
                        "actual": catalog_entry.get(field),
                    }
                )
        expected_expectation, expected_classification = expectation_for_entry(
            inventory_entry["test_name"], inventory_entry["reason"]
        )
        if catalog_entry.get("expectation") != expected_expectation:
            errors.append(
                {
                    "kind": "catalog_expectation_mismatch",
                    "test_name": test_name,
                    "expected": expected_expectation,
                    "actual": catalog_entry.get("expectation"),
                }
            )
        if catalog_entry.get("classification") != expected_classification:
            errors.append(
                {
                    "kind": "catalog_classification_mismatch",
                    "test_name": test_name,
                    "expected": expected_classification,
                    "actual": catalog_entry.get("classification"),
                }
            )
    return errors


def observed_result_entries(payload: Any) -> list[dict[str, Any]]:
    if isinstance(payload, list):
        return [entry for entry in payload if isinstance(entry, dict)]
    if not isinstance(payload, dict):
        return []
    for key in ("results", "tests", "entries"):
        value = payload.get(key)
        if isinstance(value, list):
            return [entry for entry in value if isinstance(entry, dict)]
    return []


def detect_unexpected_passes(
    catalog: dict[str, Any],
    observed_results: Any,
) -> list[dict[str, Any]]:
    catalog_by_name = {
        entry["test_name"]: entry
        for entry in catalog_entries(catalog)
        if isinstance(entry.get("test_name"), str)
    }
    unexpected: list[dict[str, Any]] = []
    for result in observed_result_entries(observed_results):
        test_name = result.get("test_name") or result.get("name") or result.get("id")
        outcome = result.get("outcome") or result.get("status") or result.get("result")
        if (
            isinstance(test_name, str)
            and isinstance(outcome, str)
            and outcome.lower() in PASS_OUTCOMES
            and test_name in catalog_by_name
        ):
            catalog_entry = catalog_by_name[test_name]
            unexpected.append(
                {
                    "kind": "unexpected_pass",
                    "test_name": test_name,
                    "outcome": outcome,
                    "expectation": catalog_entry["expectation"],
                    "classification": catalog_entry["classification"],
                    "action": catalog_entry["unexpected_pass_action"],
                }
            )
    return unexpected


def write_catalog(catalog: dict[str, Any], path: Path) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(catalog, indent=2) + "\n", encoding="utf-8")
    return path


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Manage the Node compatibility Rust watchpoint catalog"
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    sync_parser = subparsers.add_parser(
        "sync", help="regenerate the Rust watchpoint catalog"
    )
    sync_parser.add_argument("--catalog", default=str(default_catalog_path()))

    validate_parser = subparsers.add_parser(
        "validate", help="validate the Rust watchpoint catalog against #[ignore] data"
    )
    validate_parser.add_argument("--catalog", default=str(default_catalog_path()))
    validate_parser.add_argument(
        "--observed-results",
        help="optional JSON results file used to flag unexpected passes",
    )
    return parser


def sync(args: argparse.Namespace) -> int:
    path = Path(args.catalog).resolve()
    write_catalog(build_catalog(), path)
    print(f"wrote node-compat watchpoint catalog to {path}")
    return 0


def validate(args: argparse.Namespace) -> int:
    path = Path(args.catalog).resolve()
    catalog = load_catalog(path)
    errors = validate_catalog_against_inventory(catalog, rust_ignore_inventory())
    if args.observed_results:
        errors.extend(
            detect_unexpected_passes(catalog, load_json(Path(args.observed_results)))
        )
    if errors:
        for error in errors:
            print(f"error: {json.dumps(error, sort_keys=True)}")
        return 1
    summary = summarize_catalog(catalog)
    print(
        f"validated node-compat watchpoint catalog: {summary['entry_count']} entries"
    )
    return 0


def main() -> int:
    args = build_parser().parse_args()
    if args.command == "sync":
        return sync(args)
    if args.command == "validate":
        return validate(args)
    raise AssertionError(f"unhandled command {args.command}")


if __name__ == "__main__":
    raise SystemExit(main())
