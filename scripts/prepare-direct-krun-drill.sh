#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: prepare-direct-krun-drill.sh --bundle-dir <path> [options]

Prepare a reproducible direct private-runtime krun drill layout without running
the stack yet. The helper creates deterministic state directories and
operator-facing scripts so a supported Linux host can execute the first
/usr/libexec/nimbus/crun run --bundle ... drill and record the same evidence
paths every time.

options:
  --bundle-dir <path>      OCI bundle directory (must already contain config.json)
  --state-root <path>      Root directory for pid/log/exit state
                           (default: ${TMPDIR:-/tmp}/nimbus-direct-krun-drill)
  --container-id <id>      Runtime container ID (default: nimbus-krun-probe)
  --runtime <path>         private runtime path for direct execution
                           (default: /usr/libexec/nimbus/crun)
  --host-port <port>       Override the host port derived from krun.port_map
  --probe-host <host>      Host for the HTTP connectivity probe
                           (default: 127.0.0.1)
  --probe-path <path>      HTTP path for the connectivity probe (default: /)
  --command-file <path>    Override the generated foreground run script path
  -h, --help               Show this help

examples:
  bash scripts/prepare-direct-krun-drill.sh \
    --bundle-dir /tmp/nimbus-krun-probe \
    --state-root /tmp/nimbus-direct-krun-drill

  bash scripts/prepare-direct-krun-drill.sh \
    --bundle-dir /tmp/nimbus-krun-probe \
    --state-root /tmp/nimbus-direct-krun-drill \
    --container-id nimbus-http \
    --runtime /usr/libexec/nimbus/crun
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

require_command() {
  local command_name="$1"

  if ! command -v "${command_name}" >/dev/null 2>&1; then
    echo "required command not found: ${command_name}" >&2
    exit 69
  fi
}

bundle_dir=""
state_root="${TMPDIR:-/tmp}/nimbus-direct-krun-drill"
container_id="nimbus-krun-probe"
runtime_path="/usr/libexec/nimbus/crun"
host_port=""
probe_host="127.0.0.1"
probe_path="/"
command_file=""

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
    --runtime)
      runtime_path="${2:-}"
      shift 2
      ;;
    --host-port)
      host_port="${2:-}"
      shift 2
      ;;
    --probe-host)
      probe_host="${2:-}"
      shift 2
      ;;
    --probe-path)
      probe_path="${2:-}"
      shift 2
      ;;
    --command-file)
      command_file="${2:-}"
      shift 2
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

