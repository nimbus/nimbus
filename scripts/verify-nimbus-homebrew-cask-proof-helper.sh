#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_base="${TMPDIR:-/tmp}"
tmp_base="${tmp_base%/}"
tmp_dir="$(mktemp -d "${tmp_base}/nimbus-homebrew-cask-proof-verify.XXXXXX")"
tmp_dir="$(cd "${tmp_dir}" && pwd)"
runtime_tmp_base="/private/tmp"
if [[ ! -d "${runtime_tmp_base}" ]]; then
  runtime_tmp_base="/tmp"
fi
short_runtime_root="$(mktemp -d "${runtime_tmp_base}/nimbus-homebrew-runtime.XXXXXX")"
cleanup() {
  local status="$1"
  if [[ "${status}" -eq 0 ]]; then
    rm -rf "${tmp_dir}"
    rm -rf "${short_runtime_root}"
  else
    echo "debug: preserved helper tmp dir at ${tmp_dir}" >&2
    echo "debug: preserved helper runtime root at ${short_runtime_root}" >&2
  fi
}
trap 'cleanup "$?"' EXIT

output_dir="${tmp_dir}/output"
home_dir="${tmp_dir}/home"
runtime_root="${short_runtime_root}"
bin_dir="${tmp_dir}/bin"
brew_prefix="${tmp_dir}/brew-prefix"
tap_root="${tmp_dir}/taps"
state_dir="${tmp_dir}/state"
host_binary="${bin_dir}/host-nimbus"
gvproxy_binary="${bin_dir}/gvproxy"
brew_bin="${bin_dir}/brew"
ssh_keygen_bin="${bin_dir}/ssh-keygen"
host_version="$(awk -F'"' '/^version = / { print $2; exit }' "${repo_root}/Cargo.toml")"
machine_name="default"

mkdir -p \
  "${output_dir}" \
  "${home_dir}" \
  "${runtime_root}" \
  "${bin_dir}" \
  "${brew_prefix}/bin" \
  "${brew_prefix}/Caskroom" \
  "${brew_prefix}/opt/podman/libexec/podman" \
  "${tap_root}" \
  "${state_dir}"

cat > "${host_binary}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

host_version="__HOST_VERSION__"
runtime_root="${NIMBUS_MACHINE_RUNTIME_ROOT:-/tmp/nimbus}"
machine_name="default"
machine_log="${runtime_root%/}/${machine_name}.log"
api_socket="${runtime_root%/}/${machine_name}-api.sock"
api_pid="${runtime_root%/}/${machine_name}-api.pid"
identity_record="${runtime_root%/}/${machine_name}.identity"

brew_prefix="$(cd "$(dirname "$0")/.." && pwd)"
gvproxy_path="${brew_prefix}/Caskroom/nimbus-dev/${host_version}/libexec/gvproxy"

if [[ "${1:-}" == "--version" ]]; then
  printf 'nimbus %s\n' "${host_version}"
  exit 0
fi

if [[ "${1:-}" != "machine" ]]; then
  echo "unexpected args: $*" >&2
  exit 64
fi

subcommand="${2:-}"
shift 2

case "${subcommand}" in
  init)
    identity_path=""
    while [[ "$#" -gt 0 ]]; do
      case "$1" in
        --identity)
          identity_path="${2:?missing identity path}"
          shift 2
          ;;
        *)
          shift
          ;;
      esac
    done
    if [[ -n "${identity_path}" ]]; then
      mkdir -p "$(dirname "${identity_record}")"
      printf '%s\n' "${identity_path}" > "${identity_record}"
    fi
    printf 'result: initialized\n' >&2
    ;;
  start)
    mkdir -p "$(dirname "${machine_log}")"
    cat > "${machine_log}" <<OUT
booting ${machine_name}
guest nimbus ${host_version}
machine ready
OUT
    if [[ -f "${api_pid}" ]]; then
      kill "$(cat "${api_pid}")" >/dev/null 2>&1 || true
      rm -f "${api_pid}"
    fi
    rm -f "${api_socket}"
    python3 - "${api_socket}" <<'PY' >"${runtime_root%/}/${machine_name}-api.log" 2>&1 &
import json
import os
import socket
import sys

socket_path = sys.argv[1]
try:
    os.unlink(socket_path)
except FileNotFoundError:
    pass

server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
server.bind(socket_path)
server.listen(16)

bootc_status = {
    "status": {
        "status": {
            "booted": {
                "image": {
                    "image": {
                        "image": "ghcr.io/nimbus/machine-os:v9.9.9"
                    },
                    "imageDigest": "sha256:9999999999999999999999999999999999999999999999999999999999999999",
                }
            },
            "staged": None,
            "rollback": None,
        }
    },
    "booted_image": "ghcr.io/nimbus/machine-os:v9.9.9",
    "booted_digest": "sha256:9999999999999999999999999999999999999999999999999999999999999999",
    "staged_image": None,
    "staged_digest": None,
    "rollback_image": None,
    "rollback_digest": None,
}

