#!/usr/bin/env python3

from __future__ import annotations

import argparse
import copy
import json
import os
import re
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

from schema import default_schema_path, load_json, validate_payload_against_schema  # noqa: E402


REPO_ROOT = Path(__file__).resolve().parents[3]
LATEST_TAGS_PATH = (
    REPO_ROOT
    / "tests"
    / "runtime"
    / "node"
    / "compat"
    / "node-lts-compat"
    / "node-latest-suite-tags.json"
)
SCHEMA_PATH = default_schema_path("node-latest-suite-tags.schema.json")
REGISTRY_PATH = (
    REPO_ROOT
    / "tests"
    / "runtime"
    / "node"
    / "compat"
    / "node-lts-compat"
    / "node-lts-lanes.json"
)
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

EXPECTED_LANES = {"node20", "node22", "node24", "node26"}
HEX_SHA = re.compile(r"^[0-9a-f]{40}$")


def display_path(path: Path) -> str:
    try:
        return str(path.relative_to(REPO_ROOT))
    except ValueError:
        return str(path)


def lane_manifest_path(lane: str) -> Path:
    return LANE_MANIFEST_ROOT / f"{lane}.json"


def registry_lanes_by_name() -> dict[str, dict[str, Any]]:
    registry = load_json(REGISTRY_PATH)
    return {
        lane["lane_name"]: lane
        for lane in registry.get("lanes", [])
        if isinstance(lane, dict) and isinstance(lane.get("lane_name"), str)
    }


