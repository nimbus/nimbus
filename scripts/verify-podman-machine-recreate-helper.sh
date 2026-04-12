#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_base="${TMPDIR:-/tmp}"
tmp_base="${tmp_base%/}"
tmp_dir="$(mktemp -d "${tmp_base}/neovex-podman-recreate-verify.XXXXXX")"
tmp_dir="$(cd "${tmp_dir}" && pwd)"
trap 'rm -rf "${tmp_dir}"' EXIT

home_dir="${tmp_dir}/home"
bin_dir="${tmp_dir}/bin"
tmp_parent="${tmp_dir}/runtime"
tmp_root="${tmp_parent}/podman"
output_dir="${tmp_dir}/output"

mkdir -p \
  "${home_dir}/.config/containers/podman/machine/libkrun" \
  "${home_dir}/.local/share/containers/podman/machine/libkrun" \
  "${bin_dir}" \
  "${tmp_root}" \
  "${output_dir}"

cat > "${home_dir}/.config/containers/podman/machine/libkrun/testvm.json" <<'EOF'
{
  "Name": "testvm",
  "VMType": "libkrun",
  "Mounts": [
    {
      "OriginalInput": "/Users:/Users"
    }
  ]
}
EOF

printf 'stale raw disk\n' > "${home_dir}/.local/share/containers/podman/machine/libkrun/testvm-arm64.raw"
printf 'stale machine log\n' > "${tmp_root}/testvm.log"
: > "${tmp_root}/testvm.sock"
: > "${tmp_root}/testvm-api.sock"
: > "${tmp_root}/testvm-gvproxy.sock"
: > "${tmp_root}/testvm-gvproxy.sock-krun.sock"

cat > "${bin_dir}/podman" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

machine_name="testvm"
provider="${CONTAINERS_MACHINE_PROVIDER:-libkrun}"
home_dir="${HOME:?missing HOME}"
config_path="${home_dir}/.config/containers/podman/machine/${provider}/${machine_name}.json"
data_path="${home_dir}/.local/share/containers/podman/machine/${provider}/${machine_name}-arm64.raw"
tmp_root="${TMPDIR%/}/podman"
state_file="${home_dir}/.local/share/containers/podman/machine/${provider}/${machine_name}.state"

mkdir -p \
  "$(dirname "${config_path}")" \
  "$(dirname "${data_path}")" \
  "$(dirname "${state_file}")" \
  "${tmp_root}"

state="absent"
if [[ -f "${state_file}" ]]; then
  state="$(cat "${state_file}")"
elif [[ -f "${config_path}" ]]; then
  state="present"
fi

set_state() {
  printf '%s\n' "$1" > "${state_file}"
}

write_machine_config() {
  cat > "${config_path}" <<OUT
{
  "Name": "${machine_name}",
  "VMType": "${provider}",
  "CreatedBy": "verify-podman-machine-recreate-helper",
  "Mounts": [
    {
      "OriginalInput": "/Users:/Users"
    }
  ]
}
OUT
}

write_ready_artifacts() {
  printf 'Ignition: user-provided config was applied\nStarted sshd.service\nFinished ready.service\n' > "${tmp_root}/${machine_name}.log"
  : > "${tmp_root}/${machine_name}.sock"
  : > "${tmp_root}/${machine_name}-api.sock"
  : > "${tmp_root}/${machine_name}-gvproxy.sock"
  : > "${tmp_root}/${machine_name}-gvproxy.sock-krun.sock"
}

remove_artifacts() {
  rm -f \
    "${config_path}" \
    "${data_path}" \
    "${state_file}" \
    "${tmp_root}/${machine_name}.log" \
    "${tmp_root}/${machine_name}.sock" \
    "${tmp_root}/${machine_name}-api.sock" \
    "${tmp_root}/${machine_name}-gvproxy.sock" \
    "${tmp_root}/${machine_name}-gvproxy.sock-krun.sock"
}

if [[ "${1:-}" == "--version" ]]; then
  echo "podman version 5.8.1"
  exit 0
fi

if [[ "${1:-}" == "system" && "${2:-}" == "connection" && "${3:-}" == "list" ]]; then
  cat <<OUT
Name        URI
${machine_name}      ssh://core@127.0.0.1:52363/run/user/1000/podman/podman.sock
OUT
  exit 0
fi

if [[ "${1:-}" == "machine" && "${2:-}" == "list" ]]; then
  if [[ "${state}" == "running" ]]; then
    echo "[{\"Name\":\"${machine_name}\",\"VMType\":\"${provider}\",\"Running\":true}]"
  elif [[ "${state}" == "present" ]]; then
    echo "[{\"Name\":\"${machine_name}\",\"VMType\":\"${provider}\",\"Running\":false}]"
  else
    echo "[]"
  fi
  exit 0
fi

if [[ "${1:-}" == "machine" && "${2:-}" == "inspect" && "${3:-}" == "${machine_name}" ]]; then
  if [[ "${state}" == "absent" ]]; then
    echo "machine not found" >&2
    exit 125
  fi
  cat <<OUT
[
  {
    "Name": "${machine_name}",
    "State": "$([[ "${state}" == "running" ]] && printf running || printf stopped)"
  }
]
OUT
  exit 0
fi

if [[ "${1:-}" == "machine" && "${2:-}" == "rm" && "${3:-}" == "-f" && "${4:-}" == "${machine_name}" ]]; then
  if [[ "${state}" == "absent" ]]; then
    echo "machine not found" >&2
    exit 125
  fi
  remove_artifacts
  echo "Machine \"${machine_name}\" removed"
  exit 0
