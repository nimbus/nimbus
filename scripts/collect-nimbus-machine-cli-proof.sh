#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"

usage() {
  cat <<'EOF'
usage: collect-nimbus-machine-cli-proof.sh [options]

Collect an isolated local-binary proof for the shipped `nimbus machine ...`
surface without touching the user's default machine roots or installed/Homebrew
binary. The helper creates dedicated XDG roots, runs the selected binary inside
that isolated contract, captures both human and structured status output, then
cleans up the temporary machine state.

For contract proof, prefer the default image behavior. `--image` is an
explicit debug override and can bypass the host-managed macOS machine image
contract that normal users run.

captured proof:

- selected host `nimbus --version`
- explicit isolated HOME/XDG/runtime roots
- TTY `machine start` output
- default table `machine status`
- structured `machine status --format json`
- structured `machine status --format yaml`
- isolated-root `machine stop` and `machine rm` cleanup

options:
  --machine <name>           Machine name (default: default)
  --root <path>              Isolated proof root (default: mktemp under TMPDIR)
  --output-dir <path>        Output directory (default: <root>/output)
  --nimbus <path>            Nimbus binary path
                             (default: <repo>/target/debug/nimbus)
  --image <source>           Optional machine image source passed to `machine start`
                             Debug override only; omit this to exercise the
                             shipped pinned-image contract
  --guest-binary <path>      Optional `NIMBUS_MACHINE_GUEST_BINARY` override
  --script <path>            `script` binary used for PTY capture
                             (default: script)
  --keep-machine             Skip the final isolated-root stop/rm cleanup
  -h, --help                 Show this help

examples:
  bash scripts/collect-nimbus-machine-cli-proof.sh \
    --nimbus target/debug/nimbus \
    --image "$HOME/.local/share/nimbus/machine/default/images/default.raw"

  bash scripts/collect-nimbus-machine-cli-proof.sh \
    --nimbus /opt/homebrew/bin/nimbus \
    --guest-binary "$HOME/.cache/nimbus/machine/guest-nimbus/v0.1.18-nimbus_linux_arm64-nimbus"
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

capture_pty_command() {
  local label="$1"
  local command_path="$2"
  local output_path="$3"
  shift 3

  write_command_file "${command_path}" "$@"

  local status=0
  set +e
  "${script_bin}" -q "${output_path}" "$@"
  status=$?
  set -e

  if [[ "${status}" -eq 0 ]]; then
    print_line "${label}" "ok path=${output_path} cmd=${command_path} mode=pty"
  else
    print_line "${label}" "failed status=${status} path=${output_path} cmd=${command_path} mode=pty"
  fi

  return "${status}"
}

machine_name="default"
proof_root=""
output_dir=""
nimbus_bin="${repo_root}/target/debug/nimbus"
image_source=""
guest_binary_override=""
script_bin="script"
keep_machine=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --machine)
      machine_name="${2:?missing machine name}"
      shift 2
      ;;
    --root)
      proof_root="${2:?missing proof root}"
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
    --image)
      image_source="${2:?missing image source}"
      shift 2
      ;;
    --guest-binary)
      guest_binary_override="${2:?missing guest binary override}"
      shift 2
      ;;
    --script)
      script_bin="${2:?missing script path}"
      shift 2
      ;;
    --keep-machine)
      keep_machine=1
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

if [[ ! -x "${nimbus_bin}" ]]; then
  echo "nimbus binary is not executable at ${nimbus_bin}; build it first or pass --nimbus" >&2
  exit 64
fi

if [[ -n "${guest_binary_override}" && ! -f "${guest_binary_override}" ]]; then
  echo "guest binary override does not exist at ${guest_binary_override}" >&2
  exit 64
fi

if [[ -z "${proof_root}" ]]; then
  proof_root="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-machine-cli-proof.XXXXXX")"
else
  mkdir -p "${proof_root}"
fi

proof_root="$(cd "${proof_root}" && pwd)"

if [[ -z "${output_dir}" ]]; then
  output_dir="${proof_root}/output"
fi

mkdir -p "${output_dir}"
output_dir="$(cd "${output_dir}" && pwd)"

home_dir="${proof_root}/home"
xdg_config_home="${proof_root}/config"
xdg_state_home="${proof_root}/state"
xdg_data_home="${proof_root}/data"
xdg_cache_home="${proof_root}/cache"
runtime_root="${proof_root}/runtime"

