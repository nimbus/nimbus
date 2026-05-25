#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: verify-bun-jsc-adapter-package.sh --archive <path> [options]

Verify a packaged Nimbus Bun/JSC shared adapter archive without rebuilding
Bun/WebKit. This checks the archive layout, manifest contract, checksums,
SBOM/provenance evidence, dynamic export set, and native symbol leak policy.

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
extract_root="${tmp_root}/extract"
mkdir -p "${extract_root}"
entries_path="${tmp_root}/archive-entries.txt"
tar -tzf "${archive_path}" >"${entries_path}"
while IFS= read -r entry; do
  case "${entry}" in
    ""|/*|../*|*"/../"*|*".."*|*/*)
      die "unsafe Bun/JSC adapter archive entry: ${entry}"
      ;;
  esac
done <"${entries_path}"
duplicate_entry="$(sort "${entries_path}" | uniq -d | head -n 1 || true)"
if [[ -n "${duplicate_entry}" ]]; then
  die "duplicate Bun/JSC adapter archive entry: ${duplicate_entry}"
fi
tar -xpzf "${archive_path}" -C "${extract_root}"

manifest_path="${extract_root}/${BUN_JSC_ADAPTER_MANIFEST_FILE}"
library_path="${extract_root}/${library_basename}"
checksums_path="${extract_root}/${BUN_JSC_ADAPTER_CHECKSUMS_FILE}"
readme_path="${extract_root}/${BUN_JSC_ADAPTER_README_FILE}"

[[ -f "${manifest_path}" ]] || die "missing ${BUN_JSC_ADAPTER_MANIFEST_FILE} in ${archive_path}"
[[ -f "${library_path}" ]] || die "missing ${library_basename} in ${archive_path}"
[[ -f "${checksums_path}" ]] || die "missing ${BUN_JSC_ADAPTER_CHECKSUMS_FILE} in ${archive_path}"
[[ -f "${readme_path}" ]] || die "missing ${BUN_JSC_ADAPTER_README_FILE} in ${archive_path}"

verify_safe_mode() {
  local label="$1"
  local path="$2"
  python3 - "${label}" "${path}" <<'PY'
import os
import pathlib
import sys

label = sys.argv[1]
path = pathlib.Path(sys.argv[2])
mode = path.stat().st_mode & 0o777
if mode & 0o022:
    raise SystemExit(
        f"{label} {path} has unsafe permissions {mode:o}; "
        "group/other writable packaged Bun/JSC adapter files are rejected"
    )
PY
}

verify_safe_mode "Bun/JSC adapter library" "${library_path}"
verify_safe_mode "Bun/JSC adapter manifest" "${manifest_path}"
verify_safe_mode "Bun/JSC adapter checksums file" "${checksums_path}"
verify_safe_mode "Bun/JSC adapter README" "${readme_path}"

library_sha256="$(bun_jsc_adapter_sha256_file "${library_path}")"
manifest_sha256="$(bun_jsc_adapter_sha256_file "${manifest_path}")"
readme_sha256="$(bun_jsc_adapter_sha256_file "${readme_path}")"

verify_checksum_entry() {
  local subject_name="$1"
  local digest="$2"
  awk -v expected_digest="${digest}" -v expected_subject="${subject_name}" '
    NF >= 2 {
      subject = $NF
      sub(/^\*/, "", subject)
      if (tolower($1) == tolower(expected_digest) && subject == expected_subject) {
        found = 1
        exit
      }
    }
    END { exit found ? 0 : 1 }
  ' "${checksums_path}" ||
    die "checksums file does not contain matching ${subject_name} digest"
}

verify_checksum_entry "${library_basename}" "${library_sha256}"
verify_checksum_entry "${BUN_JSC_ADAPTER_MANIFEST_FILE}" "${manifest_sha256}"
verify_checksum_entry "${BUN_JSC_ADAPTER_README_FILE}" "${readme_sha256}"

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
export BUN_JSC_ADAPTER_MANIFEST_FILE
export BUN_JSC_ADAPTER_README_FILE
export BUN_JSC_ADAPTER_SOURCE_REPOSITORY
export BUN_JSC_ADAPTER_SOURCE_REF
export BUN_JSC_ADAPTER_SOURCE_REVISION
export BUN_JSC_ADAPTER_CHECKSUMS_FILE
export target_triple
export platform
export library_basename
export library_sha256
export required_exports_json
evidence_files_path="${tmp_root}/evidence-files.txt"
export evidence_files_path

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
if not isinstance(provenance, dict):
    raise SystemExit("manifest provenance must be an object")
allowed_provenance = {"checksum_file", "sbom", "slsa"}
unknown_provenance = set(provenance) - allowed_provenance
if unknown_provenance:
    raise SystemExit(f"unknown provenance fields: {sorted(unknown_provenance)}")
