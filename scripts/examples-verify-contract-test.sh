#!/usr/bin/env bash
# Source-level contract checks for the live application verification lane.
# Later AVR tasks extend these stable condition IDs with behavioral fixtures.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEFAULT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
ROOT="${NIMBUS_AVR_ROOT:-${DEFAULT_ROOT}}"

pass_count=0
fail_count=0
selected_count=0

has() {
  local path="$1" pattern="$2"
  [ -f "${ROOT}/${path}" ] && grep -Eq -- "${pattern}" "${ROOT}/${path}"
}

lacks() {
  local path="$1" pattern="$2"
  [ -f "${ROOT}/${path}" ] && ! grep -Eq -- "${pattern}" "${ROOT}/${path}"
}

evaluate_condition() {
  local id="$1"
  case "${id}" in
    AVRC11)
      has Makefile '^examples-verify:$' &&
        has Makefile '^[[:space:]]+bash scripts/examples-verify\.sh --host-preflight$' &&
        has Makefile '^[[:space:]]+\$\(SINGLE_FLIGHT\).*\$\(MAKE\).*examples-verify-run$' &&
        has Makefile '^examples-verify-run: \$\(UI_DIST_INDEX\) \$\(EMBEDDED_PKG_MANIFEST\)$' &&
        has scripts/examples-verify.sh 'fresh-checkout prerequisites' &&
        has scripts/examples-verify.sh 'make examples-verify'
      ;;
    AVRC12)
      has Makefile '^ifeq.*NIMBUS_EXAMPLES_VERIFY_BIN' &&
        has scripts/examples-verify.sh 'require_supported_node' &&
        has scripts/examples-verify.sh 'require_supplied_binary' &&
        has scripts/examples-verify.sh 'Node\.js.*22.*24' &&
        has scripts/examples-verify.sh 'unsupported Node' &&
        has scripts/examples-verify.sh 'supplied binary.*skip.*Rust build'
      ;;
    AVRC13)
      [ -f "${ROOT}/scripts/examples-verify-cases.json" ] &&
        node - "${ROOT}/scripts/examples-verify-cases.json" <<'NODE'
