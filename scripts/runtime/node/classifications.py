#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import re
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any

TEST_FILE_SUFFIXES = {".js", ".mjs", ".cjs"}
RUST_NODE_COMPAT_PATH = Path("crates/nimbus-runtime/src/runtime/tests/node/mod.rs")
LANE_AWARE_BATCH_MACROS = {
    "node20_only_batch_case",
    "node22_default_only_batch_case",
    "node22_only_batch_case",
    "shared_batch_case",
    "shared_batch_case_with_extra",
    "shared_lane_fixture_batch_case",
    "shared_node20_node22_batch_case_with_extra",
    "shared_node20_node22_with_node24_override_case_with_extra",
    "shared_official_batch_case",
    "shared_official_batch_case_with_extra",
    "split_batch_case",
}


@dataclass(frozen=True)
class RustFixtureRefs:
    nonignored: set[str]
    ignored: dict[str, list[str]]


def repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def manifest_root() -> Path:
    return (
        repo_root()
        / "crates"
        / "nimbus-runtime"
        / "src"
        / "runtime"
        / "tests"
        / "node_compat_manifests"
    )


def load_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        json.dump(payload, handle, indent=2, sort_keys=False)
        handle.write("\n")


def lane_metadata(lane: str) -> dict[str, Any]:
    path = manifest_root() / "lanes" / f"{lane}.json"
    if not path.is_file():
        raise ValueError(f"unknown node compatibility lane {lane!r}: {path} not found")
    return load_json(path)


def lane_ids() -> list[str]:
    return sorted(path.stem for path in (manifest_root() / "lanes").glob("node*.json"))


def is_node_test_file(path: Path) -> bool:
    return path.name.startswith("test-") and path.suffix in TEST_FILE_SUFFIXES


def discover_fixture_files(lane: str) -> set[str]:
    metadata = lane_metadata(lane)
    fixture_root = repo_root() / metadata["vendored_fixture_root"]
    return {
        f"test/{path.relative_to(fixture_root)}"
        for path in fixture_root.rglob("*")
        if path.is_file() and is_node_test_file(path)
    }


def rust_source_lines() -> list[str]:
    return (repo_root() / RUST_NODE_COMPAT_PATH).read_text(encoding="utf-8").splitlines()


def fixture_literals(text: str) -> set[str]:
    return set(
        re.findall(r'"((?:node(?:20|22|24)/)?test/[^"\\]*(?:\.js|\.mjs|\.cjs))"', text)
    )


def macro_invocations(text: str) -> list[tuple[str, str, tuple[int, int]]]:
    invocations: list[tuple[str, str, tuple[int, int]]] = []
    pattern = re.compile(r"\b([A-Za-z_][A-Za-z0-9_]*)!\s*\(")
    index = 0
    while True:
        match = pattern.search(text, index)
        if match is None:
            break
        depth = 1
        cursor = match.end()
        while cursor < len(text) and depth > 0:
            char = text[cursor]
            if char == "(":
                depth += 1
            elif char == ")":
                depth -= 1
            cursor += 1
        if depth == 0:
            invocations.append((match.group(1), text[match.end() : cursor - 1], (match.start(), cursor)))
        index = max(cursor, match.end())
    return invocations


def string_args(text: str) -> list[str]:
    return re.findall(r'"([^"\\]*(?:\\.[^"\\]*)*)"', text)


def lane_relative_literal(literal: str, lane: str) -> str | None:
    if literal.startswith("node"):
        prefix, relative = literal.split("/", 1)
        if prefix != lane:
            return None
        return relative
    return literal


