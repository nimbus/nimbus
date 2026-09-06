#!/usr/bin/env bash
set -euo pipefail

CI_WORKFLOW=".github/workflows/ci.yml"
NIGHTLY_WORKFLOW=".github/workflows/node-compat-nightly.yml"
failures=0

pass() {
  printf 'PASS: %s\n' "$1"
}

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  failures=$((failures + 1))
}

require_file() {
  local path="$1"
  if [[ -f "${path}" ]]; then
    pass "${path} exists"
  else
    fail "${path} missing"
  fi
}

require_pattern() {
  local path="$1"
  local pattern="$2"
  local description="$3"
  if grep -Eq "${pattern}" "${path}"; then
    pass "${description}"
  else
    fail "${description}"
  fi
}

require_file "${CI_WORKFLOW}"
require_file "${NIGHTLY_WORKFLOW}"

require_pattern "${CI_WORKFLOW}" 'node-faas-compatibility:' "PR CI defines Node FaaS compatibility job"
require_pattern "${CI_WORKFLOW}" 'make node-compat-canaries PRESET=application LANE=node22' "PR CI gates Node22 Application canaries"
require_pattern "${CI_WORKFLOW}" 'make node-compat-canaries PRESET=application LANE=node24' "PR CI gates Node24 Application canaries"
require_pattern "${CI_WORKFLOW}" 'make node-compat-canaries-bootstrap PRESET=tooling' "PR CI bootstraps Tooling canaries"
require_pattern "${CI_WORKFLOW}" 'make node-compat-canaries PRESET=tooling LANE=node22' "PR CI gates Node22 Tooling canaries"
require_pattern "${CI_WORKFLOW}" 'make node-compat-canaries PRESET=tooling LANE=node24' "PR CI gates Node24 Tooling canaries"
require_pattern "${CI_WORKFLOW}" 'make node-compat-oracle LANE=node22' "PR CI emits Node22 oracle evidence"
require_pattern "${CI_WORKFLOW}" 'make node-compat-oracle LANE=node24' "PR CI emits Node24 oracle evidence"
require_pattern "${CI_WORKFLOW}" 'make node-compat-oracle LANE=node26' "PR CI emits Node26 current-line oracle evidence"
require_pattern "${CI_WORKFLOW}" 'node-version: 26' "PR CI provisions Node26 for current-line oracle evidence"
require_pattern "${CI_WORKFLOW}" 'oracle-\*\.json.*wc -l.*-ge 3' "PR CI requires all three oracle reports"
require_pattern "${CI_WORKFLOW}" 'make node-compat-dashboard' "PR CI builds dashboard evidence before boundary checks"
require_pattern "${CI_WORKFLOW}" 'NIMBUS_NODE_COMPAT_DASHBOARD_PATH: target/node-compat/dashboard/dashboard-summary\.json' "PR CI verifies freshly generated dashboard evidence"
require_pattern "${CI_WORKFLOW}" 'bash scripts/verify-node-lts-docs\.sh' "PR CI runs Node docs guard"
require_pattern "${CI_WORKFLOW}" 'bash scripts/verify-node-release-train\.sh' "PR CI runs release-train guard"
require_pattern "${CI_WORKFLOW}" 'bash scripts/verify-node-host-heavy-diagnostics\.sh' "PR CI verifies host-heavy diagnostics"
require_pattern "${CI_WORKFLOW}" 'node-faas-compatibility' "Rust gate summary depends on Node FaaS compatibility"

require_pattern "${NIGHTLY_WORKFLOW}" 'schedule:' "Node compatibility workflow has a schedule trigger"
require_pattern "${NIGHTLY_WORKFLOW}" 'NIMBUS_ENFORCE_CURRENT_NODE_CORPORA=1 bash scripts/verify-node-latest-suite-tags\.sh' "Nightly enforces current vendored corpora"
require_pattern "${NIGHTLY_WORKFLOW}" 'python3 scripts/runtime/node/release_train\.py probe-live' "Nightly probes official Node release feeds"
require_pattern "${NIGHTLY_WORKFLOW}" 'make node-compat-validate-watchpoints' "Nightly validates pinned watchpoints"
require_pattern "${NIGHTLY_WORKFLOW}" 'make node-compat-oracle LANE=node26' "Nightly reports Node26 current-line oracle sample"
require_pattern "${NIGHTLY_WORKFLOW}" 'make node-compat-canaries PRESET=application' "Nightly runs broad Application canary preset"
require_pattern "${NIGHTLY_WORKFLOW}" 'make node-compat-canaries PRESET=tooling' "Nightly runs broad Tooling canary preset"

if [[ "${failures}" -ne 0 ]]; then
  printf '%s Node CI/nightly lane checks failed\n' "${failures}" >&2
  exit 1
fi

printf 'Node CI/nightly lane verifier passed\n'