const fs = require("fs");
const value = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
if (!Array.isArray(value.cases) || value.cases.length !== 9) process.exit(1);
const required = ["name", "workspace", "appDir", "boot", "smoke", "surfaces", "updateSemantics"];
const names = new Set();
for (const item of value.cases) {
  if (!item || required.some((key) => !(key in item))) process.exit(1);
  if (typeof item.name !== "string" || names.has(item.name)) process.exit(1);
  names.add(item.name);
}
NODE
      ;;
    AVRC14)
      has scripts/examples-verify.sh 'disposable.*workspace' &&
        has scripts/examples-verify.sh 'prepare_case_workspace' &&
        has scripts/examples-verify-workspace.mjs 'prepareCaseWorkspace' &&
        has scripts/examples-verify-cases.json '"inputs"' &&
        lacks scripts/examples-verify.sh 'npm run codegen -w "\$\{workspace\}"'
      ;;
    AVRC15)
      has scripts/examples-verify.sh 'capture_source_byte_manifest' &&
        has scripts/examples-verify.sh 'verify_source_byte_manifest' &&
        has scripts/examples-verify.sh 'trap finalize_examples_verification EXIT' &&
        has scripts/examples-verify-workspace.mjs 'captureSourceByteManifest' &&
        has scripts/examples-verify-workspace.mjs 'verifySourceByteManifest' &&
        lacks scripts/examples-verify.sh 'git (checkout|reset|clean)'
      ;;
    AVRC16)
      grep -Rqs -- 'compose_discovery_defaults_to_enabled' "${ROOT}/crates/nimbus-cli/src/dev" &&
        grep -Rqs -- 'explicit_compose_file_still_loads' "${ROOT}/crates/nimbus-cli/src/dev"
      ;;
    AVRC17)
      has crates/nimbus-cli/src/dev.rs 'no[_-]compose[_-]discovery' &&
        grep -Rqs -- 'compose_discovery_opt_out_performs_no_discovery' "${ROOT}/crates/nimbus-cli/src/dev" &&
        lacks scripts/examples-verify.sh 'sideline_compose|restore_compose|COMPOSE_SIDELINE_PATH|compose\.yaml\.smoke-bak|mv .*compose\.yaml'
      ;;
    AVRC18)
      has scripts/examples-verify.sh '"\$\{NIMBUS_BIN\}" run functions tasks:list' &&
        has crates/nimbus-cli/src/run.rs 'bare_local_target_matches_explicit_target' &&
        has crates/nimbus-cli/src/run.rs 'bare_local_target_rejects_wrong_silo_and_invalid_credentials'
      ;;
    AVRC19)
      has scripts/examples-verify.sh 'provider_assigned_port_lease' &&
        has scripts/examples-verify.sh 'retained_listener' &&
        lacks scripts/examples-verify.sh 'socket\.socket\(\).*bind' &&
        lacks scripts/examples-verify.sh '27017|8000|9000'
      ;;
    AVRC20)
      has scripts/examples-verify.sh 'NIMBUS_NETWORK_STATE_DIR' &&
        has scripts/examples-verify.sh 'case_(auth|discovery|audit|app|data|control|log)_root' &&
        has scripts/examples-verify.sh 'cleanup_failure.*retain' &&
        has scripts/examples-verify.sh '--env-file.*SMOKE_ENV_FILE' &&
        lacks scripts/examples-verify.sh '--env[[:space:]]+"NIMBUS_ADMIN_TOKEN=' &&
        has scripts/examples-verify-lifetime.mjs 'sourceRunRoot'
      ;;
    AVRC21)
      has scripts/examples-verify-report.mjs 'REPORT_SCHEMA_VERSION' &&
        has scripts/examples-verify-report.mjs 'function redact' &&
        has scripts/examples-verify-report.mjs 'function validateReport' &&
        has scripts/examples-verify-report.mjs 'writeJsonAtomically' &&
        has scripts/examples-verify-report-test.mjs 'schema_accepts_success_golden' &&
        has scripts/examples-verify-report-test.mjs 'credential_redaction_is_recursive' &&
        has scripts/examples-verify-report-test.mjs 'interrupted_atomic_write_preserves_canonical_file' &&
        has scripts/examples-verify-cases.json '"expectedAnchors"' &&
        has scripts/examples-verify.sh 'REPORT_ADAPTER'
      ;;
    AVRC22)
      has scripts/examples-verify-report.mjs 'function junit' &&
        has scripts/examples-verify-report.mjs 'function deterministicCases' &&
        has scripts/examples-verify-report-test.mjs 'success_junit_projection_is_deterministic' &&
        has scripts/examples-verify-report-test.mjs 'failure_junit_projects_case_and_cleanup_truth' &&
        has .github/workflows/ci.yml 'Upload examples-verification reports' &&
        has .github/workflows/ci.yml 'target/examples-verify-results/.*/report\.json' &&
        has .github/workflows/ci.yml 'target/examples-verify-results/.*/junit\.xml'
      ;;
    AVRC23)
      has scripts/examples-verify-benchmark.mjs 'function evaluateSamples' &&
        has scripts/examples-verify-benchmark.mjs 'function validateEvidence' &&
        has scripts/examples-verify-benchmark.mjs 'parallel-relative-budget' &&
        has scripts/examples-verify-benchmark.mjs 'parallel-absolute-budget' &&
        has scripts/examples-verify-benchmark-test.mjs 'busy_or_different_host_sample_is_invalid_not_failed' &&
        has scripts/examples-verify-scheduler-test.mjs 'failure_drains_without_starting_later_cases' &&
        has scripts/examples-verify-scheduler-test.mjs 'signal_drains_active_workers' &&
        has scripts/examples-verify.sh 'NIMBUS_EXAMPLES_VERIFY_MAX_PARALLEL' &&
        has scripts/examples-verify.sh 'SCHEDULER_FAILURE_ROOT'
      ;;
    AVRC24)
      grep -Rqs -- 'nine.*app' "${ROOT}/examples" &&
        grep -Rqs -- 'Node\.js.*22.*24' "${ROOT}/examples" &&
        grep -Rqs -- 'push.*poll\|poll.*push' "${ROOT}/examples" &&
        grep -Rqs -- 'retained.*artifact\|artifact.*retain' "${ROOT}/examples"
      ;;
    *)
      return 2
      ;;
  esac
}

