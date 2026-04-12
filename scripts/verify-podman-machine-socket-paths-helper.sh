#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_base="${TMPDIR:-/tmp}"
tmp_base="${tmp_base%/}"
tmp_dir="$(mktemp -d "${tmp_base}/neovex-podman-socket-paths-verify.XXXXXX")"
tmp_dir="$(cd "${tmp_dir}" && pwd)"
trap 'rm -rf "${tmp_dir}"' EXIT

machine_name="neovex-libkrun-users-only"
long_tmp_root="/var/folders/kw/d608x5pn4cq73rz78ztl92cw0000gn/T/podman"
short_tmp_root="/tmp/podman"

long_output="${tmp_dir}/long-root.txt"
short_output="${tmp_dir}/short-root.txt"

bash "${repo_root}/scripts/check-podman-machine-socket-paths.sh" \
  --machine "${machine_name}" \
  --tmp-root "${long_tmp_root}" \
  > "${long_output}"

bash "${repo_root}/scripts/check-podman-machine-socket-paths.sh" \
  --machine "${machine_name}" \
  --tmp-root "${short_tmp_root}" \
  > "${short_output}"

grep -E "^machine\\.name[[:space:]]+${machine_name}\$" "${long_output}" >/dev/null
grep -E "^artifacts\\.tmp_root[[:space:]]+${long_tmp_root}\$" "${long_output}" >/dev/null
grep -E "^path\\.ready\\.length[[:space:]]+86\$" "${long_output}" >/dev/null
grep -E "^path\\.api\\.length[[:space:]]+90\$" "${long_output}" >/dev/null
grep -E "^path\\.gvproxy\\.length[[:space:]]+94\$" "${long_output}" >/dev/null
grep -E "^path\\.gvproxy_krun\\.length[[:space:]]+104\$" "${long_output}" >/dev/null
grep -E "^result[[:space:]]+too_long offending=path\\.gvproxy_krun longest=path\\.gvproxy_krun length=104 max_path_chars=103\$" "${long_output}" >/dev/null

grep -E "^artifacts\\.tmp_root[[:space:]]+${short_tmp_root}\$" "${short_output}" >/dev/null
grep -E "^path\\.ready\\.length[[:space:]]+42\$" "${short_output}" >/dev/null
grep -E "^path\\.api\\.length[[:space:]]+46\$" "${short_output}" >/dev/null
grep -E "^path\\.gvproxy\\.length[[:space:]]+50\$" "${short_output}" >/dev/null
grep -E "^path\\.gvproxy_krun\\.length[[:space:]]+60\$" "${short_output}" >/dev/null
grep -E "^result[[:space:]]+ok offending=none longest=path\\.gvproxy_krun length=60 max_path_chars=103\$" "${short_output}" >/dev/null

echo "verified: podman machine socket-path helper detects Darwin TMPDIR overflow and /tmp mitigation"
