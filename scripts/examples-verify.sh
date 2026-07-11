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

# Manifest fields, pipe-delimited:
#   name | workspace | app_dir | needs_codegen(0/1) | needs_app_dir_boot(0/1) | boot_env | boot_flags | smoke_env | boot_mode(start|dev)
# needs_app_dir_boot: whether the boot command needs `--app-dir`. Under
# `nimbus start` this means "does this app have a server-side functions
# surface at all" (Convex/native SDK functions, or a Cloud Functions
# bundle) — the plain document-CRUD apps (nimbus/tasks, mongodb/tasks,
# dynamodb/tasks) have no `convex/`/`nimbus/`/`firebase.json` source and
# boot preflight rejects `--app-dir` pointed at them with "No Convex or
# Cloud Functions surface found"; they must boot with no --app-dir at all.
# Under `nimbus dev` (firebase/tasks and cloud-functions/tasks, see
# boot_mode) --app-dir instead drives adapter *detection*; for
# firebase/tasks that computes the Firestore auto-tenant, for
# cloud-functions/tasks it both registers the functions bundle and gets the
# same auto-tenant as a side effect (dev always sets one, defaulting to
# "demo" for any non-Firestore-client adapter).
# boot_env / smoke_env: comma-separated KEY=VAL pairs, or "-" for none. Every
# app whose smoke talks to the main HTTP listener carries an explicit
# NIMBUS_NATIVE_URL/NIMBUS_FIRESTORE_URL/NIMBUS_CLOUD_FUNCTIONS_URL entry
# pinned to ${NIMBUS_URL} (built from the resolved ephemeral PORT above) —
# each smoke.ts's own "http://localhost:8080" fallback default only matched
# by coincidence when PORT was hardcoded to 8080; it does not track PORT now
# that PORT is resolved per run. mongodb/tasks and dynamodb/tasks are
# unaffected: their smokes talk to separate wire-protocol listener ports
# (27017/8000 by default), not the main HTTP PORT.
# boot_flags: a single space-free extra CLI argument (KEY=VAL form), or "-".
# boot_mode: "start" (default, no Compose auto-discovery, no sideline
# needed) or "dev". firebase/tasks and cloud-functions/tasks are "dev"
# apps: both send Firestore REST calls carrying a mock/emulator auth token,
# and Firestore admission requires a cryptographically verified project
# claim (the #24 gate,
# crates/nimbus-firebase/src/project_tenant_registry.rs). The only local
# bypass for that is `nimbus dev`'s auto-tenant handling
# (crates/nimbus-cli/src/dev/plan.rs, unconditionally sets `auto_tenant`,
# which crates/nimbus-cli/src/start/adapters/firebase.rs turns into the
# Firebase emulator-token-verification bypass) — there is no equivalent
# flag on `nimbus start`. This matches both apps' own README-documented
# `nimbus dev` instructions. A "dev" boot pays the compose.yaml sideline
# cost (see sideline_compose/restore_compose below) that "start" boots are
# otherwise designed to avoid.
# nimbus/agent-chat, nimbus/agent-worker, and convex/tasks are all
# `nimbus/`- or `convex/`-source-rooted apps that dispatch through the same
# Convex-style tenancy path, so all three need the EX3.7 dev-mode anonymous
# team envs at boot (EX4.1/EX4.2 established this live for the first two;
# EX3.7d for the third) or every mutation 403s with no bound team.
CONVEX_DEV_TENANCY_ENV="NIMBUS_CONVEX_SILO_TEAMS=demo:demo-team,NIMBUS_CONVEX_ANONYMOUS_TEAM=demo-team"