owner_for() {
  case "$1" in
    AVRC11|AVRC12) printf 'AVR3\n' ;;
    AVRC13|AVRC14|AVRC15) printf 'AVR4\n' ;;
    AVRC16|AVRC17) printf 'AVR5\n' ;;
    AVRC18) printf 'AVR6\n' ;;
    AVRC19|AVRC20) printf 'AVR7\n' ;;
    AVRC21|AVRC22) printf 'AVR8\n' ;;
    AVRC23) printf 'AVR9\n' ;;
    AVRC24) printf 'AVR10\n' ;;
    *) return 1 ;;
  esac
}

write_green_fixture() {
  local id="$1" root="$2"
  mkdir -p "${root}/scripts" "${root}/crates/nimbus-cli/src/dev" "${root}/examples/app" "${root}/.github/workflows"
  case "${id}" in
    AVRC11)
      printf '%s\n' \
        'examples-verify:' \
        $'\tbash scripts/examples-verify.sh --host-preflight' \
        $'\t$(SINGLE_FLIGHT) --key examples-verify -- $(MAKE) --no-print-directory examples-verify-run' \
        "examples-verify-run: \$(UI_DIST_INDEX) \$(EMBEDDED_PKG_MANIFEST)" \
        >"${root}/Makefile"
      printf '%s\n' '# fresh-checkout prerequisites; use make examples-verify' >"${root}/scripts/examples-verify.sh"
      ;;
    AVRC12)
      printf '%s\n' "ifeq (\$(strip \$(NIMBUS_EXAMPLES_VERIFY_BIN)),)" >"${root}/Makefile"
      printf '%s\n' \
        'require_supported_node() { :; }' \
        'require_supplied_binary() { :; }' \
        '# Node.js 22 and 24; unsupported Node; supplied binary can skip the Rust build' \
        >"${root}/scripts/examples-verify.sh"
      ;;
    AVRC13)
      node - "${root}/scripts/examples-verify-cases.json" <<'NODE'
