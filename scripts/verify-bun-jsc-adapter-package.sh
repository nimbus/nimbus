#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: verify-bun-jsc-adapter-package.sh --archive <path> [options]

Verify a packaged Nimbus Bun/JSC shared adapter archive without rebuilding
Bun/WebKit. This checks the archive layout, manifest contract, checksums,
dynamic export set, and native symbol leak policy.

Required:
  --archive <path>             Adapter archive produced by package-bun-jsc-adapter.sh

Optional:
  --target-triple <triple>     Expected target triple (default: rustc host triple)
  --nm <path>                  nm-compatible command for export audit (test hook)
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
nm_bin="${NM_BIN:-nm}"

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
    --nm)
      nm_bin="${2:-}"
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
command -v tar >/dev/null 2>&1 || die "tar is required"
command -v python3 >/dev/null 2>&1 || die "python3 is required"
command -v "${nm_bin}" >/dev/null 2>&1 || die "nm command not found: ${nm_bin}"

if [[ -z "${target_triple}" ]]; then
  target_triple="$(bun_jsc_adapter_host_triple)"
fi
platform="$(bun_jsc_adapter_platform_for_triple "${target_triple}")"
[[ "${platform}" != "unsupported" ]] || die "unsupported target triple: ${target_triple}"
library_basename="$(bun_jsc_adapter_library_basename_for_triple "${target_triple}")"

tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-bun-jsc-adapter-package.XXXXXX")"
trap 'rm -rf "${tmp_root}"' EXIT
tar -xzf "${archive_path}" -C "${tmp_root}"

manifest_path="${tmp_root}/${BUN_JSC_ADAPTER_MANIFEST_FILE}"
library_path="${tmp_root}/${library_basename}"
checksums_path="${tmp_root}/${BUN_JSC_ADAPTER_CHECKSUMS_FILE}"
readme_path="${tmp_root}/${BUN_JSC_ADAPTER_README_FILE}"

[[ -f "${manifest_path}" ]] || die "missing ${BUN_JSC_ADAPTER_MANIFEST_FILE} in ${archive_path}"
[[ -f "${library_path}" ]] || die "missing ${library_basename} in ${archive_path}"
[[ -f "${checksums_path}" ]] || die "missing ${BUN_JSC_ADAPTER_CHECKSUMS_FILE} in ${archive_path}"
[[ -f "${readme_path}" ]] || die "missing ${BUN_JSC_ADAPTER_README_FILE} in ${archive_path}"

library_sha256="$(bun_jsc_adapter_sha256_file "${library_path}")"
manifest_sha256="$(bun_jsc_adapter_sha256_file "${manifest_path}")"
readme_sha256="$(bun_jsc_adapter_sha256_file "${readme_path}")"
grep -F "${library_sha256}  ${library_basename}" "${checksums_path}" >/dev/null ||
  die "checksums file does not contain matching ${library_basename} digest"
grep -F "${manifest_sha256}  ${BUN_JSC_ADAPTER_MANIFEST_FILE}" "${checksums_path}" >/dev/null ||
  die "checksums file does not contain matching manifest digest"
grep -F "${readme_sha256}  ${BUN_JSC_ADAPTER_README_FILE}" "${checksums_path}" >/dev/null ||
  die "checksums file does not contain matching README digest"

required_exports_file="${tmp_root}/expected-exports.txt"
actual_exports_file="${tmp_root}/actual-exports.txt"
printf '%s\n' "${BUN_JSC_ADAPTER_REQUIRED_EXPORTS[@]}" | sort -u >"${required_exports_file}"
required_exports_json="$(
  printf '%s\n' "${BUN_JSC_ADAPTER_REQUIRED_EXPORTS[@]}" |
    python3 -c 'import json,sys; print(json.dumps([line.rstrip("\n") for line in sys.stdin if line.rstrip("\n")]))'
)"

export BUN_JSC_ADAPTER_SCHEMA_VERSION
export BUN_JSC_ADAPTER_KIND
export BUN_JSC_ADAPTER_ABI_NAME
export BUN_JSC_ADAPTER_ABI_VERSION
export BUN_JSC_ADAPTER_MEMORY_ENFORCEMENT
export BUN_JSC_ADAPTER_LIFECYCLE
export BUN_JSC_ADAPTER_SOURCE_REPOSITORY
export BUN_JSC_ADAPTER_SOURCE_REF
export BUN_JSC_ADAPTER_SOURCE_REVISION
export BUN_JSC_ADAPTER_CHECKSUMS_FILE
export target_triple
export platform
export library_basename
export library_sha256
export required_exports_json

python3 - "${manifest_path}" <<'PY'
import json
import os
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
manifest = json.loads(path.read_text())
allowed_top = {
    "schema_version",
    "kind",
    "adapter_version",
    "nimbus_version",
    "bun_source_repository",
    "bun_source_ref",
    "bun_source_revision",
    "target_triple",
    "platform",
    "library",
    "library_sha256",
    "abi",
    "memory_enforcement",
    "lifecycle",
    "provenance",
}
unknown = set(manifest) - allowed_top
if unknown:
    raise SystemExit(f"unknown manifest fields: {sorted(unknown)}")

