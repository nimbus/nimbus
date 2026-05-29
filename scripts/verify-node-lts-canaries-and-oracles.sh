#!/usr/bin/env bash
# Verifies active-LTS package canaries and oracle evidence are lane-local.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

python3 - <<'PY'
import json
import sys
from pathlib import Path

repo = Path.cwd()
lane_registry_path = repo / "docs/architecture/runtime/node-lts-compat/node-lts-lanes.json"
canary_registry_path = repo / "tests/runtime/node/canary-registry.json"
dashboard_path = repo / "docs/architecture/runtime/node-compat-evidence/latest/dashboard-summary.json"

required_surfaces = {
    "esm_cjs_loading",
    "process_metadata",
    "fs_path",
    "streams",
    "timers",
    "crypto",
    "fetch_http",
    "convex_use_node_action_packaging",
    "convex_use_node_action_invocation",
    "convex_use_node_package_import",
    "convex_use_node_ctx_run_query",
    "convex_use_node_ctx_run_mutation",
    "convex_use_node_ctx_run_action",
    "convex_use_node_scheduler",
    "convex_use_node_value_serialization",
    "convex_use_node_fetch_env_secret_crypto_stream_path_fs_temp",
    "convex_use_node_dangling_promise_diagnostic",
    "child_process_denial",
    "worker_threads_denial",
    "inspector_denial",
    "repl_denial",
    "node_test_runner_denial",
    "native_addon_denial",
    "persistent_filesystem_denial",
    "raw_server_listen_denial",
    "prisma_engine_service_route",
    "sharp_native_service_route",
    "esbuild_binary_service_route",
}
required_packages = {
    ("node-platform-builtins", "Application"),
    ("express", "Application"),
    ("fastify", "Application"),
    ("socket.io", "Application"),
    ("undici", "Application"),
    ("axios", "Application"),
    ("convex-use-node-action", "Application"),
    ("convex-use-node-real-app", "Application"),
    ("openai", "Application"),
    ("@anthropic-ai/sdk", "Application"),
    ("ai", "Application"),
    ("stripe", "Application"),
    ("resend", "Application"),
    ("@aws-sdk/client-s3", "Application"),
    ("@slack/web-api", "Application"),
    ("octokit", "Application"),
    ("jose", "Application"),
    ("zod", "Application"),
    ("uuid", "Application"),
    ("nanoid", "Application"),
    ("@upstash/redis", "Application"),
    ("tsx", "Tooling"),
    ("ts-node", "Tooling"),
    ("jest", "Tooling"),
    ("prisma", "Tooling"),
    ("next", "Tooling"),
    ("node:child_process", "Application"),
    ("node:worker_threads", "Application"),
    ("node:inspector", "Application"),
    ("node:repl", "Application"),
    ("node --test", "Application"),
    ("native-addon", "Application"),
    ("persistent-filesystem", "Application"),
    ("raw-server-listen", "Application"),
    ("prisma", "Application"),
    ("sharp", "Application"),
    ("esbuild", "Application"),
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


print("Node LTS canary/oracle verifier")
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
if supported_lanes == ["node22", "node24"]:
    note_pass("Supported active-LTS lane set is node22 and node24")
else:
    note_fail(f"Unexpected supported active-LTS lanes: {supported_lanes}")

claims = canary_registry["claims"]
claim_by_id = {claim["id"]: claim for claim in claims}
claim_surfaces = {
    surface
    for claim in claims
    for surface in claim.get("canary_surfaces", [])
}
missing_surfaces = sorted(required_surfaces - claim_surfaces)
if not missing_surfaces:
    note_pass("Registered canary claims cover all required NLRT10 surfaces")
else:
    note_fail(f"Missing required canary surfaces: {missing_surfaces}")

claim_package_pairs = {
    (claim["package"], claim["runtime_preset"]) for claim in claims
}
missing_packages = sorted(required_packages - claim_package_pairs)
if not missing_packages:
    note_pass("Registered canary claims cover all required packages/frameworks")
else:
    note_fail(f"Missing required package claims: {missing_packages}")

bad_claim_lanes = []
for claim in claims:
    lanes = set(claim["lane_coverage"])
    if lanes != supported_lane_set:
        bad_claim_lanes.append((claim["id"], sorted(lanes)))
if not bad_claim_lanes:
    note_pass("Every public canary claim is scoped exactly to supported LTS lanes")
else:
    note_fail(f"Canary claims with stale or missing lane coverage: {bad_claim_lanes}")

active_canaries = [
    canary for canary in canary_registry["canaries"] if canary.get("status") == "active"
]
bad_canary_lanes = []
borrowed_tests = []
claim_lane_runs: dict[tuple[str, str], int] = {}
for canary in active_canaries:
    runs_by_lane = {run["lane"]: run for run in canary["lane_runs"]}
    missing = sorted(supported_lane_set - set(runs_by_lane))
    if missing:
        bad_canary_lanes.append((canary["id"], missing))
    for lane in supported_lanes:
        run = runs_by_lane.get(lane)
        if not run:
            continue
        test_name = run["cargo_test"]
        if lane not in test_name:
            borrowed_tests.append((canary["id"], lane, test_name))
        for claim_id in canary["claim_ids"]:
            claim_lane_runs[(claim_id, lane)] = claim_lane_runs.get((claim_id, lane), 0) + 1
if not bad_canary_lanes:
    note_pass("Every active canary has lane-local runs for each supported LTS lane")
else:
    note_fail(f"Active canaries missing supported lane runs: {bad_canary_lanes}")
if not borrowed_tests:
    note_pass("Supported lane canary runs do not borrow another lane's cargo test")
else:
    note_fail(f"Canary runs borrowing default-lane tests: {borrowed_tests}")

missing_claim_runs = []
for claim_id in claim_by_id:
    for lane in supported_lanes:
        if claim_lane_runs.get((claim_id, lane), 0) == 0:
            missing_claim_runs.append((claim_id, lane))
if not missing_claim_runs:
    note_pass("Every claim has at least one observed canary run per supported LTS lane")
else:
    note_fail(f"Claims missing lane-local canary runs: {missing_claim_runs}")

dashboard_claims = dashboard.get("claim_summaries", [])
bad_dashboard_claims = [
    (claim["id"], claim["status"], claim.get("missing_lanes", []))
    for claim in dashboard_claims
    if claim["status"] != "passed" or claim.get("missing_lanes")
]
if not bad_dashboard_claims:
    note_pass("Published dashboard reports all canary claims as passed")
else:
    note_fail(f"Published dashboard has failed or missing canary claims: {bad_dashboard_claims}")

dashboard_results = [
    result
    for report in dashboard.get("canary_reports", [])
    for result in report.get("canary_results", [])
]
missing_dashboard_results = []
for claim_id in claim_by_id:
    for lane in supported_lanes:
        if not any(
            lane == result.get("lane")
            and claim_id in result.get("claim_ids", [])
            and result.get("status") == "pass"
            for result in dashboard_results
        ):
            missing_dashboard_results.append((claim_id, lane))
if not missing_dashboard_results:
    note_pass("Published dashboard includes passed canary result for every claim/lane")
else:
    note_fail(f"Published dashboard missing passed canary results: {missing_dashboard_results}")

oracle_reports = dashboard.get("oracle_reports", [])
oracle_by_lane = {report["lane"]: report for report in oracle_reports}
missing_oracle_lanes = sorted(supported_lane_set - set(oracle_by_lane))
if not missing_oracle_lanes:
    note_pass("Published dashboard includes oracle reports for every supported LTS lane")
else:
    note_fail(f"Missing oracle report lanes: {missing_oracle_lanes}")

bad_oracle_versions = []
for lane in supported_lanes:
    report = oracle_by_lane.get(lane)
    if not report:
        continue
    expected_major = lane.removeprefix("node")
    version = report.get("node_version", "")
    if not version.startswith(f"v{expected_major}."):
        bad_oracle_versions.append((lane, version))
if not bad_oracle_versions:
    note_pass("Oracle reports use version-matched Node major binaries")
else:
    note_fail(f"Oracle reports with mismatched Node versions: {bad_oracle_versions}")

if dashboard.get("required_canary_gaps"):
    note_fail(f"Published dashboard reports canary gaps: {dashboard['required_canary_gaps']}")
else:
    note_pass("Published dashboard reports no required canary gaps")

print(f"Summary: {pass_count} passed, {len(failures)} failed")
if failures:
    for failure in failures:
        print(f"  - {failure}")
    sys.exit(1)
PY
