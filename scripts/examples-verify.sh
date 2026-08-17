#!/usr/bin/env bash
# Boot each examples/ app against a fresh local server and run its smoke.
#
# Every app in the manifest below is verified independently and sequentially:
# build the nimbus binary once, then for each app, optionally run its client
# codegen script first (strictly before boot — running it against a live
# server races the server's own watcher-free boot preflight, see the "3b"
# follow-up recorded in the plan), boot the server on a fresh --data-dir,
# wait for /health, run the smoke, and stop the server before moving to the
# next app.
#
# Every app boots via `nimbus start`, which performs no Compose
# auto-discovery by default (the --compose-file doc comment in
# crates/nimbus-cli/src/start/mod.rs: that is `nimbus dev`/`nimbus
# compose`-only), so it does not need the compose.yaml sideline workaround
# `nimbus dev` boots require elsewhere in this plan — except firebase/tasks
# (see boot_mode below), which genuinely needs `nimbus dev` and so pays that
# cost for just the one app.
#
# The convex/tasks step also exercises the `nimbus run functions` process
# stdio contract: spawn the real binary, capture stdout and stderr to
# separate files, and assert stdout alone parses as the result JSON while
# the resolved-target banner lands on stderr. It passes an explicit TARGET
# URL rather than omitting TARGET — see check_run_stdio_contract() below for
# why that is required, not a style choice.

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

# A fixed port (8080 was the previous default) risks a pre-existing,
# unrelated local server already answering /health on that port — the lane
# would then read green without ever exercising the binary under test. Bind
# to an OS-assigned ephemeral port per run instead (same pattern as
# scripts/nimbus-kv-conformance.sh); NIMBUS_EXAMPLES_VERIFY_PORT still
# overrides it for anyone who wants a fixed port.
PORT="${NIMBUS_EXAMPLES_VERIFY_PORT:-$(python3 - <<'PY'
import socket
sock = socket.socket()
sock.bind(("127.0.0.1", 0))
print(sock.getsockname()[1])
sock.close()
PY
)}"
NIMBUS_URL="http://127.0.0.1:${PORT}"
DATA_ROOT="$(mktemp -d -t nimbus-examples-verify.XXXXXX)"
CASE_MANIFEST="${REPO_ROOT}/scripts/examples-verify-cases.json"
WORKSPACE_ADAPTER="${REPO_ROOT}/scripts/examples-verify-workspace.mjs"
SOURCE_BYTE_SNAPSHOT="${DATA_ROOT}/source-bytes.before.json"
CASE_ROWS="${DATA_ROOT}/cases.pipe"

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

SERVER_PID=""
SERVER_LOG=""

cleanup_server() {
  if [ -n "${SERVER_PID}" ] && kill -0 "${SERVER_PID}" 2>/dev/null; then
    kill "${SERVER_PID}" 2>/dev/null || true
    wait "${SERVER_PID}" 2>/dev/null || true
  fi
  SERVER_PID=""
}

capture_source_byte_manifest() {
  node "${WORKSPACE_ADAPTER}" capture-source \
    --manifest "${CASE_MANIFEST}" \
    --repo-root "${REPO_ROOT}" \
    --output "${SOURCE_BYTE_SNAPSHOT}"
}

verify_source_byte_manifest() {
  node "${WORKSPACE_ADAPTER}" verify-source \
    --manifest "${CASE_MANIFEST}" \
    --repo-root "${REPO_ROOT}" \
    --snapshot "${SOURCE_BYTE_SNAPSHOT}"
}

finalize_examples_verification() {
  local run_status=$?
  local final_status="${run_status}"
  trap - EXIT
  cleanup_server || final_status=1
  if ! verify_source_byte_manifest; then
    final_status=1
  fi
  exit "${final_status}"
}

capture_source_byte_manifest
trap finalize_examples_verification EXIT

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
  local port="$1"
  for _ in $(seq 1 60); do
    if ! kill -0 "${SERVER_PID}" 2>/dev/null; then
      echo "server process (pid ${SERVER_PID}) for port ${port} exited before becoming healthy" >&2
      return 1
    fi
    if curl -fsS "http://127.0.0.1:${port}/health" 2>/dev/null | grep -Eq '"ok"[[:space:]]*:[[:space:]]*true'; then
      # The port was ephemeral-assigned to be free at bind-test time, but a
      # TOCTOU race (or a leftover process on an operator-pinned
      # NIMBUS_EXAMPLES_VERIFY_PORT) could still let something other than
      # our own launched binary answer /health. Re-check the pid right
      # after a successful curl: if it's already gone, the response did not
      # come from the server this run just launched — treat that as a hard
      # failure rather than a silent pass.
      if ! kill -0 "${SERVER_PID}" 2>/dev/null; then
        echo "health check on port ${port} succeeded but pid ${SERVER_PID} is no longer running — a different process answered /health on this port, not the binary under test" >&2
        return 1
      fi
      return 0
    fi
    sleep 0.5
  done
  return 1
}