responses = {
    "/healthz": {
        "status": "ok",
        "role": "guest-machine-api",
        "protocol_version": "v1alpha2",
    },
    "/v1/machine-api/capabilities": {
        "protocol_version": "v1alpha2",
        "service_execution_ready": True,
        "service_execution_mode": "standard_containers",
        "supported_service_backends": ["container"],
        "supported_operations": [
            "healthz",
            "capabilities",
            "service-sandboxes.image-start",
            "service-sandboxes.stop",
            "service-sandboxes.logs",
            "os.bootc.status",
            "os.bootc.switch",
            "os.bootc.upgrade",
            "os.bootc.rollback",
        ],
        "binary_statuses": [],
        "operation_statuses": [],
        "service_execution_blockers": [],
    },
    "/v1/machine-api/os/bootc/status": bootc_status,
}

while True:
    connection, _ = server.accept()
    with connection:
        request = connection.recv(4096).decode("utf-8", "replace")
        first_line = request.splitlines()[0] if request else ""
        parts = first_line.split()
        request_path = parts[1] if len(parts) >= 2 else "/"
        body_object = responses.get(request_path)
        if body_object is None:
            status = "404 Not Found"
            body = json.dumps({"error": "not found"}, separators=(",", ":"))
        else:
            status = "200 OK"
            body = json.dumps(body_object, separators=(",", ":"))
        response = (
            f"HTTP/1.1 {status}\r\n"
            "content-type: application/json\r\n"
            f"content-length: {len(body.encode('utf-8'))}\r\n"
            "\r\n"
            f"{body}"
        )
        connection.sendall(response.encode("utf-8"))
PY
    printf '%s\n' "$!" > "${api_pid}"
    for _ in $(seq 1 50); do
      if [[ -S "${api_socket}" ]]; then
        break
      fi
      sleep 0.1
    done
    printf 'result: started\n' >&2
    ;;
  status)
    cat <<OUT
result: status
lifecycle: running
manager: ready
runtime:
  helper_binaries:
    gvproxy: ${gvproxy_path}
machine_api:
  reachable: true
  protocol_version: v1alpha2
guest_binary_contract:
  source: release-asset
  desired_version: v${host_version}
OUT
    ;;
  inspect)
    cat <<OUT
{"config":{"guest":{}},"state":{"runtime":{"ssh_port":10000}}}
OUT
    ;;
  stop)
    if [[ -f "${api_pid}" ]]; then
      kill "$(cat "${api_pid}")" >/dev/null 2>&1 || true
      rm -f "${api_pid}"
    fi
    rm -f "${api_socket}"
    printf 'result: stopped\n' >&2
    ;;
  rm)
    if [[ -f "${api_pid}" ]]; then
      kill "$(cat "${api_pid}")" >/dev/null 2>&1 || true
      rm -f "${api_pid}"
    fi
    rm -f "${api_socket}" "${identity_record}"
    printf 'result: removed\n' >&2
    ;;
  ssh)
    if [[ "${1:-}" == "${machine_name}" ]]; then
      shift
    fi
    if [[ "${1:-}" != "--" ]]; then
      echo "expected machine ssh -- ..." >&2
      exit 64
    fi
    shift
    command_string="$*"
    if [[ "${command_string}" == "/usr/local/bin/nimbus --version" ]]; then
      printf 'nimbus %s\n' "${host_version}"
    elif [[ "${command_string}" == *"mount | grep virtiofs"* ]]; then
      printf 'Linux fake-guest 6.8.0 aarch64 GNU/Linux\n'
      printf 'usershare on /Users type virtiofs (rw,nosuid,nodev,relatime)\n'
    elif [[ "${command_string}" == *"sha256sum /usr/local/bin/nimbus"* ]]; then
      printf 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  /usr/local/bin/nimbus\n'
    elif [[ "${command_string}" == *"for bin in buildah conmon crun netavark aardvark-dns fuse-overlayfs"* ]]; then
      cat <<OUT
present buildah /usr/bin/buildah
present conmon /usr/bin/conmon
present crun /usr/bin/crun
present netavark /usr/libexec/podman/netavark
present aardvark-dns /usr/libexec/podman/aardvark-dns
present fuse-overlayfs /usr/bin/fuse-overlayfs
OUT
    elif [[ "${command_string}" == *"systemctl show --no-pager --property=Id,LoadState,UnitFileState,ActiveState,SubState nimbus.socket"* ]]; then
      cat <<OUT
Id=nimbus.socket
LoadState=loaded
UnitFileState=enabled
ActiveState=active
SubState=listening
OUT
    elif [[ "${command_string}" == *"systemctl show --no-pager --property=Id,LoadState,UnitFileState,ActiveState,SubState nimbus.service"* ]]; then
      cat <<OUT