def lane_fixture_literals(text: str, lane: str) -> set[str]:
    """Return fixture paths that the Rust source actually wires into a lane.

    Batch macros take an unprefixed test identity as their first argument, then
    lane-specific source paths. Counting every string literal made Node20 look
    green for node22-only entries, so the inventory needs to mirror the macro
    contracts instead of treating the source as a flat text file.
    """

    references: set[str] = set()
    masked = list(text)

    def add(literal: str | None) -> None:
        if literal is None:
            return
        relative = lane_relative_literal(literal, lane)
        if relative is not None:
            references.add(relative)

    for name, body, (start, end) in macro_invocations(text):
        if name not in LANE_AWARE_BATCH_MACROS:
            continue
        args = string_args(body)
        for offset in range(start, end):
            masked[offset] = " "
        if not args:
            continue
        test_relative_path = args[0]
        has_fixture_source_path = len(args) > 1
        has_node20_fixture_source_path = len(args) > 1
        has_node22_fixture_source_path = len(args) > 2
        has_node24_fixture_source_path = len(args) > 1

        if name in {"shared_batch_case", "shared_batch_case_with_extra"}:
            if lane in {"node20", "node22"} and has_fixture_source_path:
                add(test_relative_path)
            elif lane == "node24":
                add(test_relative_path)
        elif name == "split_batch_case":
            if lane == "node20" and has_node20_fixture_source_path:
                add(test_relative_path)
            elif lane == "node22" and has_node22_fixture_source_path:
                add(test_relative_path)
            elif lane == "node24":
                add(test_relative_path)
        elif name == "shared_lane_fixture_batch_case":
            if has_fixture_source_path:
                add(test_relative_path)
        elif name == "node20_only_batch_case":
            if lane == "node20" and has_fixture_source_path:
                add(test_relative_path)
            elif lane == "node24":
                add(test_relative_path)
        elif name == "node22_only_batch_case":
            if lane == "node22" and has_fixture_source_path:
                add(test_relative_path)
            elif lane == "node24":
                add(test_relative_path)
        elif name == "node22_default_only_batch_case":
            if lane == "node22" and has_fixture_source_path:
                add(test_relative_path)
        elif name in {
            "shared_official_batch_case",
            "shared_official_batch_case_with_extra",
            "shared_node20_node22_batch_case_with_extra",
        }:
            add(test_relative_path)
        elif name == "shared_node20_node22_with_node24_override_case_with_extra":
            if lane == "node24" and has_node24_fixture_source_path:
                add(test_relative_path)
            else:
                add(test_relative_path)

    for literal in fixture_literals("".join(masked)):
        add(literal)
    return references


def collect_const_blocks(lines: list[str]) -> dict[str, str]:
    blocks: dict[str, str] = {}
    index = 0
    while index < len(lines):
        match = re.match(r"\s*const\s+([A-Z0-9_]+):", lines[index])
        if match is None:
            index += 1
            continue
        name = match.group(1)
        block: list[str] = []
        depth = 0
        start = index
        while index < len(lines):
            line = lines[index]
            block.append(line)
            depth += (
                line.count("[")
                + line.count("{")
                + line.count("(")
                - line.count("]")
                - line.count("}")
                - line.count(")")
            )
            if ";" in line and index > start and depth <= 0:
                break
            index += 1
        blocks[name] = "\n".join(block)
        index += 1
    return blocks


def expand_const_literals(
    const_name: str,
    const_blocks: dict[str, str],
    lane: str,
    visiting: set[str] | None = None,
) -> set[str]:
    if visiting is None:
        visiting = set()
    if const_name in visiting or const_name not in const_blocks:
        return set()
    visiting.add(const_name)
    block = const_blocks[const_name]
    expanded = set(lane_fixture_literals(block, lane))
    for nested in re.findall(r"\b[A-Z][A-Z0-9_]+\b", block):
        if nested != const_name:
            expanded.update(expand_const_literals(nested, const_blocks, lane, visiting))
    visiting.remove(const_name)
    return expanded


def collect_test_functions(
    lines: list[str], const_blocks: dict[str, str], lane: str
) -> list[dict[str, Any]]:
    functions: list[dict[str, Any]] = []
    index = 0
    while index < len(lines):
        match = re.search(r"\bfn\s+([A-Za-z0-9_]+)\s*\(", lines[index])
        if match is None:
            index += 1
            continue
        name = match.group(1)
        attrs: list[str] = []
        attr_index = index - 1
        while attr_index >= 0 and (
            lines[attr_index].strip().startswith("#[") or lines[attr_index].strip() == ""
        ):
            if lines[attr_index].strip().startswith("#["):
                attrs.append(lines[attr_index].strip())
            attr_index -= 1
        if not any(attr.startswith("#[test") for attr in attrs):
            index += 1
            continue
        block: list[str] = []
        depth = 0
        started = False
        while index < len(lines):
            line = lines[index]
            block.append(line)
            if "{" in line:
                started = True
            if started:
                depth += line.count("{") - line.count("}")
                if depth <= 0:
                    break
            index += 1
        body = "\n".join(block)
        literals = set(lane_fixture_literals(body, lane))
        for const_name in re.findall(r"\b[A-Z][A-Z0-9_]+\b", body):
            literals.update(expand_const_literals(const_name, const_blocks, lane))
        functions.append(
            {
                "name": name,
                "ignored": any("ignore" in attr for attr in attrs),
                "literals": literals,
            }
        )
        index += 1
    return functions


