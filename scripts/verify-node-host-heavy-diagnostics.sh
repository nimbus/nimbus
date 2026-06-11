#!/usr/bin/env bash
# Verifies host-heavy Node diagnostic canaries are registered and published.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

python3 - <<'PY'
import json
import sys
from pathlib import Path

repo = Path.cwd()
lane_registry_path = repo / "tests/runtime/node/compat/node-lts-compat/node-lts-lanes.json"
canary_registry_path = repo / "tests/runtime/node/canary-registry.json"
dashboard_path = repo / "tests/runtime/node/compat/node-compat-evidence/latest/dashboard-summary.json"

required_surfaces = {
    "child_process_denial": "node:child_process",
    "worker_threads_denial": "node:worker_threads",
    "inspector_denial": "node:inspector",
    "repl_denial": "node:repl",
    "node_test_runner_denial": "node --test",
    "native_addon_denial": "native-addon",
    "persistent_filesystem_denial": "persistent-filesystem",
    "raw_server_listen_denial": "raw-server-listen",
    "prisma_engine_service_route": "prisma",
    "sharp_native_service_route": "sharp",
    "esbuild_binary_service_route": "esbuild",
}

pass_count = 0
failures: list[str] = []


def note_pass(message: str) -> None:
    global pass_count
    pass_count += 1
    print(f"  PASS  {message}")


def note_fail(message: str) -> None:
    failures.append(message)
    print(f"  FAIL  {message}")


def load_json(path: Path) -> dict:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


print("Node host-heavy diagnostic verifier")
lane_registry = load_json(lane_registry_path)
canary_registry = load_json(canary_registry_path)
dashboard = load_json(dashboard_path)

supported_lanes = [
    lane["lane_name"]
    for lane in lane_registry["lanes"]
    if lane["support_phase"] in {"maintenance_lts", "active_lts"}
    and lane["evidence_policy"] == "supported_lts_lane_local_evidence"
]
supported_lane_set = set(supported_lanes)
current_lanes = {
    lane["lane_name"]
    for lane in lane_registry["lanes"]
    if lane["support_phase"] == "current_non_lts"
}
if supported_lanes == ["node22", "node24"] and current_lanes == {"node26"}:
    note_pass("Supported lanes are Node22/Node24 and Current lane is Node26")
else:
    note_fail(
        f"Unexpected lane roles: supported={supported_lanes}, current={sorted(current_lanes)}"
    )

claims = canary_registry["claims"]
claims_by_surface: dict[str, list[dict]] = {}
for claim in claims:
    for surface in claim.get("canary_surfaces", []):
        claims_by_surface.setdefault(surface, []).append(claim)

missing_surfaces = sorted(set(required_surfaces) - set(claims_by_surface))
if not missing_surfaces:
    note_pass("Registry has every required host-heavy diagnostic surface")
else:
    note_fail(f"Missing host-heavy diagnostic surfaces: {missing_surfaces}")

bad_claims = []
required_claim_ids: set[str] = set()
for surface, package in required_surfaces.items():
    for claim in claims_by_surface.get(surface, []):
        required_claim_ids.add(claim["id"])
        if claim.get("package") != package:
            bad_claims.append((claim["id"], "package", claim.get("package"), package))
        if claim.get("runtime_preset") != "Application":
            bad_claims.append((claim["id"], "runtime_preset", claim.get("runtime_preset")))
        if claim.get("evidence_kind") != "diagnostic":
            bad_claims.append((claim["id"], "evidence_kind", claim.get("evidence_kind")))
        if claim.get("support_status") != "service_microvm_required":
            bad_claims.append((claim["id"], "support_status", claim.get("support_status")))
        if set(claim.get("lane_coverage", [])) != supported_lane_set:
            bad_claims.append((claim["id"], "lane_coverage", claim.get("lane_coverage")))
if not bad_claims:
    note_pass("Host-heavy claims are diagnostic, service/microVM-required, and LTS-scoped")