expected = {
    "schema_version": int(os.environ["BUN_JSC_ADAPTER_SCHEMA_VERSION"]),
    "kind": os.environ["BUN_JSC_ADAPTER_KIND"],
    "bun_source_repository": os.environ["BUN_JSC_ADAPTER_SOURCE_REPOSITORY"],
    "bun_source_ref": os.environ["BUN_JSC_ADAPTER_SOURCE_REF"],
    "bun_source_revision": os.environ["BUN_JSC_ADAPTER_SOURCE_REVISION"],
    "target_triple": os.environ["target_triple"],
    "platform": os.environ["platform"],
    "library": os.environ["library_basename"],
    "library_sha256": os.environ["library_sha256"],
    "memory_enforcement": os.environ["BUN_JSC_ADAPTER_MEMORY_ENFORCEMENT"],
    "lifecycle": os.environ["BUN_JSC_ADAPTER_LIFECYCLE"],
}
for key, value in expected.items():
    if manifest.get(key) != value:
        raise SystemExit(f"manifest {key} mismatch: expected {value!r}, got {manifest.get(key)!r}")
for key in ("adapter_version", "nimbus_version"):
    if not isinstance(manifest.get(key), str) or not manifest[key].strip():
        raise SystemExit(f"manifest {key} must be a non-empty string")

abi = manifest.get("abi")
if not isinstance(abi, dict):
    raise SystemExit("manifest abi must be an object")
allowed_abi = {"name", "version", "required_exports"}
unknown_abi = set(abi) - allowed_abi
if unknown_abi:
    raise SystemExit(f"unknown abi fields: {sorted(unknown_abi)}")
if abi.get("name") != os.environ["BUN_JSC_ADAPTER_ABI_NAME"]:
    raise SystemExit("manifest abi.name mismatch")
if abi.get("version") != int(os.environ["BUN_JSC_ADAPTER_ABI_VERSION"]):
    raise SystemExit("manifest abi.version mismatch")
expected_exports = json.loads(os.environ["required_exports_json"])
if abi.get("required_exports") != expected_exports:
    raise SystemExit("manifest abi.required_exports mismatch")

provenance = manifest.get("provenance")
if provenance is not None:
    if not isinstance(provenance, dict):
        raise SystemExit("manifest provenance must be an object")
    allowed_provenance = {"checksum_file", "sbom", "slsa"}
    unknown_provenance = set(provenance) - allowed_provenance
    if unknown_provenance:
        raise SystemExit(f"unknown provenance fields: {sorted(unknown_provenance)}")
    if provenance.get("checksum_file") != os.environ["BUN_JSC_ADAPTER_CHECKSUMS_FILE"]:
        raise SystemExit("manifest provenance.checksum_file mismatch")
PY

list_shared_adapter_exports() {
  local shared_library="$1"
  case "${target_triple}" in
    *-apple-darwin)
      "${nm_bin}" -gU "${shared_library}" 2>/dev/null |
        awk '{ print $3 }' |
        sed -E 's/^_//; s/@.*$//' |
        sort -u
      ;;
    *)
      "${nm_bin}" -D --defined-only -C "${shared_library}" 2>/dev/null |
        awk '{ print $3 }' |
        sed -E 's/@@.*$//; s/@.*$//' |
        sort -u
      ;;
  esac
}

list_shared_adapter_exports "${library_path}" >"${actual_exports_file}"

leak_pattern='v8::|hwy::|rust_eh_personality|simdutf::|simdutf__|nimbus_bun_simdutf::|nimbus_bun_simdutf__'
case "${target_triple}" in
  *-apple-darwin)
    leaked_count="$("${nm_bin}" -gU -C "${library_path}" 2>/dev/null |
      awk -v pattern="${leak_pattern}" '$0 ~ pattern { count++ } END { print count + 0 }')"
    ;;
  *)
    leaked_count="$("${nm_bin}" -D --defined-only -C "${library_path}" 2>/dev/null |
      awk -v pattern="${leak_pattern}" '$0 ~ pattern { count++ } END { print count + 0 }')"
    if command -v readelf >/dev/null 2>&1 &&
      readelf -d "${library_path}" 2>/dev/null | grep -q TEXTREL; then
      die "Bun/JSC adapter archive has TEXTREL dynamic entries"
    fi
    if command -v readelf >/dev/null 2>&1 &&
      readelf -d "${library_path}" 2>/dev/null | grep -q STATIC_TLS; then
      die "Bun/JSC adapter archive has STATIC_TLS and is not safe for late dlopen"
    fi
    ;;
esac
[[ "${leaked_count}" -eq 0 ]] ||
  die "Bun/JSC adapter archive exports bundled native implementation symbols"

if ! diff -u "${required_exports_file}" "${actual_exports_file}"; then
  die "Bun/JSC adapter archive export set drifted"
fi

printf 'verified: Bun/JSC adapter package archive matches manifest, checksum, export, and native-symbol contracts\n'