# Populates the global ENV_ARGS array with "KEY=VAL" elements from a
# comma-separated list, suitable for `env "${ENV_ARGS[@]}" <command>`.
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
  local app_dir="$1" data_dir="$2" boot_flags="$3" boot_env="$4" needs_app_dir_boot="$5" boot_mode="$6"
  build_flag_args "${boot_flags}"
  local extra_flag=(${FLAG_ARGS[@]+"${FLAG_ARGS[@]}"})
  if [ "${needs_app_dir_boot}" = "1" ]; then
    extra_flag+=("--app-dir" "${app_dir}")
  fi
  build_env_args "${boot_env}"
  local subcommand=(start)
  if [ "${boot_mode}" = "dev" ]; then
    subcommand=(dev --no-open --once)
  fi
  # The `${ARR[@]+"${ARR[@]}"}` form (rather than a bare `"${ARR[@]}"`) is
  # required because bash 3.2 (macOS system bash) treats expanding an empty
  # array under `set -u` as an unbound-variable error; this form guards it.
  env ${ENV_ARGS[@]+"${ENV_ARGS[@]}"} "${NIMBUS_BIN}" "${subcommand[@]}" \
    --port "${PORT}" \
    --data-dir "${data_dir}" \
    ${extra_flag[@]+"${extra_flag[@]}"} \
    >"${SERVER_LOG}" 2>&1 &
  SERVER_PID=$!
  local health_status=0
  wait_for_health "${PORT}" || health_status=$?
  if [ "${health_status}" -ne 0 ]; then
    echo "server for ${app_dir} did not become healthy; log:" >&2
    cat "${SERVER_LOG}" >&2
    return 1
  fi
}

CASE_APP_DIR=""
prepare_case_workspace() {
  local name="$1" workspace="$2"
  CASE_APP_DIR="${DATA_ROOT}/workspaces/${workspace}"
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
  if [ -n "${SERVER_PID}" ]; then
    kill "${SERVER_PID}" 2>/dev/null || true
    wait "${SERVER_PID}" 2>/dev/null || true
    SERVER_PID=""
  fi
}

# EX5.1's required process-level stdio-contract check: spawn the real
# `nimbus run` binary, capture stdout and stderr to separate files, assert
# stdout alone parses as the result JSON and the resolved-target banner is
# stderr-only. Runs against the already-booted convex/tasks server.
#
# Passes an explicit TARGET URL rather than relying on bare local discovery.
# `nimbus run functions ...` with TARGET omitted resolves through
# LocalDiscovery, which unconditionally attaches the on-disk local admin
# token as a bearer (crates/nimbus-cli/src/local_server_client.rs); the
# server's Convex-style auth verifier then 401s that bearer on every example
# app because none configure convex/auth.config.ts
# (crates/nimbus-convex/src/auth/verifier/identity.rs) — a genuine, systemic
# nimbus-cli defect outside this plan's crates/** ownership, reproduced live
# against both convex/tasks and nimbus/agent-worker. Passing the TARGET URL
# explicitly instead routes through invoke_remote_run_function
# (crates/nimbus-cli/src/run.rs), which sends no Authorization header at
# all; the request lands anonymous, matching how the smoke.ts scripts' own
# REST clients already succeed under the dev-mode anonymous-team bypass.
check_run_stdio_contract() {
  local app_dir="$1" target_url="$2"
  local stdout_file="${DATA_ROOT}/run-stdio-contract.stdout"
  local stderr_file="${DATA_ROOT}/run-stdio-contract.stderr"

  echo "    stdio-contract: nimbus run ${target_url} functions tasks:list"
  if ! "${NIMBUS_BIN}" run "${target_url}" functions tasks:list \
      --app "${app_dir}" --tenant demo \
      >"${stdout_file}" 2>"${stderr_file}"; then
    echo "FAIL stdio-contract: nimbus run functions tasks:list exited non-zero" >&2
    echo "--- stdout ---" >&2
    cat "${stdout_file}" >&2
    echo "--- stderr ---" >&2
    cat "${stderr_file}" >&2
    return 1
  fi

  if ! node -e "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8'))" "${stdout_file}"; then
    echo "FAIL stdio-contract: stdout did not parse as JSON" >&2
    echo "--- stdout ---" >&2
    cat "${stdout_file}" >&2
    return 1
  fi

  if ! grep -q "Running against" "${stderr_file}"; then
    echo "FAIL stdio-contract: resolved-target banner missing from stderr" >&2
    echo "--- stderr ---" >&2
    cat "${stderr_file}" >&2
    return 1
  fi

  if grep -q "Running against" "${stdout_file}"; then
    echo "FAIL stdio-contract: resolved-target banner leaked into stdout" >&2
    return 1
  fi

  echo "PASS stdio-contract: stdout is clean JSON, banner is stderr-only"
}

