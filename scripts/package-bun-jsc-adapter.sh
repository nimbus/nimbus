#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: package-bun-jsc-adapter.sh --output-dir <path> --shared-library <path> --nimbus-version <version> [options]

Package an existing Nimbus Bun/JSC shared adapter into the release/archive
layout consumed by the runtime manifest discovery path. This helper does not
build Bun/WebKit; source-backed builds remain owned by
scripts/verify-bun-jsc-linked-adapter.sh and future release lanes.

Required:
  --output-dir <path>          Output root for staging and archive artifacts
  --shared-library <path>      Built libnimbus_bun_jsc_embedder.so/dylib
  --nimbus-version <version>   Nimbus release version or tag

Optional:
  --adapter-version <version>  Adapter version (default: <nimbus-version>-bun-<bun-ref>)
  --target-triple <triple>     Target triple (default: rustc host triple)
  --bun-source-repository <url>
  --bun-source-ref <ref>
  --bun-source-revision <sha>
  --sbom <path>                CycloneDX SBOM content override (archive filename is fixed)
  --slsa <path>                SLSA/in-toto provenance content override (archive filename is fixed)
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
shared_library=""
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
    --shared-library)
      shared_library="${2:-}"
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
[[ -n "${shared_library}" ]] || die "--shared-library is required"
[[ -n "${nimbus_version}" ]] || die "--nimbus-version is required"
[[ -f "${shared_library}" ]] || die "shared library not found: ${shared_library}"
[[ -n "${bun_source_repository}" ]] || die "--bun-source-repository must be non-empty"
[[ -n "${bun_source_ref}" ]] || die "--bun-source-ref must be non-empty"
[[ -n "${bun_source_revision}" ]] || die "--bun-source-revision must be non-empty"
command -v python3 >/dev/null 2>&1 || die "python3 is required"

if [[ -z "${target_triple}" ]]; then
  target_triple="$(bun_jsc_adapter_host_triple)"
fi
platform="$(bun_jsc_adapter_platform_for_triple "${target_triple}")"
[[ "${platform}" != "unsupported" ]] || die "unsupported target triple: ${target_triple}"
platform_arch="$(bun_jsc_adapter_archive_platform_arch_for_triple "${target_triple}")"
[[ "${platform_arch}" != unsupported-* ]] || die "unsupported archive platform for target: ${target_triple}"
library_basename="$(bun_jsc_adapter_library_basename_for_triple "${target_triple}")"

if [[ -z "${adapter_version}" ]]; then
  adapter_version="${nimbus_version}-bun-${bun_source_ref}"
fi

if [[ -n "${sbom_path}" && ! -f "${sbom_path}" ]]; then
  die "SBOM file not found: ${sbom_path}"
fi
if [[ -n "${slsa_path}" && ! -f "${slsa_path}" ]]; then
  die "SLSA provenance file not found: ${slsa_path}"
fi

mkdir -p "${output_dir}"
output_dir="$(cd "${output_dir}" && pwd)"
artifact_name="nimbus-bun-jsc-adapter-${platform_arch}"
stage_dir="${output_dir}/staging/${artifact_name}"
archive_path="${output_dir}/${artifact_name}.tar.gz"
rm -rf "${stage_dir}" "${archive_path}"
mkdir -p "${stage_dir}"

install -m 0755 "${shared_library}" "${stage_dir}/${library_basename}"
library_sha256="$(bun_jsc_adapter_sha256_file "${stage_dir}/${library_basename}")"