def rust_fixture_refs(lane: str, fixtures: set[str]) -> RustFixtureRefs:
    lines = rust_source_lines()
    const_blocks = collect_const_blocks(lines)
    functions = collect_test_functions(lines, const_blocks, lane)
    nonignored: set[str] = set()
    ignored: dict[str, list[str]] = defaultdict(list)
    for function in functions:
        for literal in function["literals"]:
            relative = lane_relative_literal(literal, lane)
            if relative not in fixtures:
                continue
            if function["ignored"]:
                ignored[relative].append(function["name"])
            else:
                nonignored.add(relative)
    return RustFixtureRefs(
        nonignored=nonignored,
        ignored={path: sorted(names) for path, names in ignored.items()},
    )


def owner_for_path(path: str) -> str:
    name = Path(path).name
    if name.startswith("test-"):
        name = name[len("test-") :]
    prefix = name.split("-", 1)[0].split(".", 1)[0]
    owner_by_prefix = {
        "assert": "core-semantics/assert",
        "buffer": "core-semantics/buffer",
        "console": "core-semantics/console",
        "crypto": "networking/crypto",
        "dgram": "networking/dgram",
        "diagnostics": "process-and-timing/diagnostics-channel",
        "domain": "loader-context/domain",
        "events": "core-semantics/events",
        "fs": "streams-local-io/fs-host-io",
        "http": "networking/http",
        "http2": "networking/http2",
        "https": "networking/https",
        "module": "loader-context/module",
        "net": "networking/net",
        "os": "process-and-timing/os",
        "path": "core-semantics/path",
        "perf": "process-and-timing/perf-hooks",
        "process": "process-and-timing/process-host",
        "readline": "streams-local-io/readline-tty",
        "stream": "streams-local-io/stream",
        "timers": "process-and-timing/timers",
        "tls": "networking/tls",
        "tty": "streams-local-io/tty-host",
        "url": "core-semantics/url",
        "util": "loader-context/util",
        "v8": "runtime/v8",
        "vm": "loader-context/vm",
        "worker": "loader-context/workers",
        "zlib": "networking/zlib",
    }
    return owner_by_prefix.get(prefix, "node-compat/unpromoted-surface")


def classification_for_unpromoted(path: str, fixture_root: Path) -> dict[str, str]:
    fixture_path = fixture_root / path.removeprefix("test/")
    if fixture_path.is_file() and fixture_path.stat().st_size == 0:
        return {
            "expectation": "expected_skip",
            "classification": "vendored_non_official_placeholder",
            "owner": "node-compat-denominator/fixture-sync",
            "reason": "The vendored file is empty fixture corpus residue, so it is excluded from green support claims until a runnable upstream counterpart is proven and promoted.",
        }
    if path.startswith("test/fixtures/"):
        return {
            "expectation": "expected_skip",
            "classification": "support_fixture_not_top_level_test",
            "owner": "node-compat-denominator/fixture-sync",
            "reason": "This file lives under test/fixtures and is support data for other official Node tests, not a top-level runnable compatibility test.",
        }
    directory_classifications = [
        ("test/addons/", "requires_native_addon_harness", "loader-context/native-addon-host"),
        ("test/known_issues/", "upstream_known_issue_or_platform_boundary", "node-compat/platform-boundary"),
        ("test/pseudo-tty/", "requires_pseudo_tty_host_harness", "process-and-timing/tty-host"),
        ("test/pummel/", "requires_pummel_stress_harness", "node-compat/stress-harness"),
        ("test/sequential/", "requires_sequential_host_state_harness", "node-compat/sequential-host-state"),
        ("test/wpt/", "requires_wpt_harness", "node-compat/wpt-harness"),
    ]
    for prefix, classification, owner in directory_classifications:
        if path.startswith(prefix):
            return {
                "expectation": "expected_gap",
                "classification": classification,
                "owner": owner,
                "reason": "This official fixture requires a dedicated host, ordering, stress, native, or standards harness before it can become a green support claim.",
            }
    return {
        "expectation": "expected_gap",
        "classification": "requires_unpromoted_node_surface",
        "owner": owner_for_path(path),
        "reason": "This official fixture is not referenced by the non-ignored Rust compatibility lane yet, so it remains an owner-backed promotion gap rather than a green support claim.",
    }


