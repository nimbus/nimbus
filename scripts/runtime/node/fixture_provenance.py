#!/usr/bin/env python3
"""Validate vendored Node fixture provenance and published LTS coverage."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

from schema import load_json  # noqa: E402


HEX_SHA = re.compile(r"^[0-9a-f]{40}$")
SUPPORTED_LTS_PHASES = {"maintenance_lts", "active_lts"}
SUPPORTED_LTS_POLICY = "supported_lts_lane_local_evidence"
SOURCE_KIND = "vendored_official_fixture_corpus"

REPO_ROOT = Path(__file__).resolve().parents[3]
LANE_MANIFEST_ROOT = (
    REPO_ROOT
    / "crates"
    / "nimbus-runtime"
    / "src"
    / "runtime"
    / "tests"
    / "node_compat_manifests"
    / "lanes"
)
REGISTRY_PATH = (
    REPO_ROOT
    / "docs"
    / "private"
    / "architecture"
    / "runtime"
    / "node-lts-compat"
    / "node-lts-lanes.json"
)
PUBLISHED_STATUS_PATH = (
    REPO_ROOT
    / "docs"
    / "private"
    / "architecture"
    / "runtime"
    / "node-compat-evidence"
    / "latest"
    / "status-summary.json"
)


def display_path(path: Path) -> str:
    try:
        return str(path.relative_to(REPO_ROOT))
    except ValueError:
        return str(path)


def as_object(value: Any, context: str, errors: list[str]) -> dict[str, Any]:
    if isinstance(value, dict):
        return value
    errors.append(f"{context} must be a JSON object")
    return {}


def required_string(
    payload: dict[str, Any],
    key: str,
    context: str,
    errors: list[str],
) -> str:
    value = payload.get(key)
    if not isinstance(value, str) or not value:
        errors.append(f"{context}.{key} must be a non-empty string")
        return ""
    return value


def load_lane_manifests() -> list[tuple[Path, dict[str, Any]]]:
    return [
        (path, load_json(path))
        for path in sorted(LANE_MANIFEST_ROOT.glob("*.json"))
    ]


def registry_lanes_by_name() -> dict[str, dict[str, Any]]:
    registry = load_json(REGISTRY_PATH)
    lanes = registry.get("lanes", []) if isinstance(registry, dict) else []
    return {
        lane["lane_name"]: lane
        for lane in lanes
        if isinstance(lane, dict) and isinstance(lane.get("lane_name"), str)
    }


def status_lanes_by_name(status_path: Path) -> dict[str, dict[str, Any]]:
    status = load_json(status_path)
    lanes = status.get("lane_summaries", []) if isinstance(status, dict) else []
    return {
        lane["lane"]: lane
        for lane in lanes
        if isinstance(lane, dict) and isinstance(lane.get("lane"), str)
    }


def validate_sha(value: str, context: str, errors: list[str]) -> None:
    if not HEX_SHA.match(value):
        errors.append(f"{context} must be a 40-character lowercase hex SHA")


def expected_selection_command(lane: str, tag: str) -> str:
    return (
        "python3 scripts/runtime/node/sync.py "
        f"--lane {lane} --upstream-tag {tag} --apply"
    )


def validate_lane_manifest(
    path: Path,
    manifest: dict[str, Any],
    registry_lane: dict[str, Any] | None,
) -> list[str]:
    errors: list[str] = []
    context = display_path(path)
    lane = required_string(manifest, "lane", context, errors)
    upstream = as_object(manifest.get("upstream"), f"{context}.upstream", errors)
    provenance = as_object(
        manifest.get("fixture_provenance"),
        f"{context}.fixture_provenance",
        errors,
    )

    if upstream.get("source_kind") != SOURCE_KIND:
        errors.append(f"{context}.upstream.source_kind must be {SOURCE_KIND}")

    repo = required_string(upstream, "repo", f"{context}.upstream", errors)
    if repo != "nodejs/node":
        errors.append(f"{context}.upstream.repo must be nodejs/node")

    tag = required_string(upstream, "tag", f"{context}.upstream", errors)
    commit = required_string(upstream, "commit", f"{context}.upstream", errors)
    tag_object = required_string(upstream, "tag_object", f"{context}.upstream", errors)
    fixture_subtree = required_string(
        upstream, "fixture_subtree", f"{context}.upstream", errors
    )
    if fixture_subtree != "test":
        errors.append(f"{context}.upstream.fixture_subtree must be test")
    validate_sha(commit, f"{context}.upstream.commit", errors)
    validate_sha(tag_object, f"{context}.upstream.tag_object", errors)

    synced_at = required_string(
        provenance, "synced_at", f"{context}.fixture_provenance", errors
    )
    if len(synced_at) < 10:
        errors.append(f"{context}.fixture_provenance.synced_at must include a date")
    selection_command = required_string(
        provenance,
        "selection_command",
        f"{context}.fixture_provenance",
        errors,
    )
    nimbus_sync_commit = required_string(
        provenance,
        "nimbus_sync_commit",
        f"{context}.fixture_provenance",
        errors,
    )
    validate_sha(
        nimbus_sync_commit,
        f"{context}.fixture_provenance.nimbus_sync_commit",
        errors,
    )
    required_string(
        provenance, "recorded_at", f"{context}.fixture_provenance", errors
    )
    required_string(
        provenance, "recorded_from", f"{context}.fixture_provenance", errors
    )

    expected_selection = expected_selection_command(lane, tag)
    if selection_command != expected_selection:
        errors.append(
            f"{context}.fixture_provenance.selection_command must be {expected_selection!r}"
        )

    vendored_fixture_root = required_string(
        manifest, "vendored_fixture_root", context, errors
    )
    if vendored_fixture_root and not (REPO_ROOT / vendored_fixture_root).is_dir():
        errors.append(f"{context}.vendored_fixture_root does not exist")

    if registry_lane is None:
        errors.append(f"{context} has no matching lane registry entry")
    else:
        if registry_lane.get("fixture_corpus_path") != vendored_fixture_root:
            errors.append(
                f"{context}.vendored_fixture_root does not match lane registry fixture_corpus_path"
            )
        if registry_lane.get("fixture_corpus_upstream_tag") != tag:
            errors.append(
                f"{context}.upstream.tag does not match lane registry fixture_corpus_upstream_tag"
            )

    return errors


def supported_lts_registry_lanes(
    lanes_by_name: dict[str, dict[str, Any]],
) -> list[dict[str, Any]]:
    return [
        lane
        for lane in lanes_by_name.values()
        if lane.get("support_phase") in SUPPORTED_LTS_PHASES
        and lane.get("evidence_policy") == SUPPORTED_LTS_POLICY
    ]


def validate_published_supported_lts_status(
    lanes_by_name: dict[str, dict[str, Any]],
    status_path: Path,
) -> list[str]:
    errors: list[str] = []
    status_lanes = status_lanes_by_name(status_path)
    for lane in supported_lts_registry_lanes(lanes_by_name):
        lane_name = lane["lane_name"]
        status = status_lanes.get(lane_name)
        if status is None:
            errors.append(
                f"{display_path(status_path)} is missing supported LTS lane {lane_name}"
            )
            continue
        unclassified = status.get("unmanifested_or_unclassified_count")
        if not isinstance(unclassified, int):
            errors.append(
                f"{display_path(status_path)} {lane_name} unmanifested_or_unclassified_count must be an integer"
            )
            continue
        if unclassified != 0:
            errors.append(
                f"{display_path(status_path)} {lane_name} has {unclassified} unclassified published fixtures"
            )
    return errors


def validate_fixture_provenance(status_path: Path = PUBLISHED_STATUS_PATH) -> list[str]:
    errors: list[str] = []
    lanes_by_name = registry_lanes_by_name()
    vendored_count = 0

    for path, manifest in load_lane_manifests():
        if not isinstance(manifest, dict):
            errors.append(f"{display_path(path)} must contain a JSON object")
            continue
        upstream = manifest.get("upstream", {})
        if isinstance(upstream, dict) and upstream.get("source_kind") == SOURCE_KIND:
            vendored_count += 1
        lane_name = manifest.get("lane") if isinstance(manifest.get("lane"), str) else None
        registry_lane = lanes_by_name.get(lane_name) if lane_name else None
        errors.extend(validate_lane_manifest(path, manifest, registry_lane))

    if vendored_count == 0:
        errors.append("no vendored Node fixture corpora were found")

    errors.extend(validate_published_supported_lts_status(lanes_by_name, status_path))
    return errors


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Validate Node fixture provenance and supported LTS coverage"
    )
    subparsers = parser.add_subparsers(dest="command", required=True)
    validate_parser = subparsers.add_parser("validate")
    validate_parser.add_argument(
        "--status-summary",
        type=Path,
        default=PUBLISHED_STATUS_PATH,
        help="published status-summary.json to guard for unclassified supported LTS results",
    )
    return parser


def main() -> int:
    args = build_parser().parse_args()
    if args.command != "validate":
        raise AssertionError(f"unhandled command {args.command}")

    status_path = args.status_summary.resolve()
    errors = validate_fixture_provenance(status_path)
    if errors:
        for error in errors:
            print(f"error: {error}")
        return 1

    registry_lanes = registry_lanes_by_name()
    vendored_lanes = [
        manifest["lane"]
        for _, manifest in load_lane_manifests()
        if isinstance(manifest, dict)
        and isinstance(manifest.get("upstream"), dict)
        and manifest["upstream"].get("source_kind") == SOURCE_KIND
    ]
    supported_lts = supported_lts_registry_lanes(registry_lanes)
    print(
        "validated Node fixture provenance: "
        f"{len(vendored_lanes)} vendored corpora, "
        f"{len(supported_lts)} supported LTS lanes with zero unclassified published results"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