APPS=(
  "nimbus/tasks|nimbus-tasks|examples/nimbus/tasks|0|0|-|-|NIMBUS_NATIVE_URL=${NIMBUS_URL}|start"
  "nimbus/agent-chat|nimbus-agent-chat|examples/nimbus/agent-chat|1|1|${CONVEX_DEV_TENANCY_ENV}|-|NIMBUS_NATIVE_URL=${NIMBUS_URL}|start"
  "nimbus/agent-worker|nimbus-agent-worker|examples/nimbus/agent-worker|1|1|${CONVEX_DEV_TENANCY_ENV}|-|NIMBUS_NATIVE_URL=${NIMBUS_URL}|start"
  "convex/tasks|convex-tasks|examples/convex/tasks|1|1|${CONVEX_DEV_TENANCY_ENV}|-|NIMBUS_NATIVE_URL=${NIMBUS_URL}|start"
  "firebase/tasks|firebase-tasks|examples/firebase/tasks|0|1|-|-|NIMBUS_FIRESTORE_URL=${NIMBUS_URL}|dev"
  "mongodb/tasks|mongodb-tasks|examples/mongodb/tasks|0|0|NIMBUS_MONGODB_PASSWORD=nimbus|--mongodb-username=nimbus|NIMBUS_MONGODB_USERNAME=nimbus,NIMBUS_MONGODB_PASSWORD=nimbus|start"
  "dynamodb/tasks|dynamodb-tasks|examples/dynamodb/tasks|0|0|-|--dynamodb-access-key=nimbus:nimbus:default|-|start"
  "cloud-functions/tasks|cloud-functions-tasks|examples/cloud-functions/tasks|0|1|-|-|NIMBUS_CLOUD_FUNCTIONS_URL=${NIMBUS_URL}|dev"
)

SERVER_PID=""
SERVER_LOG=""
COMPOSE_SIDELINE_PATH="${REPO_ROOT}/compose.yaml"
COMPOSE_SIDELINED=0

# Sideline/restore compose.yaml around a `nimbus dev` boot (see boot_mode in
# the manifest comment above for why this is only needed for one app).
sideline_compose() {
  if [ -f "${COMPOSE_SIDELINE_PATH}" ]; then
    mv "${COMPOSE_SIDELINE_PATH}" "${COMPOSE_SIDELINE_PATH}.smoke-bak"
    COMPOSE_SIDELINED=1
  fi
}

restore_compose() {
  if [ "${COMPOSE_SIDELINED}" = "1" ]; then
    mv "${COMPOSE_SIDELINE_PATH}.smoke-bak" "${COMPOSE_SIDELINE_PATH}"
    COMPOSE_SIDELINED=0
  fi
}

cleanup_server() {
  if [ -n "${SERVER_PID}" ] && kill -0 "${SERVER_PID}" 2>/dev/null; then
    kill "${SERVER_PID}" 2>/dev/null || true
    wait "${SERVER_PID}" 2>/dev/null || true
  fi
  SERVER_PID=""
  # Defense in depth: restore compose.yaml even on an unexpected abort
  # mid-boot, so a failed run never leaves the repo in a sidelined state.
  restore_compose
}
trap cleanup_server EXIT

# SIGKILL bypasses the EXIT trap above, so a prior run killed mid-`dev`-boot
# (e.g. an operator's Ctrl-\, a CI job timeout using SIGKILL, or a `kill -9`)
# can leave compose.yaml.smoke-bak on disk with compose.yaml missing. A
# checkout in that state silently changes the meaning of every subsequent
# `nimbus dev`/`compose` invocation in the repo, not just this lane. Heal it
# at lane start, before anything else runs, rather than only guarding the
# happy-path exit. The real fix is the recorded product follow-up (an
# opt-out so `nimbus dev` skips Compose auto-discovery for a plain example
# boot instead of needing this sideline dance at all — see the
# "compose-auto-discovery-on-app-boot DX question" follow-up in
# docs/private/plans/examples-and-target-resolution-plan.md); this is a
# lane-local recovery, not that fix.
heal_stranded_compose_sideline() {
  if [ -f "${COMPOSE_SIDELINE_PATH}.smoke-bak" ] && [ ! -f "${COMPOSE_SIDELINE_PATH}" ]; then
    echo "==> found compose.yaml.smoke-bak with no compose.yaml (a prior run was likely killed mid-boot); restoring compose.yaml before proceeding"
    mv "${COMPOSE_SIDELINE_PATH}.smoke-bak" "${COMPOSE_SIDELINE_PATH}"
  fi
}
heal_stranded_compose_sideline

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