Id=nimbus.service
LoadState=loaded
UnitFileState=static
ActiveState=inactive
SubState=dead
OUT
    elif [[ "${command_string}" == *"findmnt --noheadings --output TARGET,SOURCE,FSTYPE,OPTIONS -T \"/Users\""* ]]; then
      printf '/Users usershare virtiofs rw,nosuid,nodev,relatime\n'
    elif [[ "${command_string}" == *"command -v getenforce"* ]]; then
      printf 'Enforcing\n'
    elif [[ "${command_string}" == *"rpm -q bootupd selinux-policy selinux-policy-targeted systemd util-linux-core podman crun netavark aardvark-dns bootc policycoreutils"* ]]; then
      cat <<OUT
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
bootc-1.15.2-1.fc44.aarch64
policycoreutils-3.9-1.fc44.aarch64
# bootloader units
bootloader-update.service enabled
# bootloader-update.service
ExecStart=/usr/bin/bootupctl update
OUT
    elif [[ "${command_string}" == *"http://localhost/healthz"* ]]; then
      cat <<OUT
HTTP/1.1 200 OK
content-type: application/json

{"status":"ok","role":"guest-machine-api","protocol_version":"v1alpha2"}
OUT
    elif [[ "${command_string}" == *"http://localhost/v1/machine-api/capabilities"* ]]; then
      cat <<OUT
HTTP/1.1 200 OK
content-type: application/json

{"protocol_version":"v1alpha2","service_execution_ready":true,"service_execution_mode":"standard_containers","supported_service_backends":["container"],"supported_operations":["healthz","capabilities"],"binary_statuses":[],"operation_statuses":[],"service_execution_blockers":[]}
OUT
    else
      echo "unexpected machine ssh command: ${command_string}" >&2
      exit 64
    fi
    ;;
  *)
    echo "unexpected machine subcommand: ${subcommand}" >&2
    exit 64
    ;;
esac
EOF

cat > "${brew_bin}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

brew_prefix="__BREW_PREFIX__"
tap_root="__TAP_ROOT__"
state_dir="__STATE_DIR__"
taps_file="${state_dir}/taps.txt"

sanitize_tap() {
  local tap_name="$1"
  printf '%s' "${tap_name//\//__}"
}

tap_repo_for() {
  local tap_name="$1"
  printf '%s/%s\n' "${tap_root}" "$(sanitize_tap "${tap_name}")"
}

ensure_taps_file() {
  mkdir -p "${state_dir}"
  touch "${taps_file}"
}

ensure_taps_file

case "${1:-}" in
  list)
    if [[ "${2:-}" == "--cask" ]]; then
      token="${3:?missing cask token}"
      shopt -s nullglob
      matches=("${brew_prefix}/Caskroom/${token}"/*)
      shopt -u nullglob
      if [[ "${#matches[@]}" -gt 0 ]]; then
        exit 0
      fi
      exit 1
    fi
    echo "unsupported brew list args: $*" >&2
    exit 64
    ;;
  tap)
    if [[ "$#" -eq 1 ]]; then
      cat "${taps_file}"
      exit 0
    fi
    echo "unsupported brew tap args: $*" >&2
    exit 64
    ;;
  tap-new)
    tap_name="${2:?missing tap name}"
    tap_repo="$(tap_repo_for "${tap_name}")"
    mkdir -p "${tap_repo}/Casks"
    if ! grep -Fxq "${tap_name}" "${taps_file}" 2>/dev/null; then
      printf '%s\n' "${tap_name}" >> "${taps_file}"
    fi
    printf 'Created %s\n' "${tap_repo}"
    ;;
  --repository)
    tap_name="${2:?missing tap name}"
    tap_repo="$(tap_repo_for "${tap_name}")"
    mkdir -p "${tap_repo}"
    printf '%s\n' "${tap_repo}"
    ;;
  install)
    if [[ "${2:-}" != "--cask" ]]; then
      echo "unsupported brew install args: $*" >&2
      exit 64
    fi
    ref="${3:?missing cask ref}"
    tap_name="${ref%/*}"
    token="${ref##*/}"
    tap_repo="$(tap_repo_for "${tap_name}")"
    cask_file="${tap_repo}/Casks/${token}.rb"
    version="$(awk -F'"' '$1 == "  version " { print $2; exit }' "${cask_file}")"
    url="$(awk -F'"' '$1 == "  url " { value=$2; sub(/^file:\/\//, "", value); print value; exit }' "${cask_file}")"
    caskroom="${brew_prefix}/Caskroom/${token}/${version}"
    mkdir -p "${caskroom}" "${brew_prefix}/bin"
    tar -xzf "${url}" -C "${caskroom}"
    ln -sf "${caskroom}/nimbus" "${brew_prefix}/bin/${token}"
    ;;
  uninstall)
    if [[ "${2:-}" != "--cask" || "${3:-}" != "--force" ]]; then
      echo "unsupported brew uninstall args: $*" >&2
      exit 64
    fi
    token="${4:?missing cask token}"
    rm -f "${brew_prefix}/bin/${token}"
    rm -rf "${brew_prefix}/Caskroom/${token}"
    ;;
  untap)
    tap_name="${2:?missing tap name}"
    tap_repo="$(tap_repo_for "${tap_name}")"
    rm -rf "${tap_repo}"
    grep -Fxv "${tap_name}" "${taps_file}" > "${taps_file}.tmp" || true
    mv "${taps_file}.tmp" "${taps_file}"
    ;;
  *)
    echo "unsupported brew args: $*" >&2
    exit 64
    ;;
