#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "${repo_root}/scripts/bun-jsc-adapter-contract.sh"

tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-bun-jsc-adapter-helper.XXXXXX")"
trap 'rm -rf "${tmp_root}"' EXIT

target_triple="$(bun_jsc_adapter_host_triple)"
library_basename="$(bun_jsc_adapter_library_basename_for_triple "${target_triple}")"
evidence_sbom="nimbus-bun-jsc-adapter.sbom.cdx.json"
evidence_slsa="nimbus-bun-jsc-adapter.intoto.jsonl"
fixture_library="${tmp_root}/${library_basename}"
printf 'fixture shared adapter bytes\n' >"${fixture_library}"
chmod 0755 "${fixture_library}"

fake_nm="${tmp_root}/fake-nm"
cat >"${fake_nm}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
source "${repo_root}/scripts/bun-jsc-adapter-contract.sh"
mode="\${NIMBUS_BUN_JSC_FAKE_NM_MODE:-good}"
case "\${mode}" in
  good)
    for symbol in "\${BUN_JSC_ADAPTER_REQUIRED_EXPORTS[@]}"; do
      printf '0000000000000000 T %s\n' "\${symbol}"
    done
    ;;
  wrong-exports)
    first=1
    for symbol in "\${BUN_JSC_ADAPTER_REQUIRED_EXPORTS[@]}"; do
      if [[ "\${first}" -eq 1 ]]; then
        first=0
        continue
      fi
      printf '0000000000000000 T %s\n' "\${symbol}"
    done
    ;;
  leaked-native)
    for symbol in "\${BUN_JSC_ADAPTER_REQUIRED_EXPORTS[@]}"; do
      printf '0000000000000000 T %s\n' "\${symbol}"
    done
    printf '0000000000000000 T v8::LeakedSymbol\n'
    ;;
  *)
    printf 'unknown fake nm mode: %s\n' "\${mode}" >&2
    exit 2
    ;;
esac
EOF
chmod 0755 "${fake_nm}"

package_output="${tmp_root}/package"
bash "${repo_root}/scripts/package-bun-jsc-adapter.sh" \
  --output-dir "${package_output}" \
  --shared-library "${fixture_library}" \
  --nimbus-version v0.1.0 \
  --adapter-version v0.1.0-bun-proof-main-20260525 \
  --target-triple "${target_triple}" \
  >"${tmp_root}/package.out"

archive_path="$(awk -F= '$1 == "archive.path" { print $2 }' "${tmp_root}/package.out")"
[[ -f "${archive_path}" ]] || {
  printf 'package helper did not create archive: %s\n' "${archive_path}" >&2
  exit 1
}

bash "${repo_root}/scripts/verify-bun-jsc-adapter-package.sh" \
  --archive "${archive_path}" \
  --target-triple "${target_triple}" \
  --nm "${fake_nm}" \
  >"${tmp_root}/verify-good.out"
grep -F "verified: Bun/JSC adapter package archive matches manifest, checksum, SBOM/provenance" \
  "${tmp_root}/verify-good.out" >/dev/null
tar -tzf "${archive_path}" >"${tmp_root}/archive-entries.txt"
grep -Fx "${evidence_sbom}" "${tmp_root}/archive-entries.txt" >/dev/null
grep -Fx "${evidence_slsa}" "${tmp_root}/archive-entries.txt" >/dev/null

rewrite_checksums_for_extract() {
  local extract_dir="$1"
  (
    cd "${extract_dir}"
    : >"${BUN_JSC_ADAPTER_CHECKSUMS_FILE}"
    for file in $(find . -maxdepth 1 -type f -print | sed 's#^\./##' | sort); do
      [[ "${file}" != "${BUN_JSC_ADAPTER_CHECKSUMS_FILE}" ]] || continue
      printf '%s  %s\n' "$(bun_jsc_adapter_sha256_file "${file}")" "${file}" \
        >>"${BUN_JSC_ADAPTER_CHECKSUMS_FILE}"
    done
  )
}