const fs = require("fs");
const path = process.argv[2];
const cases = Array.from({length: 9}, (_, i) => ({
  name: `case-${i}`, workspace: `workspace-${i}`, appDir: `app-${i}`,
  boot: {}, smoke: {}, surfaces: ["fixture"], updateSemantics: "push"
}));
fs.writeFileSync(path, JSON.stringify({cases}));
NODE
      ;;
    AVRC14)
      printf '%s\n' '# disposable workspace' 'prepare_case_workspace() { :; }' >"${root}/scripts/examples-verify.sh"
      printf '%s\n' 'export function prepareCaseWorkspace() {}' >"${root}/scripts/examples-verify-workspace.mjs"
      printf '%s\n' '{"inputs":[]}' >"${root}/scripts/examples-verify-cases.json"
      ;;
    AVRC15)
      printf '%s\n' \
        'capture_source_byte_manifest() { :; }' \
        'verify_source_byte_manifest() { :; }' \
        'trap finalize_examples_verification EXIT' \
        >"${root}/scripts/examples-verify.sh"
      printf '%s\n' \
        'export function captureSourceByteManifest() {}' \
        'export function verifySourceByteManifest() {}' \
        >"${root}/scripts/examples-verify-workspace.mjs"
      ;;
    AVRC16)
      printf '%s\n' '# compose_discovery_defaults_to_enabled' '# explicit_compose_file_still_loads' >"${root}/crates/nimbus-cli/src/dev/tests.rs"
      ;;
    AVRC17)
      printf '%s\n' '# no-compose-discovery' >"${root}/crates/nimbus-cli/src/dev.rs"
      printf '%s\n' '# compose_discovery_opt_out_performs_no_discovery' >"${root}/crates/nimbus-cli/src/dev/tests.rs"
      printf '%s\n' '# no tracked rename' >"${root}/scripts/examples-verify.sh"
      ;;
    AVRC18)
      printf '%s\n' "\"\${NIMBUS_BIN}\" run functions tasks:list" >"${root}/scripts/examples-verify.sh"
      printf '%s\n' '# bare_local_target_matches_explicit_target' '# bare_local_target_rejects_wrong_silo_and_invalid_credentials' >"${root}/crates/nimbus-cli/src/run.rs"
      ;;
    AVRC19)
      printf '%s\n' '# provider_assigned_port_lease with retained_listener; no shell port allocation' >"${root}/scripts/examples-verify.sh"
      ;;
    AVRC20)
      printf '%s\n' 'NIMBUS_NETWORK_STATE_DIR=/run/global' 'case_auth_root case_discovery_root case_audit_root case_app_root case_data_root case_control_root case_log_root' '# cleanup_failure must retain evidence' >"${root}/scripts/examples-verify.sh"
      printf '%s\n' "--env-file \"\${SMOKE_ENV_FILE}\"" >>"${root}/scripts/examples-verify.sh"
      printf '%s\n' 'const sourceRunRoot = true;' >"${root}/scripts/examples-verify-lifetime.mjs"
      ;;
    AVRC21)
      printf '%s\n' 'const REPORT_SCHEMA_VERSION = 1; function redact() {} function validateReport() {} function writeJsonAtomically() {}' >"${root}/scripts/examples-verify-report.mjs"
      printf '%s\n' 'schema_accepts_success_golden credential_redaction_is_recursive interrupted_atomic_write_preserves_canonical_file' >"${root}/scripts/examples-verify-report-test.mjs"
      printf '%s\n' '{"expectedAnchors":["fixture.pass"]}' >"${root}/scripts/examples-verify-cases.json"
      printf '%s\n' 'REPORT_ADAPTER=scripts/examples-verify-report.mjs' >"${root}/scripts/examples-verify.sh"
      ;;
    AVRC22)
      printf '%s\n' 'function junit() {} function deterministicCases() {}' >"${root}/scripts/examples-verify-report.mjs"
      printf '%s\n' 'success_junit_projection_is_deterministic failure_junit_projects_case_and_cleanup_truth' >"${root}/scripts/examples-verify-report-test.mjs"
      printf '%s\n' 'Upload examples-verification reports' 'target/examples-verify-results/*/report.json' 'target/examples-verify-results/*/junit.xml' >"${root}/.github/workflows/ci.yml"
      ;;
    AVRC23)
      printf '%s\n' 'function evaluateSamples() {} function validateEvidence() {} parallel-relative-budget parallel-absolute-budget' >"${root}/scripts/examples-verify-benchmark.mjs"
      printf '%s\n' 'busy_or_different_host_sample_is_invalid_not_failed' >"${root}/scripts/examples-verify-benchmark-test.mjs"
      printf '%s\n' 'failure_drains_without_starting_later_cases signal_drains_active_workers' >"${root}/scripts/examples-verify-scheduler-test.mjs"
      printf '%s\n' 'NIMBUS_EXAMPLES_VERIFY_MAX_PARALLEL SCHEDULER_FAILURE_ROOT' >"${root}/scripts/examples-verify.sh"
      ;;
    AVRC24)
      printf '%s\n' 'nine app checks use Node.js 22 and 24. push is distinct from polling. retained artifact instructions.' >"${root}/examples/app/README.md"
      ;;
  esac
}