mkdir -p \
  "${home_dir}" \
  "${xdg_config_home}" \
  "${xdg_state_home}" \
  "${xdg_data_home}" \
  "${xdg_cache_home}" \
  "${runtime_root}"

summary_file="${output_dir}/summary.txt"
: > "${summary_file}"

print_line "proof.root" "${proof_root}"
print_line "output.dir" "${output_dir}"
print_line "machine.name" "${machine_name}"
print_line "nimbus.bin" "${nimbus_bin}"
print_line "script.bin" "${script_bin}"
print_line "machine.image" "${image_source:-<default>}"
print_line "guest.binary.override" "${guest_binary_override:-<none>}"
print_line "home.dir" "${home_dir}"
print_line "xdg.config_home" "${xdg_config_home}"
print_line "xdg.state_home" "${xdg_state_home}"
print_line "xdg.data_home" "${xdg_data_home}"
print_line "xdg.cache_home" "${xdg_cache_home}"
print_line "runtime.root" "${runtime_root}"
print_line "cleanup.keep_machine" "${keep_machine}"

base_cmd=(
  env
  "HOME=${home_dir}"
  "XDG_CONFIG_HOME=${xdg_config_home}"
  "XDG_STATE_HOME=${xdg_state_home}"
  "XDG_DATA_HOME=${xdg_data_home}"
  "XDG_CACHE_HOME=${xdg_cache_home}"
  "NIMBUS_MACHINE_RUNTIME_ROOT=${runtime_root}"
)

if [[ -n "${guest_binary_override}" ]]; then
  base_cmd+=("NIMBUS_MACHINE_GUEST_BINARY=${guest_binary_override}")
fi

base_cmd+=("${nimbus_bin}")

host_version_cmd=("${base_cmd[@]}" --version)
capture_command \
  "capture.host_nimbus_version" \
  "${output_dir}/host-nimbus-version-command.txt" \
  "${output_dir}/host-nimbus-version.txt" \
  "${host_version_cmd[@]}"

start_cmd=("${base_cmd[@]}" machine start)
if [[ -n "${image_source}" ]]; then
  start_cmd+=(--image "${image_source}")
fi
start_cmd+=("${machine_name}")

status_cmd=("${base_cmd[@]}" machine status "${machine_name}")
status_json_cmd=("${base_cmd[@]}" machine status --format json "${machine_name}")
status_yaml_cmd=("${base_cmd[@]}" machine status --format yaml "${machine_name}")
stop_cmd=("${base_cmd[@]}" machine stop "${machine_name}")
rm_cmd=("${base_cmd[@]}" machine rm "${machine_name}")

overall_status=0

if ! capture_pty_command \
  "capture.machine_start" \
  "${output_dir}/machine-start-command.txt" \
  "${output_dir}/machine-start-pty.txt" \
  "${start_cmd[@]}"; then
  overall_status=1
fi

capture_command \
  "capture.machine_status" \
  "${output_dir}/machine-status-command.txt" \
  "${output_dir}/machine-status.txt" \
  "${status_cmd[@]}" || true

capture_command \
  "capture.machine_status_json" \
  "${output_dir}/machine-status-json-command.txt" \
  "${output_dir}/machine-status.json" \
  "${status_json_cmd[@]}" || true

capture_command \
  "capture.machine_status_yaml" \
  "${output_dir}/machine-status-yaml-command.txt" \
  "${output_dir}/machine-status.yaml" \
  "${status_yaml_cmd[@]}" || true

if [[ "${keep_machine}" -eq 0 ]]; then
  capture_command \
    "cleanup.machine_stop" \
    "${output_dir}/machine-stop-command.txt" \
    "${output_dir}/machine-stop.txt" \
    "${stop_cmd[@]}" || true

  capture_command \
    "cleanup.machine_rm" \
    "${output_dir}/machine-rm-command.txt" \
    "${output_dir}/machine-rm.txt" \
    "${rm_cmd[@]}" || true
else
  print_line "cleanup.machine" "skipped keep-machine=1"
fi

if [[ "${overall_status}" -eq 0 ]]; then
  print_line "result" "captured"
else
  print_line "result" "partial start_failed"
fi

exit "${overall_status}"