fi

if [[ "${1:-}" == "machine" && "${2:-}" == "init" ]]; then
  write_machine_config
  printf 'fresh raw disk\n' > "${data_path}"
  set_state "present"
  echo "Machine \"${machine_name}\" initialized"
  exit 0
fi

if [[ "${1:-}" == "machine" && "${2:-}" == "start" && "${3:-}" == "${machine_name}" ]]; then
  if [[ ! -f "${config_path}" ]]; then
    echo "machine not initialized" >&2
    exit 125
  fi
  write_ready_artifacts
  set_state "running"
  echo "Machine \"${machine_name}\" started successfully"
  exit 0
fi

if [[ "${1:-}" == "--connection" && "${2:-}" == "${machine_name}" && "${3:-}" == "info" && "${4:-}" == "--debug" ]]; then
  if [[ "${state}" != "running" ]]; then
    echo "machine not running" >&2
    exit 125
  fi
  cat <<OUT
host:
  remoteSocket:
    path: /run/user/1000/podman/podman.sock
version:
  Version: 5.8.1
OUT
  exit 0
fi

if [[ "${1:-}" == "machine" && "${2:-}" == "ssh" && "${3:-}" == "${machine_name}" ]]; then
  if [[ "${state}" != "running" ]]; then
    echo "machine not running" >&2
    exit 255
  fi
  echo "Linux ${machine_name} 6.18.10-200.fc43.aarch64 #1 SMP"
  exit 0
fi

echo "unexpected podman args: $*" >&2
exit 64
EOF

cat > "${bin_dir}/ps" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
cat <<'OUT'
101 1 Ss /opt/homebrew/bin/krunkit --cpus 2 --memory 2048
102 1 Ss /opt/homebrew/bin/gvproxy -listen
OUT
EOF

cat > "${bin_dir}/system_profiler" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == "SPHardwareDataType" ]]; then
  echo "Chip: Apple M2 Max"
  exit 0
fi
if [[ "${1:-}" == "SPSoftwareDataType" ]]; then
  echo "System Version: macOS 15.7.2"
  exit 0
fi
echo "unexpected system_profiler args: $*" >&2
exit 64
EOF

chmod +x "${bin_dir}/podman" "${bin_dir}/ps" "${bin_dir}/system_profiler"

HOME="${home_dir}" bash "${repo_root}/scripts/recreate-podman-machine.sh" \
  --machine testvm \
  --connection testvm \
  --provider libkrun \
  --tmp-root "${tmp_root}" \
  --output-dir "${output_dir}" \
  --podman "${bin_dir}/podman" \
  --ps "${bin_dir}/ps" \
  --system-profiler "${bin_dir}/system_profiler" \
  --cpus 2 \
  --memory 2048 \
  --disk-size 20 \
  --volume /Users:/Users \
  --ssh-command "uname -a"

summary_file="${output_dir}/summary.txt"

for expected_file in \
  "${output_dir}/socket-budget.txt" \
  "${output_dir}/podman-machine-rm-command.txt" \
  "${output_dir}/podman-machine-init-command.txt" \
  "${output_dir}/podman-machine-start-command.txt" \
  "${output_dir}/pre-diagnostics/summary.txt" \
  "${output_dir}/readiness/summary.txt" \
  "${output_dir}/podman-machine-rm.txt" \
  "${output_dir}/podman-machine-init.txt" \
  "${output_dir}/podman-machine-start.txt"
do
  if [[ ! -f "${expected_file}" ]]; then
    echo "expected recreate artifact missing: ${expected_file}" >&2
    exit 70
  fi
done

grep -E "^machine\\.name[[:space:]]+testvm$" "${summary_file}" >/dev/null
grep -E "^machine\\.preexisting[[:space:]]+yes$" "${summary_file}" >/dev/null
grep -E "^artifacts\\.tmp_root[[:space:]]+${tmp_root//\//\\/}$" "${summary_file}" >/dev/null
grep -E "^capture\\.pre_diagnostics[[:space:]]+ok path=.*pre-diagnostics-run\\.txt$" "${summary_file}" >/dev/null
grep -E "^recreate\\.remove_existing[[:space:]]+ok path=.*podman-machine-rm\\.txt$" "${summary_file}" >/dev/null
grep -E "^recreate\\.init[[:space:]]+ok path=.*podman-machine-init\\.txt$" "${summary_file}" >/dev/null
grep -E "^recreate\\.start[[:space:]]+ok path=.*podman-machine-start\\.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.readiness[[:space:]]+ok path=.*readiness-run\\.txt$" "${summary_file}" >/dev/null
grep -E "^readiness\\.result[[:space:]]+ready info=ok ssh=ok$" "${summary_file}" >/dev/null
grep -E "^result[[:space:]]+ready remove_status=0 init_status=0 start_status=0 readiness_status=0$" "${summary_file}" >/dev/null

grep -F "/Users:/Users" "${output_dir}/podman-machine-init-command.txt" >/dev/null
grep -F "Machine \"testvm\" started successfully" "${output_dir}/podman-machine-start.txt" >/dev/null
grep -F "Ignition: user-provided config was applied" "${output_dir}/readiness/diagnostics/machine-log-tail.txt" >/dev/null
grep -F "Linux testvm" "${output_dir}/readiness/podman-machine-ssh.txt" >/dev/null

echo "verified: podman machine recreate helper preserves pre-diagnostics and reaches ready state with the fresh-machine recipe"
