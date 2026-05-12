#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import Counter, defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

from schema import default_schema_path, validate_payload_against_schema  # noqa: E402
from classifications import rust_fixture_refs  # noqa: E402
from status import build_summary as build_status_summary  # noqa: E402


TEST_FILE_SUFFIXES = {".js", ".mjs", ".cjs"}
RUST_NODE_COMPAT_PATH = Path("crates/neovex-runtime/src/runtime/tests/node/mod.rs")
INVENTORY_SCHEMA_PATH = default_schema_path("fixture-inventory.schema.json")


def repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


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
    return repo_root() / "target" / "node-compat" / "inventory"


def load_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def lane_metadata(lane: str) -> dict[str, Any]:
    path = manifest_root() / "lanes" / f"{lane}.json"
    if not path.is_file():
        raise ValueError(f"unknown node compatibility lane {lane!r}: {path} not found")
    return load_json(path)


def is_node_test_file(path: Path) -> bool:
    return path.name.startswith("test-") and path.suffix in TEST_FILE_SUFFIXES


def canonical_test_path(path: Path, fixture_root: Path) -> str:
    return f"test/{path.relative_to(fixture_root)}"


def discover_fixture_files(fixture_root: Path) -> list[str]:
    return sorted(
        canonical_test_path(path, fixture_root)
        for path in fixture_root.rglob("*")
        if path.is_file() and is_node_test_file(path)
    )


def normalize_rust_fixture_path(path: str) -> str:
    return re.sub(r"^node(?:20|22|24)/", "", path)


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


def lane_status(summary: dict[str, Any], lane: str) -> dict[str, Any]:
    for candidate in summary["lane_summaries"]:
        if candidate["lane"] == lane:
            return candidate
    raise ValueError(f"status summary did not include lane {lane!r}")


def prefix_for_test(path: str) -> str:
    name = Path(path).name
    stem = name.rsplit(".", 1)[0]
    if stem.startswith("test-"):
        stem = stem[len("test-") :]
    return stem.split("-", 1)[0].split(".", 1)[0]


def summarize_groups(paths: list[str], *, mode: str) -> list[dict[str, Any]]:
    counter: Counter[str] = Counter()
    members: dict[str, list[str]] = defaultdict(list)
    for path in paths:
        key = str(Path(path).parent) if mode == "directory" else prefix_for_test(path)
        counter[key] += 1
        members[key].append(path)
    return [
        {
            "id": key,
            "count": count,
            "sample": sorted(members[key])[:10],
        }
        for key, count in sorted(counter.items(), key=lambda item: (-item[1], item[0]))
    ]


