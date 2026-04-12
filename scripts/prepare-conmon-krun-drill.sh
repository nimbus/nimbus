#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: prepare-conmon-krun-drill.sh --bundle-dir <path> [options]

Prepare a reproducible conmon -> patched-crun -> krun drill layout without
running the stack yet. The helper creates deterministic state directories and
operator-facing scripts so a supported Linux host can execute the same flow and
record the same evidence paths every time.

options:
  --bundle-dir <path>      OCI bundle directory (must already contain config.json)
  --state-root <path>      Root directory for pid/log/exit/persist state
                           (default: ${TMPDIR:-/tmp}/neovex-conmon-drill)
  --container-id <id>      Container ID for conmon/crun (default: neovex-krun-probe)
  --name <name>            Human-readable container name (default: container ID)
  --conmon <path>          conmon binary path to embed in the run script
                           (default: conmon)
  --runtime <path>         private runtime path for conmon -r
                           (default: /usr/libexec/neovex/crun)
  --log-level <level>      conmon log level (default: debug)
  --command-file <path>    Override the generated run script path
  --terminal               Include -t in the generated conmon command
  -h, --help               Show this help

examples:
  bash scripts/prepare-conmon-krun-drill.sh \
    --bundle-dir /tmp/neovex-krun-probe \
    --state-root /tmp/neovex-conmon-drill

  bash scripts/prepare-conmon-krun-drill.sh \
    --bundle-dir /tmp/neovex-krun-probe \
    --state-root /tmp/neovex-conmon-drill \
    --container-id neovex-http \
    --name neovex-http \
    --conmon /usr/bin/conmon \
    --runtime /usr/libexec/neovex/crun
EOF
}

resolve_existing_dir() {
  local dir_path="$1"

  if [[ ! -d "${dir_path}" ]]; then
    echo "directory not found: ${dir_path}" >&2
    exit 66
  fi

  (
    cd "${dir_path}"
    pwd
  )
}

resolve_dir_path() {
  local dir_path="$1"

  mkdir -p "${dir_path}"
  (
    cd "${dir_path}"
    pwd
  )
}

