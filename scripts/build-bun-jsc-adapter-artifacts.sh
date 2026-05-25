#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: build-bun-jsc-adapter-artifacts.sh --output-dir <path> [options]

Build, package, and verify the source-backed Nimbus Bun/JSC shared adapter for
the host platform. This is the heavy release/nightly/manual lane wrapper around
the linked-adapter proof plus the deterministic archive packager.

Required:
  --output-dir <path>          Output root for archives, logs, and proof summary

Optional:
  --bun-repo <path>            Nimbus Bun source checkout (default: $NIMBUS_BUN_REPO or ~/src/github.com/nimbus/bun)
  --nimbus-version <version>   Nimbus release version or tag (default: git describe)
  --adapter-version <version>  Adapter version (default: package helper default)
  --target-triple <triple>     Host target triple (default: rustc host triple)
  --bun-source-ref <ref>       Expected Bun source ref
  --bun-source-revision <sha>  Expected Bun source revision
  --bun-source-repository <url>
  --sbom <path>                Optional CycloneDX SBOM file to include
  --slsa <path>                Optional SLSA/in-toto provenance file to include
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

output_dir=""
bun_repo="${NIMBUS_BUN_REPO:-${HOME}/src/github.com/nimbus/bun}"
nimbus_version=""
adapter_version=""
target_triple=""
bun_source_repository="${BUN_JSC_ADAPTER_SOURCE_REPOSITORY}"
bun_source_ref="${BUN_JSC_ADAPTER_SOURCE_REF}"
bun_source_revision="${BUN_JSC_ADAPTER_SOURCE_REVISION}"
sbom_path=""
slsa_path=""

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --output-dir)
      output_dir="${2:-}"
      shift 2
      ;;
    --bun-repo)
      bun_repo="${2:-}"
      shift 2
      ;;
    --nimbus-version)
      nimbus_version="${2:-}"
      shift 2
      ;;
    --adapter-version)
      adapter_version="${2:-}"
      shift 2
      ;;
    --target-triple)
      target_triple="${2:-}"
      shift 2
      ;;
    --bun-source-repository)
      bun_source_repository="${2:-}"
      shift 2
      ;;
    --bun-source-ref)
      bun_source_ref="${2:-}"
      shift 2
      ;;
    --bun-source-revision)
      bun_source_revision="${2:-}"
      shift 2
      ;;
    --sbom)
      sbom_path="${2:-}"
      shift 2
      ;;
    --slsa)
      slsa_path="${2:-}"
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

[[ -n "${output_dir}" ]] || die "--output-dir is required"
[[ -d "${bun_repo}" ]] || die "Bun source checkout not found: ${bun_repo}"
[[ -n "${bun_source_repository}" ]] || die "--bun-source-repository must be non-empty"
[[ -n "${bun_source_ref}" ]] || die "--bun-source-ref must be non-empty"
[[ -n "${bun_source_revision}" ]] || die "--bun-source-revision must be non-empty"
command -v git >/dev/null 2>&1 || die "git is required"
command -v rustc >/dev/null 2>&1 || die "rustc is required"

if [[ -n "${NIMBUS_BUN_EMBED_SHARED_LIBRARY:-}" ]]; then
  die "NIMBUS_BUN_EMBED_SHARED_LIBRARY is not accepted by the source-backed artifact builder; use package-bun-jsc-adapter.sh for prebuilt libraries"
fi

if [[ -z "${target_triple}" ]]; then
  target_triple="$(bun_jsc_adapter_host_triple)"
fi
host_triple="$(bun_jsc_adapter_host_triple)"
if [[ "${target_triple}" != "${host_triple}" ]]; then
  die "Bun/JSC adapter artifact builds are host-native only for now: target ${target_triple}, host ${host_triple}"
fi
platform_arch="$(bun_jsc_adapter_archive_platform_arch_for_triple "${target_triple}")"
[[ "${platform_arch}" != unsupported-* ]] || die "unsupported target triple: ${target_triple}"
library_basename="$(bun_jsc_adapter_library_basename_for_triple "${target_triple}")"

if [[ -z "${nimbus_version}" ]]; then
  nimbus_version="$(git -C "${repo_root}" describe --tags --dirty --always 2>/dev/null || git -C "${repo_root}" rev-parse --short HEAD)"
fi

ref_rev="$(git -C "${bun_repo}" rev-parse "${bun_source_ref}^{commit}" 2>/dev/null || true)"
if [[ "${ref_rev}" != "${bun_source_revision}" ]]; then
  die "unexpected Bun source ref: expected ${bun_source_ref} at ${bun_source_revision}, got ${ref_rev:-missing}"
fi
actual_bun_rev="$(git -C "${bun_repo}" rev-parse HEAD)"
if [[ "${actual_bun_rev}" != "${bun_source_revision}" ]]; then
  die "unexpected Bun checkout revision: expected ${bun_source_revision}, got ${actual_bun_rev}"
fi
bun_status="$(git -C "${bun_repo}" status --short)"
if [[ -n "${bun_status}" ]]; then
  printf 'Bun source checkout must be clean for release artifact builds:\n%s\n' "${bun_status}" >&2
  exit 65
