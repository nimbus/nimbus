#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"

usage() {
  cat <<'EOF'
usage: collect-nimbus-machine-service-proof.sh --compose-file <path> --service <name> [options]

Collect host-side proof for the macOS forwarded machine-API and explicit
`nimbus compose ...` workflow. This is the checked-in MAC5/MAC6/MAC7 evidence
lane for:

- host `<machine>-api.sock` health and capabilities
- direct host-side machine-API sandbox listing through the forwarded socket
- `nimbus compose up/ps/inspect/top/logs/down` against a container-backed
  image-backed or build-backed Compose project
- optional localhost connectivity proof for one published guest service

options:
  --compose-file <path>          Compose file to exercise (required)
  --service <name>               Service name for inspect/logs/ps (required)
  --machine <name>               Machine name (default: default)
  --home <path>                  HOME to use for XDG-style machine roots
  --runtime-root <path>          Runtime root (default: /tmp/nimbus)
  --output-dir <path>            Output directory for captured artifacts
  --nimbus <path>                Nimbus binary path
                                 (default: <repo>/target/debug/nimbus)
  --curl <path>                  Curl binary path (default: curl)
  --published-url <url>          Optional localhost URL to prove after service up
  -h, --help                     Show this help

examples:
  bash scripts/collect-nimbus-machine-service-proof.sh \
    --home /tmp/nimbus-home \
    --runtime-root /tmp/nimbus \
    --compose-file /Users/jack/src/my-app/compose.yaml \
    --service db \
    --published-url http://127.0.0.1:18080/healthz
EOF
}

print_line() {
  local label="$1"
  local value="$2"
  printf '%-34s %s\n' "${label}" "${value}" | tee -a "${summary_file}"
}

write_command_file() {
  local output_path="$1"
  shift

  local -a rendered=()
  local arg=""

  for arg in "$@"; do
    rendered+=( "$(printf '%q' "${arg}")" )
  done

  printf '%s\n' "${rendered[*]}" > "${output_path}"
}

capture_command() {
  local label="$1"
  local command_path="$2"
  local output_path="$3"
  shift 3

  write_command_file "${command_path}" "$@"

  local status=0
  set +e
  "$@" >"${output_path}" 2>&1
  status=$?
  set -e

  if [[ "${status}" -eq 0 ]]; then
    print_line "${label}" "ok path=${output_path} cmd=${command_path}"
  else
    print_line "${label}" "failed status=${status} path=${output_path} cmd=${command_path}"
  fi

  return "${status}"
}

capture_command_with_retries() {
  local label="$1"
  local command_path="$2"
  local output_path="$3"
  local success_pattern="$4"
  local terminal_pattern="${5:-}"
  local timeout_secs="$6"
  shift 6

  write_command_file "${command_path}" "$@"

  local deadline=$((SECONDS + timeout_secs))
  local attempt=0
  local status=1

  while :; do
    attempt=$((attempt + 1))
    set +e
    "$@" >"${output_path}" 2>&1
    status=$?
    set -e

    if [[ "${status}" -eq 0 ]] && tr -d '\r' <"${output_path}" | grep -Eq "${success_pattern}"; then
      print_line "${label}" "ok attempts=${attempt} path=${output_path} cmd=${command_path}"
      return 0
    fi

    if [[ -n "${terminal_pattern}" ]] && tr -d '\r' <"${output_path}" | grep -Eq "${terminal_pattern}"; then
      print_line "${label}" "failed attempts=${attempt} terminal_state path=${output_path} cmd=${command_path}"
      return 1
    fi

    if (( SECONDS >= deadline )); then
      print_line "${label}" "failed status=${status} attempts=${attempt} timeout=${timeout_secs}s path=${output_path} cmd=${command_path}"
      return 1
    fi

    sleep 1
  done
}

machine_name="default"
home_dir="${HOME:-}"
runtime_root="${NIMBUS_MACHINE_RUNTIME_ROOT:-/tmp/nimbus}"
output_dir=""
nimbus_bin="${repo_root}/target/debug/nimbus"
curl_bin="curl"
compose_file=""
service_name=""
published_url=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --compose-file)
      compose_file="${2:?missing compose file path}"
      shift 2
      ;;
    --service)
      service_name="${2:?missing service name}"
      shift 2
      ;;
    --machine)
      machine_name="${2:?missing machine name}"
      shift 2
      ;;
    --home)
      home_dir="${2:?missing home path}"
      shift 2
      ;;
    --runtime-root)
      runtime_root="${2:?missing runtime root}"
      shift 2
      ;;
    --output-dir)
      output_dir="${2:?missing output dir}"
      shift 2
      ;;
    --nimbus)
      nimbus_bin="${2:?missing nimbus path}"
      shift 2
      ;;
    --curl)
      curl_bin="${2:?missing curl path}"
      shift 2
      ;;
    --published-url)
      published_url="${2:?missing published URL}"
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

if [[ -z "${home_dir}" ]]; then
  echo "set --home or HOME so the machine roots can be derived" >&2
  exit 64
fi

if [[ -z "${compose_file}" ]]; then
  echo "missing required --compose-file argument" >&2
  usage >&2
  exit 64
fi

if [[ -z "${service_name}" ]]; then
  echo "missing required --service argument" >&2
  usage >&2
  exit 64
fi

if [[ ! -f "${compose_file}" ]]; then
  echo "compose file does not exist at ${compose_file}" >&2
  exit 64
