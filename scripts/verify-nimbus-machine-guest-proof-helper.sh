#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_base="${TMPDIR:-/tmp}"
tmp_base="${tmp_base%/}"
tmp_dir="$(mktemp -d "${tmp_base}/nimbus-machine-guest-proof-verify.XXXXXX")"
tmp_dir="$(cd "${tmp_dir}" && pwd)"
trap 'rm -rf "${tmp_dir}"' EXIT
expected_nimbus_version="$(
  sed -n 's/^version = "\(.*\)"$/\1/p' \
    "${repo_root}/Cargo.toml" \
    | head -n1
)"

if [[ -z "${expected_nimbus_version}" ]]; then
  echo "failed to resolve expected nimbus version from workspace Cargo.toml" >&2
  exit 70
fi
export EXPECTED_NIMBUS_VERSION="${expected_nimbus_version}"

home_dir="${tmp_dir}/home"
runtime_root="${tmp_dir}/runtime-root"
bin_dir="${tmp_dir}/bin"
output_dir="${tmp_dir}/output"
image_path="${tmp_dir}/nimbus-machine-os.raw.gz"
selinux_checker="${tmp_dir}/selinux-avc-checker"
identity_path="${tmp_dir}/machine-key"
ssh_port="20022"

mkdir -p "${home_dir}" "${runtime_root}" "${bin_dir}" "${output_dir}"
printf 'fake image artifact\n' > "${image_path}"
printf 'fake ssh identity\n' > "${identity_path}"
chmod 0600 "${identity_path}"
printf 'guest boot line one\nguest boot line two\n' > "${runtime_root}/default.log"
export FAKE_NIMBUS_RUNTIME_ROOT="${runtime_root}"
export FAKE_SSH_IDENTITY_PATH="${identity_path}"
export FAKE_SSH_PORT="${ssh_port}"

cat > "${bin_dir}/nimbus" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" != "machine" ]]; then
  echo "unexpected args: $*" >&2
  exit 64
fi

case "${2:-}" in
  status)
    if [[ -n "${3:-}" && "${3:-}" != "default" ]]; then
      echo "expected default machine status, got ${3:-<missing>}" >&2
      exit 64
    fi
    cat <<'OUT'
result: status
lifecycle: running
manager: ready
OUT
    ;;
  inspect)
    if [[ "${3:-}" != "default" || "${4:-}" != "-f" || "${5:-}" != "json" ]]; then
      echo "expected machine inspect default -f json" >&2
      exit 64
    fi
    cat <<OUT
{
  "config": {
    "name": "default",
    "guest": {
      "ssh_user": "nimbus",
      "ssh_identity_path": "${FAKE_SSH_IDENTITY_PATH}"
    },
    "roots": {
      "runtime_root": "${FAKE_NIMBUS_RUNTIME_ROOT}"
    }
  },
  "state": {
    "lifecycle": "running",
    "manager": "ready",
    "runtime": {
      "ssh_port": ${FAKE_SSH_PORT}
    }
  }
}
OUT
    ;;
  ssh)
    shift 2
    if [[ "${1:-}" != "--" ]]; then
      if [[ "${1:-}" != "default" ]]; then
        echo "expected default machine name before --, got ${1:-<missing>}" >&2
        exit 64
      fi
      shift
    fi
    if [[ "${1:-}" != "--" ]]; then
      echo "expected machine ssh -- ..." >&2
      exit 64
    fi
    shift
    rendered="$*"
    case "${rendered}" in
      "/usr/local/bin/nimbus --version")
        echo "nimbus ${EXPECTED_NIMBUS_VERSION}"
        ;;
      *"sha256sum /usr/local/bin/nimbus"*)
        echo "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  /usr/local/bin/nimbus"
        ;;
      *"for bin in buildah conmon crun netavark aardvark-dns fuse-overlayfs"*)
        cat <<'OUT'
present buildah /usr/bin/buildah
present conmon /usr/bin/conmon
present crun /usr/bin/crun
present netavark /usr/libexec/podman/netavark
present aardvark-dns /usr/libexec/podman/aardvark-dns
present fuse-overlayfs /usr/bin/fuse-overlayfs
OUT
        ;;
      *"systemctl show --no-pager --property=Id,LoadState,UnitFileState,ActiveState,SubState nimbus.socket || true"*)
        cat <<'OUT'
Id=nimbus.socket
LoadState=loaded
UnitFileState=enabled
ActiveState=active
SubState=listening
OUT
        ;;
      *"systemctl show --no-pager --property=Id,LoadState,UnitFileState,ActiveState,SubState nimbus.service || true"*)
        cat <<'OUT'