mutate_fixture() {
  local id="$1" root="$2"
  case "${id}" in
    AVRC11) printf '%s\n' 'examples-verify:' >"${root}/Makefile" ;;
    AVRC12) printf '%s\n' '# Node.js version not checked' >"${root}/scripts/examples-verify.sh" ;;
    AVRC13) printf '%s\n' '{"cases":[]}' >"${root}/scripts/examples-verify-cases.json" ;;
    AVRC14) printf '%s\n' "npm run codegen -w \"\${workspace}\"" >"${root}/scripts/examples-verify.sh" ;;
    AVRC15) printf '%s\n' '# no byte manifest' >"${root}/scripts/examples-verify.sh" ;;
    AVRC16) printf '%s\n' '# default behavior untested' >"${root}/crates/nimbus-cli/src/dev/tests.rs" ;;
    AVRC17) printf '%s\n' 'mv compose.yaml compose.yaml.bak' >"${root}/scripts/examples-verify.sh" ;;
    AVRC18) printf '%s\n' "\"\${NIMBUS_BIN}\" run \"\${target_url}\" functions tasks:list" >"${root}/scripts/examples-verify.sh" ;;
    AVRC19) printf '%s\n' 'socket.socket().bind(("127.0.0.1", 0))' >"${root}/scripts/examples-verify.sh" ;;
    AVRC20) printf '%s\n' '# shared state and best-effort cleanup' >"${root}/scripts/examples-verify.sh" ;;
    AVRC21) printf '%s\n' 'const schemaVersion = 1;' >"${root}/scripts/examples-verify-report.mjs" ;;
    AVRC22) printf '%s\n' 'const junit = true;' >"${root}/scripts/examples-verify-report.mjs" ;;
    AVRC23) printf '%s\n' '# serial only' >"${root}/scripts/examples-verify-benchmark.mjs" ;;
    AVRC24) printf '%s\n' 'eight apps use Node.js 22. updates happen.' >"${root}/examples/app/README.md" ;;
  esac
}

run_condition() {
  local id="$1"
  if ! owner_for "${id}" >/dev/null; then
    printf 'unknown condition: %s\n' "${id}" >&2
    return 2
  fi
  if evaluate_condition "${id}"; then
    printf 'PASS %s (%s)\n' "${id}" "$(owner_for "${id}")"
    pass_count=$((pass_count + 1))
  else
    printf 'FAIL %s (%s)\n' "${id}" "$(owner_for "${id}")"
    fail_count=$((fail_count + 1))
  fi
  selected_count=$((selected_count + 1))
}

self_test_condition() {
  local id="$1" tmp old_root
  tmp="$(mktemp -d -t nimbus-avr-contract.XXXXXX)"
  old_root="${ROOT}"
  ROOT="${tmp}"
  write_green_fixture "${id}" "${tmp}"
  if ! evaluate_condition "${id}"; then
    printf 'self-test %s: green fixture did not pass\n' "${id}" >&2
    ROOT="${old_root}"
    rm -rf "${tmp}"
    return 1
  fi
  mutate_fixture "${id}" "${tmp}"
  if evaluate_condition "${id}"; then
    printf 'self-test %s: mutation did not fail closed\n' "${id}" >&2
    ROOT="${old_root}"
    rm -rf "${tmp}"
    return 1
  fi
  ROOT="${old_root}"
  rm -rf "${tmp}"
  return 0
}

write_node_stub() {
  local path="$1" version="$2"
  mkdir -p "$(dirname "${path}")"
  printf '%s\n' \
    '#!/usr/bin/env bash' \
    "if [ \"\${1:-}\" = \"--version\" ]; then" \
    "  printf '%s\\n' 'v${version}'" \
    '  exit 0' \
    'fi' \
    'printf "unexpected node invocation: %s\\n" "$*" >&2' \
    'exit 97' \
    >"${path}"
  chmod +x "${path}"
}

write_work_stub() {
  local path="$1" name="$2"
  printf '%s\n' \
    '#!/usr/bin/env bash' \
    "printf '%s\\n' '${name}' >>\"\${NIMBUS_AVR_WORK_MARKER:?}\"" \
    'exit 98' \
    >"${path}"
  chmod +x "${path}"
}

avr3_behavior_pass() {
  printf 'PASS AVR3 behavior: %s\n' "$1"
}

avr3_behavior_fail() {
  printf 'FAIL AVR3 behavior: %s\n' "$1" >&2
  fail_count=$((fail_count + 1))
}

