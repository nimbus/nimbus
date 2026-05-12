#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_base="${TMPDIR:-/tmp}"
tmp_base="${tmp_base%/}"
tmp_dir="$(mktemp -d "${tmp_base}/nimbus-machine-diag-verify.XXXXXX")"
tmp_dir="$(cd "${tmp_dir}" && pwd)"
trap 'rm -rf "${tmp_dir}"' EXIT

home_dir="${tmp_dir}/home"
runtime_root="${tmp_dir}/runtime-root"
bin_dir="${tmp_dir}/bin"
output_dir="${tmp_dir}/output"

mkdir -p \
  "${home_dir}/.config/nimbus/machine/default" \
  "${home_dir}/.local/state/nimbus/machine/default" \
  "${runtime_root}" \
  "${bin_dir}" \
  "${output_dir}"

cat > "${home_dir}/.config/nimbus/machine/default/config.json" <<'EOF'
{"name":"default","provider":"krunkit"}
EOF

cat > "${home_dir}/.local/state/nimbus/machine/default/status.json" <<'EOF'
{"lifecycle":"running","manager":"ready"}
EOF

printf '123\n' > "${runtime_root}/default-gvproxy.pid"
printf '456\n' > "${runtime_root}/default-krunkit.pid"
printf 'machine one\nmachine two\n' > "${runtime_root}/default.log"
printf 'gvproxy one\ngvproxy two\n' > "${runtime_root}/default-gvproxy.log"
printf 'krunkit one\nkrunkit two\n' > "${runtime_root}/default-krunkit.log"
: > "${runtime_root}/default.sock"
: > "${runtime_root}/default-ignition.sock"
: > "${runtime_root}/default-gvproxy.sock"
: > "${runtime_root}/default-krunkit.sock"

cat > "${bin_dir}/nimbus" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == "machine" && "${2:-}" == "status" ]]; then
  cat <<'OUT'
result: status
lifecycle: running
manager: ready
OUT
  exit 0
fi

echo "unexpected nimbus args: $*" >&2
exit 64
EOF

cat > "${bin_dir}/ps" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
cat <<'OUT'
123 1 /opt/homebrew/bin/gvproxy -listen-vfkit unixgram:///tmp/test.sock
456 1 /opt/homebrew/bin/krunkit --cpus 2 --memory 2048
789 1 /Users/jack/src/github.com/nimbus/nimbus/target/debug/nimbus machine start
OUT
EOF

chmod +x "${bin_dir}/nimbus" "${bin_dir}/ps"

bash "${repo_root}/scripts/collect-nimbus-machine-diagnostics.sh" \
  --home "${home_dir}" \
  --runtime-root "${runtime_root}" \
  --output-dir "${output_dir}" \
  --nimbus "${bin_dir}/nimbus" \
  --ps "${bin_dir}/ps" \
  --log-lines 1 \
  > "${output_dir}/stdout.txt"

summary_file="${output_dir}/summary.txt"

for expected_file in \
  "${output_dir}/machine-config.json" \
  "${output_dir}/machine-state.json" \
  "${output_dir}/runtime-dir-listing.txt" \
  "${output_dir}/socket-inventory.txt" \
  "${output_dir}/machine-log-tail.txt" \
  "${output_dir}/gvproxy-log-tail.txt" \
  "${output_dir}/krunkit-log-tail.txt" \
  "${output_dir}/processes-all.txt" \
  "${output_dir}/processes-matching.txt" \
  "${output_dir}/nimbus-machine-status.txt"
do
  if [[ ! -f "${expected_file}" ]]; then
    echo "expected diagnostics artifact missing: ${expected_file}" >&2
    exit 70
  fi
done

grep -F "machine.name                       default" "${summary_file}" >/dev/null
grep -F "artifact.machine_config            present source=${home_dir}/.config/nimbus/machine/default/config.json copy=${output_dir}/machine-config.json" "${summary_file}" >/dev/null
grep -F "artifact.machine_state             present source=${home_dir}/.local/state/nimbus/machine/default/status.json copy=${output_dir}/machine-state.json" "${summary_file}" >/dev/null
grep -E "^artifact\\.socket_inventory[[:space:]]+ok path=${output_dir}/socket-inventory.txt$" "${summary_file}" >/dev/null
grep -E "^artifact\\.socket_presence[[:space:]]+ok path=${output_dir}/socket-presence.txt$" "${summary_file}" >/dev/null
grep -F "capture.nimbus_machine_status      ok path=${output_dir}/nimbus-machine-status.txt" "${summary_file}" >/dev/null
grep -F "result                             captured" "${summary_file}" >/dev/null

grep -F "default-api.sock missing" "${output_dir}/socket-presence.txt" >/dev/null
grep -F "default.sock present" "${output_dir}/socket-presence.txt" >/dev/null
grep -F "machine two" "${output_dir}/machine-log-tail.txt" >/dev/null
grep -F "krunkit" "${output_dir}/processes-matching.txt" >/dev/null
grep -F "manager: ready" "${output_dir}/nimbus-machine-status.txt" >/dev/null

echo "verified: nimbus machine diagnostics helper captured deterministic artifacts"