Id=nimbus.service
LoadState=loaded
UnitFileState=disabled
ActiveState=active
SubState=running
OUT
        ;;
      *"findmnt --noheadings --output TARGET,SOURCE,FSTYPE,OPTIONS -T \"/Users\" || stat \"/Users\""*)
        echo "/Users nimbus-users virtiofs rw,nosuid,nodev"
        ;;
      *"command -v getenforce"*)
        echo "Enforcing"
        ;;
      *"rpm -q bootupd selinux-policy selinux-policy-targeted systemd util-linux-core podman crun netavark aardvark-dns bootc policycoreutils"*)
        cat <<'OUT'
# package versions
bootupd-0.2.33-1.fc44.aarch64
selinux-policy-44.1-1.fc44.noarch
selinux-policy-targeted-44.1-1.fc44.noarch
systemd-259.5-1.fc44.aarch64
util-linux-core-2.41.4-7.fc44.aarch64
podman-5.8.2-1.fc44.aarch64
crun-1.27.1-1.fc44.aarch64
netavark-1.17.2-1.fc44.aarch64
aardvark-dns-1.17.2-1.fc44.aarch64
bootc-1.3.0-1.fc44.aarch64
policycoreutils-3.9-1.fc44.aarch64
# bootloader units
bootloader-update.service enabled -
# bootloader-update.service
[Service]
ExecStart=/usr/bin/bootupctl update
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

chmod +x "${bin_dir}/nimbus"

cat > "${bin_dir}/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

rendered="$*"
case "${rendered}" in
  *"--unix-socket ${FAKE_NIMBUS_RUNTIME_ROOT}/default-api.sock http://localhost/healthz"*)
    cat <<'OUT'
HTTP/1.1 200 OK
content-type: application/json

{"status":"ok","role":"guest-machine-api","protocol_version":"v1alpha2","listen_mode":"systemd-socket-activation","control_data_dir":"/var/lib/nimbus/control"}
OUT
    ;;
  *"--unix-socket ${FAKE_NIMBUS_RUNTIME_ROOT}/default-api.sock http://localhost/v1/machine-api/capabilities"*)
    cat <<'OUT'
HTTP/1.1 200 OK
content-type: application/json

{"protocol_version":"v1alpha2","service_execution_ready":true,"service_execution_mode":"standard_containers","supported_service_backends":["container"],"supported_operations":["healthz","capabilities","service-sandboxes.image-start","service-sandboxes.list","service-sandboxes.inspect","service-sandboxes.stop","service-sandboxes.logs","service-sandboxes.ps","os.bootc.status","os.bootc.switch","os.bootc.upgrade","os.bootc.rollback"],"binary_statuses":[{"name":"buildah","present":true,"resolved_path":"/usr/bin/buildah","required_for_operations":["service-sandboxes.image-start"]}],"operation_statuses":[{"name":"service-sandboxes.image-start","available":true,"blockers":[]}],"service_execution_blockers":[]}
OUT
    ;;
  *"--unix-socket ${FAKE_NIMBUS_RUNTIME_ROOT}/default-api.sock http://localhost/v1/machine-api/os/bootc/status"*)
    cat <<'OUT'
HTTP/1.1 200 OK
content-type: application/json

{"status":{"status":{"booted":{"image":{"image":{"image":"ghcr.io/nimbus/nimbus-machine-os:v9.9.9"},"imageDigest":"sha256:9999999999999999999999999999999999999999999999999999999999999999"}},"staged":null,"rollback":null}},"booted_image":"ghcr.io/nimbus/nimbus-machine-os:v9.9.9","booted_digest":"sha256:9999999999999999999999999999999999999999999999999999999999999999","staged_image":null,"staged_digest":null,"rollback_image":null,"rollback_digest":null}
OUT
    ;;
  *)
    echo "unexpected curl command: ${rendered}" >&2
    exit 64
    ;;
esac
EOF
chmod +x "${bin_dir}/curl"

cat > "${bin_dir}/ssh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

