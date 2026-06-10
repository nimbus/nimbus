#!/usr/bin/env bash
# Verifies the completed Node FaaS Runtime Compatibility (NFRC) control plane.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}" || exit 1

PASS=0
FAIL=0
FAIL_DETAIL=()

pass() {
  PASS=$((PASS + 1))
  printf '  PASS  %s\n' "$1"
}

fail() {
  FAIL=$((FAIL + 1))
  printf '  FAIL  %s\n' "$1"
  if [ $# -ge 2 ]; then
    printf '        %s\n' "$2"
    FAIL_DETAIL+=("$1 - $2")
  else
    FAIL_DETAIL+=("$1")
  fi
}

step() {
  printf '\n[%s] %s\n' "$1" "$2"
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

printf 'Node FaaS runtime compatibility verifier\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

python_check "Plan, archive, proof, docs, release-train, and CI invariants" <<'PY'
import json
import re
import sys
from pathlib import Path

repo = Path.cwd()
errors: list[str] = []

active_plan = repo / "docs/private/plans/node-faas-runtime-compatibility-plan.md"
archived_plan = repo / "docs/private/plans/archive/node-faas-runtime-compatibility-plan.md"
plan_readme = repo / "docs/private/plans/README.md"
proof_root = repo / "docs/private/plans/proof/node-faas-runtime-compatibility"
proof_readme = proof_root / "README.md"
research = repo / "docs/private/plans/research/node-faas-runtime-compatibility-2026.md"
faas_profile = repo / "docs/private/staging/architecture/runtime/node-faas-compatibility-profile.json"
release_train = repo / "docs/private/staging/architecture/runtime/node-lts-compat/node-release-train.json"
compat_doc = repo / "docs/private/staging/runtimes/nodejs/compatibility.md"
api_reference = repo / "docs/private/staging/runtimes/nodejs/reference/node-apis.md"
package_reference = repo / "docs/private/staging/runtimes/nodejs/reference/packages.md"
fundamentals = repo / "docs/private/staging/runtimes/nodejs/fundamentals.md"
ci_workflow = repo / ".github/workflows/ci.yml"
nightly_workflow = repo / ".github/workflows/node-compat-nightly.yml"

expected_proofs = {
    0: "nfrc0-baseline-and-control-plane.md",
    1: "nfrc1-faas-compat-profile.md",
    2: "nfrc2-latest-node-suite-tags.md",
    3: "nfrc3-node26-current-target.md",
    4: "nfrc4-latest-fixture-corpora.md",
    5: "nfrc5-node26-and-refresh-classification.md",
    6: "nfrc6-node24-default.md",
    7: "nfrc7-convex-app-canaries.md",
    8: "nfrc8-realistic-sdk-canaries.md",
    9: "nfrc9-host-heavy-diagnostics.md",
    10: "nfrc10-deno-style-docs.md",
    11: "nfrc11-release-train-automation.md",
    12: "nfrc12-ci-nightly-lanes.md",
    13: "nfrc13-closeout.md",
}

if active_plan.exists():
    errors.append("active Node FaaS runtime compatibility plan still exists; closeout must archive it")
if not archived_plan.is_file():
    errors.append("archived Node FaaS runtime compatibility plan is missing")
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
if "scripts/verify-node-faas-runtime-compatibility.sh" not in plan_text:
    errors.append("archived plan does not name the final verifier")
if "wide-then-focused" not in plan_text:
    errors.append("archived plan does not preserve the wide-then-focused strategy")
if re.search(r"^\| NFRC\d+ \|.*\| (pending|in_progress) \|$", plan_text, re.MULTILINE):
    errors.append("archived plan still has pending or in_progress ledger rows")
for idx in range(14):
    if not re.search(rf"^\| NFRC{idx} \|.*\| done \|$", plan_text, re.MULTILINE):
        errors.append(f"NFRC{idx} is not marked done in the ledger")
    proof_path = proof_root / expected_proofs[idx]
    if not proof_path.is_file():
        errors.append(f"missing proof file: {proof_path.relative_to(repo)}")
    elif expected_proofs[idx] not in proof_readme_text:
        errors.append(f"proof README does not list {expected_proofs[idx]}")
if "| 2026-05-28 | NFRC13 | done |" not in plan_text:
    errors.append("execution log is missing the NFRC13 done row")
if "docs/private/plans/node-faas-runtime-compatibility-plan.md" in plan_index_text:
    errors.append("plans index still routes to the active Node FaaS plan")
if "docs/private/plans/archive/node-faas-runtime-compatibility-plan.md" not in plan_index_text:
    errors.append("plans index does not list the archived Node FaaS baseline")

profile = json.loads(faas_profile.read_text(encoding="utf-8"))
if profile.get("owning_plan") != "docs/private/plans/archive/node-faas-runtime-compatibility-plan.md":
    errors.append("FaaS profile owning_plan does not point at archived plan")
lanes = {lane["lane"]: lane for lane in profile.get("lane_targets", [])}
expected_lanes = {
    "node20": ("eol_legacy", False, False),
    "node22": ("maintenance_lts", False, True),
    "node24": ("active_lts", True, True),
    "node26": ("current_non_lts", False, False),
}
for lane_name, (phase, is_default, is_enterprise_lts) in expected_lanes.items():
    lane = lanes.get(lane_name)
    if lane is None:
        errors.append(f"FaaS profile missing {lane_name}")
        continue
    if lane.get("node_release_phase") != phase:
        errors.append(f"{lane_name} phase is {lane.get('node_release_phase')}, expected {phase}")
    if lane.get("product_default_after_nfrc") is not is_default:
        errors.append(f"{lane_name} product_default_after_nfrc mismatch")
    if lane.get("enterprise_lts_support") is not is_enterprise_lts:
        errors.append(f"{lane_name} enterprise_lts_support mismatch")
    if lane.get("verification_state") != "current_evidence":
        errors.append(f"{lane_name} verification_state is not current_evidence")

if profile.get("wide_then_focused_strategy", {}).get("required") is not True:
    errors.append("FaaS profile does not require wide-then-focused strategy")

release = json.loads(release_train.read_text(encoding="utf-8"))
if release.get("drift_detected") is not False:
    errors.append("release-train summary reports drift")
roles = {lane["lane"]: lane["release_train_role"] for lane in release.get("lane_contracts", [])}
if roles != {
    "node20": "legacy_grace",
    "node22": "supported_lts",
    "node24": "product_default",
    "node26": "current_non_lts",
}:
    errors.append(f"release-train lane roles are wrong: {roles}")
proof_gate = release.get("proof_gate", {})
if proof_gate.get("missing_digest_markers"):
    errors.append("release-train proof gate has missing digest markers")
if proof_gate.get("proof_file_present") is not True or proof_gate.get("proof_readme_lists_artifact") is not True:
    errors.append("release-train proof gate does not see the proof artifact")

docs_text = "\n".join(
    path.read_text(encoding="utf-8")
    for path in (compat_doc, api_reference, package_reference, fundamentals)
    if path.is_file()
)
required_doc_phrases = [
    "Node22 and Node24 are supported LTS targets with lane-local evidence.",
    "Node26 is Current/non-LTS compatibility evidence",
    "Product default is a routing default, not an evidence priority.",
    "A diagnostic pass means Nimbus proved the denial or service/microVM route; it is not positive in-process support.",
    "`Diagnostic` rows prove an intentional denial or service route",
]
for phrase in required_doc_phrases:
    if phrase not in docs_text:
        errors.append(f"public docs missing required phrase: {phrase}")
if re.search(r"\|\s*Node26\s*\|[^\n]*\bPreview\b", docs_text, re.IGNORECASE):
    errors.append("public docs still label Node26 as Preview")
if re.search(r"Host-heavy behavior\s*\|\s*Supported In Process", docs_text, re.IGNORECASE):
    errors.append("public docs overclaim host-heavy in-process support")

ci_text = ci_workflow.read_text(encoding="utf-8")
nightly_text = nightly_workflow.read_text(encoding="utf-8")
for snippet in (
    "node-faas-compatibility:",
    "make node-compat-canaries PRESET=application LANE=node22",
    "make node-compat-canaries PRESET=application LANE=node24",
    "bash scripts/verify-node-release-train.sh",
):
    if snippet not in ci_text:
        errors.append(f"CI workflow missing snippet: {snippet}")
for snippet in (
    "NIMBUS_ENFORCE_CURRENT_NODE_CORPORA=1 bash scripts/verify-node-latest-suite-tags.sh",
    "python3 scripts/runtime/node/release_train.py probe-live",
    "make node-compat-validate-watchpoints",
    "make node-compat-oracle LANE=node26",
):
    if snippet not in nightly_text:
        errors.append(f"nightly workflow missing snippet: {snippet}")

if errors:
    for error in errors:
        print(f"error: {error}")
    sys.exit(1)

print("verified archived plan, proof ledger, FaaS profile, public docs, release train, and CI/nightly wiring")
PY

run_check "Node FaaS compatibility profile verifier" bash scripts/verify-node-faas-compat-profile.sh
run_check "Node LTS lane registry verifier" bash scripts/verify-node-lts-lanes.sh
run_check "Node latest suite tag verifier" bash scripts/verify-node-latest-suite-tags.sh
run_check "Node latest suite enforced corpus verifier" env NIMBUS_ENFORCE_CURRENT_NODE_CORPORA=1 bash scripts/verify-node-latest-suite-tags.sh
run_check "Node release-train verifier" bash scripts/verify-node-release-train.sh
run_check "Node CI/nightly lane verifier" bash scripts/verify-node-ci-nightly-lanes.sh
run_check "Node fixture provenance verifier" python3 scripts/runtime/node/fixture_provenance.py validate
run_check "Node canary claim verifier" bash scripts/runtime/node/validate-claims.sh
run_check "Node active-LTS canary and oracle verifier" bash scripts/verify-node-lts-canaries-and-oracles.sh
run_check "Node host-heavy diagnostic verifier" bash scripts/verify-node-host-heavy-diagnostics.sh
run_check "Node generated docs verifier" bash scripts/verify-node-lts-docs.sh
run_check "Node public docs publish check" make node-compat-publish-docs CHECK=1
run_check "Node release-train publish check" make node-compat-release-train CHECK=1
run_check "Node watchpoint catalog verifier" make node-compat-validate-watchpoints
run_check "Runtime Node LTS metadata tests" cargo test -p nimbus-runtime node_lts -- --nocapture
run_check "Runtime Node26 metadata tests" cargo test -p nimbus-runtime node26 -- --nocapture
run_check "Runtime Node permission/profile tests" cargo test -p nimbus-runtime node_permission -- --nocapture
run_check "Tenant Node profile tests" cargo test -p nimbus-tenant node_profile -- --nocapture
run_check "Bridge execution admission tests" cargo test -p nimbus-bridge runtime_execution_admission -- --nocapture
run_check "Convex runtime access lane tests" cargo test -p nimbus-convex runtime_access -- --nocapture
run_check "Application canary preset" make node-compat-canaries PRESET=application
run_check "Tooling canary preset" make node-compat-canaries PRESET=tooling
run_check "Rust formatting" cargo fmt --all --check
run_check "Markdown reference validation" npm run docs:validate-refs:strict
run_check "Diff whitespace check" git diff --check

printf '\nSummary: %s passed, %s failed\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -ne 0 ]; then
  printf '\nFailures:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${detail}"
  done
  exit 1
fi
