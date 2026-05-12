#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from expectations import (  # noqa: E402
    DEFAULT_CATALOG_PATH,
    detect_unexpected_passes,
    load_catalog,
    load_json as load_expectations_json,
    rust_ignore_inventory,
    summarize_catalog,
    validate_catalog_against_inventory,
)


TEST_FILE_SUFFIXES = {".js", ".mjs", ".cjs"}
VALID_LANE_CLASSIFICATION_EXPECTATIONS = {
    "expected_failure",
    "expected_gap",
    "expected_skip",
}
RUST_NODE_COMPAT_PATH = Path("crates/neovex-runtime/src/runtime/tests/node_compat.rs")


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def manifest_root() -> Path:
    return (
        repo_root()
        / "crates"
        / "neovex-runtime"
        / "src"
        / "runtime"
        / "tests"
        / "node_compat_manifests"
    )


def default_output_root() -> Path:
    return repo_root() / "target" / "node-compat" / "status"


def load_json(path: Path) -> dict:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def lane_metadata_files() -> list[Path]:
    return sorted((manifest_root() / "lanes").glob("*.json"))


def family_catalog_files() -> list[Path]:
    return sorted((manifest_root() / "fixtures").glob("*.json"))


def is_node_test_file(path: Path) -> bool:
    return path.name.startswith("test-") and path.suffix in TEST_FILE_SUFFIXES


def normalize_rust_fixture_path(path: str) -> str:
    return re.sub(r"^node(?:20|22|24)/", "", path)


def discover_fixture_files(fixture_root: Path) -> list[str]:
    return sorted(
        str(path.relative_to(fixture_root))
        for path in fixture_root.rglob("*")
        if path.is_file() and is_node_test_file(path)
    )


def extract_rust_referenced_tests(vendored_tests: set[str]) -> list[str]:
    source = repo_root() / RUST_NODE_COMPAT_PATH
    text = source.read_text(encoding="utf-8")
    pattern = re.compile(
        r'"((?:node(?:20|22|24)/)?test/[^"\\]*(?:\.js|\.mjs|\.cjs))"'
    )
    referenced: set[str] = set()
    for match in pattern.finditer(text):
        candidate = normalize_rust_fixture_path(match.group(1))
        if candidate in vendored_tests:
            referenced.add(candidate)
    return sorted(referenced)


def documented_manifested_count(manifest_doc: Path, lane: str) -> int | None:
    lane_label_by_id = {
        "node20": "Node20 validation lane",
        "node22": "Node22 primary lane",
        "node24": "Node24 preview lane",
    }
    label = lane_label_by_id[lane]
    pattern = re.compile(
        rf"- {re.escape(label)}: `(?P<count>[0-9]+)` (?:staged )?official files"
    )
    text = manifest_doc.read_text(encoding="utf-8")
    match = pattern.search(text)
    if match is None:
        return None
    return int(match.group("count"))


def public_node22_claim_count() -> int | None:
    doc_path = repo_root() / "docs/architecture/runtime/deno-vs-neovex-node-compat.md"
    if not doc_path.is_file():
        return None
    text = doc_path.read_text(encoding="utf-8")
    match = re.search(r"Official Node test files green \(Node22\).*?\|\s*([0-9]+)\+?", text)
    if match is None:
        return None
    return int(match.group(1))


def default_expectation_catalog_path() -> Path:
    return repo_root() / DEFAULT_CATALOG_PATH


def lane_classification_catalog_path(lane: str) -> Path:
    return repo_root() / "tests" / "node-compat" / "classifications" / f"{lane}.json"


def display_path(path: Path) -> str:
    try:
        return str(path.relative_to(repo_root()))
    except ValueError:
        return str(path)