esac
EOF

cat > "${ssh_keygen_bin}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

output=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    -f)
      output="${2:?missing -f path}"
      shift 2
      ;;
    *)
      shift
      ;;
  esac
done

if [[ -z "${output}" ]]; then
  echo "missing ssh-keygen output path" >&2
  exit 64
fi

mkdir -p "$(dirname "${output}")"
printf 'fake-private-key\n' > "${output}"
printf 'fake-public-key\n' > "${output}.pub"
EOF

cat > "${gvproxy_binary}" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF

sed -i.bak \
  -e "s|__HOST_VERSION__|${host_version}|g" \
  "${host_binary}"
rm -f "${host_binary}.bak"

sed -i.bak \
  -e "s|__BREW_PREFIX__|${brew_prefix}|g" \
  -e "s|__TAP_ROOT__|${tap_root}|g" \
  -e "s|__STATE_DIR__|${state_dir}|g" \
  "${brew_bin}"
rm -f "${brew_bin}.bak"

chmod +x "${host_binary}" "${brew_bin}" "${ssh_keygen_bin}" "${gvproxy_binary}"

bash "${repo_root}/scripts/collect-nimbus-homebrew-cask-proof.sh" \
  --output-dir "${output_dir}" \
  --home "${home_dir}" \
  --runtime-root "${runtime_root}" \
  --host-binary "${host_binary}" \
  --gvproxy "${gvproxy_binary}" \
  --brew "${brew_bin}" \
  --brew-prefix "${brew_prefix}" \
  --readlink "$(command -v readlink)" \
  --ssh-keygen "${ssh_keygen_bin}" \
  > "${output_dir}/stdout.txt"

summary_file="${output_dir}/summary.txt"
guest_proof_dir="${output_dir}/guest-proof"

for expected_file in \
  "${summary_file}" \
  "${output_dir}/cask-symlink.txt" \
  "${output_dir}/machine-status-running.txt" \
  "${output_dir}/guest-nimbus-version.txt" \
  "${guest_proof_dir}/guest-machine-api-health.txt" \
  "${guest_proof_dir}/guest-machine-api-capabilities.txt"; do
  if [[ ! -f "${expected_file}" ]]; then
    echo "expected cask-proof artifact missing: ${expected_file}" >&2
    exit 1
  fi
done

grep -Eq '^guest\.binary\.override[[:space:]]+<none>$' "${summary_file}" || {
  echo "expected release-asset default in summary" >&2
  exit 1
}

grep -Eq "^brew\\.prefix[[:space:]]+${brew_prefix}$" "${summary_file}" || {
  echo "expected brew prefix in summary" >&2
  exit 1
}

grep -Eq '^result[[:space:]]+ok$' "${summary_file}" || {
  echo "expected successful result in summary" >&2
  exit 1
}

grep -Fq "${brew_prefix}/Caskroom/nimbus-dev/${host_version}/nimbus" "${output_dir}/cask-symlink.txt" || {
  echo "expected cask symlink to point at fake caskroom payload" >&2
  exit 1
}

grep -Fq "${brew_prefix}/Caskroom/nimbus-dev/${host_version}/libexec/gvproxy" "${output_dir}/machine-status-running.txt" || {
  echo "expected machine status to report packaged gvproxy path" >&2
  exit 1
}

grep -Fq "nimbus ${host_version}" "${output_dir}/guest-nimbus-version.txt" || {
  echo "expected guest version proof to match host version" >&2
  exit 1
}

grep -Fq 'HTTP/1.1 200 OK' "${guest_proof_dir}/guest-machine-api-health.txt" || {
  echo "expected guest proof health response" >&2
  exit 1
}

grep -Fq '"protocol_version":"v1alpha2"' "${guest_proof_dir}/guest-machine-api-capabilities.txt" || {
  echo "expected guest proof capabilities response" >&2
  exit 1
}

echo "verified: nimbus homebrew cask proof helper captures the packaged macOS release-asset contract deterministically"