def build_inventory(lane: str) -> dict[str, Any]:
    metadata = lane_metadata(lane)
    fixture_root = repo_root() / metadata["vendored_fixture_root"]
    vendored_tests = discover_fixture_files(fixture_root)
    vendored_test_set = set(vendored_tests)
    refs = rust_fixture_refs(lane, vendored_test_set)
    rust_referenced_tests = sorted(refs.nonignored)
    rust_referenced_set = set(rust_referenced_tests)
    unreferenced_tests = sorted(vendored_test_set - rust_referenced_set)

    status_summary = build_status_summary()
    status = lane_status(status_summary, lane)
    documented_green = status["documented_manifested_green_count"]
    classified_red_skip = status["classified_red_skip_count"]
    documented_unclassified = status["unmanifested_or_unclassified_count"]
    classified_paths = {
        entry["test_path"]
        for entry in status["classification_catalog"]["entries"]
        if isinstance(entry.get("test_path"), str)
    }
    unreferenced_test_set = set(unreferenced_tests)
    rust_unreferenced_classified = sorted(unreferenced_test_set & classified_paths)
    rust_unreferenced_unclassified = sorted(unreferenced_test_set - classified_paths)
    classified_not_rust_unreferenced = sorted(classified_paths - unreferenced_test_set)
    path_owned_green_tests = sorted(rust_referenced_set - classified_paths)
    reconstructability_gap = max(0, documented_green - len(rust_referenced_tests))

    warnings: list[dict[str, Any]] = []
    if reconstructability_gap:
        warnings.append(
            {
                "kind": "documented_green_exceeds_reconstructable_rust_path_inventory",
                "documented_manifested_green_count": documented_green,
                "rust_referenced_test_file_count": len(rust_referenced_tests),
                "gap_count": reconstructability_gap,
                "action": "move documented-green fixture membership into manifest-owned or generated inventory before treating the per-file green list as complete",
            }
        )
    if len(rust_unreferenced_unclassified) != reconstructability_gap:
        warnings.append(
            {
                "kind": "rust_unreferenced_unclassified_count_differs_from_documented_green_reconstructability_gap",
                "rust_unreferenced_unclassified_count": len(
                    rust_unreferenced_unclassified
                ),
                "documented_green_reconstructability_gap_count": reconstructability_gap,
                "reason": "the Rust-reference audit is path-based while the documented green count is still prose/count-based",
            }
        )
    if len(rust_unreferenced_unclassified) != documented_unclassified:
        warnings.append(
            {
                "kind": "rust_unreferenced_count_differs_from_status_unclassified_count",
                "rust_unreferenced_unclassified_count": len(
                    rust_unreferenced_unclassified
                ),
                "status_unmanifested_or_unclassified_count": documented_unclassified,
                "reason": "status is denominator-based and may be fully classified while the Rust-reference inventory still has a documented-green reconstructability gap",
            }
        )

    return {
        "schema_version": 1,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "report_kind": "node_compat_fixture_inventory",
        "lane": lane,
        "upstream": metadata["upstream"],
        "vendored_fixture_root": metadata["vendored_fixture_root"],
        "counts": {
            "vendored_test_file_count": len(vendored_tests),
            "documented_manifested_green_count": documented_green,
            "classified_red_skip_count": classified_red_skip,
            "documented_or_classified_count": documented_green + classified_red_skip,
            "documented_unmanifested_or_unclassified_count": documented_unclassified,
            "rust_referenced_test_file_count": len(rust_referenced_tests),
            "rust_unreferenced_test_file_count": len(unreferenced_tests),
            "rust_unreferenced_classified_red_skip_count": len(
                rust_unreferenced_classified
            ),
            "rust_unreferenced_unclassified_count": len(rust_unreferenced_unclassified),
            "classified_red_skip_not_rust_unreferenced_count": len(
                classified_not_rust_unreferenced
            ),
            "path_owned_green_test_count": len(path_owned_green_tests),
            "documented_green_reconstructability_gap_count": reconstructability_gap,
        },
        "contracts": [
            "vendored_test_file_count counts lane-local test-* JS/CJS/MJS files under the checked-in fixture root",
            "documented_manifested_green_count is imported from the existing suite status workflow and remains the public evidence numerator",
            "path_owned_green_tests is the machine-owned path list behind the lane green numerator",
            "rust_referenced_test_file_count is a path-based audit of fixture literals currently referenced by non-ignored Rust node_compat tests",
            "unreferenced_tests is an actionable candidate list, not a failure list and not a support claim",
        ],
        "path_owned_green_by_directory": summarize_groups(
            path_owned_green_tests, mode="directory"
        ),
        "path_owned_green_by_prefix": summarize_groups(
            path_owned_green_tests, mode="prefix"
        ),
        "path_owned_green_tests": path_owned_green_tests,
        "unreferenced_by_directory": summarize_groups(
            rust_unreferenced_unclassified, mode="directory"
        ),
        "unreferenced_by_prefix": summarize_groups(
            rust_unreferenced_unclassified, mode="prefix"
        ),
        "unreferenced_tests": rust_unreferenced_unclassified,
        "rust_unreferenced_classified_red_skip_tests": rust_unreferenced_classified,
        "classified_red_skip_not_rust_unreferenced_tests": classified_not_rust_unreferenced,
        "warnings": warnings,
    }


