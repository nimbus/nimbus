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
image_path="docker://quay.io/podman/machine-os:5.0"

mkdir -p "${home_dir}" "${runtime_root}" "${bin_dir}" "${output_dir}"

cat > "${bin_dir}/nimbus" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

machine_name="default"
home_dir="${HOME:?HOME must be set}"
runtime_root="${NIMBUS_MACHINE_RUNTIME_ROOT:?runtime root must be set}"

set_paths() {
  config_dir="${home_dir}/.config/nimbus/machine/${machine_name}"
  state_dir="${home_dir}/.local/state/nimbus/machine/${machine_name}"
  mkdir -p "${config_dir}" "${state_dir}"
}

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
    machine_name="${3:-default}"
    set_paths
    if [[ -f "${state_dir}/status.json" ]]; then
      write_status "stopped" "helpers-resolved"
    fi
    echo "stopped"
    ;;
  rm)
    machine_name="${3:-default}"
    set_paths
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
    bootc_native="false"
    ssh_identity=""
    ignition_file=""
    efi_store=""
    volumes=()
    shift 2
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --cpus|--memory|--disk-size)
          shift 2
          ;;
        --image)
          image="${2:?missing image path}"
          shift 2
          ;;
        --bootc-native)
          bootc_native="true"
          shift
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
          machine_name="$1"
          shift
          ;;
      esac
    done
    set_paths
    mkdir -p "${config_dir}" "${state_dir}"
    cat > "${config_dir}/config.json" <<OUT
{"image":"${image}","bootc_native":${bootc_native},"ssh_identity_path":"${ssh_identity}","ignition_file_path":"${ignition_file}","efi_variable_store_path":"${efi_store}","volumes":"${volumes[*]}"}
OUT
    write_status "stopped" "helpers-resolved"
    echo "initialized"
    ;;
  start)
    machine_name="${3:-default}"
    set_paths
    if [[ "${NIMBUS_FAKE_START_FAIL:-0}" == "1" ]]; then
      write_status "failed" "failed"
      echo "start failed" >&2
      exit 1
    fi
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
    machine_name="${3:-default}"
    set_paths
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

cat > "${bin_dir}/ssh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

target="${1:?missing ssh target}"
shift
if [[ -z "${target}" ]]; then
  echo "missing ssh target" >&2
  exit 64
fi

bash -c "$*"
EOF

cat > "${bin_dir}/scp" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

recursive=0
if [[ "${1:-}" == "-r" ]]; then
  recursive=1
  shift
fi

src="${1:?missing source}"
dest="${2:?missing destination}"

remote_path() {
  printf '%s\n' "${1#*:}"
}

if [[ "${src}" == *:* && "${dest}" != *:* ]]; then
  from="$(remote_path "${src}")"
  mkdir -p "${dest}"
  if [[ "${from}" == */. ]]; then
    cp -R "${from%/.}/." "${dest}/"
  elif [[ "${recursive}" -eq 1 ]]; then
    cp -R "${from}" "${dest}/"
  else
    cp "${from}" "${dest}/"
  fi
elif [[ "${dest}" == *:* && "${src}" != *:* ]]; then
  to="$(remote_path "${dest}")"
  mkdir -p "$(dirname "${to}")"
  cp "${src}" "${to}"
else
  if [[ "${recursive}" -eq 1 ]]; then
    cp -R "${src}" "${dest}"
  else
    cp "${src}" "${dest}"
  fi
fi
EOF

cat > "${bin_dir}/sudo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
exec "$@"
EOF

chmod +x "${bin_dir}/nimbus" "${bin_dir}/ps" "${bin_dir}/ssh" "${bin_dir}/scp" "${bin_dir}/sudo"

HOME="${home_dir}" bash "${repo_root}/scripts/recreate-nimbus-machine.sh" \
  --machine team-a \
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
grep -F "machine.name                       team-a" "${summary_file}" >/dev/null
grep -E "^recreate\\.init[[:space:]]+ok path=.*nimbus-machine-init\\.txt$" "${summary_file}" >/dev/null
grep -E "^recreate\\.start[[:space:]]+ok path=.*nimbus-machine-start\\.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.post_diagnostics[[:space:]]+ok path=.*post-diagnostics$" "${summary_file}" >/dev/null
grep -F "result                             ready" "${summary_file}" >/dev/null

grep -F -- "--image ${image_path}" "${output_dir}/nimbus-machine-init-command.txt" >/dev/null
grep -F -- "--volume /Users:/Users" "${output_dir}/nimbus-machine-init-command.txt" >/dev/null
grep -F "team-a" "${output_dir}/nimbus-machine-stop-command.txt" >/dev/null
grep -F "team-a" "${output_dir}/nimbus-machine-rm-command.txt" >/dev/null
grep -F "team-a" "${output_dir}/nimbus-machine-init-command.txt" >/dev/null
grep -F "team-a" "${output_dir}/nimbus-machine-start-command.txt" >/dev/null
grep -F "team-a" "${output_dir}/nimbus-machine-status-command.txt" >/dev/null
grep -F "started" "${output_dir}/nimbus-machine-start.txt" >/dev/null
grep -F "machine booted" "${output_dir}/post-diagnostics/machine-log-tail.txt" >/dev/null
grep -F "team-a-api.sock missing" "${output_dir}/post-diagnostics/socket-presence.txt" >/dev/null
grep -F "team-a.sock present" "${output_dir}/post-diagnostics/socket-presence.txt" >/dev/null

