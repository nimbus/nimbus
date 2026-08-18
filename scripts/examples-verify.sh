#!/usr/bin/env bash
# Boot each examples/ app against a fresh local server and run its smoke.
#
# Every app in the validated manifest is independent. The bounded scheduler
# builds Nimbus once. Each case can then run client codegen before boot, start a
# server with fresh state, wait for health, run smoke assertions, and stop its
# owned process. Codegen never runs against a live server for the same app.
#
# The manifest selects `nimbus start` for main-listener cases and `nimbus dev`
# for framework provisioning or generated wire credentials. Every dev case
# passes the explicit Compose-discovery opt-out. All listeners use product
# provider-assigned leases; the runner learns the main endpoint from exact
# case-local discovery and wire endpoints from Nimbus-owned `.env.local` keys.
#
# The convex/tasks step also exercises the `nimbus run functions` process
# contract through explicit and bare-local target resolution. Both forms must
# return the same result JSON on stdout, keep their banners on stderr, and
# preserve the silo and local-admin trust boundaries.

# Each case process intentionally owns isolated copies of the server and report
# lifecycle variables. The parent must not observe those per-case mutations.
# shellcheck disable=SC2030,SC2031

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

NIMBUS_BIN="${NIMBUS_EXAMPLES_VERIFY_BIN:-${REPO_ROOT}/target/debug/nimbus}"
NIMBUS_BIN_SUPPLIED=0
if [ -n "${NIMBUS_EXAMPLES_VERIFY_BIN:-}" ]; then
  NIMBUS_BIN_SUPPLIED=1
fi

# Node.js >=22 <25 is the supported range. Node.js 22 and 24 are the tested
# anchors. Reject an unsupported Node host before port allocation, temporary
# state, generated prerequisites, Cargo, npm, or any application process.
require_supported_node() {
  local node_version node_major
  if ! command -v node >/dev/null 2>&1; then
    echo "Node.js >=22 <25 is required (tested on Node.js 22 and 24); node was not found" >&2
    return 1
  fi
  if ! node_version="$(node --version 2>/dev/null)"; then
    echo "Node.js >=22 <25 is required (tested on Node.js 22 and 24); node --version failed" >&2
    return 1
  fi
  node_major="${node_version#v}"
  node_major="${node_major%%.*}"
  case "${node_major}" in
    ''|*[!0-9]*)
      echo "unsupported Node.js version ${node_version}; require Node.js >=22 <25 (tested on Node.js 22 and 24)" >&2
      return 1
      ;;
  esac
  if [ "${node_major}" -lt 22 ] || [ "${node_major}" -ge 25 ]; then
    echo "unsupported Node.js version ${node_version}; require Node.js >=22 <25 (tested on Node.js 22 and 24)" >&2
    return 1
  fi
}

# A supplied binary can skip generated build prerequisites and the Rust build,
# but it must be an executable before the Make entry starts any generation.
require_supplied_binary() {
  if [ "${NIMBUS_BIN_SUPPLIED}" = "1" ] && [ ! -x "${NIMBUS_BIN}" ]; then
    echo "supplied binary is missing or not executable: ${NIMBUS_BIN}" >&2
    return 1
  fi
}

run_host_preflight() {
  require_supported_node
  require_supplied_binary
  if [ "${NIMBUS_BIN_SUPPLIED}" = "1" ]; then
    printf 'supplied binary %s is executable; skip generated build prerequisites and the Rust build\n' "${NIMBUS_BIN}"
  fi
}

run_host_preflight

case "$#" in
  0) ;;
  1)
    if [ "$1" = "--host-preflight" ]; then
      exit 0
    fi
    echo "unknown argument: $1" >&2
    exit 2
    ;;
  *)
    echo "usage: $0 [--host-preflight]" >&2
    exit 2
    ;;
esac

# Direct invocation cannot generate a buildable binary from a tracked-files-
# only checkout. Fail before work with the canonical Make recovery command.
# The Make entry owns these fresh-checkout prerequisites under single-flight.
require_fresh_checkout_prerequisites() {
  local missing=()
  if [ -x "${NIMBUS_BIN}" ]; then
    return
  fi
  if [ ! -f "${REPO_ROOT}/packages/nimbus-ui/dist/index.html" ]; then
    missing+=("packages/nimbus-ui/dist/index.html")
  fi
  if [ ! -f "${REPO_ROOT}/crates/nimbus-assets/embedded/packages/manifest.json" ]; then
    missing+=("crates/nimbus-assets/embedded/packages/manifest.json")
  fi
  if [ "${#missing[@]}" -eq 0 ]; then
    return
  fi
  echo "fresh-checkout prerequisites are missing for direct script invocation:" >&2
  local path
  for path in "${missing[@]}"; do
    echo "  ${path}" >&2
  done
  echo "run the supported entry point: make examples-verify" >&2
  return 1
}

require_fresh_checkout_prerequisites