run_one() {
  local name="$1" workspace="$2" _source_app_dir="$3" needs_codegen="$4" needs_app_dir_boot="$5"
  local boot_env="$6" boot_flags="$7" smoke_env="$8" boot_mode="$9" smoke_command="${10}"
  local stdio_contract="${11}" _update_semantics="${12}"
  echo "==> ${name}"

  prepare_case_workspace "${name}" "${workspace}"
  local app_dir="${CASE_APP_DIR}"
  boot_env="${boot_env//\$\{NIMBUS_URL\}/${NIMBUS_URL}}"
  smoke_env="${smoke_env//\$\{NIMBUS_URL\}/${NIMBUS_URL}}"

  if [ "${needs_codegen}" = "1" ]; then
    echo "    codegen (before boot, avoids the live-server bundle race)"
    (cd "${app_dir}" && npm run codegen)
  fi

  local data_dir
  data_dir="${DATA_ROOT}/$(echo "${workspace}" | tr '/' '-')"
  mkdir -p "${data_dir}"
  SERVER_LOG="${data_dir}.server.log"

  if ! boot_server "${app_dir}" "${data_dir}" "${boot_flags}" "${boot_env}" "${needs_app_dir_boot}" "${boot_mode}"; then
    exit 1
  fi
  if [ "${needs_app_dir_boot}" = "1" ]; then
    refresh_case_dependencies "${app_dir}"
  fi

  local admin_token=""
  admin_token="$("${NIMBUS_BIN}" auth token 2>/dev/null || true)"

  build_env_args "${smoke_env}"
  local smoke_status=0
  if [ "${smoke_command}" = "node" ]; then
    # Codegen already ran above; `npm run smoke` re-chains `codegen &&`,
    # which would redundantly re-trigger client codegen against a server
    # that already read the bundle once at its own boot preflight. Call
    # smoke.ts directly instead.
    (cd "${app_dir}" && env NIMBUS_ADMIN_TOKEN="${admin_token}" ${ENV_ARGS[@]+"${ENV_ARGS[@]}"} \
      node --experimental-strip-types ./smoke.ts) || smoke_status=$?
  else
    (cd "${app_dir}" && env NIMBUS_ADMIN_TOKEN="${admin_token}" ${ENV_ARGS[@]+"${ENV_ARGS[@]}"} \
      npm run smoke) || smoke_status=$?
  fi

  if [ "${smoke_status}" -eq 0 ] && [ "${stdio_contract}" = "1" ]; then
    check_run_stdio_contract "${app_dir}" "http://127.0.0.1:${PORT}" || smoke_status=$?
  fi

  stop_server

  if [ "${smoke_status}" -ne 0 ]; then
    echo "FAIL ${name}" >&2
    # A request-level smoke failure is invisible without the server's side of
    # the story; dump its log tail like the health-failure path already does.
    if [ -f "${SERVER_LOG}" ]; then
      echo "server log tail for ${name}:" >&2
      tail -n 60 "${SERVER_LOG}" >&2
    fi
    exit "${smoke_status}"
  fi
  echo "PASS ${name}"
}

ensure_nimbus_binary
ensure_firebase_protobuf_stubs

# Restrict to a single app by name for local debugging, e.g.
# NIMBUS_EXAMPLES_VERIFY_ONLY=nimbus/tasks bash scripts/examples-verify.sh
ONLY="${NIMBUS_EXAMPLES_VERIFY_ONLY:-}"
ONLY_MATCHED=0

for entry in "${APPS[@]}"; do
  IFS='|' read -r name workspace app_dir needs_codegen needs_app_dir_boot boot_env boot_flags smoke_env boot_mode smoke_command stdio_contract update_semantics <<<"${entry}"
  if [ -n "${ONLY}" ] && [ "${name}" != "${ONLY}" ]; then
    continue
  fi
  ONLY_MATCHED=1
  run_one "${name}" "${workspace}" "${app_dir}" "${needs_codegen}" "${needs_app_dir_boot}" "${boot_env}" "${boot_flags}" "${smoke_env}" "${boot_mode}" "${smoke_command}" "${stdio_contract}" "${update_semantics}"
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

echo "==> all examples verified"
