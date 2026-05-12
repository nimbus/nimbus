#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_base="${TMPDIR:-/tmp}"
tmp_base="${tmp_base%/}"
tmp_dir="$(mktemp -d "${tmp_base}/nimbus-podman-machine-diag-verify.XXXXXX")"
tmp_dir="$(cd "${tmp_dir}" && pwd)"
trap 'rm -rf "${tmp_dir}"' EXIT

config_root="${tmp_dir}/config"
data_root="${tmp_dir}/data"
tmp_root="${tmp_dir}/podman"
bin_dir="${tmp_dir}/bin"
output_dir="${tmp_dir}/output"

mkdir -p \
  "${config_root}/libkrun" \
  "${data_root}/libkrun" \
  "${tmp_root}" \
  "${bin_dir}" \
  "${output_dir}"

cat > "${config_root}/libkrun/testvm.json" <<'EOF'
{
  "Name": "testvm",
  "VMType": "libkrun"
}
EOF

printf 'fake raw disk\n' > "${data_root}/libkrun/testvm-arm64.raw"
printf 'line one\nline two\nline three\n' > "${tmp_root}/testvm.log"
: > "${tmp_root}/testvm-api.sock"
: > "${tmp_root}/testvm.sock"
: > "${tmp_root}/testvm-gvproxy.sock"

cat > "${bin_dir}/podman" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == "--version" ]]; then
  echo "podman version 5.8.1"
  exit 0
fi

if [[ "${1:-}" == "info" && "${2:-}" == "--debug" ]]; then
  echo "provider: libkrun"
  echo "version: 5.8.1"
  exit 0
fi

if [[ "${1:-}" == "machine" && "${2:-}" == "list" ]]; then
  echo '[{"Name":"testvm","VMType":"libkrun","Running":true}]'
  exit 0
fi

if [[ "${1:-}" == "machine" && "${2:-}" == "inspect" && "${3:-}" == "testvm" ]]; then
  cat <<'OUT'
[
  {
    "Name": "testvm",
    "State": "running",
    "Rootful": false
  }
]
OUT
  exit 0
fi

echo "unexpected podman args: $*" >&2
exit 64
EOF

cat > "${bin_dir}/ps" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
cat <<'OUT'
101 1 Ss /opt/homebrew/bin/krunkit --cpus 4 --memory 4096
102 1 Ss /opt/homebrew/bin/gvproxy -listen
103 1 Ss podman machine start testvm
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

NIMBUS_DIAGNOSTICS_TEST_UNAME=Darwin \
bash "${repo_root}/scripts/collect-podman-machine-diagnostics.sh" \
  --machine testvm \
  --provider libkrun \
  --output-dir "${output_dir}" \
  --config-root "${config_root}" \
  --data-root "${data_root}" \
  --tmp-root "${tmp_root}" \
  --podman "${bin_dir}/podman" \
  --ps "${bin_dir}/ps" \
  --system-profiler "${bin_dir}/system_profiler" \
  --log-lines 2 \
  > "${output_dir}/stdout.txt"

summary_file="${output_dir}/summary.txt"

for expected_file in \
  "${output_dir}/machine-config.json" \
  "${output_dir}/machine-log-tail.txt" \
  "${output_dir}/podman-version.txt" \
  "${output_dir}/podman-info-debug.txt" \
  "${output_dir}/podman-machine-list.json" \
  "${output_dir}/podman-machine-inspect.txt" \
  "${output_dir}/processes-all.txt" \
  "${output_dir}/processes-matching.txt" \
  "${output_dir}/tmp-root-listing.txt" \
  "${output_dir}/system-profiler-hardware.txt" \
  "${output_dir}/system-profiler-software.txt"
do
  if [[ ! -f "${expected_file}" ]]; then
    echo "expected diagnostics artifact missing: ${expected_file}" >&2
    exit 70
  fi
done

grep -F "machine.name                       testvm" "${summary_file}" >/dev/null
grep -F "machine.provider                   libkrun" "${summary_file}" >/dev/null
grep -F "artifact.machine_config            present source=${config_root}/libkrun/testvm.json copy=${output_dir}/machine-config.json" "${summary_file}" >/dev/null
grep -F "artifact.machine_disk              present path=${data_root}/libkrun/testvm-arm64.raw" "${summary_file}" >/dev/null
grep -F "artifact.machine_log               present path=${tmp_root}/testvm.log tail=${output_dir}/machine-log-tail.txt" "${summary_file}" >/dev/null
grep -F "capture.machine_list               ok path=${output_dir}/podman-machine-list.json" "${summary_file}" >/dev/null
grep -F "capture.processes                  ok all=${output_dir}/processes-all.txt filtered=${output_dir}/processes-matching.txt" "${summary_file}" >/dev/null
grep -F "result                             captured" "${summary_file}" >/dev/null

grep -F "line two" "${output_dir}/machine-log-tail.txt" >/dev/null
grep -F "line three" "${output_dir}/machine-log-tail.txt" >/dev/null
grep -F "provider: libkrun" "${output_dir}/podman-info-debug.txt" >/dev/null
grep -F "testvm" "${output_dir}/podman-machine-inspect.txt" >/dev/null
grep -F "krunkit" "${output_dir}/processes-matching.txt" >/dev/null
grep -F "Chip: Apple M2 Max" "${output_dir}/system-profiler-hardware.txt" >/dev/null

echo "verified: podman machine diagnostics helper captured deterministic artifacts"