if provenance.get("checksum_file") != os.environ["BUN_JSC_ADAPTER_CHECKSUMS_FILE"]:
    raise SystemExit("manifest provenance.checksum_file mismatch")
reserved = {
    os.environ["library_basename"],
    os.environ["BUN_JSC_ADAPTER_MANIFEST_FILE"],
    os.environ["BUN_JSC_ADAPTER_README_FILE"],
    os.environ["BUN_JSC_ADAPTER_CHECKSUMS_FILE"],
}
evidence_files = []
for key in ("sbom", "slsa"):
    value = provenance.get(key)
    if not isinstance(value, str) or not value.strip():
        raise SystemExit(f"manifest provenance.{key} must be a non-empty filename")
    if value in {".", ".."} or "/" in value or "\\" in value or ".." in value:
        raise SystemExit(f"manifest provenance.{key} must be a single safe filename")
    if value in reserved:
        raise SystemExit(f"manifest provenance.{key} collides with required archive file")
    evidence_files.append(value)
pathlib.Path(os.environ["evidence_files_path"]).write_text("\n".join(evidence_files) + "\n")
PY

expected_entries="${tmp_root}/expected-entries.txt"
{
  printf '%s\n' "${library_basename}"
  printf '%s\n' "${BUN_JSC_ADAPTER_MANIFEST_FILE}"
  printf '%s\n' "${BUN_JSC_ADAPTER_README_FILE}"
  printf '%s\n' "${BUN_JSC_ADAPTER_CHECKSUMS_FILE}"
  cat "${evidence_files_path}"
} | sort -u >"${expected_entries}"
sort -u "${entries_path}" >"${tmp_root}/actual-entries.txt"
if ! diff -u "${expected_entries}" "${tmp_root}/actual-entries.txt" >"${tmp_root}/entry-diff.txt"; then
  cat "${tmp_root}/entry-diff.txt" >&2
  die "Bun/JSC adapter archive entries do not match manifest provenance contract"
fi

sbom_file="$(sed -n '1p' "${evidence_files_path}")"
slsa_file="$(sed -n '2p' "${evidence_files_path}")"
for evidence_file in "${sbom_file}" "${slsa_file}"; do
  evidence_path="${extract_root}/${evidence_file}"
  [[ -f "${evidence_path}" ]] || die "missing provenance evidence file: ${evidence_file}"
  verify_safe_mode "Bun/JSC adapter provenance evidence" "${evidence_path}"
  verify_checksum_entry "${evidence_file}" "$(bun_jsc_adapter_sha256_file "${evidence_path}")"
done

python3 - "${extract_root}/${sbom_file}" "${extract_root}/${slsa_file}" "${library_basename}" "${library_sha256}" <<'PY'
import json
import pathlib
import sys

sbom_path = pathlib.Path(sys.argv[1])
slsa_path = pathlib.Path(sys.argv[2])
library_name = sys.argv[3]
library_sha256 = sys.argv[4]

sbom = json.loads(sbom_path.read_text())
if sbom.get("bomFormat") != "CycloneDX":
    raise SystemExit("SBOM evidence must be CycloneDX JSON")
if not isinstance(sbom.get("components"), list):
    raise SystemExit("SBOM evidence must contain components")
component_names = {component.get("name") for component in sbom["components"] if isinstance(component, dict)}
if library_name not in component_names:
    raise SystemExit("SBOM evidence must identify the adapter shared library")
if "bun" not in component_names:
    raise SystemExit("SBOM evidence must identify the Bun source component")
if library_sha256 not in json.dumps(sbom, sort_keys=True):
    raise SystemExit("SBOM evidence must contain the adapter shared library SHA-256")

statements = [
    json.loads(line)
    for line in slsa_path.read_text().splitlines()
    if line.strip()
]
if len(statements) != 1:
    raise SystemExit("SLSA evidence must contain exactly one JSON statement")
statement = statements[0]
if statement.get("_type") != "https://in-toto.io/Statement/v1":
    raise SystemExit("SLSA evidence must be an in-toto statement")
if statement.get("predicateType") != "https://slsa.dev/provenance/v1":
    raise SystemExit("SLSA evidence must use the SLSA provenance v1 predicate")
subjects = statement.get("subject")
if not isinstance(subjects, list):
    raise SystemExit("SLSA evidence must contain subjects")
matched = False
for subject in subjects:
    if not isinstance(subject, dict):
        continue
    if subject.get("name") == library_name and subject.get("digest", {}).get("sha256") == library_sha256:
        matched = True
if not matched:
    raise SystemExit("SLSA evidence must bind the adapter shared library SHA-256")
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

printf 'verified: Bun/JSC adapter package archive matches manifest, checksum, SBOM/provenance, export, and native-symbol contracts\n'