rendered="$*"
case "${rendered}" in
  *"-i ${FAKE_SSH_IDENTITY_PATH}"*"-p ${FAKE_SSH_PORT}"*"root@127.0.0.1"*"/bin/sh -lc "*"bootc status --json"*)
    cat <<'OUT'
{"status":{"booted":{"image":{"image":{"image":"ghcr.io/nimbus/nimbus-machine-os:v9.9.9"},"imageDigest":"sha256:9999999999999999999999999999999999999999999999999999999999999999"}},"staged":null,"rollback":null}}
OUT
    ;;
  *"-i ${FAKE_SSH_IDENTITY_PATH}"*"-p ${FAKE_SSH_PORT}"*"root@127.0.0.1"*"/bin/sh -lc "*"ps -eZ"*"semodule --list-modules=full"*)
    cat <<'OUT'
# process labels
system_u:system_r:container_runtime_t:s0 1000 ? 00:00:00 nimbus
system_u:system_r:sshd_t:s0 999 ? 00:00:00 sshd
# file labels
system_u:object_r:container_var_run_t:s0 /run/nimbus/nimbus.sock
system_u:object_r:bin_t:s0 /usr/local/bin/nimbus
# selinux modules
400 nimbus-machine-api cil
# relevant booleans
container_manage_cgroup --> off
OUT
    ;;
  *"-i ${FAKE_SSH_IDENTITY_PATH}"*"-p ${FAKE_SSH_PORT}"*"root@127.0.0.1"*"/bin/sh -lc "*"ausearch -m AVC -ts boot"*)
    echo "<no matches>"
    ;;
  *)
    echo "unexpected ssh command: ${rendered}" >&2
    exit 64
    ;;
esac
EOF
chmod +x "${bin_dir}/ssh"

cat > "${selinux_checker}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" != "--audit-log" || ! -f "${2:-}" ]]; then
  echo "expected --audit-log <path>" >&2
  exit 64
fi
printf 'checked %s\n' "$2"
EOF
chmod +x "${selinux_checker}"

PATH="${bin_dir}:${PATH}" bash "${repo_root}/scripts/collect-nimbus-machine-guest-proof.sh" \
  --home "${home_dir}" \
  --runtime-root "${runtime_root}" \
  --output-dir "${output_dir}" \
  --nimbus "${bin_dir}/nimbus" \
  --image "${image_path}" \
  --selinux-avc-checker "${selinux_checker}" \
  > "${output_dir}/stdout.txt"

summary_file="${output_dir}/summary.txt"

for expected_file in \
  "${output_dir}/machine-status.txt" \
  "${output_dir}/machine-inspect.txt" \
  "${output_dir}/guest-nimbus-version.txt" \
  "${output_dir}/guest-nimbus-sha256.txt" \
  "${output_dir}/guest-required-binaries.txt" \
  "${output_dir}/guest-nimbus-socket-status.txt" \
  "${output_dir}/guest-nimbus-service-status.txt" \
  "${output_dir}/guest-virtiofs-mount.txt" \
  "${output_dir}/guest-machine-api-health.txt" \
  "${output_dir}/guest-machine-api-capabilities.txt" \
  "${output_dir}/guest-machine-api-bootc-status.txt" \
  "${output_dir}/guest-bootc-status.txt" \
  "${output_dir}/guest-selinux-mode.txt" \
  "${output_dir}/guest-package-context.txt" \
  "${output_dir}/guest-selinux-context.txt" \
  "${output_dir}/guest-selinux-avcs.txt" \
  "${output_dir}/guest-selinux-avc-check.txt" \
  "${output_dir}/machine-log-tail.txt"
do
  if [[ ! -f "${expected_file}" ]]; then
    echo "expected guest-proof artifact missing: ${expected_file}" >&2
    exit 70
  fi
done

