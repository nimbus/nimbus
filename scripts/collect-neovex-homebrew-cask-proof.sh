#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"

usage() {
  cat <<'EOF'
usage: collect-neovex-homebrew-cask-proof.sh [options]

Collect real-host proof for the supported macOS Homebrew/cask install surface
without touching the user's shipped `neovex` cask token or default machine
roots. The collector packages a local release binary plus bundled `gvproxy`
into a temporary proof cask, installs it under an isolated token, and then
exercises the packaged `neovex machine ...` path against isolated machine roots.

options:
  --machine <name>               Machine name (default: default)
  --home <path>                  HOME for isolated machine roots
                                 (default: <output-dir>/home)
  --runtime-root <path>          Runtime root for machine helpers
                                 (default: <output-dir>/runtime)
  --output-dir <path>            Output directory for captured artifacts
  --host-binary <path>           Host macOS neovex binary to package
                                 (default: <repo>/target/release/neovex)
  --guest-binary <path>          Optional Linux guest neovex override to sync
                                 into the VM instead of the tagged release asset
  --gvproxy <path>               gvproxy binary to bundle
                                 (default: <brew-prefix>/opt/podman/libexec/podman/gvproxy)
  --brew <path>                  Brew binary path (default: brew)
  --brew-prefix <path>           Homebrew prefix to target
                                 (default: /opt/homebrew)
  --readlink <path>              readlink binary path (default: readlink)
  --ssh-keygen <path>            ssh-keygen binary path (default: ssh-keygen)
  --tap <name>                   Local tap to create (default: local/neovex-proof)
  --cask <token>                 Temporary cask token (default: neovex-dev)
  --keep-installed               Keep the proof cask/tap installed for debugging
  -h, --help                     Show this help

examples:
  bash scripts/collect-neovex-homebrew-cask-proof.sh

  bash scripts/collect-neovex-homebrew-cask-proof.sh \
    --output-dir /tmp/neovex-d4a-proof \
    --home /tmp/neovex-d4a-proof/home \
    --runtime-root /tmp/neovex-d4a-proof/runtime
EOF
}

print_line() {
  local label="$1"
  local value="$2"
  printf '%-34s %s\n' "${label}" "${value}" | tee -a "${summary_file}"
}

write_command_file() {
  local output_path="$1"
  shift

  local -a rendered=()
  local arg=""

  for arg in "$@"; do
    rendered+=( "$(printf '%q' "${arg}")" )
  done

  printf '%s\n' "${rendered[*]}" > "${output_path}"
}

capture_command() {
  local label="$1"
  local command_path="$2"
  local output_path="$3"
  shift 3

  write_command_file "${command_path}" "$@"

  local status=0
  set +e
  "$@" >"${output_path}" 2>&1
  status=$?
  set -e

  if [[ "${status}" -eq 0 ]]; then
    print_line "${label}" "ok path=${output_path} cmd=${command_path}"
  else
    print_line "${label}" "failed status=${status} path=${output_path} cmd=${command_path}"
  fi

  return "${status}"
}

capture_command_allow_failure() {
  local label="$1"
  local command_path="$2"
  local output_path="$3"
  shift 3

  write_command_file "${command_path}" "$@"

  local status=0
  set +e
  "$@" >"${output_path}" 2>&1
  status=$?
  set -e

  if [[ "${status}" -eq 0 ]]; then
    print_line "${label}" "ok path=${output_path} cmd=${command_path}"
  else
    print_line "${label}" "failed status=${status} path=${output_path} cmd=${command_path}"
  fi

  return 0
}

assert_file_contains() {
  local label="$1"
  local file_path="$2"
  local pattern="$3"

  if grep -Eq "${pattern}" "${file_path}"; then
    print_line "${label}" "ok file=${file_path} pattern=${pattern}"
  else
    print_line "${label}" "failed file=${file_path} pattern=${pattern}"
    return 1
  fi
}

