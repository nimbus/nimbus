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


def discover_fixture_files(fixture_root: Path) -> list[str]:
    return sorted(
        str(path.relative_to(fixture_root))
        for path in fixture_root.rglob("*")
        if path.is_file() and is_node_test_file(path)
    )


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


def build_lane_summaries(lanes: list[dict], family_summaries: list[dict]) -> list[dict]:
    summaries: list[dict] = []
    for lane in lanes:
        fixture_root = repo_root() / lane["vendored_fixture_root"]
        fixtures = discover_fixture_files(fixture_root)
        documented_green = sum(
            family["documented_manifested_green_by_lane"].get(lane["lane"]) or 0
            for family in family_summaries
        )
        unmanifested_or_unclassified = max(0, len(fixtures) - documented_green)
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
                "unmanifested_or_unclassified_count": unmanifested_or_unclassified,
                "documented_manifested_green_ratio": round(ratio, 6),
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
            "compares that denominator to the documented manifested green subset. "
            "The remainder is intentionally reported as unmanifested_or_unclassified, "
            "not as pass or fail."
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
        "| Lane | Role | Upstream | Vendored test files | Documented green | Unmanifested/unclassified | Ratio |",
        "| --- | --- | --- | ---: | ---: | ---: | ---: |",
    ]
    for lane in summary["lane_summaries"]:
        ratio = lane["documented_manifested_green_ratio"] * 100
        lines.append(
            f"| `{lane['lane']}` | `{lane['lane_role']}` | "
            f"`{lane['upstream']['tag']}` | "
            f"{lane['vendored_test_file_count']} | "
            f"{lane['documented_manifested_green_count']} | "
            f"{lane['unmanifested_or_unclassified_count']} | "
            f"{ratio:.1f}% |"
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
