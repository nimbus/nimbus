#!/usr/bin/env bash
# Verifies the completed Node LTS Runtime Trust (NLRT) control plane.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}" || exit 1

PASS=0
FAIL=0
FAIL_DETAIL=()

pass() {
  PASS=$((PASS + 1))
  printf '  \033[32mPASS\033[0m  %s\n' "$1"
}

fail() {
  FAIL=$((FAIL + 1))
  printf '  \033[31mFAIL\033[0m  %s\n' "$1"
  if [ $# -ge 2 ]; then
    printf '        %s\n' "$2"
    FAIL_DETAIL+=("$1 - $2")
  else
    FAIL_DETAIL+=("$1")
  fi
}

step() {
  printf '\n\033[1m[%s]\033[0m %s\n' "$1" "$2"
}

run_check() {
  local label="$1"
  shift

  step "${label}" "$*"
  if "$@"; then
    pass "${label}"
  else
    fail "${label}" "Command failed: $*"
  fi
}

python_check() {
  local label="$1"
  shift

  step "${label}" "python"
  if python3 - "$@"; then
    pass "${label}"
  else
    fail "${label}" "Python verifier failed"
  fi
}

printf '\033[1mNode LTS runtime trust verifier\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

python_check "Plan, archive, proof, registry, docs, and diagnostics invariants" <<'PY'
import json
import re
import sys
from pathlib import Path

repo = Path.cwd()
errors: list[str] = []

active_plan = repo / "docs/private/plans/node-lts-runtime-trust-plan.md"
archived_plan = repo / "docs/private/plans/archive/node-lts-runtime-trust-plan.md"
plan_readme = repo / "docs/private/plans/README.md"
proof_root = repo / "docs/private/plans/proof/node-lts-runtime-trust"
proof_readme = proof_root / "README.md"
research = repo / "docs/private/plans/research/node-lts-runtime-and-deno-fork-strategy.md"
lane_registry = repo / "docs/private/architecture/runtime/node-lts-compat/node-lts-lanes.json"
compat_doc = repo / "docs/private/staging/runtimes/nodejs/compatibility.md"
node_readme = repo / "docs/private/staging/runtimes/nodejs/README.md"
evidence_latest = repo / "docs/private/staging/runtimes/nodejs/evidence/latest.md"
supplementary_failures = repo / "docs/private/architecture/runtime/node-compat-supplementary-failures.md"
harness_doc = repo / "docs/private/architecture/runtime/node-lts-compat/harness-timeouts-and-hangs.md"
permission_doc = repo / "docs/private/architecture/runtime/permission-model.md"
dashboard = repo / "docs/private/architecture/runtime/node-compat-evidence/latest/dashboard-summary.json"
process_shape_tests = repo / "crates/nimbus-runtime/src/runtime/tests/node/cases/watchpoints_extended.rs"

expected_proofs = {
    0: "nlrt0-baseline-and-control-plane.md",
    1: "nlrt1-deno-fork-provenance.md",
    2: "nlrt2-node-lts-lane-registry.md",
    3: "nlrt3-runtime-target-metadata.md",
    4: "nlrt4-truthful-process-metadata.md",
    5: "nlrt5-equal-lane-evidence-docs.md",
    6: "nlrt6-fixture-provenance-sync.md",
    7: "nlrt7-harness-timeouts-and-hangs.md",
    8: "nlrt8-permission-profile-split.md",
    9: "nlrt9-deno-fork-upstream-policy.md",
    10: "nlrt10-active-lts-canaries-and-oracles.md",
    11: "nlrt11-closeout.md",
}

if active_plan.exists():
    errors.append("active Node LTS runtime trust plan still exists; closeout must archive it")
if not archived_plan.is_file():
    errors.append("archived Node LTS runtime trust plan is missing")
if not research.is_file():
    errors.append("research baseline is missing")
if not proof_readme.is_file():
    errors.append("proof README is missing")

plan_text = archived_plan.read_text(encoding="utf-8") if archived_plan.is_file() else ""
plan_index_text = plan_readme.read_text(encoding="utf-8") if plan_readme.is_file() else ""
proof_readme_text = proof_readme.read_text(encoding="utf-8") if proof_readme.is_file() else ""

if "Status: `done`" not in plan_text:
    errors.append("archived plan status is not done")
if str(research.relative_to(repo)) not in plan_text:
    errors.append("archived plan does not link the research baseline")
if "scripts/verify-node-lts-runtime-trust.sh" not in plan_text:
    errors.append("archived plan does not name the final verifier")
if re.search(r"^\| NLRT\d+ \|.*\| (pending|in_progress) \|$", plan_text, re.MULTILINE):
    errors.append("archived plan still has pending or in_progress ledger rows")
for idx in range(12):
    if not re.search(rf"^\| NLRT{idx} \|.*\| done \|$", plan_text, re.MULTILINE):
        errors.append(f"NLRT{idx} is not marked done in the ledger")
    proof_path = proof_root / expected_proofs[idx]
    if not proof_path.is_file():
        errors.append(f"missing proof file: {proof_path.relative_to(repo)}")
if "| 2026-05-28 | NLRT11 | done |" not in plan_text:
    errors.append("execution log is missing the NLRT11 done row")
if "docs/private/plans/node-lts-runtime-trust-plan.md" in plan_index_text:
    errors.append("plans index still routes to the active Node LTS plan")
if "docs/private/plans/archive/node-lts-runtime-trust-plan.md" not in plan_index_text:
    errors.append("plans index does not list the archived Node LTS baseline")
if "NLRT0 through NLRT11 completed" not in proof_readme_text:
    errors.append("proof README does not record completed NLRT0 through NLRT11 state")

registry = json.loads(lane_registry.read_text(encoding="utf-8"))
lanes = {lane["lane_name"]: lane for lane in registry["lanes"]}
expected_phases = {
    "node20": "eol_legacy",
    "node22": "maintenance_lts",
    "node24": "active_lts",
    "node26": "current_non_lts",
}
for lane_name, phase in expected_phases.items():
    lane = lanes.get(lane_name)
    if lane is None:
        errors.append(f"missing lane registry entry for {lane_name}")
        continue
    if lane.get("support_phase") != phase:
        errors.append(f"{lane_name} support_phase is {lane.get('support_phase')}, expected {phase}")
if lanes.get("node20", {}).get("evidence_policy") != "legacy_grace_regression_only":
    errors.append("Node20 is not marked legacy-grace regression only")
if lanes.get("node20", {}).get("eol_date") != "2026-04-30":
    errors.append("Node20 EOL date is not recorded as 2026-04-30")
supported = [
    lane["lane_name"]
    for lane in registry["lanes"]
    if lane.get("evidence_policy") == "supported_lts_lane_local_evidence"
]
if supported != ["node22", "node24"]:
    errors.append(f"supported LTS lane set should be ['node22', 'node24'], got {supported}")
if registry.get("product_default_lane") != "node24":
    errors.append("product default lane is not explicitly node24")

docs_text = "\n".join(
    path.read_text(encoding="utf-8")
    for path in (compat_doc, node_readme, evidence_latest)
    if path.is_file()
)
if "Node20" not in docs_text or "legacy-grace" not in docs_text:
    errors.append("public Node docs do not describe Node20 as legacy-grace")
compat_text_normalized = " ".join(compat_doc.read_text(encoding="utf-8").split())
if "not active enterprise LTS support" not in compat_text_normalized:
    errors.append("compatibility doc does not state Node20 is not active enterprise LTS support")
if "This page is generated" not in evidence_latest.read_text(encoding="utf-8"):
    errors.append("latest public evidence page does not identify itself as generated")
if "Node22 and Node24 are supported LTS targets with lane-local evidence" not in compat_doc.read_text(encoding="utf-8"):
    errors.append("compatibility doc does not describe Node22 and Node24 as lane-local peers")

failure_text = supplementary_failures.read_text(encoding="utf-8")
active_section = failure_text.split("Active measured failure slice:", 1)[-1]
if "supplementary-process-release-shape" in active_section:
    errors.append("process-release-shape is still listed as an active supplementary failure")
if "supplementary-process-release-shape" not in failure_text or "Green slice" not in failure_text:
    errors.append("process-release-shape green inventory is missing")
process_shape_text = process_shape_tests.read_text(encoding="utf-8")
for lane in ("node20", "node22", "node24"):
    test_name = f"fn node_compat_supplementary_process_shape_{lane}"
    if test_name not in process_shape_text:
        errors.append(f"missing lane-specific process-shape test: {test_name}")

harness_text = harness_doc.read_text(encoding="utf-8")
for family in ("event_loop", "vm", "worker", "message_port", "subprocess"):
    if family not in harness_text:
        errors.append(f"harness diagnostic family missing from docs: {family}")
if "target/node-compat/diagnostics" not in harness_text:
    errors.append("harness docs do not name the diagnostic artifact root")

permission_text = permission_doc.read_text(encoding="utf-8")
required_permission_phrases = [
    "production in-process profile has no generic loopback, listen, worker, inspector, subprocess, FFI, or ambient TLS-disable env grants",
    "local-development application functions",
    "service profiles",
    "Node compatibility target is API shape only",
]
for phrase in required_permission_phrases:
    if phrase not in permission_text:
        errors.append(f"permission-model doc is missing phrase: {phrase}")

dashboard_payload = json.loads(dashboard.read_text(encoding="utf-8"))
if dashboard_payload.get("canary_claim_count") != 12:
    errors.append("dashboard canary_claim_count is not 12")
if dashboard_payload.get("canary_check_count") != 26:
    errors.append("dashboard canary_check_count is not 26")
if dashboard_payload.get("oracle_report_count") != 2:
    errors.append("dashboard oracle_report_count is not 2")
if dashboard_payload.get("required_canary_gaps"):
    errors.append("dashboard reports required canary gaps")

if errors:
    for error in errors:
        print(f"error: {error}")
    sys.exit(1)

print("verified archived plan, proof ledger, Node lane semantics, public docs, harness docs, permission docs, and dashboard evidence")
PY

run_check "Node LTS lane registry verifier" bash scripts/verify-node-lts-lanes.sh
run_check "Deno fork provenance verifier" bash scripts/verify-deno-fork-provenance.sh
run_check "Deno fork upstream policy verifier" bash scripts/verify-deno-fork-upstream-policy.sh
run_check "Node fixture provenance verifier" bash scripts/verify-node-fixture-provenance.sh
run_check "Node compatibility harness hardening verifier" bash scripts/verify-node-compat-harness-hardening.sh
run_check "Node active-LTS canary and oracle verifier" bash scripts/verify-node-lts-canaries-and-oracles.sh
run_check "Node LTS generated docs verifier" bash scripts/verify-node-lts-docs.sh
run_check "Runtime Node LTS metadata tests" cargo test -p nimbus-runtime node_lts -- --nocapture
run_check "Runtime supplementary process-shape tests" cargo test -p nimbus-runtime node_compat_supplementary_process_shape -- --nocapture --test-threads=1
run_check "Runtime Node permission profile tests" cargo test -p nimbus-runtime node_permission_profiles -- --nocapture
run_check "Tenant production admission tests" cargo test -p nimbus-tenant production_untrusted_runtime_admission -- --nocapture
run_check "Bridge execution admission tests" cargo test -p nimbus-bridge runtime_execution_admission -- --nocapture
run_check "Convex runtime access lane tests" cargo test -p nimbus-convex runtime_access -- --nocapture
run_check "Rust formatting" cargo fmt --all --check
run_check "Markdown reference validation" npm run docs:validate-refs:strict

printf '\n\033[1mSummary:\033[0m %s passed, %s failed\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -ne 0 ]; then
  printf '\nFailures:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${detail}"
  done
  exit 1
fi