def existing_classified_paths(catalog: dict[str, Any] | None) -> set[str]:
    if catalog is None:
        return set()
    paths = {
        entry["test_path"]
        for entry in catalog.get("entries", [])
        if isinstance(entry, dict) and isinstance(entry.get("test_path"), str)
    }
    for group in catalog.get("groups", []):
        if not isinstance(group, dict) or not isinstance(group.get("test_paths"), list):
            continue
        paths.update(path for path in group["test_paths"] if isinstance(path, str))
    return paths


def classification_catalog_path(lane: str) -> Path:
    return repo_root() / "tests" / "runtime" / "node" / "classifications" / f"{lane}.json"


def build_catalog(lane: str, *, preserve_existing: bool) -> dict[str, Any]:
    fixtures = discover_fixture_files(lane)
    metadata = lane_metadata(lane)
    fixture_root = repo_root() / metadata["vendored_fixture_root"]
    refs = rust_fixture_refs(lane, fixtures)
    catalog_path = classification_catalog_path(lane)
    existing = load_json(catalog_path) if preserve_existing and catalog_path.is_file() else None
    existing_paths = existing_classified_paths(existing)
    nongreen_paths = set(fixtures - refs.nonignored)
    nongreen_paths.update(existing_paths & fixtures)

    entries: list[dict[str, Any]] = []
    for path in sorted(nongreen_paths & set(refs.ignored)):
        watchpoints = refs.ignored[path]
        entries.append(
            {
                "test_path": path,
                "expectation": "expected_failure",
                "classification": "rust_watchpoint_expected_failure",
                "owner": owner_for_path(path),
                "reason": (
                    "This fixture is referenced by ignored Rust watchpoint(s), so it "
                    "is a measured red path until the watchpoint is removed: "
                    + ", ".join(watchpoints)
                ),
            }
        )

    grouped_paths: dict[tuple[str, str, str, str], list[str]] = defaultdict(list)
    for path in sorted(nongreen_paths - {entry["test_path"] for entry in entries}):
        classification = classification_for_unpromoted(path, fixture_root)
        key = (
            classification["expectation"],
            classification["classification"],
            classification["owner"],
            classification["reason"],
        )
        grouped_paths[key].append(path)

    groups = []
    for index, ((expectation, classification, owner, reason), paths) in enumerate(
        sorted(grouped_paths.items(), key=lambda item: (item[0], item[1][0])),
        start=1,
    ):
        groups.append(
            {
                "id": f"{lane}-{classification}-{index}",
                "expectation": expectation,
                "classification": classification,
                "owner": owner,
                "reason": reason,
                "test_paths": paths,
            }
        )

    return {
        "schema_version": 1,
        "catalog_kind": "node_compat_lane_classifications",
        "lane": lane,
        "contract": (
            "Classifies vendored lane-local test files that are not green in the "
            "non-ignored Rust compatibility lane. Entries must not be counted as "
            "pass claims; they reduce only the unmanifested/unclassified remainder."
        ),
        "entries": entries,
        "groups": groups,
    }


def sync(args: argparse.Namespace) -> None:
    lanes = lane_ids() if args.lane == "all" else [args.lane]
    for lane in lanes:
        catalog = build_catalog(lane, preserve_existing=args.preserve_existing)
        path = classification_catalog_path(lane)
        if args.check:
            expected = json.dumps(catalog, indent=2, sort_keys=False) + "\n"
            actual = path.read_text(encoding="utf-8") if path.is_file() else ""
            if actual != expected:
                raise SystemExit(f"{path} is not up to date")
            print(f"{path} is up to date")
        else:
            write_json(path, catalog)
            print(f"wrote {path}")


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Generate Node compatibility lane classification catalogs"
    )
    subparsers = parser.add_subparsers(dest="command", required=True)
    sync_parser = subparsers.add_parser("sync")
    sync_parser.add_argument("--lane", default="all")
    sync_parser.add_argument(
        "--preserve-existing",
        action="store_true",
        help="keep existing classified paths in the generated catalog",
    )
    sync_parser.add_argument("--check", action="store_true")
    sync_parser.set_defaults(func=sync)
    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