if [[ "${probe_path}" != /* ]]; then
  probe_path="/${probe_path}"
fi

bundle_dir="$(resolve_existing_dir "${bundle_dir}")"
bundle_config="${bundle_dir}/config.json"
if [[ ! -f "${bundle_config}" ]]; then
  echo "bundle config not found: ${bundle_config}" >&2
  exit 66
fi

require_command python3

if [[ -z "${host_port}" ]]; then
  host_port="$(python3 - "${bundle_config}" <<'PY'
import json
import sys
from pathlib import Path

config_path = Path(sys.argv[1])
with config_path.open("r", encoding="utf-8") as fh:
    config = json.load(fh)

annotation = (
    config.get("annotations", {}).get("krun.port_map", "")
)
if not annotation:
    raise SystemExit("missing krun.port_map annotation")

first_pair = annotation.split(",", 1)[0]
parts = first_pair.split(":", 1)
if len(parts) != 2 or not parts[0]:
    raise SystemExit(f"invalid krun.port_map entry: {first_pair}")

print(parts[0])
PY
)"
fi

state_root="$(resolve_dir_path "${state_root}")"
container_state_dir="${state_root}/containers/${container_id}"
mkdir -p "${container_state_dir}"

stdout_log="${container_state_dir}/runtime.stdout.log"
stderr_log="${container_state_dir}/runtime.stderr.log"
runtime_pidfile="${container_state_dir}/runtime.pid"
launcher_pidfile="${container_state_dir}/launcher.pid"
exit_status_file="${container_state_dir}/exit.status"

if [[ -z "${command_file}" ]]; then
  command_file="${container_state_dir}/run-runtime.sh"
fi
command_file="$(resolve_file_path "${command_file}")"

start_script="${container_state_dir}/start-runtime.sh"
probe_http_script="${container_state_dir}/probe-http.sh"
wait_for_http_script="${container_state_dir}/wait-for-http.sh"
wait_for_exit_script="${container_state_dir}/wait-for-exit.sh"
show_exit_status_script="${container_state_dir}/show-exit-status.sh"
graceful_stop_script="${container_state_dir}/graceful-stop.sh"
force_stop_script="${container_state_dir}/force-stop.sh"
metadata_file="${container_state_dir}/drill.env"

probe_url="http://${probe_host}:${host_port}${probe_path}"
runtime_command=(
  "${runtime_path}"
  "run"
  "--bundle" "${bundle_dir}"
  "${container_id}"
)

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
  printf 'rm -f %q %q %q\n' "${runtime_pidfile}" "${launcher_pidfile}" "${exit_status_file}"
  printf 'trap '\''if [[ -f %q ]]; then runtime_pid="$(cat %q)"; kill -TERM "${runtime_pid}" 2>/dev/null || true; fi'\'' TERM INT\n' "${runtime_pidfile}" "${runtime_pidfile}"
  printf '%s\n' ''
  printf 'if ! command -v %q >/dev/null 2>&1; then\n' "${runtime_path}"
  printf '  echo "runtime not found: %s" >&2\n' "${runtime_path}"
  printf '%s\n' '  exit 69'
  printf '%s\n' 'fi'
  printf '%s\n' ''
  printf '%s ' "${runtime_command[0]}"
  printf '%q ' "${runtime_command[@]:1}"
  printf '> %q 2> %q &\n' "${stdout_log}" "${stderr_log}"
  printf '%s\n' 'runtime_pid=$!'
  printf 'printf '\''%%s\\n'\'' "${runtime_pid}" > %q\n' "${runtime_pidfile}"
  printf '%s\n' ''
  printf 'wait "${runtime_pid}"\n'
  printf '%s\n' 'status=$?'
  printf 'printf '\''%%s\\n'\'' "${status}" > %q\n' "${exit_status_file}"
  printf '%s\n' 'exit "${status}"'
} > "${command_file}"
chmod 0755 "${command_file}"

{
  printf '%s\n' '#!/usr/bin/env bash'
  printf '%s\n' 'set -euo pipefail'
  printf '%s\n' ''
  printf 'bash %q &\n' "${command_file}"
  printf '%s\n' 'launcher_pid=$!'
  printf 'printf '\''%%s\\n'\'' "${launcher_pid}" > %q\n' "${launcher_pidfile}"
  printf '%s\n' 'printf '\''launcher.pid=%s\\n'\'' "${launcher_pid}"'
  printf 'printf '\''runtime.pidfile=%s\\n'\'' %q\n' "${runtime_pidfile}"
} > "${start_script}"
chmod 0755 "${start_script}"

{
  printf '%s\n' '#!/usr/bin/env bash'
  printf '%s\n' 'set -euo pipefail'
  printf '%s\n' ''
  printf 'exec curl -fsS %q\n' "${probe_url}"
} > "${probe_http_script}"
chmod 0755 "${probe_http_script}"

{
  printf '%s\n' '#!/usr/bin/env bash'
  printf '%s\n' 'set -euo pipefail'
  printf '%s\n' ''
  printf '%s\n' 'timeout_seconds="${1:-30}"'
  printf '%s\n' 'deadline=$((SECONDS + timeout_seconds))'
  printf '%s\n' ''
  printf '%s\n' 'while (( SECONDS <= deadline )); do'
  printf '  if curl -fsS %q >/dev/null 2>&1; then\n' "${probe_url}"
  printf '    printf '\''ready.url=%s\\n'\'' %q\n' "${probe_url}"
  printf '%s\n' '    exit 0'
  printf '%s\n' '  fi'
  printf '%s\n' '  sleep 1'
  printf '%s\n' 'done'
  printf '%s\n' ''
  printf 'echo "probe did not succeed within ${timeout_seconds}s: %s" >&2\n' "${probe_url}"
  printf '%s\n' 'exit 1'
} > "${wait_for_http_script}"
chmod 0755 "${wait_for_http_script}"

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
  printf 'runtime_pid="$(cat %q)"\n' "${runtime_pidfile}"
  printf '%s\n' 'kill -TERM "${runtime_pid}"'
  printf 'bash %q "${1:-30}"\n' "${wait_for_exit_script}"
} > "${graceful_stop_script}"
chmod 0755 "${graceful_stop_script}"

{
  printf '%s\n' '#!/usr/bin/env bash'
  printf '%s\n' 'set -euo pipefail'
  printf '%s\n' ''
  printf 'runtime_pid="$(cat %q)"\n' "${runtime_pidfile}"
  printf '%s\n' 'kill -KILL "${runtime_pid}"'
  printf 'bash %q "${1:-30}"\n' "${wait_for_exit_script}"
} > "${force_stop_script}"
chmod 0755 "${force_stop_script}"

{
  printf 'CONTAINER_ID=%q\n' "${container_id}"
  printf 'BUNDLE_DIR=%q\n' "${bundle_dir}"
  printf 'BUNDLE_CONFIG=%q\n' "${bundle_config}"
  printf 'STATE_ROOT=%q\n' "${state_root}"
  printf 'CONTAINER_STATE_DIR=%q\n' "${container_state_dir}"
  printf 'RUNTIME=%q\n' "${runtime_path}"
  printf 'COMMAND_FILE=%q\n' "${command_file}"
  printf 'START_SCRIPT=%q\n' "${start_script}"
  printf 'STDOUT_LOG=%q\n' "${stdout_log}"
  printf 'STDERR_LOG=%q\n' "${stderr_log}"
  printf 'RUNTIME_PIDFILE=%q\n' "${runtime_pidfile}"
  printf 'LAUNCHER_PIDFILE=%q\n' "${launcher_pidfile}"
  printf 'EXIT_STATUS_FILE=%q\n' "${exit_status_file}"
  printf 'HOST_PORT=%q\n' "${host_port}"
  printf 'PROBE_URL=%q\n' "${probe_url}"
  printf 'PROBE_HTTP=%q\n' "${probe_http_script}"
  printf 'WAIT_FOR_HTTP=%q\n' "${wait_for_http_script}"
  printf 'WAIT_FOR_EXIT=%q\n' "${wait_for_exit_script}"
  printf 'SHOW_EXIT_STATUS=%q\n' "${show_exit_status_script}"
  printf 'GRACEFUL_STOP=%q\n' "${graceful_stop_script}"
  printf 'FORCE_STOP=%q\n' "${force_stop_script}"
} > "${metadata_file}"

echo "drill.container_id=${container_id}"
echo "drill.bundle_dir=${bundle_dir}"
echo "drill.bundle_config=${bundle_config}"
echo "drill.state_root=${state_root}"
echo "drill.command_file=${command_file}"
echo "drill.start_script=${start_script}"
echo "drill.stdout_log=${stdout_log}"
echo "drill.stderr_log=${stderr_log}"
echo "drill.runtime_pidfile=${runtime_pidfile}"
echo "drill.launcher_pidfile=${launcher_pidfile}"
echo "drill.exit_status_file=${exit_status_file}"
echo "drill.host_port=${host_port}"
echo "drill.probe_url=${probe_url}"
echo "drill.probe_http_cmd=bash ${probe_http_script}"
echo "drill.wait_for_http_cmd=bash ${wait_for_http_script}"
echo "drill.graceful_stop_cmd=bash ${graceful_stop_script}"
echo "drill.force_stop_cmd=bash ${force_stop_script}"
echo "drill.show_exit_status_cmd=bash ${show_exit_status_script}"
printf 'drill.command='
printf '%q ' "${runtime_command[@]}"
printf '\n'