wait_for_health() {
  local port="$1"
  local attempt
  for attempt in $(seq 1 60); do
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

boot_server() {
  local app_dir="$1" data_dir="$2" boot_flags="$3" boot_env="$4" needs_app_dir_boot="$5" boot_mode="$6"
  local extra_flag=()
  if [ "${boot_flags}" != "-" ]; then
    extra_flag=("${boot_flags}")
  fi
  if [ "${needs_app_dir_boot}" = "1" ]; then
    extra_flag+=("--app-dir" "${app_dir}")
  fi
  build_env_args "${boot_env}"
  local subcommand=(start)
  if [ "${boot_mode}" = "dev" ]; then
    sideline_compose
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
  # Restore compose.yaml as soon as the boot has resolved (health or
  # failure) rather than holding it sidelined for the smoke's whole
  # duration — the sideline only needs to cover the compose
  # auto-discovery window at startup.
  if [ "${boot_mode}" = "dev" ]; then
    restore_compose
  fi
  if [ "${health_status}" -ne 0 ]; then
    echo "server for ${app_dir} did not become healthy; log:" >&2
    cat "${SERVER_LOG}" >&2
    return 1
  fi
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
  local name="$1" workspace="$2" app_dir="$3" needs_codegen="$4" needs_app_dir_boot="$5"
  local boot_env="$6" boot_flags="$7" smoke_env="$8" boot_mode="$9"
  echo "==> ${name}"

  if [ "${needs_codegen}" = "1" ]; then
    echo "    codegen (before boot, avoids the live-server bundle race)"
    npm run codegen -w "${workspace}"
  fi

  local data_dir="${DATA_ROOT}/$(echo "${workspace}" | tr '/' '-')"
  mkdir -p "${data_dir}"
  SERVER_LOG="${data_dir}.server.log"

  if ! boot_server "${app_dir}" "${data_dir}" "${boot_flags}" "${boot_env}" "${needs_app_dir_boot}" "${boot_mode}"; then
    exit 1
  fi

  local admin_token=""
  admin_token="$("${NIMBUS_BIN}" auth token 2>/dev/null || true)"

  build_env_args "${smoke_env}"
  local smoke_status=0
  if [ "${needs_codegen}" = "1" ]; then
    # Codegen already ran above; `npm run smoke` re-chains `codegen &&`,
    # which would redundantly re-trigger client codegen against a server
    # that already read the bundle once at its own boot preflight. Call
    # smoke.ts directly instead.
    (cd "${app_dir}" && env NIMBUS_ADMIN_TOKEN="${admin_token}" ${ENV_ARGS[@]+"${ENV_ARGS[@]}"} \
      node --experimental-strip-types ./smoke.ts) || smoke_status=$?
  else
    (env NIMBUS_ADMIN_TOKEN="${admin_token}" ${ENV_ARGS[@]+"${ENV_ARGS[@]}"} \
      npm run smoke -w "${workspace}") || smoke_status=$?
  fi

  if [ "${smoke_status}" -eq 0 ] && [ "${name}" = "convex/tasks" ]; then
    check_run_stdio_contract "${app_dir}" "http://127.0.0.1:${PORT}" || smoke_status=$?
  fi

  stop_server

  if [ "${smoke_status}" -ne 0 ]; then
    echo "FAIL ${name}" >&2
    exit "${smoke_status}"
  fi
  echo "PASS ${name}"
}

ensure_nimbus_binary

# Restrict to a single app by name for local debugging, e.g.
# NIMBUS_EXAMPLES_VERIFY_ONLY=nimbus/tasks bash scripts/examples-verify.sh
ONLY="${NIMBUS_EXAMPLES_VERIFY_ONLY:-}"
ONLY_MATCHED=0

for entry in "${APPS[@]}"; do
  IFS='|' read -r name workspace app_dir needs_codegen needs_app_dir_boot boot_env boot_flags smoke_env boot_mode <<<"${entry}"
  if [ -n "${ONLY}" ] && [ "${name}" != "${ONLY}" ]; then
    continue
  fi
  ONLY_MATCHED=1
  run_one "${name}" "${workspace}" "${app_dir}" "${needs_codegen}" "${needs_app_dir_boot}" "${boot_env}" "${boot_flags}" "${smoke_env}" "${boot_mode}"
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