assert_file_empty() {
  local label="$1"
  local file_path="$2"

  if [[ ! -s "${file_path}" ]]; then
    print_line "${label}" "ok file=${file_path}"
  else
    print_line "${label}" "failed file=${file_path}"
    return 1
  fi
}

machine_name="default"
output_dir=""
home_dir=""
runtime_root=""
host_binary="${repo_root}/target/release/neovex"
guest_binary=""
gvproxy_binary=""
brew_bin="brew"
brew_prefix="/opt/homebrew"
readlink_bin="readlink"
ssh_keygen_bin="ssh-keygen"
tap_name="local/neovex-proof"
cask_token="neovex-dev"
keep_installed=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --machine)
      machine_name="${2:?missing machine name}"
      shift 2
      ;;
    --home)
      home_dir="${2:?missing home path}"
      shift 2
      ;;
    --runtime-root)
      runtime_root="${2:?missing runtime root}"
      shift 2
      ;;
    --output-dir)
      output_dir="${2:?missing output dir}"
      shift 2
      ;;
    --host-binary)
      host_binary="${2:?missing host binary path}"
      shift 2
      ;;
    --guest-binary)
      guest_binary="${2:?missing guest binary path}"
      shift 2
      ;;
    --gvproxy)
      gvproxy_binary="${2:?missing gvproxy path}"
      shift 2
      ;;
    --brew)
      brew_bin="${2:?missing brew path}"
      shift 2
      ;;
    --brew-prefix)
      brew_prefix="${2:?missing brew prefix}"
      shift 2
      ;;
    --readlink)
      readlink_bin="${2:?missing readlink path}"
      shift 2
      ;;
    --ssh-keygen)
      ssh_keygen_bin="${2:?missing ssh-keygen path}"
      shift 2
      ;;
    --tap)
      tap_name="${2:?missing tap name}"
      shift 2
      ;;
    --cask)
      cask_token="${2:?missing cask token}"
      shift 2
      ;;
    --keep-installed)
      keep_installed=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 64
      ;;
  esac
done

if [[ -z "${output_dir}" ]]; then
  output_dir="$(mktemp -d "${TMPDIR:-/tmp}/neovex-homebrew-cask-proof.XXXXXX")"
else
  mkdir -p "${output_dir}"
fi

output_dir="$(cd "${output_dir}" && pwd)"
summary_file="${output_dir}/summary.txt"
: > "${summary_file}"
brew_prefix="${brew_prefix%/}"

if [[ -z "${home_dir}" ]]; then
  home_dir="${output_dir}/home"
fi
if [[ -z "${runtime_root}" ]]; then
  runtime_root="${output_dir}/runtime"
fi

mkdir -p "${home_dir}" "${runtime_root}"

if [[ -z "${gvproxy_binary}" ]]; then
  gvproxy_binary="${brew_prefix}/opt/podman/libexec/podman/gvproxy"
fi

machine_stopped=0