repack_bad_archive() {
  local name="$1"
  local mutation="$2"
  local extract_dir="${tmp_root}/${name}"
  local bad_archive="${tmp_root}/${name}.tar.gz"
  mkdir -p "${extract_dir}"
  tar -xzf "${archive_path}" -C "${extract_dir}"
  case "${mutation}" in
    missing-library)
      rm -f "${extract_dir}/${library_basename}"
      ;;
    bad-checksum)
      printf 'tamper\n' >>"${extract_dir}/${library_basename}"
      ;;
    bad-checksum-subject)
      library_digest="$(bun_jsc_adapter_sha256_file "${extract_dir}/${library_basename}")"
      awk -v digest="${library_digest}" -v subject="${library_basename}.evil" -v library="${library_basename}" '
        $2 == library {
          printf "%s  %s\n", digest, subject
          next
        }
        { print }
      ' "${extract_dir}/${BUN_JSC_ADAPTER_CHECKSUMS_FILE}" \
        >"${extract_dir}/${BUN_JSC_ADAPTER_CHECKSUMS_FILE}.tmp"
      mv "${extract_dir}/${BUN_JSC_ADAPTER_CHECKSUMS_FILE}.tmp" \
        "${extract_dir}/${BUN_JSC_ADAPTER_CHECKSUMS_FILE}"
      ;;
    missing-sbom)
      rm -f "${extract_dir}/${evidence_sbom}"
      ;;
    bad-provenance-checksum)
      printf 'tamper\n' >>"${extract_dir}/${evidence_slsa}"
      ;;
    wrong-provenance-subject)
      python3 - "${extract_dir}/${evidence_slsa}" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
statement = json.loads(path.read_text())
statement["subject"][0]["digest"]["sha256"] = "0" * 64
path.write_text(json.dumps(statement, separators=(",", ":")) + "\n")
PY
      rewrite_checksums_for_extract "${extract_dir}"
      ;;
    bad-manifest)
      python3 - "${extract_dir}/${BUN_JSC_ADAPTER_MANIFEST_FILE}" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
manifest = json.loads(path.read_text())
manifest["kind"] = "wrong.kind"
path.write_text(json.dumps(manifest, indent=2) + "\n")
PY
      rewrite_checksums_for_extract "${extract_dir}"
      ;;
    unsafe-mode)
      chmod 0664 "${extract_dir}/${BUN_JSC_ADAPTER_MANIFEST_FILE}"
      ;;
    *)
      printf 'unknown mutation: %s\n' "${mutation}" >&2
      exit 2
      ;;
  esac
  (cd "${extract_dir}" && tar -czf "${bad_archive}" $(find . -maxdepth 1 -type f -print | sed 's#^\./##' | sort))
  printf '%s\n' "${bad_archive}"
}

missing_library_archive="$(repack_bad_archive missing-library missing-library)"
if bash "${repo_root}/scripts/verify-bun-jsc-adapter-package.sh" \
  --archive "${missing_library_archive}" \
  --target-triple "${target_triple}" \
  --nm "${fake_nm}" \
  >"${tmp_root}/missing-library.out" 2>&1; then
  printf 'expected missing-library archive verification to fail\n' >&2
  exit 1
fi
grep -F "missing ${library_basename}" "${tmp_root}/missing-library.out" >/dev/null

bad_checksum_archive="$(repack_bad_archive bad-checksum bad-checksum)"
if bash "${repo_root}/scripts/verify-bun-jsc-adapter-package.sh" \
  --archive "${bad_checksum_archive}" \
  --target-triple "${target_triple}" \
  --nm "${fake_nm}" \
  >"${tmp_root}/bad-checksum.out" 2>&1; then
  printf 'expected bad-checksum archive verification to fail\n' >&2
  exit 1
fi
grep -F "checksums file does not contain matching ${library_basename} digest" \
  "${tmp_root}/bad-checksum.out" >/dev/null

bad_checksum_subject_archive="$(repack_bad_archive bad-checksum-subject bad-checksum-subject)"
if bash "${repo_root}/scripts/verify-bun-jsc-adapter-package.sh" \
  --archive "${bad_checksum_subject_archive}" \
  --target-triple "${target_triple}" \
  --nm "${fake_nm}" \
  >"${tmp_root}/bad-checksum-subject.out" 2>&1; then
  printf 'expected bad-checksum-subject archive verification to fail\n' >&2
  exit 1
fi