def build_expectation_summary(
    rust_ignores: list[dict],
    catalog_path: Path,
    observed_results_path: Path | None,
) -> dict:
    summary: dict = {
        "catalog_path": display_path(catalog_path),
        "catalog_present": catalog_path.is_file(),
        "catalog_entry_count": 0,
        "by_expectation": {},
        "by_classification": {},
        "validation_errors": [],
        "observed_results_path": (
            display_path(observed_results_path)
            if observed_results_path is not None
            else None
        ),
        "unexpected_passes": [],
    }
    if not catalog_path.is_file():
        summary["validation_errors"].append(
            {
                "kind": "missing_expectation_catalog",
                "catalog_path": summary["catalog_path"],
            }
        )
        return summary

    catalog = load_catalog(catalog_path)
    catalog_summary = summarize_catalog(catalog)
    summary.update(
        {
            "catalog_entry_count": catalog_summary["entry_count"],
            "by_expectation": catalog_summary["by_expectation"],
            "by_classification": catalog_summary["by_classification"],
            "validation_errors": validate_catalog_against_inventory(
                catalog, rust_ignores
            ),
        }
    )
    if observed_results_path is not None:
        observed_results = load_expectations_json(observed_results_path)
        summary["unexpected_passes"] = detect_unexpected_passes(
            catalog, observed_results
        )
    return summary


def build_family_summaries(lanes: list[dict]) -> list[dict]:
    summaries: list[dict] = []
    lane_ids = [lane["lane"] for lane in lanes]
    for path in family_catalog_files():
        catalog = load_json(path)
        manifest_doc = repo_root() / catalog["manifest_doc"]
        if "node-lts-compat/manifests" not in catalog["manifest_doc"]:
            continue
        lane_counts: dict[str, int | None] = {}
        missing_lanes: list[str] = []
        for lane in lane_ids:
            count = documented_manifested_count(manifest_doc, lane)
            lane_counts[lane] = count
            if count is None:
                missing_lanes.append(lane)
        summaries.append(
            {
                "family": catalog["family"],
                "nlc_item": catalog["nlc_item"],
                "manifest_doc": catalog["manifest_doc"],
                "failure_doc": catalog["failure_doc"],
                "documented_manifested_green_by_lane": lane_counts,
                "has_complete_lane_counts": not missing_lanes,
                "missing_count_lanes": missing_lanes,
            }
        )
    return summaries


def validate_lane_classification_catalog(
    lane: str,
    catalog: dict,
    fixtures: set[str],
) -> list[dict]:
    errors: list[dict] = []
    if catalog.get("schema_version") != 1:
        errors.append(
            {
                "kind": "lane_classification_invalid_schema_version",
                "lane": lane,
                "actual": catalog.get("schema_version"),
            }
        )
    if catalog.get("catalog_kind") != "node_compat_lane_classifications":
        errors.append(
            {
                "kind": "lane_classification_invalid_catalog_kind",
                "lane": lane,
                "actual": catalog.get("catalog_kind"),
            }
        )
    if catalog.get("lane") != lane:
        errors.append(
            {
                "kind": "lane_classification_lane_mismatch",
                "lane": lane,
                "actual": catalog.get("lane"),
            }
        )
    seen_paths: set[str] = set()
    required_fields = {"test_path", "expectation", "classification", "owner", "reason"}
    entries = catalog.get("entries", [])
    if not isinstance(entries, list):
        errors.append(
            {
                "kind": "lane_classification_entries_not_array",
                "lane": lane,
            }
        )
        entries = []
    for index, entry in enumerate(entries):
        if not isinstance(entry, dict):
            errors.append(
                {
                    "kind": "lane_classification_entry_not_object",
                    "lane": lane,
                    "index": index,
                }
            )
            continue
        errors.extend(
            validate_lane_classification_entry(
                lane=lane,
                entry=entry,
                source=f"entries[{index}]",
                fixtures=fixtures,
                seen_paths=seen_paths,
                required_fields=required_fields,
            )
        )

    groups = catalog.get("groups", [])
    if not isinstance(groups, list):
        errors.append(
            {
                "kind": "lane_classification_groups_not_array",
                "lane": lane,
            }
        )
        groups = []
    group_required_fields = {
        "id",
        "test_paths",
        "expectation",
        "classification",
        "owner",
        "reason",
    }
    seen_group_ids: set[str] = set()
    for group_index, group in enumerate(groups):
        if not isinstance(group, dict):
            errors.append(
                {
                    "kind": "lane_classification_group_not_object",
                    "lane": lane,
                    "index": group_index,
                }
            )
            continue
        missing = sorted(group_required_fields - set(group))
        if missing:
            errors.append(
                {
                    "kind": "lane_classification_group_missing_fields",
                    "lane": lane,
                    "index": group_index,
                    "fields": missing,
                }
            )
        group_id = group.get("id")
        if not isinstance(group_id, str) or not group_id:
            errors.append(
                {
                    "kind": "lane_classification_invalid_group_id",
                    "lane": lane,
                    "index": group_index,
                }
            )
            continue
        if group_id in seen_group_ids:
            errors.append(
                {
                    "kind": "lane_classification_duplicate_group_id",
                    "lane": lane,
                    "group_id": group_id,
                }
            )
        seen_group_ids.add(group_id)
        test_paths = group.get("test_paths")
        if not isinstance(test_paths, list):
            errors.append(
                {
                    "kind": "lane_classification_group_test_paths_not_array",
                    "lane": lane,
                    "group_id": group_id,
                }
            )
            continue
        if not test_paths:
            errors.append(
                {
                    "kind": "lane_classification_group_empty_test_paths",
                    "lane": lane,
                    "group_id": group_id,
                }
            )
        for field in ("classification", "owner", "reason"):
            if not isinstance(group.get(field), str) or not group[field]:
                errors.append(
                    {
                        "kind": f"lane_classification_group_invalid_{field}",
                        "lane": lane,
                        "group_id": group_id,
                    }
                )
        if group.get("expectation") not in VALID_LANE_CLASSIFICATION_EXPECTATIONS:
            errors.append(
                {
                    "kind": "lane_classification_group_invalid_expectation",
                    "lane": lane,
                    "group_id": group_id,
                    "expectation": group.get("expectation"),
                }
            )
        for path_index, test_path in enumerate(test_paths):
            entry = {
                "test_path": test_path,
                "expectation": group.get("expectation"),
                "classification": group.get("classification"),
                "owner": group.get("owner"),
                "reason": group.get("reason"),
            }
            errors.extend(
                validate_lane_classification_entry(
                    lane=lane,
                    entry=entry,
                    source=f"groups[{group_index}].test_paths[{path_index}]",
                    fixtures=fixtures,
                    seen_paths=seen_paths,
                    required_fields=required_fields,
                    group_id=group_id,
                )
            )
    return errors


