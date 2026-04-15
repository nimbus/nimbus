#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_base="${TMPDIR:-/tmp}"
tmp_base="${tmp_base%/}"
tmp_dir="$(mktemp -d "${tmp_base}/neovex-machine-guest-proof-verify.XXXXXX")"
tmp_dir="$(cd "${tmp_dir}" && pwd)"
trap 'rm -rf "${tmp_dir}"' EXIT

home_dir="${tmp_dir}/home"
runtime_root="${tmp_dir}/runtime-root"
bin_dir="${tmp_dir}/bin"
output_dir="${tmp_dir}/output"
image_path="${tmp_dir}/neovex-machine-os.raw.gz"

mkdir -p "${home_dir}" "${runtime_root}" "${bin_dir}" "${output_dir}"
printf 'fake image artifact\n' > "${image_path}"
printf 'guest boot line one\nguest boot line two\n' > "${runtime_root}/default.log"

cat > "${bin_dir}/neovex" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" != "machine" ]]; then
  echo "unexpected args: $*" >&2
  exit 64
fi

case "${2:-}" in
  status)
    cat <<'OUT'
result: status
lifecycle: running
manager: ready
OUT
    ;;
  ssh)
    shift 2
    if [[ "${1:-}" != "--" ]]; then
      echo "expected machine ssh -- ..." >&2
      exit 64
    fi
    shift
    rendered="$*"
    case "${rendered}" in
      "/usr/local/bin/neovex --version")
        echo "neovex 0.1.2"
        ;;
      *"sha256sum /usr/local/bin/neovex"*)
        echo "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  /usr/local/bin/neovex"
        ;;
      *"for bin in buildah conmon crun netavark aardvark-dns fuse-overlayfs"*)
        cat <<'OUT'
present buildah /usr/bin/buildah
present conmon /usr/bin/conmon
present crun /usr/bin/crun
present netavark /usr/bin/netavark
present aardvark-dns /usr/libexec/aardvark-dns
present fuse-overlayfs /usr/bin/fuse-overlayfs
OUT
        ;;
      *"systemctl show --no-pager --property=Id,LoadState,UnitFileState,ActiveState,SubState neovex.socket || true"*)
        cat <<'OUT'
Id=neovex.socket
LoadState=loaded
UnitFileState=enabled
ActiveState=active
SubState=listening
OUT
        ;;
      *"systemctl show --no-pager --property=Id,LoadState,UnitFileState,ActiveState,SubState neovex.service || true"*)
        cat <<'OUT'
Id=neovex.service
LoadState=loaded
UnitFileState=disabled
ActiveState=active
SubState=running
OUT
        ;;
      *"findmnt --noheadings --output TARGET,SOURCE,FSTYPE,OPTIONS -T '/Users' || stat '/Users'"*)
        echo "/Users neovex-users virtiofs rw,nosuid,nodev"
        ;;
      *"GET /healthz HTTP/1.0"*)
        cat <<'OUT'
HTTP/1.0 200 OK
content-type: application/json

{"status":"ok","role":"guest-machine-api","protocol_version":"v1alpha1"}
OUT
        ;;
      *"GET /v1/machine-api/capabilities HTTP/1.0"*)
        cat <<'OUT'
HTTP/1.0 200 OK
content-type: application/json

{"protocol_version":"v1alpha1","service_execution_ready":false,"service_execution_mode":"standard_containers","supported_service_backends":["container"],"supported_operations":["healthz","capabilities"],"required_binaries":[{"name":"buildah","present":true,"resolved_path":"/usr/bin/buildah"}],"service_execution_blockers":["guest machine API does not yet expose service lifecycle operations"]}
OUT
        ;;
      *)
        echo "unexpected machine ssh command: ${rendered}" >&2
        exit 64
        ;;
    esac
    ;;
  *)
    echo "unexpected machine subcommand: ${2:-}" >&2
    exit 64
    ;;
esac
EOF

chmod +x "${bin_dir}/neovex"

bash "${repo_root}/scripts/collect-neovex-machine-guest-proof.sh" \
  --home "${home_dir}" \
  --runtime-root "${runtime_root}" \
  --output-dir "${output_dir}" \
  --neovex "${bin_dir}/neovex" \
  --image "${image_path}" \
  > "${output_dir}/stdout.txt"

summary_file="${output_dir}/summary.txt"

for expected_file in \
  "${output_dir}/machine-status.txt" \
  "${output_dir}/guest-neovex-version.txt" \
  "${output_dir}/guest-neovex-sha256.txt" \
  "${output_dir}/guest-required-binaries.txt" \
  "${output_dir}/guest-neovex-socket-status.txt" \
  "${output_dir}/guest-neovex-service-status.txt" \
  "${output_dir}/guest-virtiofs-mount.txt" \
  "${output_dir}/guest-machine-api-health.txt" \
  "${output_dir}/guest-machine-api-capabilities.txt" \
  "${output_dir}/machine-log-tail.txt"
do
  if [[ ! -f "${expected_file}" ]]; then
    echo "expected guest-proof artifact missing: ${expected_file}" >&2
    exit 70
  fi
done

grep -F "image.artifact                     ${image_path}" "${summary_file}" >/dev/null
grep -E "^capture\\.machine_status[[:space:]]+ok path=${output_dir}/machine-status.txt cmd=${output_dir}/machine-status-command.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.guest_neovex_version[[:space:]]+ok path=${output_dir}/guest-neovex-version.txt cmd=${output_dir}/guest-neovex-version-command.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.guest_neovex_sha256[[:space:]]+ok path=${output_dir}/guest-neovex-sha256.txt cmd=${output_dir}/guest-neovex-sha256-command.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.guest_machine_api_health[[:space:]]+ok path=${output_dir}/guest-machine-api-health.txt cmd=${output_dir}/guest-machine-api-health-command.txt$" "${summary_file}" >/dev/null
grep -E "^artifact\\.machine_log_tail[[:space:]]+present path=${runtime_root}/default.log tail=${output_dir}/machine-log-tail.txt$" "${summary_file}" >/dev/null
grep -F "result                             captured" "${summary_file}" >/dev/null

grep -F "manager: ready" "${output_dir}/machine-status.txt" >/dev/null
grep -F "neovex 0.1.2" "${output_dir}/guest-neovex-version.txt" >/dev/null
grep -F "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  /usr/local/bin/neovex" "${output_dir}/guest-neovex-sha256.txt" >/dev/null
grep -F "present buildah /usr/bin/buildah" "${output_dir}/guest-required-binaries.txt" >/dev/null
grep -F "SubState=listening" "${output_dir}/guest-neovex-socket-status.txt" >/dev/null
grep -F "SubState=running" "${output_dir}/guest-neovex-service-status.txt" >/dev/null
grep -F "virtiofs" "${output_dir}/guest-virtiofs-mount.txt" >/dev/null
grep -F '"status":"ok"' "${output_dir}/guest-machine-api-health.txt" >/dev/null
grep -F '"service_execution_mode":"standard_containers"' "${output_dir}/guest-machine-api-capabilities.txt" >/dev/null
grep -F "guest boot line two" "${output_dir}/machine-log-tail.txt" >/dev/null

echo "verified: neovex machine guest proof helper captured deterministic guest-image evidence"