def latest_lanes_by_name(payload: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return {
        lane["lane"]: lane
        for lane in payload.get("lanes", [])
        if isinstance(lane, dict) and isinstance(lane.get("lane"), str)
    }


def validate_sha(errors: list[str], value: Any, owner: str) -> None:
    if not isinstance(value, str) or not HEX_SHA.match(value):
        errors.append(f"{owner} must be a 40-character lowercase hex SHA")


def validate_latest_payload(payload: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    for schema_error in validate_payload_against_schema(payload, SCHEMA_PATH):
        errors.append(json.dumps(schema_error, sort_keys=True))

    lanes = latest_lanes_by_name(payload)
    if set(lanes) != EXPECTED_LANES:
        errors.append("latest suite tags must include exactly node20, node22, node24, and node26")

    registry_lanes = registry_lanes_by_name()
    for lane_name in sorted(EXPECTED_LANES):
        lane = lanes.get(lane_name)
        registry_lane = registry_lanes.get(lane_name)
        if not isinstance(lane, dict):
            continue
        owner = f"latest_suite_tags {lane_name}"
        latest_tag = lane.get("latest_official_tag")
        current_tag = lane.get("fixture_corpus_current_tag")
        sync_required = lane.get("fixture_sync_required")

        if registry_lane is None:
            errors.append(f"{owner} has no lane registry entry")
        else:
            if registry_lane.get("upstream_tag") != latest_tag:
                errors.append(
                    f"{owner} latest_official_tag must match lane registry upstream_tag"
                )
            registry_fixture_tag = registry_lane.get("fixture_corpus_upstream_tag")
            if registry_fixture_tag != current_tag:
                errors.append(
                    f"{owner} fixture_corpus_current_tag must match lane registry fixture_corpus_upstream_tag"
                )

        validate_sha(errors, lane.get("latest_official_tag_object"), f"{owner}.latest_official_tag_object")
        validate_sha(errors, lane.get("latest_official_commit"), f"{owner}.latest_official_commit")

        has_current_corpus = current_tag is not None
        if has_current_corpus:
            validate_sha(
                errors,
                lane.get("fixture_corpus_current_tag_object"),
                f"{owner}.fixture_corpus_current_tag_object",
            )
            validate_sha(
                errors,
                lane.get("fixture_corpus_current_commit"),
                f"{owner}.fixture_corpus_current_commit",
            )
            manifest_path = lane_manifest_path(lane_name)
            if not manifest_path.is_file():
                errors.append(f"{owner} references missing lane manifest {display_path(manifest_path)}")
            else:
                manifest = load_json(manifest_path)
                manifest_tag = manifest.get("upstream", {}).get("tag")
                manifest_commit = manifest.get("upstream", {}).get("commit")
                manifest_tag_object = manifest.get("upstream", {}).get("tag_object")
                if manifest_tag != current_tag:
                    errors.append(f"{owner} current tag does not match lane manifest tag")
                if manifest_commit != lane.get("fixture_corpus_current_commit"):
                    errors.append(f"{owner} current commit does not match lane manifest commit")
                if manifest_tag_object != lane.get("fixture_corpus_current_tag_object"):
                    errors.append(
                        f"{owner} current tag object does not match lane manifest tag object"
                    )
        else:
            for key in ("fixture_corpus_current_tag_object", "fixture_corpus_current_commit"):
                if lane.get(key) is not None:
                    errors.append(f"{owner}.{key} must be null when no corpus is vendored")

        expected_sync_required = current_tag != latest_tag
        if sync_required is not expected_sync_required:
            errors.append(
                f"{owner}.fixture_sync_required must be {str(expected_sync_required).lower()}"
            )
        expected_command = (
            "python3 scripts/runtime/node/sync.py "
            f"--lane {lane_name} --upstream-tag {latest_tag} --apply"
        )
        if lane.get("intended_sync_command") != expected_command:
            errors.append(f"{owner}.intended_sync_command must be {expected_command!r}")

    return errors


def validate_latest_tags() -> list[str]:
    payload = load_json(LATEST_TAGS_PATH)
    if not isinstance(payload, dict):
        return [f"{display_path(LATEST_TAGS_PATH)} must contain a JSON object"]
    return validate_latest_payload(payload)


def stale_corpus_errors(payload: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    for lane_name, lane in sorted(latest_lanes_by_name(payload).items()):
        if lane.get("fixture_sync_required") is True:
            errors.append(
                f"{lane_name} fixture corpus is not current: "
                f"{lane.get('fixture_corpus_current_tag') or 'none'} -> "
                f"{lane.get('latest_official_tag')}"
            )
    return errors


def command_validate(_: argparse.Namespace) -> int:
    errors = validate_latest_tags()
    if errors:
        for error in errors:
            print(f"error: {error}")
        return 1
    payload = load_json(LATEST_TAGS_PATH)
    lanes = latest_lanes_by_name(payload)
    stale = [
        lane_name
        for lane_name, lane in sorted(lanes.items())
        if lane.get("fixture_sync_required") is True
    ]
    print(
        "validated Node latest suite tags: "
        f"{len(lanes)} lanes, {len(stale)} needing fixture sync"
    )
    return 0


def command_enforce_current_corpora(_: argparse.Namespace) -> int:
    errors = validate_latest_tags()
    if errors:
        for error in errors:
            print(f"error: {error}")
        return 1
    stale = stale_corpus_errors(load_json(LATEST_TAGS_PATH))
    if stale:
        for error in stale:
            print(f"error: {error}")
        return 1
    print("all targeted Node fixture corpora are current")
    return 0


def expect_invalid(name: str, mutator) -> list[str]:
    payload = copy.deepcopy(load_json(LATEST_TAGS_PATH))
    mutator(payload)
    errors = validate_latest_payload(payload)
    if not errors:
        return [f"negative self-test failed: {name} was accepted"]
    return []


def command_self_test(_: argparse.Namespace) -> int:
    errors: list[str] = []

    def stale_registry_tag(payload: dict[str, Any]) -> None:
        latest_lanes_by_name(payload)["node22"]["latest_official_tag"] = "v22.0.0"

    def false_sync_required(payload: dict[str, Any]) -> None:
        latest_lanes_by_name(payload)["node24"]["fixture_sync_required"] = True

    def missing_manifest_alignment(payload: dict[str, Any]) -> None:
        latest_lanes_by_name(payload)["node20"]["fixture_corpus_current_commit"] = "0" * 40

    for name, mutator in (
        ("stale_registry_tag", stale_registry_tag),
        ("false_sync_required", false_sync_required),
        ("missing_manifest_alignment", missing_manifest_alignment),
    ):
        errors.extend(expect_invalid(name, mutator))

    if errors:
        for error in errors:
            print(f"error: {error}")
        return 1
    print("Node latest suite tag negative self-tests passed")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Validate latest Node suite tag metadata")
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("validate").set_defaults(func=command_validate)
    subparsers.add_parser("self-test").set_defaults(func=command_self_test)
    subparsers.add_parser("enforce-current-corpora").set_defaults(
        func=command_enforce_current_corpora
    )
    return parser


def main() -> int:
    args = build_parser().parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
