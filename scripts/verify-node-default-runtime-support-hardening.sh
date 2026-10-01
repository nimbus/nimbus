#!/usr/bin/env bash
# Completion gate for the Node Default Runtime Support Hardening work. It
# checks the published, generated Node compatibility evidence in the tree.
# The pass condition is a summary with `0 failed`.
#
# Run from anywhere; it cd's to the repo root.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

CANARY_REGISTRY="tests/runtime/node/canary-registry.json"

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

run_public_generated_gate() {
  local public_compatibility_md="tests/runtime/node/published/nodejs/compatibility.md"
  local public_node_api_md="tests/runtime/node/published/nodejs/reference/node-apis.md"
  local public_shim_inventory_md="tests/runtime/node/published/nodejs/reference/shims-and-boundaries.md"

  printf 'Mode: public generated-evidence gate\n'

  step 1 "Published Node compatibility artifacts"
  if [ -f "${public_compatibility_md}" ] &&
     [ -f "${public_node_api_md}" ] &&
     [ -f "${public_shim_inventory_md}" ]; then
    pass "Published Node compatibility artifacts exist"
  else
    fail "Published Node compatibility artifacts missing" \
      "Expected ${public_compatibility_md}, ${public_node_api_md}, and ${public_shim_inventory_md}"
  fi

  step 2 "Node22/Node24/Node26 V8-isolate-required green"
  if [ -f "${public_compatibility_md}" ] && python3 - "${public_compatibility_md}" <<'PY'
from pathlib import Path
import sys

rows = {}
for line in Path(sys.argv[1]).read_text(encoding="utf-8").splitlines():
    if line.startswith("| Node"):
        cells = [cell.strip() for cell in line.strip("|").split("|")]
        if len(cells) == 10:
            rows[cells[0]] = cells
for lane_name in ("Node22", "Node24", "Node26"):
    cells = rows.get(lane_name)
    if cells is None:
        raise SystemExit(1)
    passed, total = (int(value.strip()) for value in cells[4].split("/"))
    if passed != total or int(cells[5]) != 0:
        raise SystemExit(1)
raise SystemExit(0)
PY
  then
    pass "Node22, Node24, and Node26 V8-isolate-required fixtures are 100%"
  else
    fail "V8-isolate-required fixtures not proven green" "Expected generated posture metrics with 0 gaps and 100% pass rate for node22/node24/node26"
  fi

  step 3 "Node24 unpromoted surface eliminated"
  if [ -f "${public_compatibility_md}" ] && python3 - "${public_compatibility_md}" <<'PY'
from pathlib import Path
import sys

for line in Path(sys.argv[1]).read_text(encoding="utf-8").splitlines():
    if line.startswith("| Node24"):
        cells = [cell.strip() for cell in line.strip("|").split("|")]
        if len(cells) == 10 and int(cells[5]) == 0:
            raise SystemExit(0)
raise SystemExit(1)
PY
  then
    pass "Node24 has no remaining unpromoted surface entries in the default-support posture"
  else
    fail "Node24 still has Requires Unpromoted Node Surface" "Expected generated posture metrics"
  fi

  step 4 "Published posture agrees across compatibility references"
  if [ -f "${public_compatibility_md}" ] &&
     [ -f "${public_node_api_md}" ] &&
     python3 - "${public_compatibility_md}" "${public_node_api_md}" <<'PY'
from pathlib import Path
import sys

def posture_rows(path):
    return {
        cells[0]: cells
        for line in Path(path).read_text(encoding="utf-8").splitlines()
        if line.startswith("| Node")
        for cells in ([cell.strip() for cell in line.strip("|").split("|")],)
        if len(cells) == 10
    }

expected = posture_rows(sys.argv[1])
actual = posture_rows(sys.argv[2])
raise SystemExit(0 if expected and actual == expected else 1)
PY
  then
    pass "Published compatibility and API references report the same posture"
  else
    fail "Published posture references disagree" \
      "Expected ${public_compatibility_md} and ${public_node_api_md} to carry identical generated posture rows"
  fi

  step 5 "Package registry category schema and breadth"
  if [ -f "${CANARY_REGISTRY}" ] &&
     grep -q '"compat_category"' "${CANARY_REGISTRY}" &&
     grep -q '"compat_family"' "${CANARY_REGISTRY}" &&
     grep -q '"canary_surfaces"' "${CANARY_REGISTRY}" &&
     python3 - "${CANARY_REGISTRY}" <<'PY'
import json
import sys

data = json.load(open(sys.argv[1], encoding="utf-8"))
claims = [claim for claim in data.get("claims", []) if claim.get("runtime_preset") == "Application"]
categories = {claim.get("compat_category") for claim in claims if claim.get("compat_category")}
if len(claims) >= 50 and len(categories) >= 12:
    raise SystemExit(0)
raise SystemExit(1)
PY
  then
    pass "Application canary registry has >=50 claims across >=12 categories"
  else
    fail "Application package breadth incomplete" "Expected >=50 Application claims across >=12 compat_category values"
  fi

  step 6 "Required canary gaps are zero"
  if [ -f "tests/runtime/node/compat/node-compat-evidence/latest/dashboard-summary.md" ] &&
     grep -q 'required canary gaps: `0`' tests/runtime/node/published/nodejs/compatibility.md 2>/dev/null; then
    pass "Required Application canary gaps are zero"
  else
    fail "Required canary gap proof missing" "Expected generated docs/dashboard to show 0"
  fi

  step 7 "Generated public docs expose package/API/shim boundaries"
  if [ -f tests/runtime/node/published/nodejs/reference/packages.md ] &&
     [ -f tests/runtime/node/published/nodejs/reference/node-apis.md ] &&
     [ -f tests/runtime/node/published/nodejs/reference/shims-and-boundaries.md ] &&
     grep -q 'Node22' tests/runtime/node/published/nodejs/reference/packages.md &&
     grep -q 'Node24' tests/runtime/node/published/nodejs/reference/packages.md &&
     grep -q 'Node26' tests/runtime/node/published/nodejs/reference/packages.md &&
     grep -q 'Node22' tests/runtime/node/published/nodejs/reference/node-apis.md &&
     grep -q 'Service/microVM required' tests/runtime/node/published/nodejs/reference/node-apis.md &&
     grep -q 'test-harness-only' tests/runtime/node/published/nodejs/reference/shims-and-boundaries.md &&
     grep -q 'diagnostic' tests/runtime/node/published/nodejs/reference/shims-and-boundaries.md &&
     grep -q 'unsupported' tests/runtime/node/published/nodejs/reference/shims-and-boundaries.md; then
    pass "Generated package, API, and shim references are per-version and boundary-aware"
  else
    fail "Generated package/API/shim references incomplete" "Expected per-version package support plus non-isolate and shim boundaries"
  fi

  step 8 "Release-train and latest-suite drift"
  if [ -f tests/runtime/node/compat/node-lts-compat/node-release-train.json ] &&
     grep -q '"drift_detected": false' tests/runtime/node/compat/node-lts-compat/node-release-train.json; then
    pass "Release-train drift check is clean"
  else
    fail "Release-train drift proof missing or dirty" "Expected drift_detected=false"
  fi

  step 9 "CI and nightly gate wiring"
  if grep -Rq 'verify-node-default-runtime-support-hardening' .github/workflows 2>/dev/null &&
     [ -f .github/workflows/node-compat-nightly.yml ] &&
     grep -q 'node26' .github/workflows/node-compat-nightly.yml &&
     grep -q 'fixture' .github/workflows/node-compat-nightly.yml; then
    pass "PR CI and nightly include NDS/Node26 compatibility gates"
  else
    fail "CI or nightly gate wiring missing" "Expected PR verifier and broad Node26 nightly lanes"
  fi

  printf '\n\033[1mSummary:\033[0m %s passed, %s failed\n' "${PASS}" "${FAIL}"
  if [ "${FAIL}" -ne 0 ]; then
    printf '\nFailures:\n'
    for detail in "${FAIL_DETAIL[@]}"; do
      printf '  - %s\n' "${detail}"
    done
    exit 1
  fi
  exit 0
}

printf '\033[1mNDS verification gate - node-default-runtime-support-hardening\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"

run_public_generated_gate