def validate_lane_classification_entry(
    *,
    lane: str,
    entry: dict,
    source: str,
    fixtures: set[str],
    seen_paths: set[str],
    required_fields: set[str],
    group_id: str | None = None,
) -> list[dict]:
    errors: list[dict] = []
    missing = sorted(required_fields - set(entry))
    if missing:
        errors.append(
            {
                "kind": "lane_classification_entry_missing_fields",
                "lane": lane,
                "source": source,
                "fields": missing,
            }
        )
    test_path = entry.get("test_path")
    if not isinstance(test_path, str) or not test_path:
        errors.append(
            {
                "kind": "lane_classification_invalid_test_path",
                "lane": lane,
                "source": source,
                "group_id": group_id,
            }
        )
        return errors
    if test_path in seen_paths:
        errors.append(
            {
                "kind": "lane_classification_duplicate_test_path",
                "lane": lane,
                "test_path": test_path,
                "source": source,
                "group_id": group_id,
            }
        )
    seen_paths.add(test_path)
    normalized_path = test_path[5:] if test_path.startswith("test/") else test_path
    if normalized_path not in fixtures:
        errors.append(
            {
                "kind": "lane_classification_unknown_fixture",
                "lane": lane,
                "test_path": test_path,
                "source": source,
                "group_id": group_id,
            }
        )
    if entry.get("expectation") not in VALID_LANE_CLASSIFICATION_EXPECTATIONS:
        errors.append(
            {
                "kind": "lane_classification_invalid_expectation",
                "lane": lane,
                "test_path": test_path,
                "source": source,
                "group_id": group_id,
                "expectation": entry.get("expectation"),
            }
        )
    for field in ("classification", "owner", "reason"):
        if not isinstance(entry.get(field), str) or not entry[field]:
            errors.append(
                {
                    "kind": f"lane_classification_invalid_{field}",
                    "lane": lane,
                    "test_path": test_path,
                    "source": source,
                    "group_id": group_id,
                }
            )
    return errors