run_avr3_behavior_tests() {
  local tmp fixture runner output status major command marker make_marker real_node
  tmp="$(mktemp -d -t nimbus-avr3-contract.XXXXXX)"
  tmp="$(cd "${tmp}" && pwd -P)"
  fixture="${tmp}/fixture"
  runner="${fixture}/scripts/examples-verify.sh"
  mkdir -p "${fixture}/scripts" "${fixture}/stub-bin" "${fixture}/tmp"
  cp "${ROOT}/scripts/examples-verify.sh" "${runner}"
  cp "${ROOT}/scripts/examples-verify-cases.json" "${fixture}/scripts/examples-verify-cases.json"
  cp "${ROOT}/scripts/examples-verify-lifetime.mjs" "${fixture}/scripts/examples-verify-lifetime.mjs"
  cp "${ROOT}/scripts/examples-verify-report.mjs" "${fixture}/scripts/examples-verify-report.mjs"
  cp "${ROOT}/scripts/examples-verify-workspace.mjs" "${fixture}/scripts/examples-verify-workspace.mjs"
  chmod +x "${runner}"
  real_node="$(command -v node)"

  # The supported range accepts all in-range majors. Node.js 22 and 24 are
  # the acceptance anchors; 23 proves that this is a range, not a two-value
  # allowlist.
  for major in 22 23 24; do
    write_node_stub "${fixture}/stub-bin/node" "${major}.0.0"
    output="${tmp}/node-${major}.out"
    if env PATH="${fixture}/stub-bin:/usr/bin:/bin:/usr/sbin:/sbin" \
        /bin/bash "${runner}" --host-preflight >"${output}" 2>&1; then
      avr3_behavior_pass "Node.js ${major} passes host preflight"
    else
      avr3_behavior_fail "Node.js ${major} must pass host preflight ($(tr '\n' ' ' <"${output}"))"
    fi
  done

  for major in 20 21 25; do
    write_node_stub "${fixture}/stub-bin/node" "${major}.0.0"
    output="${tmp}/node-${major}.out"
    status=0
    env PATH="${fixture}/stub-bin:/usr/bin:/bin:/usr/sbin:/sbin" \
      /bin/bash "${runner}" --host-preflight >"${output}" 2>&1 || status=$?
    if [ "${status}" -ne 0 ] && grep -q 'unsupported Node.js version' "${output}"; then
      avr3_behavior_pass "Node.js ${major} fails host preflight"
    else
      avr3_behavior_fail "Node.js ${major} must fail host preflight with the supported range"
    fi
  done

  output="${tmp}/node-missing.out"
  status=0
  env PATH="/usr/bin:/bin:/usr/sbin:/sbin" \
    /bin/bash "${runner}" --host-preflight >"${output}" 2>&1 || status=$?
  if [ "${status}" -ne 0 ] && grep -q 'node was not found' "${output}"; then
    avr3_behavior_pass 'missing Node.js fails host preflight'
  else
    avr3_behavior_fail 'missing Node.js did not fail host preflight'
  fi

  printf '%s\n' '#!/usr/bin/env bash' 'exit 7' >"${fixture}/stub-bin/node"
  chmod +x "${fixture}/stub-bin/node"
  output="${tmp}/node-version-failed.out"
  status=0
  env PATH="${fixture}/stub-bin:/usr/bin:/bin:/usr/sbin:/sbin" \
    /bin/bash "${runner}" --host-preflight >"${output}" 2>&1 || status=$?
  if [ "${status}" -ne 0 ] && grep -q 'node --version failed' "${output}"; then
    avr3_behavior_pass 'failing node --version fails host preflight'
  else
    avr3_behavior_fail 'failing node --version did not fail host preflight'
  fi

  write_node_stub "${fixture}/stub-bin/node" 'not-semver'
  output="${tmp}/node-malformed.out"
  status=0
  env PATH="${fixture}/stub-bin:/usr/bin:/bin:/usr/sbin:/sbin" \
    /bin/bash "${runner}" --host-preflight >"${output}" 2>&1 || status=$?
  if [ "${status}" -ne 0 ] && grep -q 'unsupported Node.js version vnot-semver' "${output}"; then
    avr3_behavior_pass 'malformed Node.js version fails host preflight'
  else
    avr3_behavior_fail 'malformed Node.js version did not fail host preflight'
  fi

  # Direct invocation from tracked files fails before port allocation,
  # temporary state, Cargo, npm, or an application process.
  write_node_stub "${fixture}/stub-bin/node" '22.0.0'
  marker="${tmp}/direct-work.marker"
  for command in python3 mktemp cargo npm; do
    write_work_stub "${fixture}/stub-bin/${command}" "${command}"
  done
  output="${tmp}/direct-missing.out"
  status=0
  env PATH="${fixture}/stub-bin:/usr/bin:/bin:/usr/sbin:/sbin" \
    NIMBUS_AVR_WORK_MARKER="${marker}" \
    /bin/bash "${runner}" >"${output}" 2>&1 || status=$?
  if [ "${status}" -ne 0 ] &&
      grep -q 'packages/nimbus-ui/dist/index.html' "${output}" &&
      grep -q 'crates/nimbus-assets/embedded/packages/manifest.json' "${output}" &&
      grep -q 'run the supported entry point: make examples-verify' "${output}" &&
      [ ! -e "${marker}" ]; then
    avr3_behavior_pass 'direct invocation reports both prerequisites before work'
  else
    avr3_behavior_fail "direct invocation did not fail before work with the Make recovery command ($(tr '\n' ' ' <"${output}"))"
  fi

  # An explicit invalid binary is an input error. It fails before Make can
  # generate artifacts or start Cargo.
  output="${tmp}/invalid-binary.out"
  status=0
  env PATH="${fixture}/stub-bin:/usr/bin:/bin:/usr/sbin:/sbin" \
    NIMBUS_EXAMPLES_VERIFY_BIN="${fixture}/missing-nimbus" \
    NIMBUS_AVR_WORK_MARKER="${marker}" \
    /bin/bash "${runner}" --host-preflight >"${output}" 2>&1 || status=$?
  if [ "${status}" -ne 0 ] && grep -q 'supplied binary is missing or not executable' "${output}" && [ ! -e "${marker}" ]; then
    avr3_behavior_pass 'invalid supplied binary fails before work'
  else
    avr3_behavior_fail 'invalid supplied binary did not fail before work'
  fi

  # A valid supplied binary bypasses both generated inputs and Cargo. Use an
  # invalid case selector to stop after preflight without starting an app.
  rm -f "${fixture}/stub-bin/python3" "${fixture}/stub-bin/mktemp"
  printf '%s\n' \
    '#!/usr/bin/env bash' \
    "if [ \"\${1:-}\" = \"--version\" ]; then" \
    "  printf '%s\\n' 'v22.0.0'" \
    '  exit 0' \
    'fi' \
    "exec '${real_node}' \"\$@\"" \
    >"${fixture}/stub-bin/node"
  chmod +x "${fixture}/stub-bin/node"
  mkdir -p "${fixture}/packages/firebase/src/gen/google/firestore/v1"
  : >"${fixture}/packages/firebase/src/gen/google/firestore/v1/firestore_pb.ts"
  marker="${tmp}/supplied-work.marker"
  output="${tmp}/supplied.out"
  status=0
  env PATH="${fixture}/stub-bin:/usr/bin:/bin:/usr/sbin:/sbin" \
    TMPDIR="${fixture}/tmp" \
    NIMBUS_EXAMPLES_VERIFY_BIN=/usr/bin/true \
    NIMBUS_EXAMPLES_VERIFY_ONLY=not-a-case \
    NIMBUS_EXAMPLES_VERIFY_PORT=49152 \
    NIMBUS_AVR_WORK_MARKER="${marker}" \
    /bin/bash "${runner}" >"${output}" 2>&1 || status=$?
  if [ "${status}" -ne 0 ] &&
      grep -q 'skip generated build prerequisites and the Rust build' "${output}" &&
      grep -q 'matched no app in the manifest' "${output}" &&
      [ ! -e "${marker}" ]; then
    avr3_behavior_pass 'valid supplied binary skips generated inputs and Cargo'
  else
    avr3_behavior_fail "supplied binary fast path performed build work or missed its evidence ($(tr '\n' ' ' <"${output}"))"
  fi

  # The public Make entry must stop at the host preflight. Override the
  # single-flight command with a marker: any nested prerequisite work is a
  # contract failure.
  write_node_stub "${fixture}/stub-bin/node" '20.20.2'
  make_marker="${tmp}/make-work.marker"
  printf '%s\n' \
    '#!/usr/bin/env bash' \
    "printf '%s\\n' work >'${make_marker}'" \
    'exit 99' \
    >"${tmp}/mark-single-flight.sh"
  output="${tmp}/make-node20.out"
  status=0
  env PATH="${fixture}/stub-bin:/usr/bin:/bin:/usr/sbin:/sbin" \
    NIMBUS_EXAMPLES_VERIFY_BIN=/usr/bin/true \
    make -C "${ROOT}" examples-verify \
      "SINGLE_FLIGHT=/bin/bash ${tmp}/mark-single-flight.sh" \
      >"${output}" 2>&1 || status=$?
  if [ "${status}" -ne 0 ] && grep -q 'unsupported Node.js version v20.20.2' "${output}" && [ ! -e "${make_marker}" ]; then
    avr3_behavior_pass 'Make rejects unsupported Node before nested prerequisites'
  else
    avr3_behavior_fail "Make did not stop before nested prerequisite work ($(tr '\n' ' ' <"${output}"))"
  fi

  rm -rf "${tmp}"
}

