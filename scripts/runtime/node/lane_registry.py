#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

from schema import default_schema_path, load_json, validate_payload_against_schema  # noqa: E402


REPO_ROOT = Path(__file__).resolve().parents[3]
REGISTRY_PATH = (
    REPO_ROOT
    / "docs"
    / "architecture"
    / "runtime"
    / "node-lts-compat"
    / "node-lts-lanes.json"
)
SCHEMA_PATH = default_schema_path("node-lts-lanes.schema.json")
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
REQUIRED_CONSUMER_CRATES = {"nimbus-runtime", "nimbus-tenant", "nimbus-convex"}
EXPECTED_PHASES = {
    "node20": "eol_legacy",
    "node22": "maintenance_lts",
    "node24": "active_lts",
    "node26": "current_non_lts",
}
EXPECTED_MODULE_VERSIONS = {
    "node20": "115",
    "node22": "127",
    "node24": "137",
    "node26": "147",
}
SUPPORTED_LTS_POLICY = "supported_lts_lane_local_evidence"


def display_path(path: Path) -> str:
    try:
        return str(path.relative_to(REPO_ROOT))
    except ValueError:
        return str(path)


def load_lane_manifest(lane_name: str) -> dict[str, Any] | None:
    path = LANE_MANIFEST_ROOT / f"{lane_name}.json"
    if not path.is_file():
        return None
    payload = load_json(path)
    if not isinstance(payload, dict):
        raise AssertionError(f"{display_path(path)} should contain a JSON object")
    return payload


def validate_registry() -> list[str]:
    errors: list[str] = []
    registry = load_json(REGISTRY_PATH)
    if not isinstance(registry, dict):
        return [f"{display_path(REGISTRY_PATH)} should contain a JSON object"]

    for schema_error in validate_payload_against_schema(registry, SCHEMA_PATH):
        errors.append(json.dumps(schema_error, sort_keys=True))

    lanes = registry.get("lanes", [])
    if not isinstance(lanes, list):
        return errors

    lanes_by_name = {
        lane.get("lane_name"): lane
        for lane in lanes
        if isinstance(lane, dict) and isinstance(lane.get("lane_name"), str)
    }
    if len(lanes_by_name) != len(lanes):
        errors.append("lane_name values must be present and unique")

    missing_lanes = sorted(set(EXPECTED_PHASES) - set(lanes_by_name))
    if missing_lanes:
        errors.append(f"missing required lanes: {', '.join(missing_lanes)}")

    default_lane = registry.get("product_default_lane")
    default_lanes = [
        lane["lane_name"]
        for lane in lanes
        if isinstance(lane, dict) and lane.get("product_default") is True
    ]
    if default_lanes != [default_lane]:
        errors.append(
            "product_default_lane must match exactly one lane product_default flag"
        )

    consumer_crates = {
        entry.get("crate")
        for entry in registry.get("consumer_crates", [])
        if isinstance(entry, dict)
    }
    missing_consumers = sorted(REQUIRED_CONSUMER_CRATES - consumer_crates)
    if missing_consumers:
        errors.append(f"missing consumer crates: {', '.join(missing_consumers)}")

    source_urls = registry.get("source_urls", [])
    if isinstance(source_urls, list):
        if not any("abi_version_registry.json" in str(url) for url in source_urls):
            errors.append("registry source_urls must include Node's ABI version registry")

    for lane_name, expected_phase in EXPECTED_PHASES.items():
        lane = lanes_by_name.get(lane_name)
        if not isinstance(lane, dict):
            continue
        if lane.get("support_phase") != expected_phase:
            errors.append(
                f"{lane_name} support_phase should be {expected_phase}, got {lane.get('support_phase')}"
            )
        if lane.get("lane_name") != f"node{lane.get('major')}":
            errors.append(f"{lane_name} major should match lane_name")
        if lane.get("release_name") != "node":
            errors.append(f"{lane_name} release_name should be node")
        if lane.get("node_module_version") != EXPECTED_MODULE_VERSIONS[lane_name]:
            errors.append(
                f"{lane_name} node_module_version should be {EXPECTED_MODULE_VERSIONS[lane_name]}, got {lane.get('node_module_version')}"
            )

        fixture_path = lane.get("fixture_corpus_path")
        fixture_tag = lane.get("fixture_corpus_upstream_tag")
        runtime_target = lane.get("runtime_compatibility_target")
        if lane_name == "node26":
            if runtime_target != "Node26":
                errors.append("node26 Current/non-LTS lane must claim the Node26 compatibility target")
            if lane.get("evidence_policy") != "current_non_lts_lane_local_evidence_until_lts_promotion":
                errors.append("node26 Current/non-LTS lane must use current-line evidence policy")

        if not isinstance(fixture_path, str):
            errors.append(f"{lane_name} fixture_corpus_path must be set")
            continue
        absolute_fixture_path = REPO_ROOT / fixture_path
        if not absolute_fixture_path.is_dir():
            errors.append(f"{lane_name} fixture_corpus_path does not exist: {fixture_path}")

        manifest = load_lane_manifest(lane_name)
        if manifest is None:
            errors.append(f"{lane_name} has no fixture lane manifest")
            continue
        manifest_path = manifest.get("vendored_fixture_root")
        manifest_tag = manifest.get("upstream", {}).get("tag")
        manifest_target = manifest.get("runtime_execution_target")
        if manifest_path != fixture_path:
            errors.append(
                f"{lane_name} fixture_corpus_path does not match fixture manifest: {fixture_path} != {manifest_path}"
            )
        if manifest_tag != fixture_tag:
            errors.append(
                f"{lane_name} fixture_corpus_upstream_tag does not match fixture manifest: {fixture_tag} != {manifest_tag}"
            )
        if manifest_target != runtime_target:
            errors.append(
                f"{lane_name} runtime_compatibility_target does not match fixture manifest: {runtime_target} != {manifest_target}"
            )

        if expected_phase in {"maintenance_lts", "active_lts"}:
            if lane.get("evidence_policy") != SUPPORTED_LTS_POLICY:
                errors.append(f"{lane_name} supported LTS lane must use lane-local evidence")
        if expected_phase == "eol_legacy":
            if lane.get("evidence_policy") != "legacy_grace_regression_only":
                errors.append(f"{lane_name} EOL lane must use legacy-grace evidence policy")

    return errors


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Validate the Nimbus Node LTS lane registry"
    )
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("validate")
    return parser


def main() -> int:
    args = build_parser().parse_args()
    if args.command != "validate":
        raise AssertionError(f"unhandled command {args.command}")

    errors = validate_registry()
    if errors:
        for error in errors:
            print(f"error: {error}")
        return 1

    registry = load_json(REGISTRY_PATH)
    lanes = registry["lanes"]
    consumers = ", ".join(entry["crate"] for entry in registry["consumer_crates"])
    print(
        "validated Node LTS lane registry: "
        f"{len(lanes)} lanes, product default {registry['product_default_lane']}, "
        f"consumers {consumers}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