def expanded_lane_classification_entries(catalog: dict) -> list[dict]:
    entries: list[dict] = []
    for entry in catalog.get("entries", []):
        if isinstance(entry, dict):
            entries.append(entry)
    for group in catalog.get("groups", []):
        if not isinstance(group, dict):
            continue
        test_paths = group.get("test_paths")
        if not isinstance(test_paths, list):
            continue
        for test_path in test_paths:
            entries.append(
                {
                    "test_path": test_path,
                    "expectation": group.get("expectation"),
                    "classification": group.get("classification"),
                    "owner": group.get("owner"),
                    "reason": group.get("reason"),
                    "group_id": group.get("id"),
                }
            )
    return entries


def build_lane_classification_summary(lane: str, fixtures: set[str]) -> dict:
    path = lane_classification_catalog_path(lane)
    summary = {
        "catalog_path": display_path(path),
        "catalog_present": path.is_file(),
        "classified_non_green_count": 0,
        "by_expectation": {},
        "by_classification": {},
        "validation_errors": [],
        "entries": [],
    }
    if not path.is_file():
        return summary

    catalog = load_json(path)
    errors = validate_lane_classification_catalog(lane, catalog, fixtures)
    entries = expanded_lane_classification_entries(catalog)
    by_expectation: dict[str, int] = {}
    by_classification: dict[str, int] = {}
    valid_entries = []
    for entry in entries:
        if not isinstance(entry, dict):
            continue
        test_path = entry.get("test_path")
        normalized_path = (
            test_path[5:]
            if isinstance(test_path, str) and test_path.startswith("test/")
            else test_path
        )
        if not isinstance(normalized_path, str) or normalized_path not in fixtures:
            continue
        valid_entries.append(entry)
        expectation = entry.get("expectation")
        classification = entry.get("classification")
        if isinstance(expectation, str):
            by_expectation[expectation] = by_expectation.get(expectation, 0) + 1
        if isinstance(classification, str):
            by_classification[classification] = (
                by_classification.get(classification, 0) + 1
            )
    summary.update(
        {
            "classified_non_green_count": len(valid_entries),
            "by_expectation": dict(sorted(by_expectation.items())),
            "by_classification": dict(sorted(by_classification.items())),
            "validation_errors": errors,
            "entries": valid_entries,
        }
    )
    return summary


def build_lane_summaries(lanes: list[dict], family_summaries: list[dict]) -> list[dict]:
    summaries: list[dict] = []
    for lane in lanes:
        fixture_root = repo_root() / lane["vendored_fixture_root"]
        fixtures = discover_fixture_files(fixture_root)
        fixture_set = set(fixtures)
        vendored_display_paths = {f"test/{fixture}" for fixture in fixtures}
        documented_family_green = sum(
            family["documented_manifested_green_by_lane"].get(lane["lane"]) or 0
            for family in family_summaries
        )
        classification_summary = build_lane_classification_summary(
            lane["lane"], fixture_set
        )
        classified_non_green = classification_summary["classified_non_green_count"]
        documented_green = documented_family_green
        documented_green_source = "family_manifest_docs"
        path_owned_green: int | None = None
        if lane["lane"] == "node22":
            rust_referenced = set(extract_rust_referenced_tests(vendored_display_paths))
            classified_paths = {
                entry["test_path"]
                for entry in classification_summary["entries"]
                if isinstance(entry.get("test_path"), str)
            }
            path_owned_green = len(rust_referenced - classified_paths)
            documented_green = path_owned_green
            documented_green_source = (
                "rust_path_owned_fixture_inventory_minus_classified_non_green"
            )
        unmanifested_or_unclassified = max(
            0, len(fixtures) - documented_green - classified_non_green
        )
        ratio = documented_green / len(fixtures) if fixtures else 0
        summaries.append(
            {
                "lane": lane["lane"],
                "upstream_fixture_line": lane["upstream_fixture_line"],
                "lane_role": lane["lane_role"],
                "public_contract_role": lane["public_contract_role"],
                "runtime_execution_target": lane["runtime_execution_target"],
                "runtime_limits_profile": lane["runtime_limits_profile"],
                "upstream": lane["upstream"],
                "vendored_fixture_root": lane["vendored_fixture_root"],
                "denominator_kind": "vendored_fixture_root_test_files",
                "vendored_test_file_count": len(fixtures),
                "documented_manifested_green_count": documented_green,
                "documented_manifested_green_source": documented_green_source,
                "documented_family_green_count": documented_family_green,
                "path_owned_manifested_green_count": path_owned_green,
                "classified_non_green_count": classified_non_green,
                "documented_or_classified_count": documented_green
                + classified_non_green,
                "unmanifested_or_unclassified_count": unmanifested_or_unclassified,
                "documented_manifested_green_ratio": round(ratio, 6),
                "classification_catalog": classification_summary,
            }
        )
    return summaries


