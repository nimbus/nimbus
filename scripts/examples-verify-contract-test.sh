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
      has Makefile '^examples-verify:.*\$\(UI_DIST_INDEX\).*\$\(EMBEDDED_PKG_MANIFEST\)' &&
        has scripts/examples-verify.sh 'fresh-checkout prerequisites' &&
        has scripts/examples-verify.sh 'make examples-verify'
      ;;
    AVRC12)
      has scripts/examples-verify.sh 'Node\.js.*22.*24' &&
        has scripts/examples-verify.sh 'unsupported Node' &&
        has scripts/examples-verify.sh 'supplied binary.*skip'
      ;;
    AVRC13)
      [ -f "${ROOT}/scripts/examples-verify-cases.json" ] &&
        node - "${ROOT}/scripts/examples-verify-cases.json" <<'NODE'
const fs = require("fs");
const value = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
if (!Array.isArray(value.cases) || value.cases.length !== 9) process.exit(1);
const required = ["name", "workspace", "appDir", "boot", "smoke", "updateSemantics"];
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
        lacks scripts/examples-verify.sh 'npm run codegen -w "\$\{workspace\}"'
      ;;
    AVRC15)
      has scripts/examples-verify.sh 'capture_source_byte_manifest' &&
        has scripts/examples-verify.sh 'verify_source_byte_manifest'
      ;;
    AVRC16)
      has crates/nimbus-cli/src/dev/tests.rs 'compose_discovery_defaults_to_enabled' &&
        has crates/nimbus-cli/src/dev/tests.rs 'explicit_compose_file_still_loads'
      ;;
    AVRC17)
      has crates/nimbus-cli/src/dev.rs 'no[_-]compose[_-]discovery' &&
        has crates/nimbus-cli/src/dev/tests.rs 'compose_opt_out_performs_no_discovery' &&
        lacks scripts/examples-verify.sh 'mv .*compose\.yaml'
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
        has scripts/examples-verify.sh 'cleanup_failure.*retain'
      ;;
    AVRC21)
      has scripts/examples-verify-report.mjs 'schemaVersion' &&
        has scripts/examples-verify-report.mjs 'redact' &&
        has scripts/examples-verify-report.mjs 'validate'
      ;;
    AVRC22)
      has scripts/examples-verify-report.mjs 'junit' &&
        has scripts/examples-verify-report.mjs 'deterministic' &&
        has scripts/examples-verify-report.mjs 'cleanup'
      ;;
    AVRC23)
      has scripts/examples-verify-benchmark.sh 'serial-samples' &&
        has scripts/examples-verify-benchmark.sh 'parallel-samples' &&
        has scripts/examples-verify-benchmark.sh 'max-seconds' &&
        has scripts/examples-verify.sh 'max_parallel' &&
        has scripts/examples-verify.sh 'drain.*failure'
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
  mkdir -p "${root}/scripts" "${root}/crates/nimbus-cli/src/dev" "${root}/examples/app"
  case "${id}" in
    AVRC11)
      printf '%s\n' "examples-verify: \$(UI_DIST_INDEX) \$(EMBEDDED_PKG_MANIFEST)" >"${root}/Makefile"
      printf '%s\n' '# fresh-checkout prerequisites; use make examples-verify' >"${root}/scripts/examples-verify.sh"
      ;;
    AVRC12)
      printf '%s\n' '# Node.js 22 and 24; unsupported Node; supplied binary can skip build' >"${root}/scripts/examples-verify.sh"
      ;;
    AVRC13)
      node - "${root}/scripts/examples-verify-cases.json" <<'NODE'
const fs = require("fs");
const path = process.argv[2];
const cases = Array.from({length: 9}, (_, i) => ({
  name: `case-${i}`, workspace: `workspace-${i}`, appDir: `app-${i}`,
  boot: {}, smoke: {}, updateSemantics: "push"
}));
fs.writeFileSync(path, JSON.stringify({cases}));
NODE
      ;;
    AVRC14)
      printf '%s\n' '# disposable workspace' 'prepare_case_workspace() { :; }' >"${root}/scripts/examples-verify.sh"
      ;;
    AVRC15)
      printf '%s\n' 'capture_source_byte_manifest() { :; }' 'verify_source_byte_manifest() { :; }' >"${root}/scripts/examples-verify.sh"
      ;;
    AVRC16)
      printf '%s\n' '# compose_discovery_defaults_to_enabled' '# explicit_compose_file_still_loads' >"${root}/crates/nimbus-cli/src/dev/tests.rs"
      ;;
    AVRC17)
      printf '%s\n' '# no-compose-discovery' >"${root}/crates/nimbus-cli/src/dev.rs"
      printf '%s\n' '# compose_opt_out_performs_no_discovery' >"${root}/crates/nimbus-cli/src/dev/tests.rs"
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
      ;;
    AVRC21)
      printf '%s\n' 'const schemaVersion = 1; function redact() {} function validate() {}' >"${root}/scripts/examples-verify-report.mjs"
      ;;
    AVRC22)
      printf '%s\n' 'const junit = true; // deterministic cleanup projection' >"${root}/scripts/examples-verify-report.mjs"
      ;;
    AVRC23)
      printf '%s\n' '# --serial-samples --parallel-samples --max-seconds' >"${root}/scripts/examples-verify-benchmark.sh"
      printf '%s\n' 'max_parallel=4' '# drain workers after failure' >"${root}/scripts/examples-verify.sh"
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
    AVRC23) printf '%s\n' '# serial only' >"${root}/scripts/examples-verify-benchmark.sh" ;;
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