cleanup() {
  local status="$1"
  trap - EXIT

  if [[ "${machine_stopped}" -eq 0 && -x "${installed_binary:-}" ]]; then
    local -a cleanup_cmd=(
      env
      -u NEOVEX_MACHINE_GVPROXY
      -u NEOVEX_MACHINE_HELPER_BINARY_DIR
      "HOME=${home_dir}"
      "NEOVEX_MACHINE_RUNTIME_ROOT=${runtime_root}"
      "PATH=${stripped_path:-${brew_prefix}/bin:/usr/bin:/bin:/usr/sbin:/sbin}"
      "${installed_binary}"
      machine
      stop
    )
    if [[ -n "${guest_binary}" ]]; then
      cleanup_cmd=(
        env
        -u NEOVEX_MACHINE_GVPROXY
        -u NEOVEX_MACHINE_HELPER_BINARY_DIR
        "HOME=${home_dir}"
        "NEOVEX_MACHINE_RUNTIME_ROOT=${runtime_root}"
        "NEOVEX_MACHINE_GUEST_BINARY=${guest_binary}"
        "PATH=${stripped_path:-${brew_prefix}/bin:/usr/bin:/bin:/usr/sbin:/sbin}"
        "${installed_binary}"
        machine
        stop
      )
    fi
    capture_command_allow_failure \
      "cleanup.machine_stop" \
      "${output_dir}/cleanup-machine-stop-command.txt" \
      "${output_dir}/cleanup-machine-stop.txt" \
      "${cleanup_cmd[@]}"
  fi

  if [[ "${keep_installed}" -eq 0 ]]; then
    if "${brew_bin}" list --cask "${cask_token}" >/dev/null 2>&1; then
      capture_command_allow_failure \
        "cleanup.brew_uninstall_cask" \
        "${output_dir}/cleanup-brew-uninstall-command.txt" \
        "${output_dir}/cleanup-brew-uninstall.txt" \
        "${brew_bin}" uninstall --cask --force "${cask_token}"
    fi

    if "${brew_bin}" tap | grep -Fxq "${tap_name}"; then
      capture_command_allow_failure \
        "cleanup.brew_untap" \
        "${output_dir}/cleanup-brew-untap-command.txt" \
        "${output_dir}/cleanup-brew-untap.txt" \
        "${brew_bin}" untap "${tap_name}"
    fi
  fi

  exit "${status}"
}

trap 'cleanup "$?"' EXIT

if [[ ! -x "${host_binary}" ]]; then
  echo "host neovex binary is not executable at ${host_binary}; build it first or pass --host-binary" >&2
  exit 64
fi

if [[ -n "${guest_binary}" && ! -x "${guest_binary}" ]]; then
  echo "guest Linux neovex binary override is not executable at ${guest_binary}; build it first or pass a valid --guest-binary" >&2
  exit 64
fi

if [[ ! -x "${gvproxy_binary}" ]]; then
  echo "gvproxy binary is not executable at ${gvproxy_binary}; install Podman or pass --gvproxy" >&2
  exit 64
fi

if ! command -v "${brew_bin}" >/dev/null 2>&1; then
  echo "brew binary is not executable as ${brew_bin}" >&2
  exit 64
fi

if ! command -v "${readlink_bin}" >/dev/null 2>&1; then
  echo "readlink binary is not executable as ${readlink_bin}" >&2
  exit 64
fi

if ! command -v "${ssh_keygen_bin}" >/dev/null 2>&1; then
  echo "ssh-keygen binary is not executable as ${ssh_keygen_bin}" >&2
  exit 64
fi

host_version="$("${host_binary}" --version | awk 'NR == 1 { print $2 }')"
if [[ -z "${host_version}" ]]; then
  echo "failed to determine host neovex version from ${host_binary} --version" >&2
  exit 64
fi

bundle_root="${output_dir}/bundle-root"
bundle_contents="${bundle_root}/contents"
archive_path="${output_dir}/neovex_darwin_arm64.tar.gz"
archive_manifest="${output_dir}/archive-contents.txt"
archive_sha_file="${output_dir}/archive-sha256.txt"
tap_info_file="${output_dir}/tap-info.txt"
cask_rendered="${output_dir}/${cask_token}.rb"
ssh_identity="${output_dir}/machine-key"
stripped_path="${brew_prefix}/bin:/usr/bin:/bin:/usr/sbin:/sbin"
installed_binary="${brew_prefix}/bin/${cask_token}"
expected_gvproxy="${brew_prefix}/Caskroom/${cask_token}/${host_version}/libexec/gvproxy"
expected_symlink="${brew_prefix}/Caskroom/${cask_token}/${host_version}/neovex"

