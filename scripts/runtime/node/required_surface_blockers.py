#!/usr/bin/env python3
"""Generate the NDS3 required-surface blocker inventory.

The default-support posture is generated from the status/classification
catalogs. This script derives the exact remaining `v8_isolate_required` gap
lists used by the NDS3 post-2000 burn-down proof.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Any


@dataclass(frozen=True)
class BlockerGroup:
    key: str
    title: str
    owner_repos: tuple[str, ...]
    follow_up: str


GROUPS: tuple[BlockerGroup, ...] = (
    BlockerGroup(
        key="module_loader_esm_cjs_hooks_wasm",
        title="Module loader, ESM/CJS, hooks, and WASM/source-phase semantics",
        owner_repos=("nimbus/nimbus", "nimbus/deno"),
        follow_up=(
            "Open a module-loader implementation wave covering package "
            "exports/imports, CJS/ESM bridge, import attributes, loader hooks, "
            "internal resolver modules, and WASM/source-phase semantics before "
            "re-entering these fixtures as promotion candidates."
        ),
    ),
    BlockerGroup(
        key="async_hooks_promises_timers_lifecycle",
        title="Async hooks, promises, timers, diagnostics, and lifecycle semantics",
        owner_repos=("nimbus/deno", "nimbus/nimbus"),
        follow_up=(
            "Open an async resource lifecycle implementation wave covering hook "
            "ordering, destroy queues, promise hook ancestry, timer resource "
            "lifecycle, diagnostics channel, perf hooks, and trace-event output."
        ),
    ),
    BlockerGroup(
        key="vm_domain_v8_runtime",
        title="VM, domain, V8 runtime, serialization, and trace/runtime boundaries",
        owner_repos=("nimbus/nimbus", "nimbus/deno", "nimbus/rusty_v8"),
        follow_up=(
            "Open a VM/V8/domain implementation wave with exact V8 API and "
            "rusty_v8 ownership before claiming byte-for-byte V8 serialization, "
            "vm/module contexts, domain propagation, ShadowRealm, or "
            "trace/runtime parity."
        ),
    ),
    BlockerGroup(
        key="process_host_os_policy",
        title="Process, OS metadata, lifecycle, and host policy semantics",
        owner_repos=("nimbus/nimbus", "nimbus/deno", "host-runtime-policy"),
        follow_up=(
            "Define the virtual process/OS contract, then implement app-visible "
            "metadata and lifecycle events while preserving fail-closed "
            "diagnostics for host-owned process control, cwd/sys access, "
            "signals, subprocesses, and raw process state."
        ),
    ),
    BlockerGroup(
        key="local_io_stream_fs_policy",
        title="Local filesystem, stream, watch, permission, and handle lifecycle semantics",
        owner_repos=("nimbus/nimbus", "nimbus/deno", "host-runtime-policy"),
        follow_up=(
            "Define the virtual/ephemeral filesystem and stream-handle contract, "
            "then implement filehandle/opendir/watch/symlink/rm/error-shape/"
            "autoclose semantics without widening host filesystem permissions."
        ),
    ),
    BlockerGroup(
        key="crypto_networking_webcrypto",
        title="Crypto, WebCrypto, DNS, TLS, QUIC, and package-critical networking semantics",
        owner_repos=("nimbus/deno", "nimbus/nimbus", "host-runtime-policy"),
        follow_up=(
            "Open a crypto/networking implementation wave covering Node crypto/"
            "WebCrypto error shape and algorithms, DNS promise behavior, TLS/"
            "HTTPS client semantics, and fail-closed network host boundaries."
        ),
    ),
    BlockerGroup(
        key="web_platform_webstreams_url_encoding",
        title="Web platform, WebStreams, WHATWG URL/encoding, Blob, and DOM-style globals",
        owner_repos=("nimbus/deno", "nimbus/nimbus"),
        follow_up=(
            "Open a Deno-owned Web API implementation wave for Blob, WebStreams "
            "adapters/compression/transfer, WHATWG encoding and URL error "
            "shapes, URLPattern, WebIDL brands, and selected globals."
        ),
    ),
    BlockerGroup(
        key="internal_native_test_surface",
        title="Internal, native-addon, SEA, snapshot, lint/test-only, and private Node surface",
        owner_repos=("nimbus/deno", "nimbus/nimbus", "host-runtime-policy"),
        follow_up=(
            "Classify internal/test-only/native fixtures precisely, preserve "
            "diagnostics, and promote only the subset demanded by real package "
            "canaries or public API semantics; native addon/SEA/snapshot host "
            "surfaces should remain outside positive isolate support unless a "
            "new architecture plan says otherwise."
        ),
    ),
    BlockerGroup(
        key="core_semantics_residual",
        title="Core JavaScript/Node semantic residuals",
        owner_repos=("nimbus/nimbus", "nimbus/deno", "nimbus/rusty_v8"),
        follow_up=(
            "Close a core-semantics implementation wave for Buffer/assert/"
            "console/events/path/URL/error/stack/string-byte/inspect behavior "
            "after the larger implementation blockers are assigned, because "
            "these are app-visible and should not remain vague residuals."
        ),
    ),
)

GROUP_BY_KEY = {group.key: group for group in GROUPS}
DEFAULT_GROUP = "internal_native_test_surface"


def repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def load_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def write_json(path: Path, data: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        json.dump(data, handle, indent=2, sort_keys=True)
        handle.write("\n")


def previous_group_hints(path: Path) -> dict[tuple[str, str], str]:
    if not path.is_file():
        return {}
    try:
        data = load_json(path)
    except json.JSONDecodeError:
        return {}
    hints: dict[tuple[str, str], str] = {}
    for group_key, group in data.get("groups", {}).items():
        if group_key not in GROUP_BY_KEY:
            continue
        lanes = group.get("lanes", {})
        for lane, lane_data in lanes.items():
            for fixture in lane_data.get("fixtures", []):
                test_path = fixture.get("test_path")
                if isinstance(test_path, str):
                    hints[(lane, test_path)] = group_key
    return hints


def fallback_group(entry: dict[str, Any]) -> str:
    owner = str(entry.get("owner", ""))
    test_path = str(entry.get("test_path", ""))
    haystack = f"{owner} {test_path}".lower().replace("_", "-")

    if owner.startswith("core-semantics/") or any(
        token in haystack
        for token in (
            "test-assert",
            "test-buffer",
            "test-console",
            "test-events",
            "test-path",
            "test-querystring",
            "test-string",
            "test-url",
            "test-util",
        )
    ):
        return "core_semantics_residual"
    if owner.startswith("networking/") or any(
        token in haystack
        for token in (
            "crypto",
            "webcrypto",
            "dns",
            "dgram",
            "http",
            "https",
            "http2",
            "tls",
            "quic",
            "zlib",
        )
    ):
        return "crypto_networking_webcrypto"
    if owner.startswith("streams-local-io/") or any(
        token in haystack
        for token in (
            "test-fs",
            "filehandle",
            "opendir",
            "read-stream",
            "write-stream",
            "watch",
            "symlink",
            "realpath",
        )
    ):
        return "local_io_stream_fs_policy"
    if owner in {
        "process-and-timing/diagnostics-channel",
        "process-and-timing/perf-hooks",
        "process-and-timing/timers",
    } or any(
        token in haystack
        for token in (
            "async-hooks",
            "asyncresource",
            "async-wrap",
            "diagnostic-channel",
            "diagnostics-channel",
            "perf-hooks",
            "promise",
            "queue-microtask",
            "timers",
            "trace",
        )
    ):
        return "async_hooks_promises_timers_lifecycle"
    if owner in {
        "process-and-timing/process-host",
        "process-and-timing/os",
    } or any(token in haystack for token in ("test-process", "test-os")):
        return "process_host_os_policy"
    if owner in {
        "loader-context/domain",
        "loader-context/vm",
        "runtime/v8",
    } or any(
        token in haystack
        for token in (
            "test-domain",
            "test-v8",
            "test-vm",
            "serdes",
            "shadowrealm",
        )
    ):
        return "vm_domain_v8_runtime"
    if owner.startswith("loader-context/") or any(
        token in haystack
        for token in (
            "cjs",
            "commonjs",
            "esm",
            "es-module",
            "exports",
            "import",
            "loader",
            "module",
            "package",
            "require",
            "source-phase",
            "typescript",
            "wasm",
        )
    ):
        return "module_loader_esm_cjs_hooks_wasm"
    if any(
        token in haystack
        for token in (
            "abortcontroller",
            "blob",
            "broadcastchannel",
            "domexception",
            "encoding",
            "eventtarget",
            "urlpattern",
            "webidl",
            "webstreams",
            "whatwg",
        )
    ):
        return "web_platform_webstreams_url_encoding"
    return DEFAULT_GROUP


def sanitized_fixture(entry: dict[str, Any]) -> dict[str, str]:
    return {
        "owner": str(entry.get("owner", "")),
        "reason_code": str(entry.get("reason_code", "")),
        "shim_classification": str(entry.get("shim_classification", "")),
        "source_classification": str(entry.get("source_classification", "")),
        "source_expectation": str(entry.get("source_expectation", "")),
        "test_path": str(entry.get("test_path", "")),
    }


def build_inventory(posture_path: Path, output_json_path: Path) -> dict[str, Any]:
    posture_bytes = posture_path.read_bytes()
    posture = json.loads(posture_bytes)
    hints = previous_group_hints(output_json_path)

    groups: dict[str, dict[str, Any]] = {
        group.key: {
            "title": group.title,
            "owner_repos": list(group.owner_repos),
            "follow_up": group.follow_up,
            "lanes": {},
        }
        for group in GROUPS
    }
    totals: dict[str, dict[str, int]] = {}

    for lane in ("node22", "node24"):
        entries = [
            entry
            for entry in posture["lanes"][lane]["entries"]
            if entry.get("support_denominator") == "v8_isolate_required"
        ]
        totals[lane] = {"required_gap_count": len(entries)}
        for entry in sorted(entries, key=lambda item: item.get("test_path", "")):
            test_path = str(entry.get("test_path", ""))
            group_key = hints.get((lane, test_path), fallback_group(entry))
            if group_key not in GROUP_BY_KEY:
                group_key = DEFAULT_GROUP
            lane_data = groups[group_key]["lanes"].setdefault(
                lane, {"count": 0, "fixtures": []}
            )
            lane_data["fixtures"].append(sanitized_fixture(entry))
            lane_data["count"] += 1

    for group in groups.values():
        for lane_data in group["lanes"].values():
            lane_data["fixtures"].sort(key=lambda item: item["test_path"])

    return {
        "schema_version": 1,
        "purpose": (
            "Exact fixture lists for the NDS3 post-2000 required-surface "
            "blocker inventory."
        ),
        "generated_from": str(posture_path.relative_to(repo_root())),
        "generated_from_sha256": hashlib.sha256(posture_bytes).hexdigest(),
        "predicate": "lane entries where support_denominator == v8_isolate_required",
        "totals": totals,
        "groups": groups,
    }


def group_lane_count(group: dict[str, Any], lane: str) -> int:
    return int(group.get("lanes", {}).get(lane, {}).get("count", 0))


def owner_repos_markdown(owner_repos: list[str]) -> str:
    return ", ".join(f"`{owner}`" for owner in owner_repos)


def render_markdown(inventory: dict[str, Any]) -> str:
    totals = inventory["totals"]
    groups = inventory["groups"]
    node24_sorted = sorted(
        groups.items(),
        key=lambda item: (group_lane_count(item[1], "node24"), item[0]),
        reverse=True,
    )
    next_key, next_group = node24_sorted[0]
    next_node22 = group_lane_count(next_group, "node22")
    next_node24 = group_lane_count(next_group, "node24")

    lines = [
        "# NDS3 Required-Surface Blocker Inventory",
        "",
        "<!-- generated by scripts/runtime/node/required_surface_blockers.py; do not edit by hand -->",
        "",
        "This proof is the post-`2000` required-surface control plane for NDS3. The",
        "Node24 full-corpus confidence gate is green, but NDS3 cannot close while",
        "Node22 and Node24 still have `v8_isolate_required` gaps unless every remaining",
        "gap is either fixed or preserved as an exact blocker with owner repository,",
        "fixture list, follow-up plan, and an unsatisfied verifier gate.",
        "",
        "This is not a terminal blocked state. It is the required inventory before the",
        "next implementation waves. The verifier must continue to fail NDS3 closeout",
        "until the required surface is green or the developer accepts a blocked closeout",
        "with linked follow-up PRs/issues.",
        "",
        "## Source Of Truth",
        "",
        f"- Source posture: `{inventory['generated_from']}`",
        "- Predicate: lane entries where",
        "  `support_denominator == \"v8_isolate_required\"`",
        "- Exact generated inventory:",
        "  `docs/private/plans/proof/node-default-runtime-support-hardening/nds3-required-surface-blockers.json`",
        f"- Source posture SHA-256: `{inventory['generated_from_sha256']}`",
        "- Coverage check: every required gap is assigned to exactly one blocker group.",
        "",
        "| Lane | Required gaps inventoried |",
        "| --- | ---: |",
        f"| `node22` | {totals['node22']['required_gap_count']} |",
        f"| `node24` | {totals['node24']['required_gap_count']} |",
        "",
        "## Blocker Groups",
        "",
        "| Blocker group | Node22 | Node24 | Owner repo | Follow-up plan |",
        "| --- | ---: | ---: | --- | --- |",
    ]
    for group_key in sorted(groups):
        group = groups[group_key]
        lines.append(
            f"| {group['title']} | {group_lane_count(group, 'node22')} | "
            f"{group_lane_count(group, 'node24')} | "
            f"{owner_repos_markdown(group['owner_repos'])} | {group['follow_up']} |"
        )

    lines.extend(
        [
            "",
            "## Execution Rules",
            "",
            "Each follow-up wave must use the NDS3 throughput rules:",
            "",
            "- Start with the exact fixture list in",
            "  `nds3-required-surface-blockers.json`, then select the broadest coherent",
            "  batch for the group.",
            "- Run the ignored broad batch before fixes and retain the failure inventory",
            "  under an absolute `NIMBUS_NODE_COMPAT_DIAGNOSTIC_ROOT`.",
            "- Use focused tests only to close root-cause clusters, then rerun the same",
            "  broad batch before promoting any support.",
            "- For `nimbus/deno` owner work, prove the fix in the canonical local Deno",
            "  worktree, batch related fixes, tag only after local broad green, repin",
            "  Nimbus, and rerun the immutable-tag broad batch before promotion.",
            "- Regenerate generated evidence only at checkpoints: local broad green,",
            "  repinned broad green, promoted non-ignored broad green, or a diagnostic",
            "  catalog update that changes public generated counts.",
            "- If a wave proves host-owned, native-addon, CLI/test-harness-only, or",
            "  otherwise low ROI, record the exact fixture list in the JSON inventory,",
            "  preserve diagnostics, leave the NDS3 verifier gate unsatisfied, and move to",
            "  the next required-surface wave.",
            "",
            "## Next Wave",
            "",
            "The next ROI-ranked implementation wave is",
            f"`{next_key}`: it is the largest remaining required blocker",
            f"(`{next_node24}` Node24 gaps and `{next_node22}` Node22 gaps), is",
            "app-visible, and must start with an implementation-scope design before",
            "returning to the exact fixture batch for proof.",
            "",
            "If that wave is split out of the current draft PR, the follow-up issue or PR",
            "must link this proof, link the exact JSON inventory, and keep the NDS3 verifier",
            "gate unsatisfied until it lands or a developer-approved blocked closeout is",
            "recorded.",
            "",
        ]
    )
    return "\n".join(lines)


def write_markdown(path: Path, inventory: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(render_markdown(inventory), encoding="utf-8")


def validate_inventory(inventory: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    for lane in ("node22", "node24"):
        seen: Counter[str] = Counter()
        counted = 0
        for group_key, group in inventory.get("groups", {}).items():
            if group_key not in GROUP_BY_KEY:
                errors.append(f"unknown group {group_key}")
            lane_data = group.get("lanes", {}).get(lane, {})
            fixtures = lane_data.get("fixtures", [])
            if lane_data.get("count", 0) != len(fixtures):
                errors.append(f"{group_key}:{lane} count does not match fixture list")
            counted += len(fixtures)
            seen.update(str(fixture.get("test_path", "")) for fixture in fixtures)
        expected = inventory["totals"][lane]["required_gap_count"]
        if counted != expected:
            errors.append(f"{lane} assigned {counted} fixtures but expected {expected}")
        duplicates = sorted(path for path, count in seen.items() if count > 1)
        if duplicates:
            errors.append(f"{lane} duplicate fixture assignments: {', '.join(duplicates[:5])}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="validate generated files")
    parser.add_argument(
        "--posture",
        default="docs/private/architecture/runtime/node-default-support-posture.json",
        help="source posture JSON path, relative to the repository root by default",
    )
    parser.add_argument(
        "--json",
        default=(
            "docs/private/plans/proof/node-default-runtime-support-hardening/"
            "nds3-required-surface-blockers.json"
        ),
        help="blocker inventory JSON path, relative to the repository root by default",
    )
    parser.add_argument(
        "--markdown",
        default=(
            "docs/private/plans/proof/node-default-runtime-support-hardening/"
            "nds3-required-surface-blockers.md"
        ),
        help="blocker inventory Markdown path, relative to the repository root by default",
    )
    args = parser.parse_args()

    repo = repo_root()
    posture_path = (repo / args.posture).resolve()
    json_path = (repo / args.json).resolve()
    markdown_path = (repo / args.markdown).resolve()

    inventory = build_inventory(posture_path, json_path)
    errors = validate_inventory(inventory)
    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1

    expected_json = json.dumps(inventory, indent=2, sort_keys=True) + "\n"
    expected_markdown = render_markdown(inventory)
    if args.check:
        if not json_path.is_file() or json_path.read_text(encoding="utf-8") != expected_json:
            print(f"error: {json_path} is stale", file=sys.stderr)
            return 1
        if not markdown_path.is_file() or markdown_path.read_text(encoding="utf-8") != expected_markdown:
            print(f"error: {markdown_path} is stale", file=sys.stderr)
            return 1
        print("node required-surface blocker inventory: pass")
        return 0

    write_json(json_path, inventory)
    write_markdown(markdown_path, inventory)
    print(f"wrote {json_path}")
    print(f"wrote {markdown_path}")
    for lane in ("node22", "node24"):
        print(f"{lane} required gaps: {inventory['totals'][lane]['required_gap_count']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