else:
    note_fail(f"Host-heavy claim metadata problems: {bad_claims}")

active_canaries = [
    canary for canary in canary_registry["canaries"] if canary.get("status") == "active"
]
canaries_by_claim: dict[str, list[dict]] = {}
for canary in active_canaries:
    for claim_id in canary.get("claim_ids", []):
        canaries_by_claim.setdefault(claim_id, []).append(canary)

missing_canaries = sorted(required_claim_ids - set(canaries_by_claim))
bad_canaries = []
for claim_id in sorted(required_claim_ids):
    for canary in canaries_by_claim.get(claim_id, []):
        lanes = {run["lane"] for run in canary.get("lane_runs", [])}
        if not supported_lane_set.issubset(lanes):
            bad_canaries.append((canary["id"], "missing_supported_lanes", sorted(supported_lane_set - lanes)))
        if "node26" not in lanes:
            bad_canaries.append((canary["id"], "missing_node26_current_lane"))
        if canary.get("evidence_kind") != "diagnostic":
            bad_canaries.append((canary["id"], "evidence_kind", canary.get("evidence_kind")))
        if canary.get("support_status") != "service_microvm_required":
            bad_canaries.append((canary["id"], "support_status", canary.get("support_status")))
if not missing_canaries and not bad_canaries:
    note_pass("Active diagnostic canaries cover supported LTS lanes plus Node26 Current")
else:
    if missing_canaries:
        note_fail(f"Missing active host-heavy canaries for claims: {missing_canaries}")
    if bad_canaries:
        note_fail(f"Host-heavy canary metadata problems: {bad_canaries}")

dashboard_claims = {
    claim["id"]: claim for claim in dashboard.get("claim_summaries", [])
}
bad_dashboard_claims = []
for claim_id in sorted(required_claim_ids):
    claim = dashboard_claims.get(claim_id)
    if not claim:
        bad_dashboard_claims.append((claim_id, "missing_dashboard_claim"))
        continue
    if claim.get("status") != "passed":
        bad_dashboard_claims.append((claim_id, "status", claim.get("status")))
    if claim.get("evidence_kind") != "diagnostic":
        bad_dashboard_claims.append((claim_id, "evidence_kind", claim.get("evidence_kind")))
    if claim.get("support_status") != "service_microvm_required":
        bad_dashboard_claims.append((claim_id, "support_status", claim.get("support_status")))
    if claim.get("missing_lanes"):
        bad_dashboard_claims.append((claim_id, "missing_lanes", claim.get("missing_lanes")))
if not bad_dashboard_claims:
    note_pass("Published dashboard reports host-heavy diagnostic claims as passed")
else:
    note_fail(f"Host-heavy dashboard claim problems: {bad_dashboard_claims}")

dashboard_results = [
    result
    for report in dashboard.get("canary_reports", [])
    for result in report.get("canary_results", [])
]
missing_results = []
for claim_id in sorted(required_claim_ids):
    for lane in [*supported_lanes, "node26"]:
        if not any(
            result.get("lane") == lane
            and claim_id in result.get("claim_ids", [])
            and result.get("status") == "pass"
            and result.get("evidence_kind") == "diagnostic"
            and result.get("support_status") == "service_microvm_required"
            for result in dashboard_results
        ):
            missing_results.append((claim_id, lane))
if not missing_results:
    note_pass("Published dashboard includes passed diagnostic result for each required lane")
else:
    note_fail(f"Missing published host-heavy diagnostic results: {missing_results}")

if dashboard.get("required_canary_gaps"):
    note_fail(f"Published dashboard reports canary gaps: {dashboard['required_canary_gaps']}")
else:
    note_pass("Published dashboard reports no canary gaps")

print(f"Summary: {pass_count} passed, {len(failures)} failed")
if failures:
    for failure in failures:
        print(f"  - {failure}")
    sys.exit(1)
PY