print_line "output.dir" "${output_dir}"
print_line "machine.name" "${machine_name}"
print_line "home.dir" "${home_dir}"
print_line "runtime.root" "${runtime_root}"
print_line "host.binary" "${host_binary}"
print_line "guest.binary.override" "${guest_binary:-<none>}"
print_line "gvproxy.binary" "${gvproxy_binary}"
print_line "brew.bin" "${brew_bin}"
print_line "brew.prefix" "${brew_prefix}"
print_line "tap.name" "${tap_name}"
print_line "cask.token" "${cask_token}"
print_line "host.version" "${host_version}"
print_line "installed.binary" "${installed_binary}"

rm -rf "${bundle_root}"
mkdir -p "${bundle_contents}/libexec"
cp "${host_binary}" "${bundle_contents}/neovex"
cp "${gvproxy_binary}" "${bundle_contents}/libexec/gvproxy"
chmod 0755 "${bundle_contents}/neovex" "${bundle_contents}/libexec/gvproxy"
if [[ -f "${repo_root}/README.md" ]]; then
  cp "${repo_root}/README.md" "${bundle_contents}/README.md"
fi
if [[ -f "${repo_root}/LICENSE" ]]; then
  cp "${repo_root}/LICENSE" "${bundle_contents}/LICENSE"
fi

archive_entries=(neovex libexec)
if [[ -f "${bundle_contents}/README.md" ]]; then
  archive_entries+=(README.md)
fi
if [[ -f "${bundle_contents}/LICENSE" ]]; then
  archive_entries+=(LICENSE)
fi

capture_command \
  "capture.bundle_archive" \
  "${output_dir}/bundle-archive-command.txt" \
  "${output_dir}/bundle-archive.txt" \
  tar -C "${bundle_contents}" -czf "${archive_path}" "${archive_entries[@]}"

capture_command \
  "capture.bundle_archive_manifest" \
  "${output_dir}/bundle-archive-manifest-command.txt" \
  "${archive_manifest}" \
  tar -tzf "${archive_path}"

capture_command \
  "capture.bundle_archive_sha256" \
  "${output_dir}/bundle-archive-sha256-command.txt" \
  "${archive_sha_file}" \
  shasum -a 256 "${archive_path}"

archive_sha="$(awk 'NR == 1 { print $1 }' "${archive_sha_file}")"
if [[ -z "${archive_sha}" ]]; then
  echo "failed to compute archive sha256 for ${archive_path}" >&2
  exit 1
fi

if "${brew_bin}" list --cask "${cask_token}" >/dev/null 2>&1; then
  capture_command_allow_failure \
    "setup.remove_stale_cask" \
    "${output_dir}/setup-remove-stale-cask-command.txt" \
    "${output_dir}/setup-remove-stale-cask.txt" \
    "${brew_bin}" uninstall --cask --force "${cask_token}"
fi

if "${brew_bin}" tap | grep -Fxq "${tap_name}"; then
  capture_command_allow_failure \
    "setup.remove_stale_tap" \
    "${output_dir}/setup-remove-stale-tap-command.txt" \
    "${output_dir}/setup-remove-stale-tap.txt" \
    "${brew_bin}" untap "${tap_name}"
fi

capture_command \
  "capture.brew_tap_new" \
  "${output_dir}/brew-tap-new-command.txt" \
  "${output_dir}/brew-tap-new.txt" \
  "${brew_bin}" tap-new "${tap_name}"

tap_repo="$("${brew_bin}" --repository "${tap_name}")"
print_line "tap.repo" "${tap_repo}"
printf '%s\n' "${tap_repo}" > "${tap_info_file}"