fi

mkdir -p "${output_dir}"
output_dir="$(cd "${output_dir}" && pwd)"
logs_dir="${output_dir}/logs"
build_root="${output_dir}/build"
mkdir -p "${logs_dir}" "${build_root}"

bun_build_dir="${NIMBUS_BUN_BUILD_DIR:-${build_root}/bun-${platform_arch}}"
bun_cache_dir="${NIMBUS_BUN_CACHE_DIR:-${build_root}/cache-${platform_arch}}"
bun_cargo_target_dir="${NIMBUS_BUN_CARGO_TARGET_DIR:-${build_root}/cargo-target-${platform_arch}}"
shared_library="${bun_build_dir}/${library_basename}"
linked_log="${logs_dir}/linked-adapter-${platform_arch}.log"
package_log="${logs_dir}/package-${platform_arch}.log"
verify_log="${logs_dir}/verify-package-${platform_arch}.log"
summary_path="${output_dir}/proof-summary-${platform_arch}.txt"

printf 'Bun/JSC adapter artifact build\n'
printf 'Nimbus repo: %s\n' "${repo_root}"
printf 'Output dir:  %s\n' "${output_dir}"
printf 'Bun repo:    %s\n' "${bun_repo}"
printf 'Bun ref:     %s\n' "${bun_source_ref}"
printf 'Bun rev:     %s\n' "${bun_source_revision}"
printf 'Target:      %s\n' "${target_triple}"
printf 'Archive:     nimbus-bun-jsc-adapter-%s.tar.gz\n\n' "${platform_arch}"

(
  cd "${repo_root}"
  NIMBUS_BUN_REPO="${bun_repo}" \
    NIMBUS_BUN_EXPECTED_REF="${bun_source_ref}" \
    NIMBUS_BUN_EXPECTED_REV="${bun_source_revision}" \
    NIMBUS_BUN_BUILD_DIR="${bun_build_dir}" \
    NIMBUS_BUN_CACHE_DIR="${bun_cache_dir}" \
    NIMBUS_BUN_CARGO_TARGET_DIR="${bun_cargo_target_dir}" \
    bash scripts/verify-bun-jsc-linked-adapter.sh
) 2>&1 | tee "${linked_log}"

[[ -f "${shared_library}" ]] || die "linked-adapter gate did not produce ${shared_library}"

package_args=(
  bash "${repo_root}/scripts/package-bun-jsc-adapter.sh"
  --output-dir "${output_dir}"
  --shared-library "${shared_library}"
  --nimbus-version "${nimbus_version}"
  --target-triple "${target_triple}"
  --bun-source-repository "${bun_source_repository}"
  --bun-source-ref "${bun_source_ref}"
  --bun-source-revision "${bun_source_revision}"
)
if [[ -n "${adapter_version}" ]]; then
  package_args+=(--adapter-version "${adapter_version}")
fi
if [[ -n "${sbom_path}" ]]; then
  package_args+=(--sbom "${sbom_path}")
fi
if [[ -n "${slsa_path}" ]]; then
  package_args+=(--slsa "${slsa_path}")
fi
package_output="$("${package_args[@]}" 2>&1 | tee "${package_log}")"
archive_path="$(awk -F= '$1 == "archive.path" { print $2; exit }' <<<"${package_output}")"
manifest_path="$(awk -F= '$1 == "manifest.path" { print $2; exit }' <<<"${package_output}")"
[[ -n "${archive_path}" && -f "${archive_path}" ]] || die "package helper did not report an archive path"
[[ -n "${manifest_path}" && -f "${manifest_path}" ]] || die "package helper did not report a manifest path"

bash "${repo_root}/scripts/verify-bun-jsc-adapter-package.sh" \
  --archive "${archive_path}" \
  --target-triple "${target_triple}" \
  2>&1 | tee "${verify_log}"

archive_sha256="$(bun_jsc_adapter_sha256_file "${archive_path}")"
{
  printf 'result=passed\n'
  printf 'nimbus.version=%s\n' "${nimbus_version}"
  printf 'bun.source.repository=%s\n' "${bun_source_repository}"
  printf 'bun.source.ref=%s\n' "${bun_source_ref}"
  printf 'bun.source.revision=%s\n' "${bun_source_revision}"
  printf 'target.triple=%s\n' "${target_triple}"
  printf 'platform.arch=%s\n' "${platform_arch}"
  printf 'shared.library=%s\n' "${shared_library}"
  printf 'archive.path=%s\n' "${archive_path}"
  printf 'archive.sha256=%s\n' "${archive_sha256}"
  printf 'manifest.path=%s\n' "${manifest_path}"
  printf 'linked.log=%s\n' "${linked_log}"
  printf 'package.log=%s\n' "${package_log}"
  printf 'verify.log=%s\n' "${verify_log}"
} >"${summary_path}"

printf '\nBun/JSC adapter artifact lane: pass\n'
printf 'archive.path=%s\n' "${archive_path}"
printf 'archive.sha256=%s\n' "${archive_sha256}"
printf 'proof.summary=%s\n' "${summary_path}"