CASE_MANIFEST="${REPO_ROOT}/scripts/examples-verify-cases.json"
WORKSPACE_ADAPTER="${REPO_ROOT}/scripts/examples-verify-workspace.mjs"
LIFETIME_ADAPTER="${REPO_ROOT}/scripts/examples-verify-lifetime.mjs"
PROCESS_SUPERVISOR="${REPO_ROOT}/scripts/examples-verify-supervisor.mjs"
REPORT_ADAPTER="${REPO_ROOT}/scripts/examples-verify-report.mjs"
RESULTS_ROOT="${NIMBUS_EXAMPLES_VERIFY_RESULTS_DIR:-${REPO_ROOT}/target/examples-verify-results}"
if [[ "${RESULTS_ROOT}" != /* ]]; then
  echo "NIMBUS_EXAMPLES_VERIFY_RESULTS_DIR must be an absolute path: ${RESULTS_ROOT}" >&2
  exit 2
fi
ONLY="${NIMBUS_EXAMPLES_VERIFY_ONLY:-}"
MAX_PARALLEL="${NIMBUS_EXAMPLES_VERIFY_MAX_PARALLEL:-1}"
case "${MAX_PARALLEL}" in
  ''|*[!0-9]*)
    echo "NIMBUS_EXAMPLES_VERIFY_MAX_PARALLEL must be an integer from 1 through 9" >&2
    exit 2
    ;;
esac
if [ "${MAX_PARALLEL}" -lt 1 ] || [ "${MAX_PARALLEL}" -gt 9 ]; then
  echo "NIMBUS_EXAMPLES_VERIFY_MAX_PARALLEL must be an integer from 1 through 9" >&2
  exit 2
fi
CREDENTIAL_DELETE_FAILURES_REMAINING="${NIMBUS_EXAMPLES_VERIFY_CREDENTIAL_DELETE_FAILURES:-0}"
case "${CREDENTIAL_DELETE_FAILURES_REMAINING}" in
  ''|*[!0-9]*)
    echo "NIMBUS_EXAMPLES_VERIFY_CREDENTIAL_DELETE_FAILURES must be a non-negative integer" >&2
    exit 2
    ;;
esac
if [ -n "${ONLY}" ]; then
  node "${REPORT_ADAPTER}" validate-selection \
    --manifest "${CASE_MANIFEST}" \
    --repo-root "${REPO_ROOT}" \
    --only "${ONLY}"
fi
RUN_ROW="$(node "${LIFETIME_ADAPTER}" create-run --repo-root "${REPO_ROOT}")"
IFS='|' read -r DATA_ROOT NETWORK_STATE_ROOT ARTIFACT_ROOT <<<"${RUN_ROW}"
SOURCE_BYTE_SNAPSHOT="${DATA_ROOT}/source-bytes.before.json"
SOURCE_BYTE_OBSERVED="${DATA_ROOT}/source-bytes.after.json"
CASE_ROWS="${DATA_ROOT}/cases.pipe"
REPORT_ONLY_ARGS=()
if [ -n "${ONLY}" ]; then
  REPORT_ONLY_ARGS=("--only" "${ONLY}")
fi
REPORT_ROOT=""
if ! REPORT_ROOT="$(node "${REPORT_ADAPTER}" init \
    --manifest "${CASE_MANIFEST}" \
    --repo-root "${REPO_ROOT}" \
    --run-root "${DATA_ROOT}" \
    --results-root "${RESULTS_ROOT}" \
    --binary "${NIMBUS_BIN}" \
    ${REPORT_ONLY_ARGS[@]+"${REPORT_ONLY_ARGS[@]}"})"; then
  node "${LIFETIME_ADAPTER}" finalize \
    --repo-root "${REPO_ROOT}" \
    --run-root "${DATA_ROOT}" \
    --artifact-root "${ARTIFACT_ROOT}" \
    --run-status 0 \
    --cleanup-status 0 \
    >/dev/null
  exit 1
fi

SOURCE_BYTE_CAPTURED=0
SERVER_PID=""
SERVER_URL=""
SERVER_LOG=""
SERVER_RECORD=""
SERVER_DISCOVERY_PATH=""
SERVER_ADMIN_TOKEN=""
SMOKE_ENV_FILE=""
CURRENT_CASE_NAME=""
CURRENT_CASE_SMOKE_LOG=""
CURRENT_CASE_RECORDED=0
CASE_WORKER_PIDS=()

# The product owns each provider_assigned_port_lease and retained_listener.
# This runner owns only the surrounding process, case roots, evidence, and
# cancellation lifetime; it never scans, closes, or reallocates a port.
cleanup_server() {
  local cleanup_status=0
  # A just-spawned Nimbus process can reserve durable listener authority before
  # it installs its signal handler. Give bootstrap one bounded second before
  # TERM so an immediate fault cut cannot strand that process-bound lease.
  local pre_signal_timeout_ms=1000
  if [ -z "${SERVER_RECORD}" ]; then
    return 0
  fi

  # An immediate post-spawn fault can run cleanup before the normal readiness
  # path has learned the endpoint and token. Recover those values from the
  # same case-local discovery and authentication roots when startup completes
  # within a bounded window. This keeps shutdown graceful without introducing
  # another socket or credential authority.
  if [ -z "${SERVER_URL}" ] && [ -n "${SERVER_DISCOVERY_PATH}" ] && [ -n "${SERVER_PID}" ]; then
    local discovered_url=""
    for _ in $(seq 1 40); do
      if ! node "${PROCESS_SUPERVISOR}" status --record "${SERVER_RECORD}"; then
        break
      fi
      if discovered_url="$(node "${LIFETIME_ADAPTER}" read-discovery \
          --path "${SERVER_DISCOVERY_PATH}" --pid "${SERVER_PID}" 2>/dev/null)" && \
          curl -fsS "${discovered_url}/health" >/dev/null 2>&1; then
        SERVER_URL="${discovered_url}"
        break
      fi
      sleep 0.05
    done
  fi
  if [ -n "${SERVER_URL}" ] && [ -z "${SERVER_ADMIN_TOKEN}" ]; then
    SERVER_ADMIN_TOKEN="$(node "${PROCESS_SUPERVISOR}" exec \
      --cwd "${REPO_ROOT}" \
      --clear-prefix NIMBUS_ \
      ${CASE_ENV_FLAGS[@]+"${CASE_ENV_FLAGS[@]}"} \
      -- "${NIMBUS_BIN}" auth token 2>/dev/null)" || SERVER_ADMIN_TOKEN=""
  fi

  if [ -n "${SERVER_URL}" ] && [ -n "${SERVER_ADMIN_TOKEN}" ] && \
      node "${PROCESS_SUPERVISOR}" status --record "${SERVER_RECORD}"; then
    if ! printf '%s' "${SERVER_ADMIN_TOKEN}" | \
        node "${LIFETIME_ADAPTER}" shutdown --url "${SERVER_URL}"; then
      echo "server did not accept graceful shutdown; applying the owned process-group fallback" >&2
    else
      pre_signal_timeout_ms=10000
    fi
  fi
  if ! node "${PROCESS_SUPERVISOR}" stop --record "${SERVER_RECORD}" \
      --pre-signal-timeout-ms "${pre_signal_timeout_ms}"; then
    cleanup_status=1
  fi
  if [ "${cleanup_status}" -ne 0 ]; then
    return "${cleanup_status}"
  fi
  if [ -n "${SERVER_DISCOVERY_PATH}" ]; then
    rm -f "${SERVER_DISCOVERY_PATH}"
  fi
  SERVER_PID=""
  SERVER_URL=""
  SERVER_LOG=""
  SERVER_RECORD=""
  SERVER_DISCOVERY_PATH=""
  SERVER_ADMIN_TOKEN=""
  return 0
}

cleanup_smoke_credentials() {
  if [ -z "${SMOKE_ENV_FILE}" ]; then
    return 0
  fi
  if [ "${CREDENTIAL_DELETE_FAILURES_REMAINING}" -gt 0 ]; then
    CREDENTIAL_DELETE_FAILURES_REMAINING=$((CREDENTIAL_DELETE_FAILURES_REMAINING - 1))
    echo "injected smoke credential deletion failure" >&2
    return 1
  fi
  if [ -e "${SMOKE_ENV_FILE}" ] && ! : >"${SMOKE_ENV_FILE}"; then
    echo "could not scrub the smoke credential file" >&2
    return 1
  fi
  if ! rm -f "${SMOKE_ENV_FILE}"; then
    echo "could not remove the smoke credential file" >&2
    return 1
  fi
  SMOKE_ENV_FILE=""
  return 0
}

capture_source_byte_manifest() {
  if ! node "${WORKSPACE_ADAPTER}" capture-source \
      --manifest "${CASE_MANIFEST}" \
      --repo-root "${REPO_ROOT}" \
      --output "${SOURCE_BYTE_SNAPSHOT}"; then
    return 1
  fi
  SOURCE_BYTE_CAPTURED=1
}

verify_source_byte_manifest() {
  node "${WORKSPACE_ADAPTER}" verify-source \
    --manifest "${CASE_MANIFEST}" \
    --repo-root "${REPO_ROOT}" \
    --snapshot "${SOURCE_BYTE_SNAPSHOT}" \
    --observed-output "${SOURCE_BYTE_OBSERVED}"
}

record_current_case() {
  local status="$1" exit_code="$2" cleanup_status="$3" endpoint="${4:-}"
  if [ -z "${CURRENT_CASE_NAME}" ] || [ "${CURRENT_CASE_RECORDED}" -eq 1 ]; then
    return 0
  fi
  local endpoint_args=()
  if [ -n "${endpoint}" ]; then
    endpoint_args=("--endpoint" "${endpoint}")
  fi
  node "${REPORT_ADAPTER}" record-case \
    --result-root "${REPORT_ROOT}" \
    --case "${CURRENT_CASE_NAME}" \
    --status "${status}" \
    --exit-code "${exit_code}" \
    --cleanup-status "${cleanup_status}" \
    --smoke-log "${CURRENT_CASE_SMOKE_LOG}" \
    ${endpoint_args[@]+"${endpoint_args[@]}"}
  CURRENT_CASE_RECORDED=1
  CURRENT_CASE_NAME=""
  CURRENT_CASE_SMOKE_LOG=""
}

finalize_case_process() {
  local case_status=$?
  local final_status="${case_status}"
  local cleanup_status=0
  local cleanup_report="passed"
  trap - EXIT INT TERM
  cleanup_server || cleanup_status=$?
  if ! cleanup_smoke_credentials; then
    cleanup_status=1
  fi
  if [ "${cleanup_status}" -ne 0 ]; then
    cleanup_report="failed"
    final_status=1
  fi
  if [ -n "${CURRENT_CASE_NAME}" ] && [ "${CURRENT_CASE_RECORDED}" -eq 0 ]; then
    record_current_case "failed" "${case_status}" "${cleanup_report}" || final_status=1
  fi
  exit "${final_status}"
}

run_case_process() (
  trap finalize_case_process EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM
  SERVER_PID=""
  SERVER_URL=""
  SERVER_LOG=""
  SERVER_RECORD=""
  SERVER_DISCOVERY_PATH=""
  SERVER_ADMIN_TOKEN=""
  SMOKE_ENV_FILE=""
  CURRENT_CASE_NAME=""
  CURRENT_CASE_SMOKE_LOG=""
  CURRENT_CASE_RECORDED=0
  IFS='|' read -r name workspace app_dir needs_codegen needs_app_dir_boot boot_env boot_flags smoke_env boot_mode smoke_command stdio_contract update_semantics surfaces <<<"$1"
  run_one "${name}" "${workspace}" "${app_dir}" "${needs_codegen}" "${needs_app_dir_boot}" "${boot_env}" "${boot_flags}" "${smoke_env}" "${boot_mode}" "${smoke_command}" "${stdio_contract}" "${update_semantics}" "${surfaces}"
)

ACTIVE_CASE_PID=""
WORKER_SIGNAL_STATUS=0
on_worker_signal() {
  local signal_status="$1"
  trap - INT TERM
  WORKER_SIGNAL_STATUS="${signal_status}"
}

case_log_path() {
  local entry="$1" name
  IFS='|' read -r name _ <<<"${entry}"
  printf '%s/%s.log\n' "${SCHEDULER_LOG_ROOT}" "${name//\//-}"
}

claim_scheduler_failure() {
  local name="$1" status="$2"
  if mkdir "${SCHEDULER_FAILURE_ROOT}" 2>/dev/null; then
    printf '%s\n' "${name}|${status}" >"${SCHEDULER_FAILURE_ROOT}/result.pipe"
  fi
}

CLAIMED_ENTRY=""
claim_next_scheduled_case() {
  local index claim_path
  CLAIMED_ENTRY=""
  for ((index = 0; index < ${#SCHEDULED_APPS[@]}; index += 1)); do
    claim_path="${SCHEDULER_CLAIM_ROOT}/${index}"
    if mkdir "${claim_path}" 2>/dev/null; then
      if [ -d "${SCHEDULER_FAILURE_ROOT}" ]; then
        return 1
      fi
      CLAIMED_ENTRY="${SCHEDULED_APPS[${index}]}"
      return 0
    fi
  done
  return 1
}

run_worker_slot() {
  local entry name log_path case_status
  ACTIVE_CASE_PID=""
  WORKER_SIGNAL_STATUS=0
  trap 'on_worker_signal 130' INT
  trap 'on_worker_signal 143' TERM
  while true; do
    if [ -d "${SCHEDULER_FAILURE_ROOT}" ]; then
      return 0
    fi
    if ! claim_next_scheduled_case; then
      return 0
    fi
    entry="${CLAIMED_ENTRY}"
    IFS='|' read -r name _ <<<"${entry}"
    log_path="$(case_log_path "${entry}")"
    run_case_process "${entry}" >"${log_path}" 2>&1 &
    ACTIVE_CASE_PID=$!
    case_status=0
    while kill -0 "${ACTIVE_CASE_PID}" 2>/dev/null; do
      case_status=0
      wait "${ACTIVE_CASE_PID}" || case_status=$?
    done
    ACTIVE_CASE_PID=""
    if [ "${WORKER_SIGNAL_STATUS}" -ne 0 ]; then
      return "${WORKER_SIGNAL_STATUS}"
    fi
    if [ "${case_status}" -ne 0 ]; then
      claim_scheduler_failure "${name}" "${case_status}"
      return "${case_status}"
    fi
  done
}

stop_case_workers() {
  local pid status cleanup_status=0
  for pid in ${CASE_WORKER_PIDS[@]+"${CASE_WORKER_PIDS[@]}"}; do
    kill -TERM "${pid}" 2>/dev/null || true
  done
  for pid in ${CASE_WORKER_PIDS[@]+"${CASE_WORKER_PIDS[@]}"}; do
    status=0
    wait "${pid}" 2>/dev/null || status=$?
    case "${status}" in
      0|130|143) ;;
      *) cleanup_status=1 ;;
    esac
  done
  CASE_WORKER_PIDS=()
  return "${cleanup_status}"
}

run_scheduled_cases() {
  local worker_count="${MAX_PARALLEL}" slot pid worker_status=0 entry log_path failure_status=1
  if [ "${worker_count}" -gt "${#SELECTED_APPS[@]}" ]; then
    worker_count="${#SELECTED_APPS[@]}"
  fi
  mkdir -p "${SCHEDULER_LOG_ROOT}" "${SCHEDULER_CLAIM_ROOT}"
  for ((slot = 0; slot < worker_count; slot += 1)); do
    (run_worker_slot) &
    CASE_WORKER_PIDS+=("$!")
  done
  for pid in ${CASE_WORKER_PIDS[@]+"${CASE_WORKER_PIDS[@]}"}; do
    wait "${pid}" || worker_status=$?
  done
  CASE_WORKER_PIDS=()
  for entry in "${SELECTED_APPS[@]}"; do
    log_path="$(case_log_path "${entry}")"
    if [ -f "${log_path}" ]; then
      cat "${log_path}"
    fi
  done
  if [ -f "${SCHEDULER_FAILURE_ROOT}/result.pipe" ]; then
    IFS='|' read -r _ failure_status <"${SCHEDULER_FAILURE_ROOT}/result.pipe"
    return "${failure_status}"
  fi
  return "${worker_status}"
}

finalize_examples_verification() {
  local run_status=$?
  local final_status="${run_status}"
  local cleanup_status=0
  local lifetime_status=0
  local lifetime_json=""
  local report_status=0
  local source_status="matched"
  local case_cleanup_report="passed"
  trap - EXIT INT TERM
  if [ "${#CASE_WORKER_PIDS[@]}" -gt 0 ] && ! stop_case_workers; then
    cleanup_status=1
  fi
  cleanup_server || cleanup_status=$?
  if ! cleanup_smoke_credentials; then
    cleanup_status=1
  fi
  if [ "${cleanup_status}" -ne 0 ]; then
    case_cleanup_report="failed"
  fi
  if [ -n "${CURRENT_CASE_NAME}" ] && [ "${CURRENT_CASE_RECORDED}" -eq 0 ]; then
    record_current_case "failed" "${run_status}" "${case_cleanup_report}" || final_status=1
  fi
  if [ "${SOURCE_BYTE_CAPTURED}" -eq 0 ]; then
    capture_source_byte_manifest || final_status=1
  fi
  if ! verify_source_byte_manifest; then
    source_status="mismatched"
    final_status=1
  fi
  if ! node "${REPORT_ADAPTER}" stage-source \
      --result-root "${REPORT_ROOT}" \
      --source-before "${SOURCE_BYTE_SNAPSHOT}" \
      --source-after "${SOURCE_BYTE_OBSERVED}" \
      --source-status "${source_status}"; then
    final_status=1
  fi
  # A cleanup_failure must retain the original root and keep the run red.
  if lifetime_json="$(node "${LIFETIME_ADAPTER}" finalize \
      --repo-root "${REPO_ROOT}" \
      --run-root "${DATA_ROOT}" \
      --artifact-root "${ARTIFACT_ROOT}" \
      --run-status "${final_status}" \
      --cleanup-status "${cleanup_status}")"; then
    lifetime_status=0
  else
    lifetime_status=$?
  fi
  if ! printf '%s' "${lifetime_json}" | node "${REPORT_ADAPTER}" finalize \
      --result-root "${REPORT_ROOT}" \
      --run-exit-code "${final_status}"; then
    report_status=1
  fi
  if [ "${lifetime_status}" -ne 0 ]; then
    final_status="${lifetime_status}"
  fi
  if [ "${report_status}" -ne 0 ]; then
    final_status="${report_status}"
  fi
  exit "${final_status}"
}

trap finalize_examples_verification EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

FAULT_CUT="${NIMBUS_EXAMPLES_VERIFY_FAULT_CUT:-}"
FAULT_CASE="${NIMBUS_EXAMPLES_VERIFY_FAULT_CASE:-}"
case "${FAULT_CUT}" in
  ""|after-run-root|after-case-root|after-server-spawn|after-server-ready|during-smoke|before-server-stop) ;;
  *)
    echo "unknown NIMBUS_EXAMPLES_VERIFY_FAULT_CUT=${FAULT_CUT}" >&2
    exit 2
    ;;
esac
fail_at_cut() {
  if [ "${FAULT_CUT}" = "$1" ] && \
      { [ -z "${FAULT_CASE}" ] || [ "${FAULT_CASE}" = "${CURRENT_CASE_NAME}" ]; }; then
    echo "injected examples verification fault at $1" >&2
    return 97
  fi
}
fail_at_cut after-run-root

# The validated manifest owns the nine application identities, declared source
# inputs, boot behavior, smoke behavior, surfaces, and update semantics. The
# runner copies only those inputs to a disposable workspace before codegen,
# provisioning, boot, or smoke can write.
if ! node "${WORKSPACE_ADAPTER}" emit-shell \
    --manifest "${CASE_MANIFEST}" \
    --repo-root "${REPO_ROOT}" \
    >"${CASE_ROWS}"; then
  exit 1
fi
APPS=()
while IFS= read -r entry; do
  APPS+=("${entry}")
done <"${CASE_ROWS}"
if [ "${#APPS[@]}" -ne 9 ]; then
  echo "case manifest emitted ${#APPS[@]} rows; expected 9" >&2
  exit 1
fi

capture_source_byte_manifest

ensure_nimbus_binary() {
  if [ -x "${NIMBUS_BIN}" ]; then
    return
  fi
  printf 'nimbus binary not found at %s; building nimbus-bin\n' "${NIMBUS_BIN}"
  cargo build -p nimbus-bin --bin nimbus
  if [ ! -x "${NIMBUS_BIN}" ]; then
    printf 'built nimbus-bin, but %s is still missing or not executable\n' "${NIMBUS_BIN}" >&2
    exit 1
  fi
}

# firebase/tasks depends on "firebase": "file:./.nimbus/packages/firebase",
# which `nimbus dev` provisions back into packages/firebase in this
# monorepo. That package's src/internal/protobuf.ts imports generated
# protobuf stubs under packages/firebase/src/gen/, which are gitignored
# build output (see packages/firebase/package.json's codegen:proto script)
# and are only ever produced as a side effect of running `npm run build` (or
# `codegen:proto` directly) for the firebase workspace. Plain `npm ci` does
# not generate them, so a fresh checkout's firebase/tasks smoke fails with
# ERR_MODULE_NOT_FOUND unless something upstream happened to run the
# firebase package's build (e.g. `npm run build:embedded-packages`) first.
# Generate them here so this script is self-contained rather than depending
# on an incidental prior step.
ensure_firebase_protobuf_stubs() {
  local gen_marker="${REPO_ROOT}/packages/firebase/src/gen/google/firestore/v1/firestore_pb.ts"
  if [ -f "${gen_marker}" ]; then
    return
  fi
  printf 'firebase protobuf stubs not found at %s; running codegen:proto\n' "${gen_marker}"
  npm run codegen:proto -w firebase
  if [ ! -f "${gen_marker}" ]; then
    printf 'ran codegen:proto for firebase, but %s is still missing\n' "${gen_marker}" >&2
    exit 1
  fi
}

wait_for_health() {
  local discovered_url=""
  for _ in $(seq 1 60); do
    if ! node "${PROCESS_SUPERVISOR}" status --record "${SERVER_RECORD}"; then
      echo "server process (pid ${SERVER_PID}) exited before becoming healthy" >&2
      return 1
    fi
    if discovered_url="$(node "${LIFETIME_ADAPTER}" read-discovery \
        --path "${SERVER_DISCOVERY_PATH}" --pid "${SERVER_PID}" 2>/dev/null)" && \
        curl -fsS "${discovered_url}/health" 2>/dev/null | \
          grep -Eq '"ok"[[:space:]]*:[[:space:]]*true'; then
      SERVER_URL="${discovered_url}"
      return 0
    fi
    sleep 0.5
  done
  return 1
}

# Populates the global ENV_ARGS array with "KEY=VAL" elements from a
# comma-separated manifest field.
# Pass "-" for an empty array.
ENV_ARGS=()
build_env_args() {
  ENV_ARGS=()
  local list="$1"
  if [ "${list}" = "-" ]; then
    return
  fi
  local old_ifs="${IFS}"
  IFS=','
  local pair
  for pair in ${list}; do
    ENV_ARGS+=("${pair}")
  done
  IFS="${old_ifs}"
}

COMMAND_ENV_FLAGS=()
build_command_env_flags() {
  build_env_args "$1"
  COMMAND_ENV_FLAGS=()
  local pair
  for pair in ${ENV_ARGS[@]+"${ENV_ARGS[@]}"}; do
    COMMAND_ENV_FLAGS+=("--env" "${pair}")
  done
}

FLAG_ARGS=()
build_flag_args() {
  FLAG_ARGS=()
  local list="$1"
  if [ "${list}" = "-" ]; then
    return
  fi
  local old_ifs="${IFS}"
  IFS=','
  local flag
  for flag in ${list}; do
    FLAG_ARGS+=("${flag}")
  done
  IFS="${old_ifs}"
}

boot_server() {
  local app_dir="$1" boot_flags="$2" boot_env="$3" needs_app_dir_boot="$4" boot_mode="$5" surfaces="$6"
  build_flag_args "${boot_flags}"
  local extra_flag=(${FLAG_ARGS[@]+"${FLAG_ARGS[@]}"})
  if [ "${needs_app_dir_boot}" = "1" ]; then
    extra_flag+=("--app-dir" "${app_dir}")
  fi
  build_command_env_flags "${boot_env}"
  local subcommand=(start)
  if [ "${boot_mode}" = "dev" ]; then
    subcommand=(dev --no-open --once)
  else
    case ",${surfaces}," in *",mongodb-wire,"*) ;; *) extra_flag+=("--no-mongodb") ;; esac
    case ",${surfaces}," in *",dynamodb-wire,"*) ;; *) extra_flag+=("--no-dynamodb") ;; esac
    case ",${surfaces}," in *",s3-wire,"*) ;; *) extra_flag+=("--no-s3") ;; esac
    case ",${surfaces}," in *",firestore-rest,"*) ;; *) extra_flag+=("--no-firestore") ;; esac
    case ",${surfaces}," in *",cloudflare-http,"*) ;; *) extra_flag+=("--no-cloudflare") ;; esac
  fi
  # The `${ARR[@]+"${ARR[@]}"}` form (rather than a bare `"${ARR[@]}"`) is
  # required because bash 3.2 (macOS system bash) treats expanding an empty
  # array under `set -u` as an unbound-variable error; this form guards it.
  SERVER_RECORD="${case_process_root}/server.json"
  SERVER_DISCOVERY_PATH="${case_discovery_path}"
  SERVER_LOG="${case_log_root}/server.log"
  local spawn_status=0
  SERVER_PID="$(node "${PROCESS_SUPERVISOR}" spawn \
    --record "${SERVER_RECORD}" \
    --log "${SERVER_LOG}" \
    --clear-prefix NIMBUS_ \
    ${CASE_ENV_FLAGS[@]+"${CASE_ENV_FLAGS[@]}"} \
    ${COMMAND_ENV_FLAGS[@]+"${COMMAND_ENV_FLAGS[@]}"} \
    -- "${NIMBUS_BIN}" "${subcommand[@]}" \
    --port 0 \
    --data-dir "${case_data_root}" \
    --control-data-dir "${case_control_root}" \
    --network-state-dir "${NETWORK_STATE_ROOT}" \
    ${extra_flag[@]+"${extra_flag[@]}"} )" || spawn_status=$?
  if [ "${spawn_status}" -ne 0 ]; then
    return "${spawn_status}"
  fi
  fail_at_cut after-server-spawn || return $?
  local health_status=0
  wait_for_health || health_status=$?
  if [ "${health_status}" -ne 0 ]; then
    echo "server for ${app_dir} did not become healthy; log:" >&2
    cat "${SERVER_LOG}" >&2
    return 1
  fi
  SERVER_ADMIN_TOKEN="$(node "${PROCESS_SUPERVISOR}" exec \
    --cwd "${REPO_ROOT}" \
    --clear-prefix NIMBUS_ \
    ${CASE_ENV_FLAGS[@]+"${CASE_ENV_FLAGS[@]}"} \
    -- "${NIMBUS_BIN}" auth token 2>/dev/null)"
  if [ -z "${SERVER_ADMIN_TOKEN}" ]; then
    echo "server for ${app_dir} did not create a case-local admin token" >&2
    return 1
  fi
  fail_at_cut after-server-ready || return $?
}

CASE_APP_DIR=""
CASE_ENV_FLAGS=()
create_case_context() {
  local name="$1" workspace="$2" case_row
  case_row="$(node "${LIFETIME_ADAPTER}" create-case \
    --repo-root "${REPO_ROOT}" \
    --run-root "${DATA_ROOT}" \
    --artifact-root "${ARTIFACT_ROOT}" \
    --name "${name}" \
    --workspace "${workspace}")"
  IFS='|' read -r _case_root case_home_root case_auth_root case_discovery_root \
    case_discovery_path case_audit_root case_config_root case_windows_root \
    case_app_root case_data_root case_control_root case_log_root \
    case_result_root case_process_root <<<"${case_row}"
  CASE_APP_DIR="${case_app_root}"
  CASE_ENV_FLAGS=(
    "--env" "HOME=${case_home_root}"
    "--env" "TMPDIR=${case_discovery_root}"
    "--env" "XDG_CONFIG_HOME=${case_config_root}"
    "--env" "XDG_DATA_HOME=${case_auth_root}"
    "--env" "XDG_STATE_HOME=${case_audit_root}"
    "--env" "XDG_RUNTIME_DIR=${case_discovery_root}"
    "--env" "LOCALAPPDATA=${case_windows_root}"
    "--env" "USERPROFILE=${case_home_root}"
    "--env" "NIMBUS_NETWORK_STATE_DIR=${NETWORK_STATE_ROOT}"
    "--env" "NIMBUS_DATA_DIR=${case_data_root}"
    "--env" "NIMBUS_CONTROL_DATA_DIR=${case_control_root}"
  )
}

prepare_case_workspace() {
  local name="$1" workspace="$2"
  create_case_context "${name}" "${workspace}"
  node "${WORKSPACE_ADAPTER}" prepare \
    --manifest "${CASE_MANIFEST}" \
    --repo-root "${REPO_ROOT}" \
    --case "${name}" \
    --destination "${CASE_APP_DIR}" \
    >/dev/null
}

refresh_case_dependencies() {
  local app_dir="$1"
  node "${WORKSPACE_ADAPTER}" refresh-dependencies \
    --destination "${app_dir}" \
    >/dev/null
}

stop_server() {
  cleanup_server
}

# Spawn the real `nimbus run` binary through explicit and bare-local target
# resolution. Each stdout must contain only result JSON. Each banner stays on
# stderr, and both target forms must return the same value.
check_run_stdio_contract() {
  local app_dir="$1" target_url="$2"
  local explicit_stdout="${case_result_root}/run-stdio-contract.explicit.stdout"
  local explicit_stderr="${case_result_root}/run-stdio-contract.explicit.stderr"
  local local_stdout="${case_result_root}/run-stdio-contract.local.stdout"
  local local_stderr="${case_result_root}/run-stdio-contract.local.stderr"
  local wrong_silo_stdout="${case_result_root}/run-stdio-contract.wrong-silo.stdout"
  local wrong_silo_stderr="${case_result_root}/run-stdio-contract.wrong-silo.stderr"
  local invalid_auth_body="${case_result_root}/run-stdio-contract.invalid-auth.json"

  echo "    stdio-contract: nimbus run ${target_url} functions tasks:list"
  if ! node "${PROCESS_SUPERVISOR}" exec --cwd "${app_dir}" \
      --clear-prefix NIMBUS_ ${CASE_ENV_FLAGS[@]+"${CASE_ENV_FLAGS[@]}"} \
      -- "${NIMBUS_BIN}" run "${target_url}" functions tasks:list \
      --app "${app_dir}" --tenant demo \
      >"${explicit_stdout}" 2>"${explicit_stderr}"; then
    echo "FAIL stdio-contract: explicit nimbus run exited non-zero" >&2
    echo "--- stdout ---" >&2
    cat "${explicit_stdout}" >&2
    echo "--- stderr ---" >&2
    cat "${explicit_stderr}" >&2
    return 1
  fi

  echo "    stdio-contract: nimbus run functions tasks:list (local discovery)"
  if ! node "${PROCESS_SUPERVISOR}" exec --cwd "${app_dir}" \
      --clear-prefix NIMBUS_ ${CASE_ENV_FLAGS[@]+"${CASE_ENV_FLAGS[@]}"} \
      -- "${NIMBUS_BIN}" run functions tasks:list \
      --app "${app_dir}" --tenant demo \
      >"${local_stdout}" 2>"${local_stderr}"; then
    echo "FAIL stdio-contract: bare-local nimbus run exited non-zero" >&2
    echo "--- stdout ---" >&2
    cat "${local_stdout}" >&2
    echo "--- stderr ---" >&2
    cat "${local_stderr}" >&2
    return 1
  fi

  if ! node -e '
const fs = require("fs");
const assert = require("assert");
const explicit = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
const local = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
assert.deepStrictEqual(local, explicit);
' "${explicit_stdout}" "${local_stdout}"; then
    echo "FAIL stdio-contract: explicit and bare-local JSON results differ" >&2
    return 1
  fi

  local stdout_file stderr_file
  for stdout_file in "${explicit_stdout}" "${local_stdout}"; do
    if grep -q "Running against" "${stdout_file}"; then
      echo "FAIL stdio-contract: resolved-target banner leaked into ${stdout_file}" >&2
      return 1
    fi
  done
  for stderr_file in "${explicit_stderr}" "${local_stderr}"; do
    if ! grep -q "Running against" "${stderr_file}"; then
      echo "FAIL stdio-contract: resolved-target banner missing from ${stderr_file}" >&2
      return 1
    fi
  done

  if node "${PROCESS_SUPERVISOR}" exec --cwd "${app_dir}" \
      --clear-prefix NIMBUS_ ${CASE_ENV_FLAGS[@]+"${CASE_ENV_FLAGS[@]}"} \
      -- "${NIMBUS_BIN}" run "${target_url}" functions tasks:list \
      --app "${app_dir}" --tenant avr6-wrong-silo \
      >"${wrong_silo_stdout}" 2>"${wrong_silo_stderr}"; then
    echo "FAIL stdio-contract: an explicit target selected an unprovisioned silo" >&2
    return 1
  fi
  if [ -s "${wrong_silo_stdout}" ]; then
    echo "FAIL stdio-contract: wrong-silo refusal wrote result data to stdout" >&2
    cat "${wrong_silo_stdout}" >&2
    return 1
  fi

  local invalid_auth_status
  invalid_auth_status="$(curl -sS -o "${invalid_auth_body}" -w '%{http_code}' \
    -H 'content-type: application/json' \
    -H 'authorization: Bearer invalid.avr6.application.credential' \
    --data '{"name":"tasks:list","args":{}}' \
    "${target_url}/convex/demo/query")"
  if [ "${invalid_auth_status}" != "401" ]; then
    echo "FAIL stdio-contract: invalid application credential returned HTTP ${invalid_auth_status}, expected 401" >&2
    cat "${invalid_auth_body}" >&2
    return 1
  fi

  echo "PASS stdio-contract: target forms match; stdio is clean; wrong silo and invalid application auth fail closed"
}

write_smoke_env_file() {
  local name="$1" app_dir="$2" generated=""
  generated="$(node "${WORKSPACE_ADAPTER}" emit-generated-env \
    --manifest "${CASE_MANIFEST}" \
    --repo-root "${REPO_ROOT}" \
    --case "${name}" \
    --destination "${app_dir}")"
  case "${SERVER_ADMIN_TOKEN}" in
    *$'\n'*|*$'\r'*)
      echo "case-local admin token contains an invalid line break" >&2
      return 1
      ;;
  esac
  SMOKE_ENV_FILE="${case_auth_root}/smoke.env"
  (
    umask 077
    {
      printf 'NIMBUS_ADMIN_TOKEN=%s\n' "${SERVER_ADMIN_TOKEN}"
      if [ -n "${generated}" ]; then
        printf '%s\n' "${generated}"
      fi
    } >"${SMOKE_ENV_FILE}"
  )
  chmod 600 "${SMOKE_ENV_FILE}"
}

run_one() {
  local name="$1" workspace="$2" _source_app_dir="$3" needs_codegen="$4" needs_app_dir_boot="$5"
  local boot_env="$6" boot_flags="$7" smoke_env="$8" boot_mode="$9" smoke_command="${10}"
  local stdio_contract="${11}" _update_semantics="${12}" surfaces="${13}"
  echo "==> ${name}"

  node "${REPORT_ADAPTER}" begin-case --result-root "${REPORT_ROOT}" --case "${name}"
  CURRENT_CASE_NAME="${name}"
  CURRENT_CASE_RECORDED=0
  prepare_case_workspace "${name}" "${workspace}"
  CURRENT_CASE_SMOKE_LOG="${case_result_root}/smoke.stdout.log"
  fail_at_cut after-case-root
  local app_dir="${CASE_APP_DIR}"

  if [ "${needs_codegen}" = "1" ]; then
    echo "    codegen (before boot, avoids the live-server bundle race)"
    node "${PROCESS_SUPERVISOR}" exec --cwd "${app_dir}" \
      --clear-prefix NIMBUS_ ${CASE_ENV_FLAGS[@]+"${CASE_ENV_FLAGS[@]}"} \
      -- npm run codegen
  fi

  if ! boot_server "${app_dir}" "${boot_flags}" "${boot_env}" "${needs_app_dir_boot}" "${boot_mode}" "${surfaces}"; then
    exit 1
  fi
  if [ "${needs_app_dir_boot}" = "1" ]; then
    refresh_case_dependencies "${app_dir}"
  fi

  smoke_env="${smoke_env//\$\{NIMBUS_URL\}/${SERVER_URL}}"
  build_command_env_flags "${smoke_env}"
  write_smoke_env_file "${name}" "${app_dir}"
  fail_at_cut during-smoke
  local smoke_status=0
  local case_cleanup_status="passed"
  if [ "${smoke_command}" = "node" ]; then
    # Codegen already ran above; `npm run smoke` re-chains `codegen &&`,
    # which would redundantly re-trigger client codegen against a server
    # that already read the bundle once at its own boot preflight. Call
    # smoke.ts directly instead.
    node "${PROCESS_SUPERVISOR}" exec --cwd "${app_dir}" \
      --clear-prefix NIMBUS_ \
      ${CASE_ENV_FLAGS[@]+"${CASE_ENV_FLAGS[@]}"} \
      --env-file "${SMOKE_ENV_FILE}" \
      --stdout-log "${CURRENT_CASE_SMOKE_LOG}" \
      --tee-stdout \
      ${COMMAND_ENV_FLAGS[@]+"${COMMAND_ENV_FLAGS[@]}"} \
      -- node --experimental-strip-types ./smoke.ts || smoke_status=$?
  else
    node "${PROCESS_SUPERVISOR}" exec --cwd "${app_dir}" \
      --clear-prefix NIMBUS_ \
      ${CASE_ENV_FLAGS[@]+"${CASE_ENV_FLAGS[@]}"} \
      --env-file "${SMOKE_ENV_FILE}" \
      --stdout-log "${CURRENT_CASE_SMOKE_LOG}" \
      --tee-stdout \
      ${COMMAND_ENV_FLAGS[@]+"${COMMAND_ENV_FLAGS[@]}"} \
      -- npm run smoke || smoke_status=$?
  fi
  if ! cleanup_smoke_credentials; then
    echo "FAIL ${name}: smoke credentials did not settle" >&2
    smoke_status=1
    case_cleanup_status="failed"
  fi

  if [ "${smoke_status}" -eq 0 ] && [ "${stdio_contract}" = "1" ]; then
    check_run_stdio_contract "${app_dir}" "${SERVER_URL}" || smoke_status=$?
  fi

  local completed_server_log="${SERVER_LOG}"
  local completed_server_url="${SERVER_URL}"
  fail_at_cut before-server-stop
  if ! stop_server; then
    smoke_status=1
    case_cleanup_status="failed"
  fi

  if [ "${smoke_status}" -eq 0 ]; then
    record_current_case "passed" 0 "${case_cleanup_status}" "${completed_server_url}"
  else
    record_current_case "failed" "${smoke_status}" "${case_cleanup_status}" "${completed_server_url}"
  fi

  if [ "${smoke_status}" -ne 0 ]; then
    echo "FAIL ${name}" >&2
    # A request-level smoke failure is invisible without the server's side of
    # the story; dump its log tail like the health-failure path already does.
    if [ -f "${completed_server_log}" ]; then
      echo "server log tail for ${name}:" >&2
      tail -n 60 "${completed_server_log}" >&2
    fi
    exit "${smoke_status}"
  fi
  echo "PASS ${name}"
}

ensure_nimbus_binary
ensure_firebase_protobuf_stubs

# Restrict to a single app by name for local debugging, e.g.
# NIMBUS_EXAMPLES_VERIFY_ONLY=nimbus/tasks bash scripts/examples-verify.sh
ONLY_MATCHED=0
SELECTED_APPS=()
SCHEDULED_LONG_APPS=()
SCHEDULED_MEDIUM_APPS=()
SCHEDULED_SHORT_APPS=()

for entry in "${APPS[@]}"; do
  IFS='|' read -r name workspace app_dir needs_codegen needs_app_dir_boot boot_env boot_flags smoke_env boot_mode smoke_command stdio_contract update_semantics surfaces <<<"${entry}"
  if [ -n "${ONLY}" ] && [ "${name}" != "${ONLY}" ]; then
    continue
  fi
  ONLY_MATCHED=1
  SELECTED_APPS+=("${entry}")
  if [ "${needs_codegen}" = "1" ] || [[ ",${surfaces}," = *",cloud-functions-http,"* ]]; then
    SCHEDULED_LONG_APPS+=("${entry}")
  elif [ "${boot_mode}" = "dev" ]; then
    SCHEDULED_MEDIUM_APPS+=("${entry}")
  else
    SCHEDULED_SHORT_APPS+=("${entry}")
  fi
done
SCHEDULED_APPS=()
for entry in ${SCHEDULED_LONG_APPS[@]+"${SCHEDULED_LONG_APPS[@]}"}; do
  SCHEDULED_APPS+=("${entry}")
done
for entry in ${SCHEDULED_MEDIUM_APPS[@]+"${SCHEDULED_MEDIUM_APPS[@]}"}; do
  SCHEDULED_APPS+=("${entry}")
done
for entry in ${SCHEDULED_SHORT_APPS[@]+"${SCHEDULED_SHORT_APPS[@]}"}; do
  SCHEDULED_APPS+=("${entry}")
done

# An ONLY value that matches nothing used to fall straight through the loop
# body zero times and exit 0 from the script's last statement, reporting
# "all examples verified" without ever booting a single app — a silent
# false green for a typo'd app name. Fail loudly instead, and name the valid
# selectors so the fix is obvious.
if [ -n "${ONLY}" ] && [ "${ONLY_MATCHED}" -eq 0 ]; then
  echo "NIMBUS_EXAMPLES_VERIFY_ONLY=${ONLY} matched no app in the manifest; valid names are:" >&2
  for entry in "${APPS[@]}"; do
    IFS='|' read -r name _ <<<"${entry}"
    echo "  ${name}" >&2
  done
  exit 1
fi

SCHEDULER_LOG_ROOT="${DATA_ROOT}/scheduler-logs"
SCHEDULER_FAILURE_ROOT="${DATA_ROOT}/scheduler-failure"
SCHEDULER_CLAIM_ROOT="${DATA_ROOT}/scheduler-claims"
echo "==> scheduling ${#SELECTED_APPS[@]} applications with max_parallel=${MAX_PARALLEL}"
run_scheduled_cases

echo "==> all examples verified"
