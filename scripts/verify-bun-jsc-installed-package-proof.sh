#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: verify-bun-jsc-installed-package-proof.sh --archive <path> [options]

Verify that a packaged Nimbus Bun/JSC adapter archive is discovered from the
real package-manager install layout without using development override
environment variables. The proof installs only into proof-owned paths, runs a
linked "use bun" invocation, removes the package layout, and then proves the
no-link fallback.

Required:
  --archive <path>             Adapter archive produced by package-bun-jsc-adapter.sh

Optional:
  --target-triple <triple>     Expected target triple (default: rustc host triple)
  --linked-cargo-jobs <n>      CARGO_BUILD_JOBS for linked proof tests (default: 1)
  -h, --help                   Show this help text
EOF
}

die() {
  printf '%s\n' "$*" >&2
  exit 64
}

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/.." && pwd)"
source "${repo_root}/scripts/bun-jsc-adapter-contract.sh"

archive_path=""
target_triple=""
linked_cargo_jobs="${NIMBUS_BUN_LINKED_CARGO_JOBS:-1}"

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --archive)
      archive_path="${2:-}"
      shift 2
      ;;
    --target-triple)
      target_triple="${2:-}"
      shift 2
      ;;
    --linked-cargo-jobs)
      linked_cargo_jobs="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

[[ -n "${archive_path}" ]] || die "--archive is required"
[[ -f "${archive_path}" ]] || die "adapter archive not found: ${archive_path}"
[[ -z "${NIMBUS_BUN_EMBED_SHARED_LIBRARY:-}" ]] ||
  die "unset NIMBUS_BUN_EMBED_SHARED_LIBRARY before running the installed-package proof"
[[ -z "${NIMBUS_BUN_JSC_ADAPTER_MANIFEST:-}" ]] ||
  die "unset NIMBUS_BUN_JSC_ADAPTER_MANIFEST before running the installed-package proof"
command -v python3 >/dev/null 2>&1 || die "python3 is required"
command -v tar >/dev/null 2>&1 || die "tar is required"

if [[ -z "${target_triple}" ]]; then
  target_triple="$(bun_jsc_adapter_host_triple)"
fi
platform="$(bun_jsc_adapter_platform_for_triple "${target_triple}")"
[[ "${platform}" != "unsupported" ]] || die "unsupported target triple: ${target_triple}"
library_basename="$(bun_jsc_adapter_library_basename_for_triple "${target_triple}")"

tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-bun-jsc-installed-proof.XXXXXX")"
extract_root="${tmp_root}/extract"
proof_prefix=""
runtime_root=""
version_dir=""
current_path=""
mac_opt_link=""
linux_marker=""
layout_installed=0

run_privileged() {
  if [[ "${platform}" == "linux" && "${EUID}" -ne 0 ]]; then
    sudo -n "$@"
  else
    "$@"
  fi
}

remove_installed_layout() {
  if [[ "${layout_installed}" != "1" ]]; then
    return
  fi

  case "${platform}" in
    darwin)
      if [[ -n "${mac_opt_link}" && -L "${mac_opt_link}" ]]; then
        link_target="$(readlink "${mac_opt_link}")"
        if [[ "${link_target}" == "${proof_prefix}" ]]; then
          rm -f "${mac_opt_link}"
        else
          printf 'refusing to remove non-proof Homebrew opt link: %s -> %s\n' \
            "${mac_opt_link}" "${link_target}" >&2
        fi
      fi
      ;;
    linux)
      if [[ -n "${runtime_root}" && -f "${linux_marker}" ]]; then
        if [[ -n "${current_path}" ]]; then
          run_privileged rm -f "${current_path}"
        fi
        if [[ -n "${version_dir}" ]]; then
          run_privileged rm -rf "${version_dir}"
        fi
        run_privileged rm -f "${linux_marker}"
        run_privileged rmdir "${runtime_root}" 2>/dev/null || true
        run_privileged rmdir "$(dirname "${runtime_root}")" 2>/dev/null || true
        run_privileged rmdir "$(dirname "$(dirname "${runtime_root}")")" 2>/dev/null || true
      fi
      ;;
  esac

  layout_installed=0
}

cleanup() {
  remove_installed_layout
  rm -rf "${tmp_root}"
}
trap cleanup EXIT

cd "${repo_root}"

printf 'Bun/JSC installed-package proof\n'
printf 'Nimbus repo:   %s\n' "${repo_root}"
printf 'Archive:       %s\n' "${archive_path}"
printf 'Target triple: %s\n' "${target_triple}"
printf 'Platform:      %s\n' "${platform}"
printf 'Archive SHA:   %s\n\n' "$(bun_jsc_adapter_sha256_file "${archive_path}")"

printf '[1/8] Adapter archive verifier\n'
bash scripts/verify-bun-jsc-adapter-package.sh \
  --archive "${archive_path}" \
  --target-triple "${target_triple}"

mkdir -p "${extract_root}"
tar -xpzf "${archive_path}" -C "${extract_root}"
manifest_path="${extract_root}/${BUN_JSC_ADAPTER_MANIFEST_FILE}"
[[ -f "${manifest_path}" ]] || die "missing ${BUN_JSC_ADAPTER_MANIFEST_FILE} after archive extraction"
adapter_version="$(
  python3 - "${manifest_path}" <<'PY'
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
print(manifest["adapter_version"])
PY
)"
[[ -n "${adapter_version}" ]] || die "adapter_version must be non-empty"