mkdir -p "${tap_repo}/Casks"
cat > "${cask_rendered}" <<EOF
cask "${cask_token}" do
  name "${cask_token}"
  desc "Local proof cask for the Neovex macOS machine contract"
  homepage "https://github.com/agentstation/neovex"
  version "${host_version}"

  livecheck do
    skip "Local proof cask."
  end

  depends_on arch: :arm64
  depends_on macos: ">= :sonoma"
  depends_on formula: "slp/krunkit/krunkit"

  url "file://${archive_path}"
  sha256 "${archive_sha}"

  binary "neovex", target: "${cask_token}"

  postflight do
    if system_command("/usr/bin/xattr", args: ["-h"]).exit_status == 0
      system_command "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", staged_path.to_s], sudo: false
    end
  end
end
EOF
cp "${cask_rendered}" "${tap_repo}/Casks/${cask_token}.rb"

capture_command \
  "capture.brew_install_cask" \
  "${output_dir}/brew-install-cask-command.txt" \
  "${output_dir}/brew-install-cask.txt" \
  env HOMEBREW_NO_AUTO_UPDATE=1 "${brew_bin}" install --cask "${tap_name}/${cask_token}"

capture_command \
  "capture.cask_symlink" \
  "${output_dir}/cask-symlink-command.txt" \
  "${output_dir}/cask-symlink.txt" \
  "${readlink_bin}" "${installed_binary}"

capture_command \
  "capture.path_gvproxy" \
  "${output_dir}/path-gvproxy-command.txt" \
  "${output_dir}/path-gvproxy.txt" \
  env PATH="${stripped_path}" /bin/sh -lc 'command -v gvproxy 2>/dev/null || true'

rm -f "${ssh_identity}" "${ssh_identity}.pub"
capture_command \
  "capture.ssh_keygen" \
  "${output_dir}/ssh-keygen-command.txt" \
  "${output_dir}/ssh-keygen.txt" \
  "${ssh_keygen_bin}" -q -t ed25519 -N "" -f "${ssh_identity}"

base_cmd=(
  env
  -u NEOVEX_MACHINE_GVPROXY
  -u NEOVEX_MACHINE_HELPER_BINARY_DIR
  "HOME=${home_dir}"
  "NEOVEX_MACHINE_RUNTIME_ROOT=${runtime_root}"
  "PATH=${stripped_path}"
  "${installed_binary}"
)

if [[ -n "${guest_binary}" ]]; then
  base_cmd=(
    env
    -u NEOVEX_MACHINE_GVPROXY
    -u NEOVEX_MACHINE_HELPER_BINARY_DIR
    "HOME=${home_dir}"
    "NEOVEX_MACHINE_RUNTIME_ROOT=${runtime_root}"
    "NEOVEX_MACHINE_GUEST_BINARY=${guest_binary}"
    "PATH=${stripped_path}"
    "${installed_binary}"
  )
fi

capture_command \
  "capture.host_neovex_version" \
  "${output_dir}/host-neovex-version-command.txt" \
  "${output_dir}/host-neovex-version.txt" \
  "${base_cmd[@]}" --version

capture_command_allow_failure \
  "capture.machine_stop_preexisting" \
  "${output_dir}/machine-stop-preexisting-command.txt" \
  "${output_dir}/machine-stop-preexisting.txt" \
  "${base_cmd[@]}" machine stop

capture_command_allow_failure \
  "capture.machine_rm_preexisting" \
  "${output_dir}/machine-rm-preexisting-command.txt" \
  "${output_dir}/machine-rm-preexisting.txt" \
  "${base_cmd[@]}" machine rm

capture_command \
  "capture.machine_init" \
  "${output_dir}/machine-init-command.txt" \
  "${output_dir}/machine-init.txt" \
  "${base_cmd[@]}" machine init --ssh-identity "${ssh_identity}"

capture_command \
  "capture.machine_start" \
  "${output_dir}/machine-start-command.txt" \
  "${output_dir}/machine-start.txt" \
  "${base_cmd[@]}" machine start

capture_command \
  "capture.machine_status_running" \
  "${output_dir}/machine-status-running-command.txt" \
  "${output_dir}/machine-status-running.txt" \
  "${base_cmd[@]}" machine status

