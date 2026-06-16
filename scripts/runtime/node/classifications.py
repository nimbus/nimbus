#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import re
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from fixture_discovery import discover_fixture_files as discover_git_fixture_files

RUST_NODE_COMPAT_ROOT = Path("crates/nimbus-runtime/src/runtime/tests/node")
RUST_EXECUTED_FIXTURE_LANES = {"node20", "node22", "node24", "node26"}
RUST_EXECUTION_MARKERS = {
    "execute_manifested_node_compat_test(",
    "execute_upstream_node_compat_test_with_extra_files(",
    "run_manifested_subset_for_lane(",
    "run_manifested_subset_for_lane_excluding(",
    "run_node_compat_watchpoint(",
    "run_node_compat_watchpoint_batch(",
    "run_node_compat_watchpoint_entry_batch(",
    "run_node_compat_watchpoint_for_lane(",
    "run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(",
}
FORCED_LANE_CLASSIFICATIONS: dict[str, dict[str, dict[str, str]]] = {
    "node22": {
        # Source-confirmed: test-util-styletext.js calls common.getTTYfd(),
        # which probes existing TTY fds and falls back to opening /dev/tty.
        "test/parallel/test-util-styletext.js": {
            "expectation": "expected_gap",
            "classification": "requires_pseudo_tty_host_harness",
            "owner": "process-and-timing/tty-host",
            "reason": "The fixture's styleText stream validation section requires a host TTY fd from common.getTTYfd(), so it is terminal-harness evidence rather than a multi-tenant isolate support claim.",
        },
    },
    "node24": {
        # Source-confirmed: test-util-styletext.js calls common.getTTYfd(),
        # which probes existing TTY fds and falls back to opening /dev/tty.
        "test/parallel/test-util-styletext.js": {
            "expectation": "expected_gap",
            "classification": "requires_pseudo_tty_host_harness",
            "owner": "process-and-timing/tty-host",
            "reason": "The fixture's styleText stream validation section requires a host TTY fd from common.getTTYfd(), so it is terminal-harness evidence rather than a multi-tenant isolate support claim.",
        },
        "test/parallel/test-buffer-tostring-rangeerror.js": {
            "expectation": "expected_skip",
            "classification": "upstream_known_issue_or_platform_boundary",
            "owner": "core-semantics/buffer",
            "reason": "The official Node24 fixture self-skips at runtime due to host memory requirements, so it is excluded from green support claims even when the containing core-semantics batch passes.",
        },
    },
    "node26": {
        "test/parallel/test-buffer-tostring-rangeerror.js": {
            "expectation": "expected_skip",
            "classification": "upstream_known_issue_or_platform_boundary",
            "owner": "core-semantics/buffer",
            "reason": "The official Node26 fixture self-skips through common.enoughTestMem because it requires allocating buffers larger than MAX_STRING_LENGTH, so it is host-memory stress evidence rather than a default V8-isolate support claim.",
        },
        "test/parallel/test-crypto-default-shake-lengths-oneshot.js": {
            "expectation": "expected_skip",
            "classification": "upstream_known_issue_or_platform_boundary",
            "owner": "networking/crypto-provider",
            "reason": "The official Node26 fixture self-skips when process.features.openssl_is_boringssl is true because default SHAKE XOF lengths are not supported by the linked BoringSSL-family provider.",
        },
        "test/parallel/test-crypto-dh-group-setters.js": {
            "expectation": "expected_skip",
            "classification": "upstream_known_issue_or_platform_boundary",
            "owner": "networking/crypto-provider",
            "reason": "The official Node26 fixture self-skips when process.features.openssl_is_boringssl is true because the Diffie-Hellman group surface is unsupported by the linked BoringSSL-family provider.",
        },
        "test/parallel/test-crypto-dh-modp2-views.js": {
            "expectation": "expected_skip",
            "classification": "upstream_known_issue_or_platform_boundary",
            "owner": "networking/crypto-provider",
            "reason": "The official Node26 fixture self-skips when process.features.openssl_is_boringssl is true because the Diffie-Hellman MODP2 surface is unsupported by the linked BoringSSL-family provider.",
        },
        "test/parallel/test-crypto-dh-modp2.js": {
            "expectation": "expected_skip",
            "classification": "upstream_known_issue_or_platform_boundary",
            "owner": "networking/crypto-provider",
            "reason": "The official Node26 fixture self-skips when process.features.openssl_is_boringssl is true because the Diffie-Hellman MODP2 surface is unsupported by the linked BoringSSL-family provider.",
        },
        "test/parallel/test-crypto-oneshot-hash-xof.js": {
            "expectation": "expected_skip",
            "classification": "upstream_known_issue_or_platform_boundary",
            "owner": "networking/crypto-provider",
            "reason": "The official Node26 fixture self-skips when process.features.openssl_is_boringssl is true because BoringSSL does not support XOF hash functions.",
        },
        "test/parallel/test-module-loading-error.js": {
            "expectation": "expected_gap",
            "classification": "requires_native_addon_harness",
            "owner": "loader-context/native-addon-host",
            "reason": "The fixture requires a .node native addon through CommonJS require(), which would dlopen host-native code outside the V8 isolate and must remain fail-closed unless a host-capable backend is selected.",
        },
        "test/embedding/test-embedding-snapshot-vm.js": {
            "expectation": "expected_gap",
            "classification": "requires_native_addon_harness",
            "owner": "loader-context/native-addon-host",
            "reason": "The fixture resolves and spawns Node's embedtest helper binary with --embedder-snapshot-blob to create and reload an embedder snapshot, so it is host embedder-binary evidence rather than a multi-tenant isolate support claim.",
        },
        "test/embedding/test-shared-embedding-v8.js": {
            "expectation": "expected_skip",
            "classification": "requires_native_addon_harness",
            "owner": "loader-context/native-addon-host",
            "reason": "The fixture self-skips unless the Node test build links against the shared Node.js library, then resolves and spawns shared_embedtest; this is host embedder-binary coverage outside the default isolate runtime.",
        },
        "test/ffi/test-ffi-module.js": {
            "expectation": "expected_gap",
            "classification": "requires_native_addon_harness",
            "owner": "loader-context/native-addon-host",
            "reason": "The fixture runs under --experimental-ffi, imports node:ffi, and exercises subprocess-gated native FFI/dlopen behavior, which must remain fail-closed for the default multi-tenant isolate runtime.",
        },
        "test/ffi/test-ffi-shared-buffer.js": {
            "expectation": "expected_gap",
            "classification": "requires_native_addon_harness",
            "owner": "loader-context/native-addon-host",
            "reason": "The fixture requires --experimental-ffi plus internal/test/binding('ffi') and dlopen-backed shared-buffer calls against a native test library, so it is native host-surface evidence outside the default isolate contract.",
        },
        "test/parallel/test-webcrypto-derivebits-argon2.js": {
            "expectation": "expected_skip",
            "classification": "upstream_known_issue_or_platform_boundary",
            "owner": "networking/crypto-provider",
            "reason": "The official Node26 fixture self-skips unless the linked provider reports OpenSSL >= 3.2 because Argon2 WebCrypto vectors are unavailable on older or BoringSSL-family providers.",
        },
        "test/parallel/test-util-styletext.js": {
            "expectation": "expected_gap",
            "classification": "requires_pseudo_tty_host_harness",
            "owner": "process-and-timing/tty-host",
            "reason": "The fixture's styleText stream validation section requires a host TTY fd from common.getTTYfd(), so it is terminal-harness evidence rather than a multi-tenant isolate support claim.",
        }
    },
}
LANE_AWARE_BATCH_MACROS = {
    "node20_only_batch_case",
    "node22_exclusive_batch_case",
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


def discover_fixture_files(lane: str) -> set[str]:
    metadata = lane_metadata(lane)
    fixture_root = repo_root() / metadata["vendored_fixture_root"]
    return {f"test/{path}" for path in discover_git_fixture_files(fixture_root)}


def rust_source_lines() -> list[str]:
    root = repo_root() / RUST_NODE_COMPAT_ROOT
    lines: list[str] = []
    for path in sorted(root.rglob("*.rs")):
        lines.extend(path.read_text(encoding="utf-8").splitlines())
    return lines


def fixture_literals(text: str) -> set[str]:
    return set(
        re.findall(r'"((?:node[0-9]+/)?test/[^"\\]*(?:\.js|\.mjs|\.cjs))"', text)
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


def node_compat_batch_entries(text: str) -> list[tuple[str, tuple[int, int]]]:
    entries: list[tuple[str, tuple[int, int]]] = []
    pattern = re.compile(r"\bNodeCompatBatchEntry\s*\{")
    index = 0
    while True:
        match = pattern.search(text, index)
        if match is None:
            break
        depth = 1
        cursor = match.end()
        while cursor < len(text) and depth > 0:
            char = text[cursor]
            if char == "{":
                depth += 1
            elif char == "}":
                depth -= 1
            cursor += 1
        if depth == 0:
            entries.append((text[match.end() : cursor - 1], (match.start(), cursor)))
        index = max(cursor, match.end())
    return entries


def string_args(text: str) -> list[str]:
    return re.findall(r'"([^"\\]*(?:\\.[^"\\]*)*)"', text)


def field_string(text: str, field_name: str) -> str | None:
    match = re.search(
        rf"\b{re.escape(field_name)}\s*:\s*(?:Some\(\s*)?\"([^\"\\]*(?:\\.[^\"\\]*)*)\"",
        text,
        flags=re.S,
    )
    if match is None:
        return None
    return match.group(1)


def field_is_none(text: str, field_name: str) -> bool:
    return re.search(rf"\b{re.escape(field_name)}\s*:\s*None\b", text) is not None


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

    for body, (start, end) in node_compat_batch_entries(text):
        for offset in range(start, end):
            masked[offset] = " "
        test_relative_path = field_string(body, "test_relative_path")
        lane_source_field = f"{lane}_fixture_source_path"
        if field_is_none(body, lane_source_field):
            continue
        if field_string(body, lane_source_field) is not None:
            add(test_relative_path)

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
            elif lane in {"node24", "node26"}:
                add(test_relative_path)
        elif name == "split_batch_case":
            if lane == "node20" and has_node20_fixture_source_path:
                add(test_relative_path)
            elif lane == "node22" and has_node22_fixture_source_path:
                add(test_relative_path)
            elif lane in {"node24", "node26"}:
                add(test_relative_path)
        elif name == "shared_lane_fixture_batch_case":
            if has_fixture_source_path:
                add(test_relative_path)
        elif name == "node20_only_batch_case":
            if lane == "node20" and has_fixture_source_path:
                add(test_relative_path)
            elif lane in {"node24", "node26"}:
                add(test_relative_path)
        elif name == "node22_only_batch_case":
            if lane == "node22" and has_fixture_source_path:
                add(test_relative_path)
            elif lane in {"node24", "node26"}:
                add(test_relative_path)
        elif name == "node22_exclusive_batch_case":
            if (lane == "node22" and has_fixture_source_path) or lane == "node26":
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
            elif lane == "node26":
                add(test_relative_path)
            else:
                add(test_relative_path)

    for literal in fixture_literals("".join(masked)):
        add(literal)
    return references


def body_executes_node_compat_fixtures(text: str) -> bool:
    return any(marker in text for marker in RUST_EXECUTION_MARKERS)


def inferred_test_function_lane(name: str, body: str) -> str | None:
    """Infer which Node lane a Rust test actually executes.

    Status numerators must come from runtime execution evidence. Topology and
    report-shape tests mention batch constants for all lanes, but they do not
    run those fixtures. Lane inference therefore follows the concrete execution
    call sites: test function names, explicit `NodeCompatLane::NodeXX`
    arguments, and lane-prefixed fixture source paths.
    """

    lanes: set[str] = set()
    for lane in RUST_EXECUTED_FIXTURE_LANES:
        if re.search(rf"(^|_){lane}($|_)", name):
            lanes.add(lane)

    for major in ("20", "22", "24", "26"):
        if f"NodeCompatLane::Node{major}" in body:
            lanes.add(f"node{major}")

    for literal in fixture_literals(body):
        if literal.startswith("node") and "/" in literal:
            prefix, _relative = literal.split("/", 1)
            if prefix in RUST_EXECUTED_FIXTURE_LANES:
                lanes.add(prefix)

    if len(lanes) == 1:
        return next(iter(lanes))
    return None


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
            if ";" in line and depth <= 0:
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
        if not body_executes_node_compat_fixtures(body):
            index += 1
            continue
        if inferred_test_function_lane(name, body) != lane:
            index += 1
            continue
        literals = set(lane_fixture_literals(body, lane))
        expands_broad_ignored_watchpoint = (
            any("ignore" in attr for attr in attrs)
            and "run_manifested_subset_for_lane(" in body
        )
        if not expands_broad_ignored_watchpoint:
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
    if lane not in RUST_EXECUTED_FIXTURE_LANES:
        return RustFixtureRefs(nonignored=set(), ignored={})
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


def forced_lane_classification(lane: str, path: str) -> dict[str, str] | None:
    return FORCED_LANE_CLASSIFICATIONS.get(lane, {}).get(path)


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
    forced_paths = set(FORCED_LANE_CLASSIFICATIONS.get(lane, {})) & fixtures
    nongreen_paths = set(fixtures - refs.nonignored)
    nongreen_paths.update(forced_paths)
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
        classification = forced_lane_classification(lane, path) or classification_for_unpromoted(
            path, fixture_root
        )
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