local_bootc_output_dir="${tmp_dir}/local-bootc-output"
machine_os_repo="${tmp_dir}/machine-os"
guest_binary="${tmp_dir}/guest-nimbus"
mkdir -p "${machine_os_repo}/scripts"
cat > "${machine_os_repo}/scripts/build.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

nimbus_binary=""
nimbus_version=""
source_revision=""
output_dir=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --nimbus-binary)
      nimbus_binary="${2:?missing nimbus binary}"
      shift 2
      ;;
    --nimbus-version)
      nimbus_version="${2:?missing nimbus version}"
      shift 2
      ;;
    --source-revision)
      source_revision="${2:?missing source revision}"
      shift 2
      ;;
    --output-dir)
      output_dir="${2:?missing output dir}"
      shift 2
      ;;
    *)
      echo "unexpected build arg: $1" >&2
      exit 64
      ;;
  esac
done

mkdir -p "${output_dir}"
{
  printf 'nimbus_binary=%s\n' "${nimbus_binary}"
  printf 'nimbus_version=%s\n' "${nimbus_version}"
  printf 'source_revision=%s\n' "${source_revision}"
} > "${output_dir}/build-args.txt"
printf 'local bootc disk\n' > "${output_dir}/nimbus-machine-os.raw"
EOF
chmod +x "${machine_os_repo}/scripts/build.sh"

cat > "${guest_binary}" <<'EOF'
#!/usr/bin/env bash
echo local guest nimbus
EOF
chmod +x "${guest_binary}"

mkdir -p "${local_bootc_output_dir}"
HOME="${home_dir}" \
NIMBUS_MACHINE_GUEST_BINARY="${guest_binary}" \
NIMBUS_MACHINE_OS_LOCAL_VERSION="dev-test" \
NIMBUS_MACHINE_OS_LOCAL_SOURCE_REVISION="test-revision" \
bash "${repo_root}/scripts/recreate-nimbus-machine.sh" \
  --machine team-local \
  --home "${home_dir}" \
  --runtime-root "${runtime_root}" \
  --output-dir "${local_bootc_output_dir}" \
  --nimbus "${bin_dir}/nimbus" \
  --machine-os-repo "${machine_os_repo}" \
  --identity "${tmp_dir}/machine-key" \
  --volume /Users:/Users \
  > "${local_bootc_output_dir}/stdout.txt"

local_bootc_raw="${local_bootc_output_dir}/local-bootc-image/nimbus-machine-os.raw"
grep -E "^local_bootc\\.build[[:space:]]+ok path=.*nimbus-machine-local-bootc-build\\.txt$" \
  "${local_bootc_output_dir}/summary.txt" >/dev/null
grep -F "image.source                       ${local_bootc_raw}" \
  "${local_bootc_output_dir}/summary.txt" >/dev/null
grep -F "guest.binary.override              ${guest_binary} (baked into local bootc dev image)" \
  "${local_bootc_output_dir}/summary.txt" >/dev/null
grep -F "machine.provisioning               bootc-native" \
  "${local_bootc_output_dir}/summary.txt" >/dev/null
grep -F -- "--nimbus-binary ${guest_binary}" \
  "${local_bootc_output_dir}/nimbus-machine-local-bootc-build-command.txt" >/dev/null
grep -F -- "--nimbus-version dev-test" \
  "${local_bootc_output_dir}/nimbus-machine-local-bootc-build-command.txt" >/dev/null
grep -F -- "--source-revision test-revision" \
  "${local_bootc_output_dir}/nimbus-machine-local-bootc-build-command.txt" >/dev/null
grep -F -- "--image ${local_bootc_raw}" \
  "${local_bootc_output_dir}/nimbus-machine-init-command.txt" >/dev/null
grep -F -- "--bootc-native" \
  "${local_bootc_output_dir}/nimbus-machine-init-command.txt" >/dev/null
grep -F '"bootc_native":true' \
  "${local_bootc_output_dir}/post-diagnostics/machine-config.json" >/dev/null
grep -F "nimbus_binary=${guest_binary}" \
  "${local_bootc_output_dir}/local-bootc-image/build-args.txt" >/dev/null
grep -F "artifact.producer=nimbus/machine-os" \
  "${local_bootc_output_dir}/local-bootc-image/nimbus-machine-local-bootc-provenance.txt" >/dev/null
grep -F "artifact.producer_script=${machine_os_repo}/scripts/build.sh" \
  "${local_bootc_output_dir}/local-bootc-image/nimbus-machine-local-bootc-provenance.txt" >/dev/null