install_archive_into_runtime_root() {
  local install_root="$1"
  runtime_root="${install_root}"
  version_dir="${runtime_root}/${adapter_version}"
  current_path="${runtime_root}/current"

  [[ ! -e "${version_dir}" && ! -L "${version_dir}" ]] ||
    die "refusing to overwrite existing Bun/JSC adapter version directory: ${version_dir}"
  [[ ! -e "${current_path}" && ! -L "${current_path}" ]] ||
    die "refusing to overwrite existing Bun/JSC adapter current pointer: ${current_path}"

  run_privileged install -d -m 0755 "${version_dir}"
  run_privileged tar -xpzf "${archive_path}" -C "${version_dir}"
  run_privileged ln -s "${adapter_version}" "${current_path}"
  layout_installed=1
}

printf '\n[2/8] Stage package-owned discovery layout\n'
case "${platform}" in
  darwin)
    opt_parent=""
    for candidate in /opt/homebrew/opt /usr/local/opt; do
      if [[ -d "${candidate}" ]]; then
        opt_parent="${candidate}"
        break
      fi
    done
    [[ -n "${opt_parent}" ]] || die "no Homebrew opt parent found under /opt/homebrew/opt or /usr/local/opt"
    mac_opt_link="${opt_parent}/nimbus"
    [[ ! -e "${mac_opt_link}" && ! -L "${mac_opt_link}" ]] ||
      die "refusing to overwrite existing Homebrew opt path: ${mac_opt_link}"
    proof_prefix="${tmp_root}/homebrew-nimbus-opt"
    install_archive_into_runtime_root "${proof_prefix}/libexec/runtime/bun-jsc"
    ln -s "${proof_prefix}" "${mac_opt_link}"
    printf 'discovery.manifest=%s/libexec/runtime/bun-jsc/current/%s\n' \
      "${mac_opt_link}" "${BUN_JSC_ADAPTER_MANIFEST_FILE}"
    ;;
  linux)
    if [[ "${EUID}" -ne 0 ]]; then
      sudo -n true ||
        die "sudo -n is required to stage the Linux package-owned /usr/libexec/nimbus path"
    fi
    runtime_root="/usr/libexec/nimbus/runtime/bun-jsc"
    linux_marker="${runtime_root}/.nimbus-bun-jsc-installed-package-proof"
    if [[ -e "${runtime_root}" && ! -f "${linux_marker}" ]]; then
      die "refusing to touch existing non-proof Bun/JSC package root: ${runtime_root}"
    fi
    if [[ -f "${linux_marker}" ]]; then
      run_privileged rm -f "${runtime_root}/current"
      run_privileged rm -rf "${runtime_root:?}/${adapter_version}"
    fi
    run_privileged install -d -m 0755 "${runtime_root}"
    run_privileged touch "${linux_marker}"
    run_privileged chmod 0644 "${linux_marker}"
    layout_installed=1
    install_archive_into_runtime_root "${runtime_root}"
    printf 'discovery.manifest=%s/current/%s\n' \
      "${runtime_root}" "${BUN_JSC_ADAPTER_MANIFEST_FILE}"
    ;;
esac

proof_rustflags="${RUSTFLAGS:-}"
case " ${proof_rustflags} " in
  *" --cfg nimbus_bun_jsc_shared_adapter "*) ;;
  *) proof_rustflags="${proof_rustflags:+${proof_rustflags} }--cfg nimbus_bun_jsc_shared_adapter" ;;
esac

run_linked_cargo_test() {
  env \
    -u NIMBUS_BUN_EMBED_SHARED_LIBRARY \
    -u NIMBUS_BUN_JSC_ADAPTER_MANIFEST \
    "RUSTFLAGS=${proof_rustflags}" \
    "CARGO_BUILD_JOBS=${linked_cargo_jobs}" \
    cargo test "$@"
}

printf '\n[3/8] Packaged discovery executes a literal "use bun" function\n'
run_linked_cargo_test \
  -p nimbus-runtime \
  --features bun-jsc-linked-adapter \
  --test bun_jsc_linked_adapter \
  bun_shared_adapter_executes_use_bun_directive_program_wrapper \
  -- --nocapture

printf '\n[4/8] Packaged discovery keeps V8 and Bun/JSC alive in one process\n'
run_linked_cargo_test \
  -p nimbus-runtime \
  --features bun-jsc-linked-adapter \
  --test bun_jsc_linked_adapter \
  bun_shared_adapter_coexists_with_v8_runtime_in_same_process \
  -- --nocapture
run_linked_cargo_test \
  -p nimbus-runtime \
  --features bun-jsc-linked-adapter \
  --lib \
  backends::bun_jsc::tests::bun_jsc_linked_adapter_coexists_with_v8_backend_in_same_process \
  -- --nocapture

printf '\n[5/8] UI build prerequisites for server diagnostics\n'
make build-ui

printf '\n[6/8] Server diagnostics see packaged linked state\n'
run_linked_cargo_test \
  -p nimbus-server \
  --features bun-jsc-linked-adapter \
  registry_and_license::registry::convex_registry_bun_jsc_lane_diagnostics_reflect_runtime_adapter_state \
  -- --nocapture

printf '\n[7/8] Remove package layout and prove no-link fallback\n'
remove_installed_layout
env \
  -u NIMBUS_BUN_EMBED_SHARED_LIBRARY \
  -u NIMBUS_BUN_JSC_ADAPTER_MANIFEST \
  cargo test \
    -p nimbus-runtime \
    --features bun-jsc-linked-adapter \
    --lib \
    backends::bun_jsc::tests::bun_jsc_linked_adapter_feature_requires_explicit_shared_library_for_execution \
    -- --nocapture

printf '\n[8/8] Default Bun/JSC runtime contract keeps V8/Node lanes green\n'
env \
  -u NIMBUS_BUN_EMBED_SHARED_LIBRARY \
  -u NIMBUS_BUN_JSC_ADAPTER_MANIFEST \
  make verify-bun-jsc-runtime-contract

printf '\nBun/JSC installed-package proof: pass\n'