def build_summary(
    catalog_path: Path | None = None,
    observed_results_path: Path | None = None,
) -> dict:
    lanes = [load_json(path) for path in lane_metadata_files()]
    lanes.sort(key=lambda lane: lane["lane"])
    family_summaries = build_family_summaries(lanes)
    lane_summaries = build_lane_summaries(lanes, family_summaries)
    rust_ignores = rust_ignore_inventory()
    expectation_summary = build_expectation_summary(
        rust_ignores,
        catalog_path or default_expectation_catalog_path(),
        observed_results_path,
    )
    public_claim = public_node22_claim_count()
    node22_summary = next(
        (summary for summary in lane_summaries if summary["lane"] == "node22"), None
    )
    warnings: list[dict] = []
    warnings.extend(
        {"kind": "expectation_catalog_validation_error", **error}
        for error in expectation_summary["validation_errors"]
    )
    warnings.extend(expectation_summary["unexpected_passes"])
    for lane_summary in lane_summaries:
        if (
            lane_summary["documented_or_classified_count"]
            > lane_summary["vendored_test_file_count"]
        ):
            warnings.append(
                {
                    "kind": "lane_documented_or_classified_count_exceeds_denominator",
                    "lane": lane_summary["lane"],
                    "vendored_test_file_count": lane_summary["vendored_test_file_count"],
                    "documented_manifested_green_count": lane_summary[
                        "documented_manifested_green_count"
                    ],
                    "classified_non_green_count": lane_summary[
                        "classified_non_green_count"
                    ],
                    "documented_or_classified_count": lane_summary[
                        "documented_or_classified_count"
                    ],
                }
            )
        warnings.extend(
            {
                "kind": "lane_classification_catalog_validation_error",
                **error,
            }
            for error in lane_summary["classification_catalog"]["validation_errors"]
        )
    if public_claim is not None and node22_summary is not None:
        documented_green = node22_summary["documented_manifested_green_count"]
        if documented_green < public_claim:
            warnings.append(
                {
                    "kind": "public_claim_exceeds_documented_manifested_green_count",
                    "public_claim_floor": public_claim,
                    "documented_manifested_green_count": documented_green,
                    "doc_path": "docs/architecture/runtime/deno-vs-neovex-node-compat.md",
                }
            )
    return {
        "schema_version": 1,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "report_kind": "node_compat_suite_status",
        "status_contract": (
            "Counts every vendored lane-local test-* JS/CJS/MJS fixture, then "
            "compares that denominator to the documented manifested green subset "
            "plus explicit lane classification catalogs. The Node22 primary lane "
            "uses path-owned Rust fixture evidence minus explicit non-green "
            "classifications as the green numerator when prose family counts are "
            "not reconstructable. Classified non-green entries are not pass "
            "claims; the remaining remainder is intentionally reported as "
            "unmanifested_or_unclassified, not as pass or fail."
        ),
        "lane_count": len(lane_summaries),
        "family_count": len(family_summaries),
        "rust_ignore_count": len(rust_ignores),
        "expectation_catalog": expectation_summary,
        "total_vendored_test_file_count": sum(
            summary["vendored_test_file_count"] for summary in lane_summaries
        ),
        "total_documented_manifested_green_count": sum(
            summary["documented_manifested_green_count"] for summary in lane_summaries
        ),
        "lane_summaries": lane_summaries,
        "family_summaries": family_summaries,
        "rust_ignored_tests": rust_ignores,
        "warnings": warnings,
    }