validate_evidence_basename() {
  local field="$1"
  local basename="$2"
  case "${basename}" in
    ""|.|..|*/*|*\\*)
      die "${field} must resolve to a single filename, got: ${basename}"
      ;;
    "${library_basename}"|"${BUN_JSC_ADAPTER_MANIFEST_FILE}"|"${BUN_JSC_ADAPTER_README_FILE}"|"${BUN_JSC_ADAPTER_CHECKSUMS_FILE}")
      die "${field} must not collide with required adapter archive file: ${basename}"
      ;;
  esac
}

sbom_basename=""
slsa_basename=""
if [[ -n "${sbom_path}" ]]; then
  sbom_basename="nimbus-bun-jsc-adapter.sbom.cdx.json"
  validate_evidence_basename "--sbom" "${sbom_basename}"
  install -m 0644 "${sbom_path}" "${stage_dir}/${sbom_basename}"
else
  sbom_basename="nimbus-bun-jsc-adapter.sbom.cdx.json"
  export adapter_version
  export nimbus_version
  export bun_source_repository
  export bun_source_ref
  export bun_source_revision
  export target_triple
  export platform
  export library_basename
  export library_sha256
  python3 - <<'PY' >"${stage_dir}/${sbom_basename}"
import json
import os

document = {
    "bomFormat": "CycloneDX",
    "specVersion": "1.5",
    "version": 1,
    "metadata": {
        "component": {
            "type": "library",
            "name": "nimbus-bun-jsc-adapter",
            "version": os.environ["adapter_version"],
        },
        "properties": [
            {"name": "nimbus.version", "value": os.environ["nimbus_version"]},
            {"name": "bun.source.repository", "value": os.environ["bun_source_repository"]},
            {"name": "bun.source.ref", "value": os.environ["bun_source_ref"]},
            {"name": "bun.source.revision", "value": os.environ["bun_source_revision"]},
            {"name": "target.triple", "value": os.environ["target_triple"]},
            {"name": "platform", "value": os.environ["platform"]},
        ],
    },
    "components": [
        {
            "type": "file",
            "name": os.environ["library_basename"],
            "hashes": [
                {"alg": "SHA-256", "content": os.environ["library_sha256"]},
            ],
        },
        {
            "type": "library",
            "name": "bun",
            "version": os.environ["bun_source_ref"],
            "purl": f"pkg:github/{os.environ['bun_source_repository'].removeprefix('https://github.com/')}"
                    f"@{os.environ['bun_source_revision']}",
        },
    ],
}
print(json.dumps(document, indent=2))
PY
fi
if [[ -n "${slsa_path}" ]]; then
  slsa_basename="nimbus-bun-jsc-adapter.intoto.jsonl"
  validate_evidence_basename "--slsa" "${slsa_basename}"
  install -m 0644 "${slsa_path}" "${stage_dir}/${slsa_basename}"
else
  slsa_basename="nimbus-bun-jsc-adapter.intoto.jsonl"
  export adapter_version
  export nimbus_version
  export bun_source_repository
  export bun_source_ref
  export bun_source_revision
  export target_triple
  export platform
  export library_basename
  export library_sha256
  python3 - <<'PY' >"${stage_dir}/${slsa_basename}"
import json
import os

statement = {
    "_type": "https://in-toto.io/Statement/v1",
    "subject": [
        {
            "name": os.environ["library_basename"],
            "digest": {"sha256": os.environ["library_sha256"]},
        }
    ],
    "predicateType": "https://slsa.dev/provenance/v1",
    "predicate": {
        "buildDefinition": {
            "buildType": "https://github.com/nimbus/nimbus/bun-jsc-adapter-build/v1",
            "externalParameters": {
                "nimbusVersion": os.environ["nimbus_version"],
                "bunSourceRepository": os.environ["bun_source_repository"],
                "bunSourceRef": os.environ["bun_source_ref"],
                "bunSourceRevision": os.environ["bun_source_revision"],
                "targetTriple": os.environ["target_triple"],
                "platform": os.environ["platform"],
                "adapterVersion": os.environ["adapter_version"],
            },
        },
        "runDetails": {
            "builder": {
                "id": "https://github.com/nimbus/nimbus/.github/workflows/bun-jsc-adapter.yml"
            },
        },
    },
}
print(json.dumps(statement, separators=(",", ":")))
PY
fi

cat >"${stage_dir}/${BUN_JSC_ADAPTER_README_FILE}" <<EOF
# Nimbus Bun/JSC Adapter

Adapter version: ${adapter_version}
Nimbus version: ${nimbus_version}
Bun source: ${bun_source_repository}
Bun ref: ${bun_source_ref}
Bun revision: ${bun_source_revision}
Target triple: ${target_triple}

This archive contains the optional in-process Bun/JSC runtime adapter for
Nimbus. The default Nimbus binary remains valid without this archive; packaged
installs discover this adapter through ${BUN_JSC_ADAPTER_MANIFEST_FILE}.
EOF

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
export BUN_JSC_ADAPTER_CHECKSUMS_FILE
export adapter_version
export nimbus_version
export bun_source_repository
export bun_source_ref
export bun_source_revision
export target_triple
export platform
export library_basename
export library_sha256
export required_exports_json
export sbom_basename
export slsa_basename

python3 - <<'PY' >"${stage_dir}/${BUN_JSC_ADAPTER_MANIFEST_FILE}"
import json
import os
import sys

provenance = {
    "checksum_file": os.environ["BUN_JSC_ADAPTER_CHECKSUMS_FILE"],
}
if os.environ.get("sbom_basename"):
    provenance["sbom"] = os.environ["sbom_basename"]
if os.environ.get("slsa_basename"):
    provenance["slsa"] = os.environ["slsa_basename"]

manifest = {
    "schema_version": int(os.environ["BUN_JSC_ADAPTER_SCHEMA_VERSION"]),
    "kind": os.environ["BUN_JSC_ADAPTER_KIND"],
    "adapter_version": os.environ["adapter_version"],
    "nimbus_version": os.environ["nimbus_version"],
    "bun_source_repository": os.environ["bun_source_repository"],
    "bun_source_ref": os.environ["bun_source_ref"],
    "bun_source_revision": os.environ["bun_source_revision"],
    "target_triple": os.environ["target_triple"],
    "platform": os.environ["platform"],
    "library": os.environ["library_basename"],
    "library_sha256": os.environ["library_sha256"],
    "abi": {
        "name": os.environ["BUN_JSC_ADAPTER_ABI_NAME"],
        "version": int(os.environ["BUN_JSC_ADAPTER_ABI_VERSION"]),
        "required_exports": json.loads(os.environ["required_exports_json"]),
    },
    "memory_enforcement": os.environ["BUN_JSC_ADAPTER_MEMORY_ENFORCEMENT"],
    "lifecycle": os.environ["BUN_JSC_ADAPTER_LIFECYCLE"],
    "provenance": provenance,
}
json.dump(manifest, sys.stdout, indent=2)
print()
PY

(
  cd "${stage_dir}"
  files=(
    "${library_basename}"
    "${BUN_JSC_ADAPTER_MANIFEST_FILE}"
    "${BUN_JSC_ADAPTER_README_FILE}"
  )
  if [[ -n "${sbom_basename}" ]]; then
    files+=("${sbom_basename}")
  fi
  if [[ -n "${slsa_basename}" ]]; then
    files+=("${slsa_basename}")
  fi
  : >"${BUN_JSC_ADAPTER_CHECKSUMS_FILE}"
  for file in "${files[@]}"; do
    printf '%s  %s\n' "$(bun_jsc_adapter_sha256_file "${file}")" "${file}" \
      >>"${BUN_JSC_ADAPTER_CHECKSUMS_FILE}"
  done
  tar -czf "${archive_path}" "${files[@]}" "${BUN_JSC_ADAPTER_CHECKSUMS_FILE}"
)

printf 'stage.dir=%s\n' "${stage_dir}"
printf 'archive.path=%s\n' "${archive_path}"
printf 'manifest.path=%s\n' "${stage_dir}/${BUN_JSC_ADAPTER_MANIFEST_FILE}"
printf 'library.sha256=%s\n' "${library_sha256}"
printf 'result=packaged\n'
