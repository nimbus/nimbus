#!/usr/bin/env python3
"""Validate and publish Node release-train drift evidence."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import sys
import urllib.request
from datetime import date
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

from latest_suite_tags import validate_latest_payload  # noqa: E402
from schema import default_schema_path, load_json, validate_payload_against_schema  # noqa: E402


REPO_ROOT = Path(__file__).resolve().parents[3]
LANE_REGISTRY_PATH = (
    REPO_ROOT
    / "docs"
    / "architecture"
    / "runtime"
    / "node-lts-compat"
    / "node-lts-lanes.json"
)
LATEST_TAGS_PATH = (
    REPO_ROOT
    / "docs"
    / "architecture"
    / "runtime"
    / "node-lts-compat"
    / "node-latest-suite-tags.json"
)
STATUS_SUMMARY_PATH = (
    REPO_ROOT
    / "docs"
    / "architecture"
    / "runtime"
    / "node-compat-evidence"
    / "latest"
    / "status-summary.json"
)
DASHBOARD_SUMMARY_PATH = (
    REPO_ROOT
    / "docs"
    / "architecture"
    / "runtime"
    / "node-compat-evidence"
    / "latest"
    / "dashboard-summary.json"
)
SUMMARY_JSON_PATH = (
    REPO_ROOT
    / "docs"
    / "architecture"
    / "runtime"
    / "node-lts-compat"
    / "node-release-train.json"
)
SUMMARY_MD_PATH = SUMMARY_JSON_PATH.with_suffix(".md")
SCHEMA_PATH = default_schema_path("node-release-train.schema.json")
PROOF_PATH = (
    REPO_ROOT
    / "docs"
    / "plans"
    / "proof"
    / "node-faas-runtime-compatibility"
    / "nfrc11-release-train-automation.md"
)
PROOF_README_PATH = PROOF_PATH.parent / "README.md"

DIST_INDEX_URL = "https://nodejs.org/dist/index.json"
SCHEDULE_URL = "https://raw.githubusercontent.com/nodejs/Release/main/schedule.json"

EXPECTED_PHASES = {
    "node20": "eol_legacy",
    "node22": "maintenance_lts",
    "node24": "active_lts",
    "node26": "current_non_lts",
}
EXPECTED_PRODUCT_DEFAULT = "node24"


def display_path(path: Path) -> str:
    try:
        return str(path.relative_to(REPO_ROOT))
    except ValueError:
        return str(path)


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_text(path: Path, lines: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines).rstrip() + "\n", encoding="utf-8")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def source_digests() -> list[dict[str, str]]:
    paths = [
        LANE_REGISTRY_PATH,
        LATEST_TAGS_PATH,
        STATUS_SUMMARY_PATH,
        DASHBOARD_SUMMARY_PATH,
    ]
    return [
        {
            "path": display_path(path),
            "sha256": sha256_file(path),
        }
        for path in paths
    ]


def version_tuple(version: str) -> tuple[int, int, int]:
    cleaned = version.removeprefix("v")
    parts = cleaned.split(".")
    if len(parts) < 3:
        raise ValueError(f"Node version should have major.minor.patch: {version}")
    return int(parts[0]), int(parts[1]), int(parts[2])


def major_from_lane(lane_name: str) -> int:
    return int(lane_name.removeprefix("node"))


def lanes_by_name(payload: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return {
        lane["lane_name"]: lane
        for lane in payload.get("lanes", [])
        if isinstance(lane, dict) and isinstance(lane.get("lane_name"), str)
    }


def latest_lanes_by_name(payload: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return {
        lane["lane"]: lane
        for lane in payload.get("lanes", [])
        if isinstance(lane, dict) and isinstance(lane.get("lane"), str)
    }


def status_lanes_by_name(payload: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return {
        lane["lane"]: lane
        for lane in payload.get("lane_summaries", [])
        if isinstance(lane, dict) and isinstance(lane.get("lane"), str)
    }


def expected_dashboard_role(lane: dict[str, Any]) -> str:
    if lane.get("product_default") is True:
        return "default"
    phase = lane.get("support_phase")
    if phase in {"maintenance_lts", "active_lts"}:
        return "supported"
    if phase == "eol_legacy":
        return "legacy"
    if phase == "current_non_lts":
        return "current"
    return "unknown"


def release_train_role(lane: dict[str, Any]) -> str:
    if lane.get("product_default") is True:
        return "product_default"
    phase = lane.get("support_phase")
    if phase in {"maintenance_lts", "active_lts"}:
        return "supported_lts"
    if phase == "eol_legacy":
        return "legacy_grace"
    if phase == "current_non_lts":
        return "current_non_lts"
    return "unknown"


def latest_dist_versions_by_major(dist_index: list[dict[str, Any]]) -> dict[int, dict[str, Any]]:
    latest: dict[int, dict[str, Any]] = {}
    for entry in dist_index:
        version = entry.get("version")
        if not isinstance(version, str) or not version.startswith("v"):
            continue
        try:
            major, _minor, _patch = version_tuple(version)
        except ValueError:
            continue
        current = latest.get(major)
        if current is None or version_tuple(version) > version_tuple(str(current["version"])):
            latest[major] = entry
    return latest


def parse_date(value: Any) -> date | None:
    if not isinstance(value, str) or not value:
        return None
    return date.fromisoformat(value)


def derive_schedule_phase(entry: dict[str, Any], as_of: date) -> str:
    end_date = parse_date(entry.get("end"))
    lts_start = parse_date(entry.get("lts"))
    maintenance_start = parse_date(entry.get("maintenance"))
    if end_date is not None and as_of > end_date:
        return "eol_legacy"
    if lts_start is not None and as_of < lts_start:
        return "current_non_lts"
    if maintenance_start is not None and as_of >= maintenance_start:
        return "maintenance_lts"
    if lts_start is not None and as_of >= lts_start:
        return "active_lts"
    return "current_non_lts"


def fetch_json(url: str) -> Any:
    request = urllib.request.Request(url, headers={"user-agent": "nimbus-node-release-train"})
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.loads(response.read().decode("utf-8"))


def collect_errors(
    registry: dict[str, Any],
    latest_tags: dict[str, Any],
    status_summary: dict[str, Any],
    dist_index: list[dict[str, Any]] | None,
    schedule: dict[str, Any] | None,
    check_proof: bool,
) -> dict[str, list[dict[str, str]]]:
    errors: dict[str, list[dict[str, str]]] = {
        "metadata": [],
        "tag_drift": [],
        "lifecycle_drift": [],
        "role_drift": [],
        "proof_drift": [],
    }

    for schema_error in validate_latest_payload(latest_tags):
        errors["metadata"].append(
            {"path": display_path(LATEST_TAGS_PATH), "message": json.dumps(schema_error)}
        )

    registry_lanes = lanes_by_name(registry)
    latest_lanes = latest_lanes_by_name(latest_tags)
    status_lanes = status_lanes_by_name(status_summary)

    if registry.get("product_default_lane") != EXPECTED_PRODUCT_DEFAULT:
        errors["role_drift"].append(
            {
                "lane": str(registry.get("product_default_lane")),
                "message": f"product_default_lane must remain {EXPECTED_PRODUCT_DEFAULT}",
            }
        )

    for lane_name, expected_phase in EXPECTED_PHASES.items():
        lane = registry_lanes.get(lane_name)
        latest_lane = latest_lanes.get(lane_name)
        status_lane = status_lanes.get(lane_name)
        if lane is None:
            errors["metadata"].append({"lane": lane_name, "message": "missing lane registry entry"})
            continue
        if latest_lane is None:
            errors["metadata"].append({"lane": lane_name, "message": "missing latest-suite entry"})
            continue
        if lane.get("support_phase") != expected_phase:
            errors["lifecycle_drift"].append(
                {
                    "lane": lane_name,
                    "message": f"support_phase {lane.get('support_phase')} != expected {expected_phase}",
                }
            )
        if latest_lane.get("support_phase") != lane.get("support_phase"):
            errors["lifecycle_drift"].append(
                {
                    "lane": lane_name,
                    "message": "latest-suite support_phase does not match lane registry",
                }
            )
        if latest_lane.get("latest_official_tag") != lane.get("upstream_tag"):
            errors["tag_drift"].append(
                {
                    "lane": lane_name,
                    "message": "latest official tag does not match lane registry upstream tag",
                }
            )
        expected_role = expected_dashboard_role(lane)
        actual_role = status_lane.get("lane_role") if status_lane else None
        if actual_role != expected_role:
            errors["role_drift"].append(
                {
                    "lane": lane_name,
                    "message": f"dashboard lane_role {actual_role} != expected {expected_role}",
                }
            )

    if dist_index is not None:
        latest_by_major = latest_dist_versions_by_major(dist_index)
        for lane_name, lane in sorted(registry_lanes.items()):
            major = major_from_lane(lane_name)
            dist_entry = latest_by_major.get(major)
            if dist_entry is None:
                errors["tag_drift"].append(
                    {"lane": lane_name, "message": f"dist index has no latest v{major}.x entry"}
                )
                continue
            if dist_entry.get("version") != lane.get("upstream_tag"):
                errors["tag_drift"].append(
                    {
                        "lane": lane_name,
                        "message": (
                            f"dist index latest {dist_entry.get('version')} "
                            f"!= registry {lane.get('upstream_tag')}"
                        ),
                    }
                )

    if schedule is not None:
        as_of = date.fromisoformat(str(registry.get("as_of")))
        for lane_name, lane in sorted(registry_lanes.items()):
            release_key = f"v{major_from_lane(lane_name)}"
            release = schedule.get(release_key)
            if not isinstance(release, dict):
                errors["lifecycle_drift"].append(
                    {"lane": lane_name, "message": f"schedule has no {release_key} entry"}
                )
                continue
            schedule_phase = derive_schedule_phase(release, as_of)
            if schedule_phase != lane.get("support_phase"):
                errors["lifecycle_drift"].append(
                    {
                        "lane": lane_name,
                        "message": (
                            f"schedule phase {schedule_phase} as of {as_of.isoformat()} "
                            f"!= registry {lane.get('support_phase')}"
                        ),
                    }
                )
            schedule_lts = parse_date(release.get("lts"))
            schedule_maintenance = parse_date(release.get("maintenance"))
            schedule_end = parse_date(release.get("end"))
            for key, actual in (
                ("lts_start", schedule_lts),
                ("maintenance_start", schedule_maintenance),
                ("eol_date", schedule_end),
            ):
                if actual is not None and lane.get(key) != actual.isoformat():
                    errors["lifecycle_drift"].append(
                        {
                            "lane": lane_name,
                            "message": f"{key} {lane.get(key)} != schedule {actual.isoformat()}",
                        }
                    )

    if check_proof:
        required_markers = digest_markers()
        proof_text = PROOF_PATH.read_text(encoding="utf-8") if PROOF_PATH.exists() else ""
        proof_readme = (
            PROOF_README_PATH.read_text(encoding="utf-8") if PROOF_README_PATH.exists() else ""
        )
        if not PROOF_PATH.is_file():
            errors["proof_drift"].append(
                {"path": display_path(PROOF_PATH), "message": "required proof file is missing"}
            )
        if PROOF_PATH.name not in proof_readme:
            errors["proof_drift"].append(
                {
                    "path": display_path(PROOF_README_PATH),
                    "message": "proof README does not list the release-train proof",
                }
            )
        for marker in required_markers:
            if marker not in proof_text:
                errors["proof_drift"].append(
                    {
                        "path": display_path(PROOF_PATH),
                        "message": f"proof is missing digest marker `{marker}`",
                    }
                )

    return errors


def has_errors(errors: dict[str, list[dict[str, str]]]) -> bool:
    return any(errors.values())


def digest_markers() -> list[str]:
    return [
        f"{entry['path']} sha256: {entry['sha256']}"
        for entry in source_digests()
        if entry["path"] in {display_path(LANE_REGISTRY_PATH), display_path(LATEST_TAGS_PATH)}
    ]


def proof_gate() -> dict[str, Any]:
    required_markers = digest_markers()
    proof_text = PROOF_PATH.read_text(encoding="utf-8") if PROOF_PATH.exists() else ""
    proof_readme = PROOF_README_PATH.read_text(encoding="utf-8") if PROOF_README_PATH.exists() else ""
    missing_markers = [marker for marker in required_markers if marker not in proof_text]
    return {
        "proof_artifact": display_path(PROOF_PATH),
        "proof_readme": display_path(PROOF_README_PATH),
        "proof_file_present": PROOF_PATH.is_file(),
        "proof_readme_lists_artifact": PROOF_PATH.name in proof_readme,
        "required_digest_markers": required_markers,
        "missing_digest_markers": missing_markers,
    }


def lane_contracts(
    registry: dict[str, Any],
    latest_tags: dict[str, Any],
    status_summary: dict[str, Any],
) -> list[dict[str, Any]]:
    latest_lanes = latest_lanes_by_name(latest_tags)
    status_lanes = status_lanes_by_name(status_summary)
    contracts: list[dict[str, Any]] = []
    for lane_name, lane in sorted(lanes_by_name(registry).items()):
        latest_lane = latest_lanes.get(lane_name, {})
        status_lane = status_lanes.get(lane_name, {})
        contracts.append(
            {
                "lane": lane_name,
                "major": lane.get("major"),
                "release_train_role": release_train_role(lane),
                "support_phase": lane.get("support_phase"),
                "product_default": lane.get("product_default") is True,
                "upstream_tag": lane.get("upstream_tag"),
                "latest_official_tag": latest_lane.get("latest_official_tag"),
                "fixture_corpus_current_tag": latest_lane.get("fixture_corpus_current_tag"),
                "fixture_sync_required": latest_lane.get("fixture_sync_required"),
                "dashboard_lane_role": status_lane.get("lane_role"),
                "runtime_compatibility_target": lane.get("runtime_compatibility_target"),
                "evidence_policy": lane.get("evidence_policy"),
            }
        )
    return contracts


def build_summary(
    *,
    dist_index: list[dict[str, Any]] | None = None,
    schedule: dict[str, Any] | None = None,
    check_proof: bool = True,
) -> dict[str, Any]:
    registry = load_json(LANE_REGISTRY_PATH)
    latest_tags = load_json(LATEST_TAGS_PATH)
    status_summary = load_json(STATUS_SUMMARY_PATH)
    dashboard_summary = load_json(DASHBOARD_SUMMARY_PATH)
    errors = collect_errors(
        registry,
        latest_tags,
        status_summary,
        dist_index,
        schedule,
        check_proof=check_proof,
    )
    return {
        "schema_version": 1,
        "report_kind": "nimbus_node_release_train",
        "as_of": registry.get("as_of"),
        "generated_from": [
            display_path(LANE_REGISTRY_PATH),
            display_path(LATEST_TAGS_PATH),
            display_path(STATUS_SUMMARY_PATH),
            display_path(DASHBOARD_SUMMARY_PATH),
        ],
        "source_urls": [
            DIST_INDEX_URL,
            SCHEDULE_URL,
        ],
        "source_digests": source_digests(),
        "lane_contracts": lane_contracts(registry, latest_tags, status_summary),
        "dashboard_summary": {
            "canary_claim_count": dashboard_summary.get("canary_claim_count", 0),
            "canary_check_count": dashboard_summary.get("canary_check_count", 0),
            "required_canary_gap_count": len(dashboard_summary.get("required_canary_gaps", [])),
        },
        "live_probe": {
            "dist_index_checked": dist_index is not None,
            "schedule_checked": schedule is not None,
        },
        "proof_gate": proof_gate(),
        "drift": errors,
        "drift_detected": has_errors(errors),
    }


def validate_summary_schema(summary: dict[str, Any]) -> list[str]:
    return [
        json.dumps(error, sort_keys=True)
        for error in validate_payload_against_schema(summary, SCHEMA_PATH)
    ]


def render_markdown(summary: dict[str, Any]) -> list[str]:
    lines = [
        "# Node Release Train",
        "",
        "<!-- generated by scripts/runtime/node/release_train.py; do not edit by hand -->",
        "",
        f"As of: `{summary['as_of']}`",
        "",
        "This generated report validates Node release-train metadata, fixture tag",
        "alignment, dashboard role separation, and the proof gate for release",
        "metadata changes.",
        "",
        "## Lane Contracts",
        "",
        "| Lane | Role | Phase | Product default | Latest official tag | Fixture tag | Sync required | Dashboard role |",
        "| --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for lane in summary["lane_contracts"]:
        lines.append(
            f"| `{lane['lane']}` | `{lane['release_train_role']}` | "
            f"`{lane['support_phase']}` | `{'yes' if lane['product_default'] else 'no'}` | "
            f"`{lane['latest_official_tag']}` | `{lane['fixture_corpus_current_tag']}` | "
            f"`{'yes' if lane['fixture_sync_required'] else 'no'}` | "
            f"`{lane['dashboard_lane_role']}` |"
        )
    lines.extend(
        [
            "",
            "## Dashboard Separation",
            "",
            f"- canary claims: `{summary['dashboard_summary']['canary_claim_count']}`",
            f"- canary checks: `{summary['dashboard_summary']['canary_check_count']}`",
            f"- required canary gaps: `{summary['dashboard_summary']['required_canary_gap_count']}`",
            "",
            "Node24 remains the product default, Node22 remains supported",
            "Maintenance LTS, Node20 remains legacy-grace regression coverage,",
            "and Node26 remains Current/non-LTS until LTS promotion gates pass.",
            "",
            "## Source Digests",
            "",
        ]
    )
    for source in summary["source_digests"]:
        lines.append(f"- `{source['path']}` sha256: `{source['sha256']}`")
    proof_gate_data = summary["proof_gate"]
    lines.extend(
        [
            "",
            "## Proof Gate",
            "",
            f"- proof artifact: `{proof_gate_data['proof_artifact']}`",
            f"- proof file present: `{'yes' if proof_gate_data['proof_file_present'] else 'no'}`",
            f"- proof README lists artifact: `{'yes' if proof_gate_data['proof_readme_lists_artifact'] else 'no'}`",
            "- required digest markers:",
        ]
    )
    for marker in proof_gate_data["required_digest_markers"]:
        lines.append(f"  - `{marker}`")
    if proof_gate_data["missing_digest_markers"]:
        lines.append("- missing digest markers:")
        for marker in proof_gate_data["missing_digest_markers"]:
            lines.append(f"  - `{marker}`")
    else:
        lines.append("- missing digest markers: `none`")
    lines.extend(
        [
            "",
            "## Drift",
            "",
        ]
    )
    if not summary["drift_detected"]:
        lines.append("No release-train drift detected.")
    else:
        for kind, entries in summary["drift"].items():
            if not entries:
                continue
            lines.extend([f"### {kind.replace('_', ' ').title()}", ""])
            for entry in entries:
                owner = entry.get("lane") or entry.get("path") or "metadata"
                lines.append(f"- `{owner}`: {entry['message']}")
            lines.append("")
    return lines


def command_publish(args: argparse.Namespace) -> int:
    summary = build_summary(check_proof=args.check_proof)
    schema_errors = validate_summary_schema(summary)
    if schema_errors:
        for error in schema_errors:
            print(f"error: {error}")
        return 1
    write_json(SUMMARY_JSON_PATH, summary)
    write_text(SUMMARY_MD_PATH, render_markdown(summary))
    print(f"published Node release-train summary to {display_path(SUMMARY_JSON_PATH)}")
    if summary["drift_detected"]:
        print("warning: release-train drift detected; see generated summary")
    return 0


def command_check(_: argparse.Namespace) -> int:
    summary = build_summary(check_proof=True)
    schema_errors = validate_summary_schema(summary)
    errors: list[str] = [f"schema: {error}" for error in schema_errors]
    expected_json = json.dumps(summary, indent=2, sort_keys=True) + "\n"
    expected_md = "\n".join(render_markdown(summary)).rstrip() + "\n"
    actual_json = SUMMARY_JSON_PATH.read_text(encoding="utf-8") if SUMMARY_JSON_PATH.exists() else ""
    actual_md = SUMMARY_MD_PATH.read_text(encoding="utf-8") if SUMMARY_MD_PATH.exists() else ""
    if actual_json != expected_json:
        errors.append(f"stale Node release-train JSON: {display_path(SUMMARY_JSON_PATH)}")
    if actual_md != expected_md:
        errors.append(f"stale Node release-train docs: {display_path(SUMMARY_MD_PATH)}")
    for kind, entries in summary["drift"].items():
        for entry in entries:
            owner = entry.get("lane") or entry.get("path") or kind
            errors.append(f"{kind}: {owner}: {entry['message']}")
    if errors:
        for error in errors:
            print(f"error: {error}")
        return 1
    print(
        "Node release-train summary is current: "
        f"{len(summary['lane_contracts'])} lanes, 0 drift entries"
    )
    return 0


def command_probe_live(args: argparse.Namespace) -> int:
    dist_index = fetch_json(DIST_INDEX_URL)
    schedule = fetch_json(SCHEDULE_URL)
    if not isinstance(dist_index, list):
        print("error: Node dist index did not return a JSON array")
        return 1
    if not isinstance(schedule, dict):
        print("error: Node release schedule did not return a JSON object")
        return 1
    summary = build_summary(dist_index=dist_index, schedule=schedule, check_proof=args.check_proof)
    output_root = args.output_root
    write_json(output_root / "node-release-train-live.json", summary)
    write_text(output_root / "node-release-train-live.md", render_markdown(summary))
    if summary["drift_detected"]:
        for kind, entries in summary["drift"].items():
            for entry in entries:
                owner = entry.get("lane") or entry.get("path") or kind
                print(f"error: {kind}: {owner}: {entry['message']}")
        return 1
    print(
        "live Node release-train probe passed: "
        f"{len(summary['lane_contracts'])} lanes matched official release feeds"
    )
    return 0


def command_self_test(_: argparse.Namespace) -> int:
    registry = load_json(LANE_REGISTRY_PATH)
    latest_tags = load_json(LATEST_TAGS_PATH)
    status_summary = load_json(STATUS_SUMMARY_PATH)
    errors: list[str] = []

    def expect_drift(name: str, mutator, expected_kind: str) -> None:
        test_registry = copy.deepcopy(registry)
        test_latest = copy.deepcopy(latest_tags)
        test_status = copy.deepcopy(status_summary)
        dist_index: list[dict[str, Any]] | None = None
        schedule: dict[str, Any] | None = None
        result = mutator(test_registry, test_latest, test_status)
        if isinstance(result, tuple):
            dist_index, schedule = result
        drift = collect_errors(
            test_registry,
            test_latest,
            test_status,
            dist_index,
            schedule,
            check_proof=False,
        )
        if not drift[expected_kind]:
            errors.append(f"negative self-test failed: {name} produced no {expected_kind}")

    def tag_drift(_registry: dict[str, Any], _latest: dict[str, Any], _status: dict[str, Any]):
        return ([{"version": "v24.99.0"}], None)

    def lifecycle_drift(
        _registry: dict[str, Any],
        _latest: dict[str, Any],
        _status: dict[str, Any],
    ):
        schedule = {
            "v20": {"lts": "2023-10-24", "maintenance": "2024-10-22", "end": "2026-04-30"},
            "v22": {"lts": "2024-10-29", "maintenance": "2025-10-21", "end": "2027-04-30"},
            "v24": {"lts": "2025-10-28", "maintenance": "2026-10-20", "end": "2028-04-30"},
            "v26": {"lts": "2026-01-01", "maintenance": "2027-10-27", "end": "2029-04-30"},
        }
        return (None, schedule)

    def role_drift(_registry: dict[str, Any], _latest: dict[str, Any], status: dict[str, Any]):
        status_lanes_by_name(status)["node26"]["lane_role"] = "supported"

    def default_drift(registry_payload: dict[str, Any], _latest: dict[str, Any], _status: dict[str, Any]):
        registry_payload["product_default_lane"] = "node22"

    expect_drift("tag_drift", tag_drift, "tag_drift")
    expect_drift("lifecycle_drift", lifecycle_drift, "lifecycle_drift")
    expect_drift("role_drift", role_drift, "role_drift")
    expect_drift("default_drift", default_drift, "role_drift")

    if errors:
        for error in errors:
            print(f"error: {error}")
        return 1
    print("Node release-train negative self-tests passed")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Validate Node release-train automation")
    subparsers = parser.add_subparsers(dest="command", required=True)
    publish = subparsers.add_parser("publish")
    publish.add_argument(
        "--check-proof",
        action="store_true",
        help="include proof-file presence and digest-marker checks in generated output",
    )
    publish.set_defaults(func=command_publish)
    subparsers.add_parser("check").set_defaults(func=command_check)
    subparsers.add_parser("self-test").set_defaults(func=command_self_test)
    probe = subparsers.add_parser("probe-live")
    probe.add_argument("--output-root", type=Path, default=REPO_ROOT / "target" / "node-compat" / "release-train")
    probe.add_argument(
        "--check-proof",
        action="store_true",
        help="include proof-file presence and digest-marker checks in live probe output",
    )
    probe.set_defaults(func=command_probe_live)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