missing_sbom_archive="$(repack_bad_archive missing-sbom missing-sbom)"
if bash "${repo_root}/scripts/verify-bun-jsc-adapter-package.sh" \
  --archive "${missing_sbom_archive}" \
  --target-triple "${target_triple}" \
  --nm "${fake_nm}" \
  >"${tmp_root}/missing-sbom.out" 2>&1; then
  printf 'expected missing-sbom archive verification to fail\n' >&2
  exit 1
fi
grep -F "entries do not match manifest provenance contract" \
  "${tmp_root}/missing-sbom.out" >/dev/null

bad_provenance_checksum_archive="$(repack_bad_archive bad-provenance-checksum bad-provenance-checksum)"
if bash "${repo_root}/scripts/verify-bun-jsc-adapter-package.sh" \
  --archive "${bad_provenance_checksum_archive}" \
  --target-triple "${target_triple}" \
  --nm "${fake_nm}" \
  >"${tmp_root}/bad-provenance-checksum.out" 2>&1; then
  printf 'expected bad-provenance-checksum archive verification to fail\n' >&2
  exit 1
fi
grep -F "checksums file does not contain matching ${evidence_slsa} digest" \
  "${tmp_root}/bad-provenance-checksum.out" >/dev/null

wrong_provenance_subject_archive="$(repack_bad_archive wrong-provenance-subject wrong-provenance-subject)"
if bash "${repo_root}/scripts/verify-bun-jsc-adapter-package.sh" \
  --archive "${wrong_provenance_subject_archive}" \
  --target-triple "${target_triple}" \
  --nm "${fake_nm}" \
  >"${tmp_root}/wrong-provenance-subject.out" 2>&1; then
  printf 'expected wrong-provenance-subject archive verification to fail\n' >&2
  exit 1
fi
grep -F "SLSA evidence must bind the adapter shared library SHA-256" \
  "${tmp_root}/wrong-provenance-subject.out" >/dev/null

bad_manifest_archive="$(repack_bad_archive bad-manifest bad-manifest)"
if bash "${repo_root}/scripts/verify-bun-jsc-adapter-package.sh" \
  --archive "${bad_manifest_archive}" \
  --target-triple "${target_triple}" \
  --nm "${fake_nm}" \
  >"${tmp_root}/bad-manifest.out" 2>&1; then
  printf 'expected bad-manifest archive verification to fail\n' >&2
  exit 1
fi
grep -F "manifest kind mismatch" "${tmp_root}/bad-manifest.out" >/dev/null

unsafe_mode_archive="$(repack_bad_archive unsafe-mode unsafe-mode)"
if bash "${repo_root}/scripts/verify-bun-jsc-adapter-package.sh" \
  --archive "${unsafe_mode_archive}" \
  --target-triple "${target_triple}" \
  --nm "${fake_nm}" \
  >"${tmp_root}/unsafe-mode.out" 2>&1; then
  printf 'expected unsafe-mode archive verification to fail\n' >&2
  exit 1
fi
grep -F "group/other writable packaged Bun/JSC adapter files are rejected" \
  "${tmp_root}/unsafe-mode.out" >/dev/null

if NIMBUS_BUN_JSC_FAKE_NM_MODE=wrong-exports \
  bash "${repo_root}/scripts/verify-bun-jsc-adapter-package.sh" \
    --archive "${archive_path}" \
    --target-triple "${target_triple}" \
    --nm "${fake_nm}" \
    >"${tmp_root}/wrong-exports.out" 2>&1; then
  printf 'expected wrong-exports archive verification to fail\n' >&2
  exit 1
fi
grep -F "export set drifted" "${tmp_root}/wrong-exports.out" >/dev/null

if NIMBUS_BUN_JSC_FAKE_NM_MODE=leaked-native \
  bash "${repo_root}/scripts/verify-bun-jsc-adapter-package.sh" \
    --archive "${archive_path}" \
    --target-triple "${target_triple}" \
    --nm "${fake_nm}" \
    >"${tmp_root}/leaked-native.out" 2>&1; then
  printf 'expected leaked-native archive verification to fail\n' >&2
  exit 1
fi
grep -F "exports bundled native implementation symbols" \
  "${tmp_root}/leaked-native.out" >/dev/null

printf 'verified: Bun/JSC adapter package helper accepts a good fixture with SBOM/provenance and rejects missing library, bad checksum, checksum subject spoofing, missing evidence, bad evidence checksum, wrong provenance subject, bad manifest, unsafe modes, wrong exports, and native leaks\n'
