#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_base="${TMPDIR:-/tmp}"
tmp_base="${tmp_base%/}"
tmp_dir="$(mktemp -d "${tmp_base}/nimbus-machine-recreate-verify.XXXXXX")"
tmp_dir="$(cd "${tmp_dir}" && pwd)"
trap 'rm -rf "${tmp_dir}"' EXIT

home_dir="${tmp_dir}/home"
runtime_root="${tmp_dir}/runtime-root"
bin_dir="${tmp_dir}/bin"
output_dir="${tmp_dir}/output"
image_path="${tmp_dir}/machine.raw"

mkdir -p "${home_dir}" "${runtime_root}" "${bin_dir}" "${output_dir}"
printf 'fake raw image\n' > "${image_path}"

cat > "${bin_dir}/nimbus" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

machine_name="default"
home_dir="${HOME:?HOME must be set}"
runtime_root="${NIMBUS_MACHINE_RUNTIME_ROOT:?runtime root must be set}"
config_dir="${home_dir}/.config/nimbus/machine/${machine_name}"
state_dir="${home_dir}/.local/state/nimbus/machine/${machine_name}"

mkdir -p "${config_dir}" "${state_dir}"

write_status() {
  local lifecycle="$1"
  local manager="$2"
  cat > "${state_dir}/status.json" <<OUT
{"lifecycle":"${lifecycle}","manager":"${manager}"}
OUT
}

if [[ "${1:-}" != "machine" ]]; then
  echo "unexpected args: $*" >&2
  exit 64
fi

case "${2:-}" in
  stop)
    if [[ -f "${state_dir}/status.json" ]]; then
      write_status "stopped" "helpers-resolved"
    fi
    echo "stopped"
    ;;
  rm)
    rm -rf "${config_dir}" "${state_dir}"
    rm -f \
      "${runtime_root}/${machine_name}.sock" \
      "${runtime_root}/${machine_name}-api.sock" \
      "${runtime_root}/${machine_name}-ignition.sock" \
      "${runtime_root}/${machine_name}-gvproxy.sock" \
      "${runtime_root}/${machine_name}-krunkit.sock" \
      "${runtime_root}/${machine_name}.log" \
      "${runtime_root}/${machine_name}-gvproxy.log" \
      "${runtime_root}/${machine_name}-krunkit.log" \
      "${runtime_root}/${machine_name}-gvproxy.pid" \
      "${runtime_root}/${machine_name}-krunkit.pid"
    echo "removed"
    ;;
  init)
    image=""
    ssh_identity=""
    ignition_file=""
    efi_store=""
    volumes=()
    shift 2
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --image)
          image="${2:?missing image path}"
          shift 2
          ;;
        --identity)
          ssh_identity="${2:?missing ssh identity path}"
          shift 2
          ;;
        --ignition-path)
          ignition_file="${2:?missing ignition file path}"
          shift 2
          ;;
        --firmware)
          efi_store="${2:?missing efi store path}"
          shift 2
          ;;
        --volume)
          volumes+=( "${2:?missing volume}" )
          shift 2
          ;;
        *)
          shift
          ;;
      esac
    done
    mkdir -p "${config_dir}" "${state_dir}"
    cat > "${config_dir}/config.json" <<OUT
{"image":"${image}","ssh_identity_path":"${ssh_identity}","ignition_file_path":"${ignition_file}","efi_variable_store_path":"${efi_store}","volumes":"${volumes[*]}"}
OUT
    write_status "stopped" "helpers-resolved"
    echo "initialized"
    ;;
  start)
    mkdir -p "${runtime_root}"
    : > "${runtime_root}/${machine_name}.sock"
    : > "${runtime_root}/${machine_name}-gvproxy.sock"
    : > "${runtime_root}/${machine_name}-krunkit.sock"
    printf '321\n' > "${runtime_root}/${machine_name}-gvproxy.pid"
    printf '654\n' > "${runtime_root}/${machine_name}-krunkit.pid"
    printf 'machine booted\n' > "${runtime_root}/${machine_name}.log"
    printf 'gvproxy started\n' > "${runtime_root}/${machine_name}-gvproxy.log"
    printf 'krunkit started\n' > "${runtime_root}/${machine_name}-krunkit.log"
    write_status "running" "ready"
    echo "started"
    ;;
  status)
    if [[ -f "${state_dir}/status.json" ]]; then
      cat <<OUT
result: status
lifecycle: running
manager: ready
OUT
    else
      cat <<OUT
result: uninitialized
lifecycle: uninitialized
manager: unconfigured
OUT
    fi
    ;;
  *)
    echo "unexpected machine subcommand: ${2:-}" >&2
    exit 64
    ;;
esac
EOF

cat > "${bin_dir}/ps" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
cat <<'OUT'
321 1 /opt/homebrew/bin/gvproxy -listen-vfkit unixgram:///tmp/test.sock
654 1 /opt/homebrew/bin/krunkit --cpus 2 --memory 2048
OUT
EOF

chmod +x "${bin_dir}/nimbus" "${bin_dir}/ps"

HOME="${home_dir}" bash "${repo_root}/scripts/recreate-nimbus-machine.sh" \
  --home "${home_dir}" \
  --runtime-root "${runtime_root}" \
  --output-dir "${output_dir}" \
  --nimbus "${bin_dir}/nimbus" \
  --image "${image_path}" \
  --identity "${tmp_dir}/machine-key" \
  --ignition-path "${tmp_dir}/machine.ign" \
  --firmware "${tmp_dir}/efi-store" \
  --volume /Users:/Users \
  > "${output_dir}/stdout.txt"

summary_file="${output_dir}/summary.txt"

for expected_file in \
  "${output_dir}/nimbus-machine-stop-command.txt" \
  "${output_dir}/nimbus-machine-rm-command.txt" \
  "${output_dir}/nimbus-machine-init-command.txt" \
  "${output_dir}/nimbus-machine-start-command.txt" \
  "${output_dir}/nimbus-machine-status.txt" \
  "${output_dir}/post-diagnostics/machine-config.json" \
  "${output_dir}/post-diagnostics/machine-state.json" \
  "${output_dir}/post-diagnostics/machine-log-tail.txt"
do
  if [[ ! -f "${expected_file}" ]]; then
    echo "expected recreate artifact missing: ${expected_file}" >&2
    exit 70
  fi
done

grep -F "image.source                       ${image_path}" "${summary_file}" >/dev/null
grep -E "^recreate\\.init[[:space:]]+ok path=.*nimbus-machine-init\\.txt$" "${summary_file}" >/dev/null
grep -E "^recreate\\.start[[:space:]]+ok path=.*nimbus-machine-start\\.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.post_diagnostics[[:space:]]+ok path=.*post-diagnostics$" "${summary_file}" >/dev/null
grep -F "result                             ready" "${summary_file}" >/dev/null

grep -F -- "--image ${image_path}" "${output_dir}/nimbus-machine-init-command.txt" >/dev/null
grep -F -- "--volume /Users:/Users" "${output_dir}/nimbus-machine-init-command.txt" >/dev/null
grep -F "started" "${output_dir}/nimbus-machine-start.txt" >/dev/null
grep -F "machine booted" "${output_dir}/post-diagnostics/machine-log-tail.txt" >/dev/null
grep -F "default-api.sock missing" "${output_dir}/post-diagnostics/socket-presence.txt" >/dev/null
grep -F "default.sock present" "${output_dir}/post-diagnostics/socket-presence.txt" >/dev/null

echo "verified: nimbus machine recreate helper captured deterministic artifacts"