resolve_file_path() {
  local file_path="$1"
  local parent_dir=""
  local base_name=""

  parent_dir="$(dirname "${file_path}")"
  base_name="$(basename "${file_path}")"
  mkdir -p "${parent_dir}"

  if [[ "${file_path}" == /* ]]; then
    printf '%s\n' "${file_path}"
    return 0
  fi

  (
    cd "${parent_dir}"
    printf '%s/%s\n' "$(pwd)" "${base_name}"
  )
}

bundle_dir=""
state_root="${TMPDIR:-/tmp}/neovex-conmon-drill"
container_id="neovex-krun-probe"
container_name=""
conmon_path="conmon"
runtime_path="/usr/libexec/neovex/crun"
log_level="debug"
command_file=""
terminal=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --bundle-dir)
      bundle_dir="${2:-}"
      shift 2
      ;;
    --state-root)
      state_root="${2:-}"
      shift 2
      ;;
    --container-id)
      container_id="${2:-}"
      shift 2
      ;;
    --name)
      container_name="${2:-}"
      shift 2
      ;;
    --conmon)
      conmon_path="${2:-}"
      shift 2
      ;;
    --runtime)
      runtime_path="${2:-}"
      shift 2
      ;;
    --log-level)
      log_level="${2:-}"
      shift 2
      ;;
    --command-file)
      command_file="${2:-}"
      shift 2
      ;;
    --terminal)
      terminal=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 64
      ;;
  esac
done

if [[ -z "${bundle_dir}" ]]; then
  usage >&2
  exit 64
fi

if [[ -z "${container_name}" ]]; then
  container_name="${container_id}"
fi

bundle_dir="$(resolve_existing_dir "${bundle_dir}")"
bundle_config="${bundle_dir}/config.json"
if [[ ! -f "${bundle_config}" ]]; then
  echo "bundle config not found: ${bundle_config}" >&2
  exit 66
fi

state_root="$(resolve_dir_path "${state_root}")"
container_state_dir="${state_root}/containers/${container_id}"
exit_dir="${state_root}/exits"
persist_dir="${state_root}/persist/${container_id}"

mkdir -p "${container_state_dir}" "${exit_dir}" "${persist_dir}"

ctr_log="${container_state_dir}/ctr.log"
oci_log="${container_state_dir}/oci.log"
pidfile="${container_state_dir}/pidfile"
conmon_pidfile="${container_state_dir}/conmon.pid"
exit_status_file="${exit_dir}/${container_id}"

if [[ -z "${command_file}" ]]; then
  command_file="${container_state_dir}/run-conmon.sh"
fi
command_file="$(resolve_file_path "${command_file}")"

start_container_script="${container_state_dir}/start-container.sh"
find_attach_sockets_script="${container_state_dir}/find-attach-sockets.sh"
capture_process_tree_script="${container_state_dir}/capture-process-tree.sh"
wait_for_exit_script="${container_state_dir}/wait-for-exit.sh"
show_exit_status_script="${container_state_dir}/show-exit-status.sh"
graceful_stop_script="${container_state_dir}/graceful-stop.sh"
force_stop_script="${container_state_dir}/force-stop.sh"
metadata_file="${container_state_dir}/drill.env"

command=(
  "${conmon_path}"
  "--api-version" "1"
  "-c" "${container_id}"
  "-u" "${container_id}"
  "-r" "${runtime_path}"
  "-b" "${bundle_dir}"
  "-p" "${pidfile}"
  "-n" "${container_name}"
  "--exit-dir" "${exit_dir}"
  "--persist-dir" "${persist_dir}"
  "--full-attach"
  "-l" "k8s-file:${ctr_log}"
  "--log-level" "${log_level}"
  "--syslog"
  "--conmon-pidfile" "${conmon_pidfile}"
  "--runtime-arg" "--log-format=json"
  "--runtime-arg" "--log"
  "--runtime-arg" "${oci_log}"
)

if [[ "${terminal}" -eq 1 ]]; then
  command+=("-t")
fi

{
  printf '%s\n' '#!/usr/bin/env bash'
  printf '%s\n' 'set -euo pipefail'
  printf '%s\n' ''
  printf '%s\n' '# The krun handler writes .krun_config.json to the rootfs via openat2 during'
  printf '%s\n' '# crun create.  In rootless mode this requires a user namespace with UID 0'
  printf '%s\n' '# mapped to the real user.  Re-exec under buildah unshare if needed.'
  printf '%s\n' 'if [[ "$(id -u)" != "0" ]] && command -v buildah >/dev/null 2>&1; then'
  printf '%s\n' '  exec buildah unshare -- "$0" "$@"'
  printf '%s\n' 'fi'
  printf '%s\n' ''
  printf 'exec '
  printf '%q ' "${command[@]}"
  printf '\n'
} > "${command_file}"
chmod 0755 "${command_file}"

# start-container.sh: waits for crun to reach 'created' state then calls
# 'crun start' to boot the krun VM.  Conmon with --full-attach holds
# crun start until something connects to the attach socket.  This script
# calls crun start directly, which is the same mechanism Podman uses via
# the start pipe / sync pipe handshake.
{
  printf '%s\n' '#!/usr/bin/env bash'
  printf '%s\n' 'set -euo pipefail'
  printf '%s\n' ''
  printf '%s\n' '# Re-exec under buildah unshare for rootless, same as run-conmon.sh.'
  printf '%s\n' 'if [[ "$(id -u)" != "0" ]] && command -v buildah >/dev/null 2>&1; then'
  printf '%s\n' '  exec buildah unshare -- "$0" "$@"'
  printf '%s\n' 'fi'
  printf '%s\n' ''
  printf '%s\n' 'timeout_seconds="${1:-30}"'
  printf '%s\n' 'deadline=$((SECONDS + timeout_seconds))'
  printf '%s\n' ''
  printf '%s\n' '# Wait for the container to reach "created" state.'
  printf '%s\n' 'while (( SECONDS <= deadline )); do'
  printf '  status="$(%q state %q 2>/dev/null | python3 -c "import json,sys; print(json.load(sys.stdin).get('"'"'status'"'"','"'"''"'"'))" 2>/dev/null || true)"\n' "${runtime_path}" "${container_id}"
  printf '%s\n' '  if [[ "${status}" == "created" ]]; then'
  printf '%s\n' '    break'
  printf '%s\n' '  fi'
  printf '%s\n' '  sleep 0.5'
  printf '%s\n' 'done'
  printf '%s\n' ''
  printf '%s\n' 'if [[ "${status}" != "created" ]]; then'
  printf '  echo "container %s did not reach created state within ${timeout_seconds}s (status=${status})" >&2\n' "${container_id}"
  printf '%s\n' '  exit 1'
  printf '%s\n' 'fi'
  printf '%s\n' ''
  printf '%q start %q\n' "${runtime_path}" "${container_id}"
  printf 'echo "start.container_id=%s"\n' "${container_id}"
  printf 'echo "start.status=started"\n'
} > "${start_container_script}"
chmod 0755 "${start_container_script}"

{
  printf '%s\n' '#!/usr/bin/env bash'
  printf '%s\n' 'set -euo pipefail'
  printf '%s\n' ''
  printf 'find %q -type s -print | sort\n' "${persist_dir}"
} > "${find_attach_sockets_script}"
chmod 0755 "${find_attach_sockets_script}"

{
  printf '%s\n' '#!/usr/bin/env bash'
  printf '%s\n' 'set -euo pipefail'
  printf '%s\n' ''
  printf 'conmon_pid="$(cat %q)"\n' "${conmon_pidfile}"
  printf 'runtime_pid="$(cat %q)"\n' "${pidfile}"
  printf '%s\n' "ps -ax -o pid=,ppid=,command= | awk -v conmon=\"\${conmon_pid}\" -v runtime=\"\${runtime_pid}\" '\$1 == conmon || \$1 == runtime || \$2 == conmon || \$2 == runtime { print }'"
} > "${capture_process_tree_script}"
chmod 0755 "${capture_process_tree_script}"

{
  printf '%s\n' '#!/usr/bin/env bash'
  printf '%s\n' 'set -euo pipefail'
  printf '%s\n' ''
  printf '%s\n' 'timeout_seconds="${1:-30}"'
  printf '%s\n' 'deadline=$((SECONDS + timeout_seconds))'
  printf '%s\n' ''
  printf '%s\n' 'while (( SECONDS <= deadline )); do'
  printf '  if [[ -f %q ]]; then\n' "${exit_status_file}"
  printf '    cat %q\n' "${exit_status_file}"
  printf '%s\n' '    exit 0'
  printf '%s\n' '  fi'
  printf '%s\n' '  sleep 1'
  printf '%s\n' 'done'
  printf '%s\n' ''
  printf 'echo "exit status file not created within ${timeout_seconds}s: %s" >&2\n' "${exit_status_file}"
  printf '%s\n' 'exit 1'
} > "${wait_for_exit_script}"
chmod 0755 "${wait_for_exit_script}"

{
  printf '%s\n' '#!/usr/bin/env bash'
  printf '%s\n' 'set -euo pipefail'
  printf '%s\n' ''
  printf 'cat %q\n' "${exit_status_file}"
} > "${show_exit_status_script}"
chmod 0755 "${show_exit_status_script}"

{
  printf '%s\n' '#!/usr/bin/env bash'
  printf '%s\n' 'set -euo pipefail'
  printf '%s\n' ''
  printf 'runtime_pid="$(cat %q)"\n' "${pidfile}"
  printf '%s\n' 'kill -TERM "${runtime_pid}"'
  printf 'bash %q "${1:-30}"\n' "${wait_for_exit_script}"
} > "${graceful_stop_script}"
chmod 0755 "${graceful_stop_script}"

{
  printf '%s\n' '#!/usr/bin/env bash'
  printf '%s\n' 'set -euo pipefail'
  printf '%s\n' ''
  printf 'runtime_pid="$(cat %q)"\n' "${pidfile}"
  printf '%s\n' 'kill -KILL "${runtime_pid}"'
  printf 'bash %q "${1:-30}"\n' "${wait_for_exit_script}"
} > "${force_stop_script}"
chmod 0755 "${force_stop_script}"

{
  printf 'CONTAINER_ID=%q\n' "${container_id}"
  printf 'CONTAINER_NAME=%q\n' "${container_name}"
  printf 'BUNDLE_DIR=%q\n' "${bundle_dir}"
  printf 'BUNDLE_CONFIG=%q\n' "${bundle_config}"
  printf 'STATE_ROOT=%q\n' "${state_root}"
  printf 'CONTAINER_STATE_DIR=%q\n' "${container_state_dir}"
  printf 'CTR_LOG=%q\n' "${ctr_log}"
  printf 'OCI_LOG=%q\n' "${oci_log}"
  printf 'PIDFILE=%q\n' "${pidfile}"
  printf 'CONMON_PIDFILE=%q\n' "${conmon_pidfile}"
  printf 'EXIT_DIR=%q\n' "${exit_dir}"
  printf 'EXIT_STATUS_FILE=%q\n' "${exit_status_file}"
  printf 'PERSIST_DIR=%q\n' "${persist_dir}"
  printf 'CONMON=%q\n' "${conmon_path}"
  printf 'RUNTIME=%q\n' "${runtime_path}"
  printf 'COMMAND_FILE=%q\n' "${command_file}"
  printf 'START_CONTAINER=%q\n' "${start_container_script}"
  printf 'FIND_ATTACH_SOCKETS=%q\n' "${find_attach_sockets_script}"
  printf 'CAPTURE_PROCESS_TREE=%q\n' "${capture_process_tree_script}"
  printf 'WAIT_FOR_EXIT=%q\n' "${wait_for_exit_script}"
  printf 'SHOW_EXIT_STATUS=%q\n' "${show_exit_status_script}"
  printf 'GRACEFUL_STOP=%q\n' "${graceful_stop_script}"
  printf 'FORCE_STOP=%q\n' "${force_stop_script}"
} > "${metadata_file}"

echo "drill.container_id=${container_id}"
echo "drill.name=${container_name}"
echo "drill.bundle_dir=${bundle_dir}"
echo "drill.bundle_config=${bundle_config}"
echo "drill.state_root=${state_root}"
echo "drill.command_file=${command_file}"
echo "drill.start_container_script=${start_container_script}"
echo "drill.ctr_log=${ctr_log}"
echo "drill.oci_log=${oci_log}"
echo "drill.pidfile=${pidfile}"
echo "drill.conmon_pidfile=${conmon_pidfile}"
echo "drill.exit_dir=${exit_dir}"
echo "drill.exit_status_file=${exit_status_file}"
echo "drill.persist_dir=${persist_dir}"
echo "drill.attach_socket_search_root=${persist_dir}"
echo "drill.start_container_cmd=bash ${start_container_script}"
echo "drill.attach_socket_search_cmd=bash ${find_attach_sockets_script}"
echo "drill.process_tree_cmd=bash ${capture_process_tree_script}"
echo "drill.graceful_stop_cmd=bash ${graceful_stop_script}"
echo "drill.force_stop_cmd=bash ${force_stop_script}"
echo "drill.show_exit_status_cmd=bash ${show_exit_status_script}"
printf 'drill.command='
printf '%q ' "${command[@]}"
printf '\n'