capture_command \
  "capture.machine_ssh_mounts" \
  "${output_dir}/machine-ssh-mounts-command.txt" \
  "${output_dir}/machine-ssh-mounts.txt" \
  "${base_cmd[@]}" machine ssh -- /bin/sh -lc 'uname -a; mount | grep virtiofs'

capture_command \
  "capture.guest_neovex_version" \
  "${output_dir}/guest-neovex-version-command.txt" \
  "${output_dir}/guest-neovex-version.txt" \
  "${base_cmd[@]}" machine ssh -- /usr/local/bin/neovex --version

guest_proof_dir="${output_dir}/guest-proof"
guest_proof_cmd=(
  env
  "HOME=${home_dir}"
  "NEOVEX_MACHINE_RUNTIME_ROOT=${runtime_root}"
  bash
  "${repo_root}/scripts/collect-neovex-machine-guest-proof.sh"
  --home
  "${home_dir}"
  --runtime-root
  "${runtime_root}"
  --output-dir
  "${guest_proof_dir}"
  --neovex
  "${installed_binary}"
)
if [[ -n "${guest_binary}" ]]; then
  guest_proof_cmd=(
    env
    "HOME=${home_dir}"
    "NEOVEX_MACHINE_RUNTIME_ROOT=${runtime_root}"
    "NEOVEX_MACHINE_GUEST_BINARY=${guest_binary}"
    bash
    "${repo_root}/scripts/collect-neovex-machine-guest-proof.sh"
    --home
    "${home_dir}"
    --runtime-root
    "${runtime_root}"
    --output-dir
    "${guest_proof_dir}"
    --neovex
    "${installed_binary}"
  )
fi

capture_command \
  "capture.guest_contract_proof" \
  "${output_dir}/guest-proof-command.txt" \
  "${output_dir}/guest-proof.txt" \
  "${guest_proof_cmd[@]}"

capture_command \
  "capture.machine_stop" \
  "${output_dir}/machine-stop-command.txt" \
  "${output_dir}/machine-stop.txt" \
  "${base_cmd[@]}" machine stop
machine_stopped=1

assert_file_contains \
  "assert.archive_manifest_has_neovex" \
  "${archive_manifest}" \
  '^neovex$'

assert_file_contains \
  "assert.archive_manifest_has_gvproxy" \
  "${archive_manifest}" \
  '^libexec/gvproxy$'

assert_file_contains \
  "assert.cask_symlink" \
  "${output_dir}/cask-symlink.txt" \
  "${expected_symlink}"

assert_file_empty \
  "assert.path_gvproxy_empty" \
  "${output_dir}/path-gvproxy.txt"

assert_file_contains \
  "assert.host_version" \
  "${output_dir}/host-neovex-version.txt" \
  "^neovex ${host_version}"

assert_file_contains \
  "assert.guest_version" \
  "${output_dir}/guest-neovex-version.txt" \
  "^neovex ${host_version}"

assert_file_contains \
  "assert.guest_proof_health" \
  "${guest_proof_dir}/guest-machine-api-health.txt" \
  'HTTP/1.1 200 OK'

assert_file_contains \
  "assert.guest_proof_capabilities" \
  "${guest_proof_dir}/guest-machine-api-capabilities.txt" \
  '"protocol_version":"v1alpha2"'

assert_file_contains \
  "assert.machine_running" \
  "${output_dir}/machine-status-running.txt" \
  'lifecycle: running'

assert_file_contains \
  "assert.machine_api_ready" \
  "${output_dir}/machine-status-running.txt" \
  'reachable: true'

assert_file_contains \
  "assert.packaged_gvproxy" \
  "${output_dir}/machine-status-running.txt" \
  "${expected_gvproxy}"

assert_file_contains \
  "assert.users_virtiofs_mount" \
  "${output_dir}/machine-ssh-mounts.txt" \
  '/Users'

print_line "result" "ok"