grep -F "image.artifact                     ${image_path}" "${summary_file}" >/dev/null
grep -F "host.api_socket_path               ${runtime_root}/default-api.sock" "${summary_file}" >/dev/null
grep -F "guest.binary_path                  /usr/local/bin/nimbus" "${summary_file}" >/dev/null
grep -F "selinux.avc_checker                ${selinux_checker}" "${summary_file}" >/dev/null
grep -F "privileged.guest_evidence          root-ssh port=${ssh_port} identity=${identity_path}" "${summary_file}" >/dev/null
grep -E "^capture\\.machine_status[[:space:]]+ok path=${output_dir}/machine-status.txt cmd=${output_dir}/machine-status-command.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.machine_inspect[[:space:]]+ok path=${output_dir}/machine-inspect.txt cmd=${output_dir}/machine-inspect-command.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.guest_nimbus_version[[:space:]]+ok path=${output_dir}/guest-nimbus-version.txt cmd=${output_dir}/guest-nimbus-version-command.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.guest_nimbus_sha256[[:space:]]+ok path=${output_dir}/guest-nimbus-sha256.txt cmd=${output_dir}/guest-nimbus-sha256-command.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.guest_machine_api_health[[:space:]]+ok path=${output_dir}/guest-machine-api-health.txt cmd=${output_dir}/guest-machine-api-health-command.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.guest_machine_api_bootc_status[[:space:]]+ok path=${output_dir}/guest-machine-api-bootc-status.txt cmd=${output_dir}/guest-machine-api-bootc-status-command.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.guest_selinux_mode[[:space:]]+ok path=${output_dir}/guest-selinux-mode.txt cmd=${output_dir}/guest-selinux-mode-command.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.guest_package_context[[:space:]]+ok path=${output_dir}/guest-package-context.txt cmd=${output_dir}/guest-package-context-command.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.guest_bootc_status[[:space:]]+ok path=${output_dir}/guest-bootc-status.txt cmd=${output_dir}/guest-bootc-status-command.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.guest_selinux_context[[:space:]]+ok path=${output_dir}/guest-selinux-context.txt cmd=${output_dir}/guest-selinux-context-command.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.guest_selinux_avcs[[:space:]]+ok path=${output_dir}/guest-selinux-avcs.txt cmd=${output_dir}/guest-selinux-avcs-command.txt$" "${summary_file}" >/dev/null
grep -E "^check\\.guest_selinux_avcs[[:space:]]+ok path=${output_dir}/guest-selinux-avc-check.txt cmd=${output_dir}/guest-selinux-avc-check-command.txt$" "${summary_file}" >/dev/null
grep -E "^artifact\\.machine_log_tail[[:space:]]+present path=${runtime_root}/default.log tail=${output_dir}/machine-log-tail.txt$" "${summary_file}" >/dev/null
grep -F "result                             captured" "${summary_file}" >/dev/null

grep -F "manager: ready" "${output_dir}/machine-status.txt" >/dev/null
grep -F "\"ssh_identity_path\": \"${identity_path}\"" "${output_dir}/machine-inspect.txt" >/dev/null
grep -F "nimbus ${expected_nimbus_version}" "${output_dir}/guest-nimbus-version.txt" >/dev/null
grep -F "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  /usr/local/bin/nimbus" "${output_dir}/guest-nimbus-sha256.txt" >/dev/null
grep -F "present buildah /usr/bin/buildah" "${output_dir}/guest-required-binaries.txt" >/dev/null
grep -F "SubState=listening" "${output_dir}/guest-nimbus-socket-status.txt" >/dev/null
grep -F "SubState=running" "${output_dir}/guest-nimbus-service-status.txt" >/dev/null
grep -F "virtiofs" "${output_dir}/guest-virtiofs-mount.txt" >/dev/null
grep -F '"status":"ok"' "${output_dir}/guest-machine-api-health.txt" >/dev/null
grep -F '"service_execution_mode":"standard_containers"' "${output_dir}/guest-machine-api-capabilities.txt" >/dev/null
grep -F '"booted_digest":"sha256:9999999999999999999999999999999999999999999999999999999999999999"' "${output_dir}/guest-machine-api-bootc-status.txt" >/dev/null
grep -F '"imageDigest":"sha256:9999999999999999999999999999999999999999999999999999999999999999"' "${output_dir}/guest-bootc-status.txt" >/dev/null
grep -F "Enforcing" "${output_dir}/guest-selinux-mode.txt" >/dev/null
grep -F "bootupd-0.2.33-1.fc44.aarch64" "${output_dir}/guest-package-context.txt" >/dev/null
grep -F "selinux-policy-targeted-44.1-1.fc44.noarch" "${output_dir}/guest-package-context.txt" >/dev/null
grep -F "ExecStart=/usr/bin/bootupctl update" "${output_dir}/guest-package-context.txt" >/dev/null
grep -F "container_runtime_t" "${output_dir}/guest-selinux-context.txt" >/dev/null
grep -F "container_var_run_t" "${output_dir}/guest-selinux-context.txt" >/dev/null
grep -F "nimbus-machine-api" "${output_dir}/guest-selinux-context.txt" >/dev/null
grep -F "<no matches>" "${output_dir}/guest-selinux-avcs.txt" >/dev/null
grep -F "checked ${output_dir}/guest-selinux-avcs.txt" "${output_dir}/guest-selinux-avc-check.txt" >/dev/null
grep -F "guest boot line two" "${output_dir}/machine-log-tail.txt" >/dev/null

echo "verified: nimbus machine guest proof helper captured deterministic guest-image evidence"