def build_markdown(inventory: dict[str, Any]) -> str:
    counts = inventory["counts"]
    ratio = (
        counts["documented_manifested_green_count"]
        / counts["vendored_test_file_count"]
        * 100
    )
    lines = [
        "# Node Compatibility Fixture Inventory",
        "",
        f"- lane: `{inventory['lane']}`",
        f"- upstream: `{inventory['upstream']['tag']}`",
        f"- fixture root: `{inventory['vendored_fixture_root']}`",
        "",
        "## Summary",
        "",
        "| Metric | Count |",
        "| --- | ---: |",
        f"| Vendored test files | {counts['vendored_test_file_count']} |",
        f"| Green | {counts['documented_manifested_green_count']} |",
        f"| Classified red/skip total | {counts['classified_red_skip_count']} |",
        f"| Documented/classified | {counts['documented_or_classified_count']} |",
        f"| Documented unmanifested/unclassified | {counts['documented_unmanifested_or_unclassified_count']} |",
        f"| Green ratio | {ratio:.1f}% |",
        f"| Rust-referenced fixture paths | {counts['rust_referenced_test_file_count']} |",
        f"| Rust-unreferenced fixture paths | {counts['rust_unreferenced_test_file_count']} |",
        f"| Rust-unreferenced classified red/skip | {counts['rust_unreferenced_classified_red_skip_count']} |",
        f"| Rust-unreferenced unclassified | {counts['rust_unreferenced_unclassified_count']} |",
        f"| Classified red/skip not Rust-unreferenced | {counts['classified_red_skip_not_rust_unreferenced_count']} |",
        f"| Path-owned green tests | {counts['path_owned_green_test_count']} |",
        f"| Documented-green reconstructability gap | {counts['documented_green_reconstructability_gap_count']} |",
        "",
        "## Largest Unreferenced Directories",
        "",
        "| Directory | Count | Sample |",
        "| --- | ---: | --- |",
    ]
    for group in inventory["unreferenced_by_directory"][:20]:
        sample = ", ".join(f"`{path}`" for path in group["sample"][:3])
        lines.append(f"| `{group['id']}` | {group['count']} | {sample} |")
    lines.extend(
        [
            "",
            "## Largest Unreferenced Prefixes",
            "",
            "| Prefix | Count | Sample |",
            "| --- | ---: | --- |",
        ]
    )
    for group in inventory["unreferenced_by_prefix"][:20]:
        sample = ", ".join(f"`{path}`" for path in group["sample"][:3])
        lines.append(f"| `{group['id']}` | {group['count']} | {sample} |")
    lines.extend(["", "## Warnings"])
    if inventory["warnings"]:
        for warning in inventory["warnings"]:
            lines.append(f"- `{warning['kind']}`: {json.dumps(warning, sort_keys=True)}")
    else:
        lines.append("- none")
    lines.append("")
    return "\n".join(lines)


def validate_inventory(inventory: dict[str, Any]) -> list[dict[str, Any]]:
    return validate_payload_against_schema(inventory, INVENTORY_SCHEMA_PATH)


def write_outputs(inventory: dict[str, Any], output_root: Path) -> tuple[Path, Path]:
    output_root.mkdir(parents=True, exist_ok=True)
    stem = f"{inventory['lane']}-inventory"
    json_path = output_root / f"{stem}.json"
    markdown_path = output_root / f"{stem}.md"
    json_path.write_text(json.dumps(inventory, indent=2) + "\n", encoding="utf-8")
    markdown_path.write_text(build_markdown(inventory), encoding="utf-8")
    return json_path, markdown_path


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Build a path-based Node compatibility fixture inventory"
    )
    parser.add_argument("--lane", default="node22")
    parser.add_argument("--output-root", default=str(default_output_root()))
    return parser


def main() -> int:
    args = build_parser().parse_args()
    inventory = build_inventory(args.lane)
    errors = validate_inventory(inventory)
    if errors:
        for error in errors:
            print(f"error: {json.dumps(error, sort_keys=True)}")
        return 1
    json_path, markdown_path = write_outputs(inventory, Path(args.output_root).resolve())
    print(f"wrote node-compat fixture inventory to {json_path}")
    print(f"wrote node-compat fixture inventory markdown to {markdown_path}")
    for warning in inventory["warnings"]:
        print(f"warning: {warning['kind']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