remote_bootc_output_dir="${tmp_dir}/remote-bootc-output"
remote_work_dir="${tmp_dir}/remote-work"
mkdir -p "${remote_bootc_output_dir}" "${remote_work_dir}"
PATH="${bin_dir}:${PATH}" \
HOME="${home_dir}" \
NIMBUS_MACHINE_GUEST_BINARY="${guest_binary}" \
NIMBUS_MACHINE_OS_LOCAL_VERSION="dev-remote" \
NIMBUS_MACHINE_OS_LOCAL_SOURCE_REVISION="remote-revision" \
bash "${repo_root}/scripts/recreate-nimbus-machine.sh" \
  --machine team-remote \
  --home "${home_dir}" \
  --runtime-root "${runtime_root}" \
  --output-dir "${remote_bootc_output_dir}" \
  --nimbus "${bin_dir}/nimbus" \
  --machine-os-builder fake-builder \
  --machine-os-builder-repo "${machine_os_repo}" \
  --machine-os-builder-work-dir "${remote_work_dir}" \
  --identity "${tmp_dir}/machine-key" \
  --volume /Users:/Users \
  > "${remote_bootc_output_dir}/stdout.txt"

remote_bootc_raw="${remote_bootc_output_dir}/local-bootc-image/nimbus-machine-os.raw"
grep -F "image.source                       ${remote_bootc_raw}" \
  "${remote_bootc_output_dir}/summary.txt" >/dev/null
grep -F -- "--builder fake-builder" \
  "${remote_bootc_output_dir}/nimbus-machine-local-bootc-build-command.txt" >/dev/null
grep -F "builder=fake-builder" \
  "${remote_bootc_output_dir}/local-bootc-image/nimbus-machine-local-bootc-builder.txt" >/dev/null
grep -F "artifact.producer=nimbus/machine-os" \
  "${remote_bootc_output_dir}/local-bootc-image/nimbus-machine-local-bootc-builder.txt" >/dev/null
grep -F "artifact.producer_script=${machine_os_repo}/scripts/build.sh" \
  "${remote_bootc_output_dir}/local-bootc-image/nimbus-machine-local-bootc-builder.txt" >/dev/null
grep -F "local_machine_os_repo=<unspecified>" \
  "${remote_bootc_output_dir}/local-bootc-image/nimbus-machine-local-bootc-builder.txt" >/dev/null
grep -F "builder_machine_os_repo=${machine_os_repo}" \
  "${remote_bootc_output_dir}/local-bootc-image/nimbus-machine-local-bootc-builder.txt" >/dev/null
grep -F "builder_work_dir=${remote_work_dir}" \
  "${remote_bootc_output_dir}/local-bootc-image/nimbus-machine-local-bootc-builder.txt" >/dev/null
grep -F "${guest_binary} fake-builder:" \
  "${remote_bootc_output_dir}/local-bootc-image/nimbus-machine-local-bootc-copy-to-builder-command.txt" >/dev/null
grep -F "fake-builder:" \
  "${remote_bootc_output_dir}/local-bootc-image/nimbus-machine-local-bootc-copy-from-builder-command.txt" >/dev/null
grep -F -- "--image ${remote_bootc_raw}" \
  "${remote_bootc_output_dir}/nimbus-machine-init-command.txt" >/dev/null
grep -F -- "--bootc-native" \
  "${remote_bootc_output_dir}/nimbus-machine-init-command.txt" >/dev/null
grep -F "nimbus_binary=${remote_work_dir}/nimbus-machine-os-dev-dev-remote-" \
  "${remote_bootc_output_dir}/local-bootc-image/build-args.txt" >/dev/null

failure_output_dir="${tmp_dir}/failure-output"
set +e
HOME="${home_dir}" NIMBUS_FAKE_START_FAIL=1 bash "${repo_root}/scripts/recreate-nimbus-machine.sh" \
  --machine team-b \
  --home "${home_dir}" \
  --runtime-root "${runtime_root}" \
  --output-dir "${failure_output_dir}" \
  --nimbus "${bin_dir}/nimbus" \
  --image "${image_path}" \
  --identity "${tmp_dir}/machine-key" \
  --ignition-path "${tmp_dir}/machine.ign" \
  --firmware "${tmp_dir}/efi-store" \
  --volume /Users:/Users \
  > "${failure_output_dir}.stdout" 2>&1
failure_status=$?
set -e

if [[ "${failure_status}" -eq 0 ]]; then
  echo "expected failed recreate helper run to return non-zero" >&2
  exit 71
fi

grep -E "^recreate\\.start[[:space:]]+failed status=1 path=.*nimbus-machine-start\\.txt$" \
  "${failure_output_dir}/summary.txt" >/dev/null
grep -E "^capture\\.final_status[[:space:]]+ok path=.*nimbus-machine-status\\.txt$" \
  "${failure_output_dir}/summary.txt" >/dev/null
grep -E "^capture\\.post_diagnostics[[:space:]]+ok path=.*post-diagnostics$" \
  "${failure_output_dir}/summary.txt" >/dev/null
grep -F "result                             failed" "${failure_output_dir}/summary.txt" >/dev/null
test -f "${failure_output_dir}/post-diagnostics/machine-state.json"

echo "verified: nimbus machine recreate helper captured deterministic artifacts"