run_avr4_behavior_tests() {
  node "${ROOT}/scripts/examples-verify-workspace-test.mjs"
}

run_avr7_behavior_tests() {
  node "${ROOT}/scripts/examples-verify-lifetime-test.mjs"
}

run_avr8_behavior_tests() {
  node "${ROOT}/scripts/examples-verify-report-test.mjs"
  node "${ROOT}/scripts/examples-verify-supervisor-test.mjs"
}

run_avr9_behavior_tests() {
  node "${ROOT}/scripts/examples-verify-benchmark-test.mjs"
  node "${ROOT}/scripts/examples-verify-scheduler-test.mjs" --bin "${NIMBUS_EXAMPLES_VERIFY_BIN:-${ROOT}/target/debug/nimbus}"
}

usage() {
  printf 'usage: %s --task AVR3..AVR10 | --condition AVRC11..AVRC24 | --self-test-condition AVRC11..AVRC24\n' "$0" >&2
}

case "${1:-}" in
  --task)
    [ "$#" -eq 2 ] || { usage; exit 2; }
    for id in AVRC11 AVRC12 AVRC13 AVRC14 AVRC15 AVRC16 AVRC17 AVRC18 AVRC19 AVRC20 AVRC21 AVRC22 AVRC23 AVRC24; do
      if [ "$(owner_for "${id}")" = "$2" ]; then
        run_condition "${id}"
      fi
    done
    if [ "${selected_count}" -eq 0 ]; then
      printf 'no conditions selected for task %s\n' "$2" >&2
      exit 2
    fi
    if [ "$2" = "AVR3" ]; then
      run_avr3_behavior_tests
    fi
    if [ "$2" = "AVR4" ]; then
      run_avr4_behavior_tests
    fi
    if [ "$2" = "AVR7" ]; then
      run_avr7_behavior_tests
    fi
    if [ "$2" = "AVR8" ]; then
      run_avr8_behavior_tests
    fi
    if [ "$2" = "AVR9" ]; then
      run_avr9_behavior_tests
    fi
    ;;
  --condition)
    [ "$#" -eq 2 ] || { usage; exit 2; }
    run_condition "$2"
    ;;
  --self-test-condition)
    [ "$#" -eq 2 ] || { usage; exit 2; }
    self_test_condition "$2"
    printf 'PASS self-test %s\n' "$2"
    exit 0
    ;;
  *)
    usage
    exit 2
    ;;
esac

printf 'Summary: %d passed, %d failed\n' "${pass_count}" "${fail_count}"
[ "${fail_count}" -eq 0 ]