fi

if [[ ! -x "${nimbus_bin}" ]]; then
  echo "nimbus binary is not executable at ${nimbus_bin}; build it first or pass --nimbus" >&2
  exit 64
fi

if ! command -v "${curl_bin}" >/dev/null 2>&1; then
  echo "curl binary is not executable as ${curl_bin}" >&2
  exit 64
fi

if [[ -z "${output_dir}" ]]; then
  output_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-machine-service-proof.XXXXXX")"
else
  mkdir -p "${output_dir}"
fi

output_dir="$(cd "${output_dir}" && pwd)"
compose_file="$(cd "$(dirname "${compose_file}")" && pwd)/$(basename "${compose_file}")"
summary_file="${output_dir}/summary.txt"
: > "${summary_file}"

api_socket="${runtime_root%/}/${machine_name}-api.sock"

print_line "output.dir" "${output_dir}"
print_line "machine.name" "${machine_name}"
print_line "home.dir" "${home_dir}"
print_line "runtime.root" "${runtime_root}"
print_line "machine.api_socket" "${api_socket}"
print_line "compose.file" "${compose_file}"
print_line "compose.dir" "$(dirname "${compose_file}")"
print_line "service.name" "${service_name}"
print_line "nimbus.bin" "${nimbus_bin}"
print_line "curl.bin" "${curl_bin}"
print_line "published.url" "${published_url:-<unspecified>}"

base_cmd=(
  env
  "HOME=${home_dir}"
  "NIMBUS_MACHINE_RUNTIME_ROOT=${runtime_root}"
  "${nimbus_bin}"
)

capture_command \
  "capture.machine_status" \
  "${output_dir}/machine-status-command.txt" \
  "${output_dir}/machine-status.txt" \
  "${base_cmd[@]}" machine status || true

capture_command \
  "capture.machine_api_health" \
  "${output_dir}/machine-api-health-command.txt" \
  "${output_dir}/machine-api-health.txt" \
  "${curl_bin}" --silent --show-error --include --unix-socket "${api_socket}" http://localhost/healthz || true

capture_command \
  "capture.machine_api_capabilities" \
  "${output_dir}/machine-api-capabilities-command.txt" \
  "${output_dir}/machine-api-capabilities.txt" \
  "${curl_bin}" --silent --show-error --include --unix-socket "${api_socket}" http://localhost/v1/machine-api/capabilities || true

capture_command \
  "capture.service_config" \
  "${output_dir}/service-config-command.txt" \
  "${output_dir}/service-config.txt" \
  "${base_cmd[@]}" compose config --file "${compose_file}" || true

capture_command \
  "capture.service_up" \
  "${output_dir}/service-up-command.txt" \
  "${output_dir}/service-up.txt" \
  "${base_cmd[@]}" compose up --file "${compose_file}" || true

capture_command_with_retries \
  "capture.service_ready" \
  "${output_dir}/service-inspect-command.txt" \
  "${output_dir}/service-inspect.txt" \
  '^[[:space:]]*status: ready$|"status"[[:space:]]*:[[:space:]]*"ready"' \
  '^[[:space:]]*status: (failed|stopped)$|"status"[[:space:]]*:[[:space:]]*"(failed|stopped)"' \
  30 \
  "${base_cmd[@]}" compose inspect "${service_name}" --file "${compose_file}" || true

capture_command \
  "capture.machine_api_service_sandboxes" \
  "${output_dir}/machine-api-service-sandboxes-command.txt" \
  "${output_dir}/machine-api-service-sandboxes.txt" \
  "${curl_bin}" --silent --show-error --include --unix-socket "${api_socket}" http://localhost/v1/machine-api/service-sandboxes || true

capture_command \
  "capture.service_list" \
  "${output_dir}/service-list-command.txt" \
  "${output_dir}/service-list.txt" \
  "${base_cmd[@]}" compose ps --file "${compose_file}" || true

capture_command \
  "capture.service_ps" \
  "${output_dir}/service-ps-command.txt" \
  "${output_dir}/service-ps.txt" \
  "${base_cmd[@]}" compose top "${service_name}" --file "${compose_file}" || true

capture_command \
  "capture.service_logs" \
  "${output_dir}/service-logs-command.txt" \
  "${output_dir}/service-logs.txt" \
  "${base_cmd[@]}" compose logs "${service_name}" --file "${compose_file}" || true

if [[ -n "${published_url}" ]]; then
  capture_command_with_retries \
    "capture.localhost_probe" \
    "${output_dir}/localhost-probe-command.txt" \
    "${output_dir}/localhost-probe.txt" \
    '^HTTP/[0-9.]+ 200 OK$' \
    '' \
    15 \
    "${curl_bin}" --silent --show-error --include "${published_url}" || true
else
  print_line "capture.localhost_probe" "skipped reason=no-published-url"
fi

capture_command \
  "capture.service_down" \
  "${output_dir}/service-down-command.txt" \
  "${output_dir}/service-down.txt" \
  "${base_cmd[@]}" compose down --file "${compose_file}" || true

capture_command \
  "capture.service_list_after_down" \
  "${output_dir}/service-list-after-down-command.txt" \
  "${output_dir}/service-list-after-down.txt" \
  "${base_cmd[@]}" compose ps --file "${compose_file}" || true

print_line "result" "captured"
