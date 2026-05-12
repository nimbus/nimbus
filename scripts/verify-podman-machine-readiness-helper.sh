#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_base="${TMPDIR:-/tmp}"
tmp_base="${tmp_base%/}"
tmp_dir="$(mktemp -d "${tmp_base}/nimbus-podman-readiness-verify.XXXXXX")"
tmp_dir="$(cd "${tmp_dir}" && pwd)"
trap 'rm -rf "${tmp_dir}"' EXIT

tmp_root="/tmp/podman"
machine_name="testvm"
output_dir="${tmp_dir}/output"
bin_dir="${tmp_dir}/bin"
config_root="${tmp_dir}/config"
data_root="${tmp_dir}/data"

mkdir -p \
  "${output_dir}" \
  "${bin_dir}" \
  "${config_root}/libkrun" \
  "${data_root}/libkrun" \
  "${tmp_root}"

cat > "${config_root}/libkrun/testvm.json" <<'EOF'
{
  "Name": "testvm",
  "VMType": "libkrun"
}
EOF

printf 'fake raw disk\n' > "${data_root}/libkrun/testvm-arm64.raw"
printf 'ready log line\n' > "${tmp_root}/testvm.log"
: > "${tmp_root}/testvm.sock"
: > "${tmp_root}/testvm-api.sock"
: > "${tmp_root}/testvm-gvproxy.sock"
: > "${tmp_root}/testvm-gvproxy.sock-krun.sock"

cat > "${bin_dir}/podman" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

args=("$@")

if [[ "${args[0]:-}" == "--version" ]]; then
  echo "podman version 5.8.1"
  exit 0
fi

if [[ "${args[0]:-}" == "system" && "${args[1]:-}" == "connection" && "${args[2]:-}" == "list" ]]; then
  cat <<'OUT'
Name        URI
testvm      ssh://core@127.0.0.1:52363/run/user/1000/podman/podman.sock
OUT
  exit 0
fi

if [[ "${args[0]:-}" == "machine" && "${args[1]:-}" == "list" ]]; then
  echo '[{"Name":"testvm","VMType":"libkrun","Running":true}]'
  exit 0
fi

if [[ "${args[0]:-}" == "machine" && "${args[1]:-}" == "inspect" && "${args[2]:-}" == "testvm" ]]; then
  cat <<'OUT'
[
  {
    "Name": "testvm",
    "State": "running"
  }
]
OUT
  exit 0
fi

if [[ "${args[0]:-}" == "--connection" && "${args[1]:-}" == "testvm" && "${args[2]:-}" == "info" && "${args[3]:-}" == "--debug" ]]; then
  echo "host:"
  echo "  remoteSocket:"
  echo "    path: /run/user/1000/podman/podman.sock"
  exit 0
fi

if [[ "${args[0]:-}" == "machine" && "${args[1]:-}" == "ssh" && "${args[2]:-}" == "testvm" ]]; then
  echo "Linux testvm 6.13.0 #1 SMP"
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
exit 64
EOF

chmod +x "${bin_dir}/podman" "${bin_dir}/ps" "${bin_dir}/system_profiler"

bash "${repo_root}/scripts/validate-podman-machine-readiness.sh" \
  --machine "${machine_name}" \
  --connection "${machine_name}" \
  --provider libkrun \
  --tmp-root "${tmp_root}" \
  --output-dir "${output_dir}" \
  --podman "${bin_dir}/podman" \
  --ps "${bin_dir}/ps" \
  --system-profiler "${bin_dir}/system_profiler" \
  --ssh-command "uname -a"

summary_file="${output_dir}/summary.txt"

for expected_file in \
  "${output_dir}/socket-budget.txt" \
  "${output_dir}/diagnostics/summary.txt" \
  "${output_dir}/podman-system-connection-list.txt" \
  "${output_dir}/podman-machine-inspect.txt" \
  "${output_dir}/podman-info-connection.txt" \
  "${output_dir}/podman-machine-ssh.txt" \
  "${output_dir}/machine-ssh-command.txt"
do
  if [[ ! -f "${expected_file}" ]]; then
    echo "expected readiness artifact missing: ${expected_file}" >&2
    exit 70
  fi
done

grep -E "^machine\\.name[[:space:]]+testvm$" "${summary_file}" >/dev/null
grep -E "^podman\\.connection[[:space:]]+testvm$" "${summary_file}" >/dev/null
grep -E "^capture\\.socket_budget[[:space:]]+ok path=.*socket-budget\\.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.diagnostics[[:space:]]+ok path=.*diagnostics-run\\.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.info_via_connection[[:space:]]+ok path=.*podman-info-connection\\.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.machine_ssh[[:space:]]+ok path=.*podman-machine-ssh\\.txt$" "${summary_file}" >/dev/null
grep -E "^result[[:space:]]+ready info=ok ssh=ok$" "${summary_file}" >/dev/null

grep -E "^path\\.gvproxy_krun[[:space:]]+/tmp/podman/testvm-gvproxy\\.sock-krun\\.sock$" "${output_dir}/socket-budget.txt" >/dev/null
grep -E "^result[[:space:]]+ok offending=none longest=path\\.gvproxy_krun length=[0-9]+ max_path_chars=103$" "${output_dir}/socket-budget.txt" >/dev/null
grep -F "Linux testvm" "${output_dir}/podman-machine-ssh.txt" >/dev/null
grep -F "remoteSocket" "${output_dir}/podman-info-connection.txt" >/dev/null
grep -F "ready log line" "${output_dir}/diagnostics/machine-log-tail.txt" >/dev/null

echo "verified: podman machine readiness helper captures connection, ssh, diagnostics, and socket-budget evidence"