def build_markdown(summary: dict) -> str:
    lines = [
        "# Node Compatibility Suite Status",
        "",
        summary["status_contract"],
        "",
        "## Lane Summary",
        "",
        "| Lane | Role | Upstream | Vendored test files | Documented green | Classified non-green | Documented/classified | Unmanifested/unclassified | Ratio |",
        "| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for lane in summary["lane_summaries"]:
        ratio = lane["documented_manifested_green_ratio"] * 100
        lines.append(
            f"| `{lane['lane']}` | `{lane['lane_role']}` | "
            f"`{lane['upstream']['tag']}` | "
            f"{lane['vendored_test_file_count']} | "
            f"{lane['documented_manifested_green_count']} | "
            f"{lane['classified_non_green_count']} | "
            f"{lane['documented_or_classified_count']} | "
            f"{lane['unmanifested_or_unclassified_count']} | "
            f"{ratio:.1f}% |"
        )
    lines.extend(
        [
            "",
            "## Lane Classification Catalogs",
            "",
            "| Lane | Catalog | Classified non-green | By expectation | By classification |",
            "| --- | --- | ---: | --- | --- |",
        ]
    )
    for lane in summary["lane_summaries"]:
        catalog = lane["classification_catalog"]
        lines.append(
            f"| `{lane['lane']}` | `{catalog['catalog_path']}` | "
            f"{catalog['classified_non_green_count']} | "
            f"`{json.dumps(catalog['by_expectation'], sort_keys=True)}` | "
            f"`{json.dumps(catalog['by_classification'], sort_keys=True)}` |"
        )
    lines.extend(
        [
            "",
            "## Family Green Denominator",
            "",
            "| Family | NLC | node20 | node22 | node24 |",
            "| --- | --- | ---: | ---: | ---: |",
        ]
    )
    for family in summary["family_summaries"]:
        counts = family["documented_manifested_green_by_lane"]
        lines.append(
            f"| `{family['family']}` | `{family['nlc_item']}` | "
            f"{counts.get('node20') or 0} | "
            f"{counts.get('node22') or 0} | "
            f"{counts.get('node24') or 0} |"
        )
    lines.extend(
        [
            "",
            "## Rust Ignored Test Inventory",
            "",
            (
                f"- ignored Rust node_compat tests: "
                f"{summary['rust_ignore_count']}"
            ),
            "- source: `crates/neovex-runtime/src/runtime/tests/node_compat.rs`",
            "",
            "## Expectation Catalog",
            "",
            f"- catalog: `{summary['expectation_catalog']['catalog_path']}`",
            (
                f"- entries: "
                f"{summary['expectation_catalog']['catalog_entry_count']}"
            ),
            (
                "- by expectation: "
                f"`{json.dumps(summary['expectation_catalog']['by_expectation'], sort_keys=True)}`"
            ),
            (
                "- by classification: "
                f"`{json.dumps(summary['expectation_catalog']['by_classification'], sort_keys=True)}`"
            ),
            (
                "- unexpected passes: "
                f"{len(summary['expectation_catalog']['unexpected_passes'])}"
            ),
        ]
    )
    lines.extend(["", "## Warnings"])
    if summary["warnings"]:
        for warning in summary["warnings"]:
            lines.append(f"- `{warning['kind']}`: {json.dumps(warning, sort_keys=True)}")
    else:
        lines.append("- none")
    lines.append("")
    return "\n".join(lines)


def write_outputs(summary: dict, output_root: Path) -> tuple[Path, Path]:
    output_root.mkdir(parents=True, exist_ok=True)
    json_path = output_root / "status-summary.json"
    markdown_path = output_root / "status-summary.md"
    json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    markdown_path.write_text(build_markdown(summary), encoding="utf-8")
    return json_path, markdown_path


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Build a truthful suite-wide Node compatibility status summary"
    )
    parser.add_argument("--output-root", default=str(default_output_root()))
    parser.add_argument("--expectation-catalog", default=str(default_expectation_catalog_path()))
    parser.add_argument(
        "--observed-results",
        help="optional JSON results file used to flag unexpected expected-failure passes",
    )
    return parser


def main() -> int:
    args = build_parser().parse_args()
    output_root = Path(args.output_root).resolve()
    observed_results_path = (
        Path(args.observed_results).resolve() if args.observed_results else None
    )
    summary = build_summary(
        catalog_path=Path(args.expectation_catalog).resolve(),
        observed_results_path=observed_results_path,
    )
    json_path, markdown_path = write_outputs(summary, output_root)
    print(f"wrote node-compat status summary to {json_path}")
    print(f"wrote node-compat status markdown to {markdown_path}")
    for warning in summary["warnings"]:
        print(f"warning: {warning['kind']}")
    return 1 if summary["warnings"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
