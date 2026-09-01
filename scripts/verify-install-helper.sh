#!/usr/bin/env bash
# Deterministic unit tests for the install script infrastructure.
#
# Tests platform detection, argument parsing, and verification logic
# without requiring actual installations or network access.
#
# See docs/private/plans/install-script-plan.md for the verification contract.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-install-helper.XXXXXX")"
trap 'rm -rf "${output_dir}"' EXIT
testable_install_sh="${output_dir}/install-lib.sh"
testable_verify_install_sh="${output_dir}/verify-install-lib.sh"

test_count=0
fail_count=0

pass() {
  test_count=$((test_count + 1))
  printf '  [pass] %s\n' "$1"
}

fail() {
  test_count=$((test_count + 1))
  fail_count=$((fail_count + 1))
  printf '  [FAIL] %s\n' "$1" >&2
}

sha256_of() {
  local file_path="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${file_path}" | awk '{print $1}'
  else
    shasum -a 256 "${file_path}" | awk '{print $1}'
  fi
}

# --- Syntax checks ----------------------------------------------------------

echo "Checking script syntax..."

if bash -n "${repo_root}/scripts/install.sh" 2>/dev/null; then
  pass "install.sh bash syntax"
else
  fail "install.sh bash syntax"
fi

if bash -n "${repo_root}/scripts/verify-install.sh" 2>/dev/null; then
  pass "verify-install.sh bash syntax"
else
  fail "verify-install.sh bash syntax"
fi

sed '$d' "${repo_root}/scripts/install.sh" > "${testable_install_sh}"
sed '$d' "${repo_root}/scripts/verify-install.sh" > "${testable_verify_install_sh}"

# Check POSIX sh compatibility for install.sh
if command -v dash >/dev/null 2>&1; then
  if dash -n "${repo_root}/scripts/install.sh" 2>/dev/null; then
    pass "install.sh POSIX sh syntax (dash)"
  else
    fail "install.sh POSIX sh syntax (dash)"
  fi
else
  printf '  [skip] install.sh POSIX sh syntax (dash not available)\n'
fi

if grep -q "nimbus start" "${repo_root}/scripts/install.sh" && \
   ! grep -q "nimbus serve" "${repo_root}/scripts/install.sh"; then
  pass "install.sh getting-started output uses nimbus start"
else
  fail "install.sh getting-started output uses nimbus start"
fi

if grep -q "SPDX-License-Identifier: LicenseRef-Nimbus-Community" "${repo_root}/scripts/install.sh" && \
   grep -q "releases/latest/download/LICENSE" "${repo_root}/scripts/install.sh"; then
  pass "install.sh carries release license pointer"
else
  fail "install.sh carries release license pointer"
fi

# --- Help output ------------------------------------------------------------

echo ""
echo "Checking help output..."

if sh "${repo_root}/scripts/install.sh" --help > "${output_dir}/help.txt" 2>&1; then
  if grep -q "Usage:" "${output_dir}/help.txt"; then
    pass "install.sh --help shows usage"
  else
    fail "install.sh --help shows usage"
  fi

  if grep -q "\-\-version" "${output_dir}/help.txt"; then
    pass "install.sh --help documents --version"
  else
    fail "install.sh --help documents --version"
  fi

  if grep -q "\-\-dry-run" "${output_dir}/help.txt"; then
    pass "install.sh --help documents --dry-run"
  else
    fail "install.sh --help documents --dry-run"
  fi

  if grep -q "\-\-uninstall" "${output_dir}/help.txt"; then
    pass "install.sh --help documents --uninstall"
  else
    fail "install.sh --help documents --uninstall"
  fi

  if grep -q "\-\-libkrun-version" "${output_dir}/help.txt"; then
    pass "install.sh --help documents --libkrun-version"
  else
    fail "install.sh --help documents --libkrun-version"
  fi

  if grep -q "\-\-with-bun-jsc" "${output_dir}/help.txt"; then
    pass "install.sh --help documents --with-bun-jsc"
  else
    fail "install.sh --help documents --with-bun-jsc"
  fi

  if grep -q "NIMBUS_REQUIRE_ATTESTATIONS" "${output_dir}/help.txt"; then
    pass "install.sh --help documents attestation enforcement"
  else
    fail "install.sh --help documents attestation enforcement"
  fi
else
  fail "install.sh --help exits successfully"
fi

# --- Argument parsing -------------------------------------------------------

echo ""
echo "Checking argument parsing..."

# Unknown option should fail
if sh "${repo_root}/scripts/install.sh" --unknown-option 2>"${output_dir}/unknown.txt"; then
  fail "install.sh rejects unknown options"
else
  if grep -q "unknown option" "${output_dir}/unknown.txt"; then
    pass "install.sh rejects unknown options"
  else
    fail "install.sh rejects unknown options with message"
  fi
fi

# --version without value should fail
if sh "${repo_root}/scripts/install.sh" --version 2>"${output_dir}/version-missing.txt"; then
  fail "install.sh --version requires value"
else
  if grep -q "requires" "${output_dir}/version-missing.txt"; then
    pass "install.sh --version requires value"
  else
    fail "install.sh --version requires value with message"
  fi
fi

# --libkrun-version without value should fail
if sh "${repo_root}/scripts/install.sh" --libkrun-version 2>"${output_dir}/libkrun-version-missing.txt"; then
  fail "install.sh --libkrun-version requires value"
else
  if grep -q "requires" "${output_dir}/libkrun-version-missing.txt"; then
    pass "install.sh --libkrun-version requires value"
  else
    fail "install.sh --libkrun-version requires value with message"
  fi
fi

# --- Checksum enforcement ----------------------------------------------------

echo ""
echo "Checking checksum enforcement..."

printf 'nimbus-test\n' > "${output_dir}/artifact.bin"
artifact_sha="$(sha256_of "${output_dir}/artifact.bin")"
printf '%s  artifact.bin\n' "${artifact_sha}" > "${output_dir}/checksums-ok.txt"
printf '%s  something-else.bin\n' "${artifact_sha}" > "${output_dir}/checksums-missing.txt"
printf '%s  artifact.bin.evil\n' "${artifact_sha}" > "${output_dir}/checksums-subject-spoof.txt"

if sh -c '. "$1"; verify_file_checksum "$2" "$3" "$4"' sh \
    "${testable_install_sh}" \
    "${output_dir}/artifact.bin" \
    "${output_dir}/checksums-ok.txt" \
    artifact.bin >/dev/null 2>&1; then
  pass "verify_file_checksum accepts matching manifest entry"
else
  fail "verify_file_checksum accepts matching manifest entry"
fi

if sh -c '. "$1"; verify_file_checksum "$2" "$3" "$4"' sh \
    "${testable_install_sh}" \
    "${output_dir}/artifact.bin" \
    "${output_dir}/checksums-missing.txt" \
    artifact.bin >/dev/null 2>&1; then
  fail "verify_file_checksum rejects missing manifest entry"
else
  pass "verify_file_checksum rejects missing manifest entry"
fi

if sh -c '. "$1"; verify_file_checksum "$2" "$3" "$4"' sh \
    "${testable_install_sh}" \
    "${output_dir}/artifact.bin" \
    "${output_dir}/checksums-subject-spoof.txt" \
    artifact.bin >/dev/null 2>&1; then
  fail "verify_file_checksum rejects checksum subject spoofing"
else
  pass "verify_file_checksum rejects checksum subject spoofing"
fi

# --- Release document preservation -----------------------------------------

echo ""
echo "Checking release document preservation..."

release_documents_archive="${output_dir}/release-documents-archive"
release_documents_prefix="${output_dir}/release-documents-prefix"
mkdir -p "${release_documents_archive}"
printf 'license\n' > "${release_documents_archive}/LICENSE"
printf 'readme\n' > "${release_documents_archive}/README.md"

if sh -c '
    . "$1"
    NIMBUS_PREFIX="$2"
    maybe_sudo() { "$@"; }
    install_nimbus_release_documents "$3"
    nimbus_release_documents_present
    cmp "$3/LICENSE" "${NIMBUS_PREFIX}/share/doc/nimbus/LICENSE"
    cmp "$3/README.md" "${NIMBUS_PREFIX}/share/doc/nimbus/README.md"
  ' sh "${testable_install_sh}" "${release_documents_prefix}" "${release_documents_archive}" \
    > "${output_dir}/release-documents-install.txt" 2>&1; then
  pass "direct installer preserves release license and README"
else
  fail "direct installer preserves release license and README"
fi

rm -f "${release_documents_prefix}/share/doc/nimbus/README.md"
if sh -c '
    . "$1"
    NIMBUS_PREFIX="$2"
    PLATFORM="linux"
    ! nimbus_release_payload_present
  ' sh "${testable_install_sh}" "${release_documents_prefix}"; then
  pass "same-version payload predicate rejects a partial document install"
else
  fail "same-version payload predicate rejects a partial document install"
fi

release_documents_incomplete="${output_dir}/release-documents-incomplete"
mkdir -p "${release_documents_incomplete}"
printf 'license\n' > "${release_documents_incomplete}/LICENSE"
if sh -c '
    . "$1"
    NIMBUS_PREFIX="$2"
    maybe_sudo() { "$@"; }
    install_nimbus_release_documents "$3"
  ' sh "${testable_install_sh}" "${output_dir}/release-documents-reject-prefix" "${release_documents_incomplete}" \
    > "${output_dir}/release-documents-reject.txt" 2>&1; then
  fail "direct installer rejects archives without required release documents"
else
  pass "direct installer rejects archives without required release documents"
fi

# --- Bun/JSC adapter installer hardening ------------------------------------

echo ""
echo "Checking Bun/JSC adapter installer hardening..."

bun_jsc_layout_dir="${output_dir}/bun-jsc-layout"
mkdir -p "${bun_jsc_layout_dir}"
touch \
  "${bun_jsc_layout_dir}/libnimbus_bun_jsc_embedder.so" \
  "${bun_jsc_layout_dir}/nimbus-bun-jsc-adapter.json" \
  "${bun_jsc_layout_dir}/checksums-sha256.txt" \
  "${bun_jsc_layout_dir}/README.md"
tar -czf "${output_dir}/bun-jsc-missing-evidence.tar.gz" \
  -C "${bun_jsc_layout_dir}" \
  libnimbus_bun_jsc_embedder.so \
  nimbus-bun-jsc-adapter.json \
  checksums-sha256.txt \
  README.md

if sh -c '. "$1"; verify_bun_jsc_adapter_archive_layout "$2" "$3" "$4"' sh \
    "${testable_install_sh}" \
    "${output_dir}/bun-jsc-missing-evidence.tar.gz" \
    "${output_dir}/bun-jsc-missing-evidence.entries" \
    libnimbus_bun_jsc_embedder.so \
    >"${output_dir}/bun-jsc-missing-evidence.out" 2>&1; then
  fail "Bun/JSC installer archive layout requires SBOM/SLSA evidence"
else
  if grep -q "missing required entry: nimbus-bun-jsc-adapter.sbom.cdx.json" \
      "${output_dir}/bun-jsc-missing-evidence.out"; then
    pass "Bun/JSC installer archive layout requires SBOM/SLSA evidence"
  else
    fail "Bun/JSC installer archive layout reports missing SBOM/SLSA evidence"
  fi
fi

bun_jsc_strict_dir="${output_dir}/bun-jsc-strict"
mkdir -p "${bun_jsc_strict_dir}"
printf 'fixture Bun/JSC shared adapter bytes\n' \
  >"${bun_jsc_strict_dir}/libnimbus_bun_jsc_embedder.so"
chmod 0755 "${bun_jsc_strict_dir}/libnimbus_bun_jsc_embedder.so"
printf 'fixture README\n' >"${bun_jsc_strict_dir}/README.md"
library_sha="$(sha256_of "${bun_jsc_strict_dir}/libnimbus_bun_jsc_embedder.so")"
required_exports_file="${output_dir}/bun-jsc-required-exports.txt"
sh -c '. "$1"; bun_jsc_adapter_required_exports' sh "${testable_install_sh}" \
  >"${required_exports_file}"
required_exports_json="$(
  python3 - "${required_exports_file}" <<'PY'
import json
import pathlib
import sys
print(json.dumps(pathlib.Path(sys.argv[1]).read_text().splitlines()))
PY
)"

python3 - \
  "${bun_jsc_strict_dir}/nimbus-bun-jsc-adapter.json" \
  "${bun_jsc_strict_dir}/nimbus-bun-jsc-adapter.sbom.cdx.json" \
  "${bun_jsc_strict_dir}/nimbus-bun-jsc-adapter.intoto.jsonl" \
  "${library_sha}" \
  "${required_exports_json}" <<'PY'
import json
import pathlib
import sys

manifest_path = pathlib.Path(sys.argv[1])
sbom_path = pathlib.Path(sys.argv[2])
slsa_path = pathlib.Path(sys.argv[3])
library_sha = sys.argv[4]
required_exports = json.loads(sys.argv[5])

manifest_path.write_text(json.dumps({
    "schema_version": 1,
    "kind": "nimbus.bun_jsc.adapter",
    "adapter_version": "v0.1.0-bun-proof-main-20260525",
    "nimbus_version": "v0.1.0",
    "bun_source_repository": "https://github.com/nimbus/bun",
    "bun_source_ref": "codex/bun-v1.4.0-release-readiness",
    "bun_source_revision": "40d63a6879c933d613d816fffb3b1f8c437f9ca3",
    "target_triple": "x86_64-unknown-linux-gnu",
    "platform": "linux",
    "library": "libnimbus_bun_jsc_embedder.so",
    "library_sha256": library_sha,
    "abi": {
        "name": "nimbus-bun-jsc-embedder",
        "version": 2,
        "required_exports": required_exports,
    },
    "memory_enforcement": "outer_quota_required",
    "lifecycle": "fresh_discard",
    "provenance": {
        "checksum_file": "checksums-sha256.txt",
        "sbom": "nimbus-bun-jsc-adapter.sbom.cdx.json",
        "slsa": "nimbus-bun-jsc-adapter.intoto.jsonl",
    },
}, indent=2) + "\n")

sbom_path.write_text(json.dumps({
    "bomFormat": "CycloneDX",
    "components": [
        {"name": "libnimbus_bun_jsc_embedder.so", "hashes": [{"alg": "SHA-256", "content": library_sha}]},
        {"name": "bun", "version": "codex/bun-v1.4.0-release-readiness"},
    ],
}, separators=(",", ":")) + "\n")

slsa_path.write_text(json.dumps({
    "_type": "https://in-toto.io/Statement/v1",
    "predicateType": "https://slsa.dev/provenance/v1",
    "subject": [
        {"name": "libnimbus_bun_jsc_embedder.so", "digest": {"sha256": library_sha}},
    ],
    "predicate": {},
}, separators=(",", ":")) + "\n")
PY

(
  cd "${bun_jsc_strict_dir}"
  : >checksums-sha256.txt
  for fixture in README.md libnimbus_bun_jsc_embedder.so nimbus-bun-jsc-adapter.intoto.jsonl nimbus-bun-jsc-adapter.json nimbus-bun-jsc-adapter.sbom.cdx.json; do
    printf '%s  %s\n' "$(sha256_of "${fixture}")" "${fixture}" >>checksums-sha256.txt
  done
)

mock_bun_jsc_tools="${output_dir}/mock-bun-jsc-tools"
mkdir -p "${mock_bun_jsc_tools}"
{
  printf '#!/bin/sh\n'
  printf 'while IFS= read -r symbol; do\n'
  # shellcheck disable=SC2016 # Emit a child script that expands $symbol.
  printf '  printf '\''0000000000000000 T %%s\\n'\'' "$symbol"\n'
  printf 'done <<'\''SYMS'\''\n'
  cat "${required_exports_file}"
  printf 'SYMS\n'
} >"${mock_bun_jsc_tools}/nm"
cat >"${mock_bun_jsc_tools}/readelf" <<'EOF'
#!/bin/sh
exit 0
EOF
chmod +x "${mock_bun_jsc_tools}/nm" "${mock_bun_jsc_tools}/readelf"

if PATH="${mock_bun_jsc_tools}:$PATH" sh -c '. "$1"; verify_bun_jsc_adapter_manifest_contract "$2" "$3" "$4" "$5"' sh \
    "${testable_install_sh}" \
    "${bun_jsc_strict_dir}" \
    x86_64-unknown-linux-gnu \
    linux \
    libnimbus_bun_jsc_embedder.so \
    >"${output_dir}/bun-jsc-strict.out" 2>&1; then
  pass "Bun/JSC installer verifies manifest, evidence, exports, and native loader policy"
else
  fail "Bun/JSC installer verifies manifest, evidence, exports, and native loader policy"
fi

# --- macOS bundled-helper resolver parity -----------------------------------

echo ""
echo "Checking macOS bundled-helper resolver parity..."

# install.sh (download/install path) and verify-install.sh (post-install audit)
# each carry a standalone copy of the bundled-first macOS helper resolver,
# because both ship as single-file distributions (curl | sh, standalone verify)
# and cannot source a shared library. This guard fails closed if the two copies
# ever probe a different ordered sequence of resolution stages -- a drift that
# would silently desync where Nimbus locates its pinned gvproxy/vfkit between
# install time and verification time. It reduces each resolver body to a
# canonical stage signature that ignores POSIX-vs-bash surface syntax
# (`[ -x ]` vs `[[ -x ]]`, `${NIMBUS_PREFIX}` vs `${install_prefix}`) and
# compares only the resolution-order semantics both copies must share. The S3
# stage additionally asserts the *intra-stage* ordering: the Homebrew-prefix
# candidate (`${brew_prefix}/bin/${helper_name}`) must be probed before the
# `/usr/local/bin/${helper_name}` fallback on that shared `for candidate` line,
# so a reordering that silently changed which install wins also trips the guard.
resolution_signature() {
  awk -v fn="$2" '
    $0 == (fn "() {") { inbody = 1; next }
    inbody && $0 == "}" { inbody = 0; next }
    !inbody { next }
    {
      if (index($0, "-x ") && index($0, "/libexec/${helper_name}") && !index($0, "real_dir") && !s1) { printf "S1:prefix-libexec "; s1 = 1 }
      if (index($0, "-x ") && index($0, "real_dir}/libexec/${helper_name}") && !s2) { printf "S2:beside-binary "; s2 = 1 }
      if (!s3 && index($0, "/usr/local/bin/${helper_name}")) {
        brew_idx = index($0, "${brew_prefix}/bin/${helper_name}")
        usrlocal_idx = index($0, "/usr/local/bin/${helper_name}")
        if (brew_idx > 0 && brew_idx < usrlocal_idx) { printf "S3:brew-then-usrlocal "; s3 = 1 }
      }
      if (index($0, "command -v \"${helper_name}\"") && !s4) { printf "S4:path "; s4 = 1 }
    }
  ' "$1"
}

expected_resolver_signature="S1:prefix-libexec S2:beside-binary S3:brew-then-usrlocal S4:path "
install_resolver_signature="$(resolution_signature "${repo_root}/scripts/install.sh" resolve_macos_bundled_helper)"
verify_resolver_signature="$(resolution_signature "${repo_root}/scripts/verify-install.sh" resolve_macos_bundled_helper_path)"

if [[ "${install_resolver_signature}" == "${expected_resolver_signature}" ]]; then
  pass "install.sh resolve_macos_bundled_helper probes the canonical stage order"
else
  fail "install.sh resolve_macos_bundled_helper stage order drifted (got '${install_resolver_signature}')"
fi

if [[ "${verify_resolver_signature}" == "${expected_resolver_signature}" ]]; then
  pass "verify-install.sh resolve_macos_bundled_helper_path probes the canonical stage order"
else
  fail "verify-install.sh resolve_macos_bundled_helper_path stage order drifted (got '${verify_resolver_signature}')"
fi

if [[ "${install_resolver_signature}" == "${verify_resolver_signature}" ]]; then
  pass "install.sh and verify-install.sh macOS helper resolvers stay in lockstep"
else
  fail "install.sh and verify-install.sh macOS helper resolvers desynced"
fi

# --- Mocked platform checks --------------------------------------------------

echo ""
echo "Checking mocked platform behavior..."

mock_linux_bin="${output_dir}/mock-linux-bin"
mkdir -p "${mock_linux_bin}"
cat > "${mock_linux_bin}/uname" <<'EOF'
#!/bin/sh
case "$1" in
  -s) echo Linux ;;
  -m) echo x86_64 ;;
  *) echo Linux ;;
esac
EOF
chmod +x "${mock_linux_bin}/uname"

linux_curl_log="${output_dir}/linux-curl.log"
cat > "${mock_linux_bin}/curl" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >> "${linux_curl_log}"
last_arg=""
for arg in "\$@"; do
  last_arg="\$arg"
done
case "\$last_arg" in
  https://api.github.com/repos/nimbus/nimbus/releases/latest)
    printf '{"tag_name":"v0.1.14"}'
    ;;
  https://api.github.com/repos/nimbus/nimbus-crun/releases/latest)
    printf '{"tag_name":"v1.29.1-nimbus.2"}'
    ;;
  https://api.github.com/repos/nimbus/nimbus-libkrun/releases/latest)
    printf '{"tag_name":"v1.19.4-nimbus.3"}'
    ;;
  *)
    exit 97
    ;;
esac
EOF
chmod +x "${mock_linux_bin}/curl"

if PATH="${mock_linux_bin}:$PATH" GITHUB_TOKEN=test-token \
    sh "${repo_root}/scripts/install.sh" --dry-run \
    > "${output_dir}/linux-dry-run.txt" 2>&1; then
  if grep -q "Authorization: Bearer test-token" "${linux_curl_log}"; then
    pass "dry-run uses GITHUB_TOKEN for nimbus GitHub API lookup"
  else
    fail "dry-run uses GITHUB_TOKEN for nimbus GitHub API lookup"
  fi
  if grep -Eq "nimbus-crun/releases/latest|nimbus-libkrun/releases/latest" "${linux_curl_log}"; then
    fail "Linux dry-run avoids fork latest lookups"
  else
    pass "Linux dry-run avoids fork latest lookups"
  fi
  if grep -q "nimbus-crun: v1.29.1-nimbus.2 (upstream 1.29.1)" "${output_dir}/linux-dry-run.txt" &&
     grep -q "nimbus-libkrun: v1.19.4-nimbus.3 (upstream 1.19.4)" "${output_dir}/linux-dry-run.txt"; then
    pass "Linux dry-run uses validated VMM tuple"
  else
    fail "Linux dry-run uses validated VMM tuple"
  fi
else
  fail "Linux mocked dry-run exits successfully"
fi

if PATH="${mock_linux_bin}:$PATH" \
    sh "${repo_root}/scripts/install.sh" --dry-run --with-bun-jsc \
    > "${output_dir}/linux-dry-run-bun-jsc.txt" 2>&1; then
  if grep -q "nimbus-bun-jsc-adapter" "${output_dir}/linux-dry-run-bun-jsc.txt"; then
    pass "Linux dry-run shows optional Bun/JSC adapter install"
  else
    fail "Linux dry-run shows optional Bun/JSC adapter install"
  fi
else
  fail "Linux mocked Bun/JSC dry-run exits successfully"
fi

mock_macos_bin="${output_dir}/mock-macos-bin"
mkdir -p "${mock_macos_bin}"
cat > "${mock_macos_bin}/uname" <<'EOF'
#!/bin/sh
case "$1" in
  -s) echo Darwin ;;
  -m) echo arm64 ;;
  *) echo Darwin ;;
esac
EOF
chmod +x "${mock_macos_bin}/uname"

cat > "${mock_macos_bin}/sw_vers" <<'EOF'
#!/bin/sh
if [ "$1" = "-productVersion" ]; then
  echo 15.0
else
  echo 15.0
fi
EOF
chmod +x "${mock_macos_bin}/sw_vers"

macos_curl_log="${output_dir}/macos-curl.log"
cat > "${mock_macos_bin}/curl" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >> "${macos_curl_log}"
exit 88
EOF
chmod +x "${mock_macos_bin}/curl"

if PATH="${mock_macos_bin}:$PATH" \
    sh "${repo_root}/scripts/install.sh" --dry-run --version v0.1.14 --libkrun-version v1.19.4-nimbus.3 --prefix /tmp/custom \
    > "${output_dir}/macos-dry-run.txt" 2>&1; then
  if [ ! -s "${macos_curl_log}" ]; then
    pass "macOS dry-run avoids GitHub API lookup"
  else
    fail "macOS dry-run avoids GitHub API lookup"
  fi

  if grep -q "ignored on macOS" "${output_dir}/macos-dry-run.txt"; then
    pass "macOS dry-run warns about ignored Linux-only flags"
  else
    fail "macOS dry-run warns about ignored Linux-only flags"
  fi
else
  fail "macOS mocked dry-run exits successfully"
fi

# The current-version fast path must not touch the network when every bundled
# helper and release document is already present. This keeps repeated installs
# useful in offline or flaky-network environments and proves the payload
# predicate is checked before any checksum/archive download.
macos_fast_path_prefix="${output_dir}/macos-fast-path-prefix"
macos_fast_path_download_log="${output_dir}/macos-fast-path-downloads.log"
if sh -c '
    . "$1"
    NIMBUS_VERSION="v0.1.14"
    NIMBUS_PREFIX="$2"
    PLATFORM="darwin"
    ARCH="arm64"
    DRY_RUN=""
    DOWNLOAD_LOG="$3"
    mkdir -p "${NIMBUS_PREFIX}/libexec" "${NIMBUS_PREFIX}/share/doc/nimbus"
    printf "#!/bin/sh\n" > "${NIMBUS_PREFIX}/libexec/gvproxy"
    printf "#!/bin/sh\n" > "${NIMBUS_PREFIX}/libexec/vfkit"
    chmod +x "${NIMBUS_PREFIX}/libexec/gvproxy" "${NIMBUS_PREFIX}/libexec/vfkit"
    printf "license\n" > "${NIMBUS_PREFIX}/share/doc/nimbus/LICENSE"
    printf "readme\n" > "${NIMBUS_PREFIX}/share/doc/nimbus/README.md"
    get_installed_nimbus_version() { printf "%s\n" "${NIMBUS_VERSION}"; }
    download_to_file() {
      printf "%s\n" "$1" >> "${DOWNLOAD_LOG}"
      return 99
    }
    download_and_install_nimbus
  ' sh "${testable_install_sh}" "${macos_fast_path_prefix}" "${macos_fast_path_download_log}" \
    > "${output_dir}/macos-fast-path.txt" 2>&1; then
  if [ ! -s "${macos_fast_path_download_log}" ] && \
      grep -q "already installed" "${output_dir}/macos-fast-path.txt"; then
    pass "macOS current install with complete release payload skips without network"
  else
    fail "macOS current install with complete release payload skips without network"
  fi
else
  fail "macOS current install with complete release payload exits successfully"
fi

# A direct install owns its requested prefix. A current binary from another
# channel on PATH must not suppress that install, while both standalone and
# inline verification must accept a valid custom-prefix payload without
# requiring that prefix to be on PATH.
custom_prefix="${output_dir}/macos-custom-prefix"
mkdir -p "${custom_prefix}/bin" "${custom_prefix}/libexec" "${custom_prefix}/share/doc/nimbus"
cat > "${custom_prefix}/bin/nimbus" <<'EOF'
#!/bin/sh
printf 'nimbus 0.1.14\n'
EOF
printf '#!/bin/sh\n' > "${custom_prefix}/libexec/gvproxy"
printf '#!/bin/sh\n' > "${custom_prefix}/libexec/vfkit"
printf 'license\n' > "${custom_prefix}/share/doc/nimbus/LICENSE"
printf 'readme\n' > "${custom_prefix}/share/doc/nimbus/README.md"
chmod +x "${custom_prefix}/bin/nimbus" "${custom_prefix}/libexec/gvproxy" "${custom_prefix}/libexec/vfkit"

if PATH="${mock_macos_bin}:/usr/bin:/bin" NIMBUS_PREFIX="${custom_prefix}" \
    bash "${repo_root}/scripts/verify-install.sh" \
    > "${output_dir}/macos-custom-prefix-standalone.txt" 2>&1 &&
   grep -Fq "present path=${custom_prefix}/bin/nimbus version=nimbus 0.1.14" \
     "${output_dir}/macos-custom-prefix-standalone.txt"; then
  pass "macOS standalone verification accepts a direct custom-prefix payload"
else
  fail "macOS standalone verification accepts a direct custom-prefix payload"
fi

if PATH="${mock_macos_bin}:/usr/bin:/bin" sh -c '
    . "$1"
    PLATFORM="darwin"
    NIMBUS_PREFIX="$2"
    verify_installation_inline
  ' sh "${testable_install_sh}" "${custom_prefix}" \
    > "${output_dir}/macos-custom-prefix-inline.txt" 2>&1 &&
   grep -Fq "present path=${custom_prefix}/bin/nimbus" \
     "${output_dir}/macos-custom-prefix-inline.txt"; then
  pass "macOS inline verification accepts a direct custom-prefix payload"
else
  fail "macOS inline verification accepts a direct custom-prefix payload"
fi

linux_package_prefix="${output_dir}/linux-package-prefix"
mkdir -p "${linux_package_prefix}/bin" "${linux_package_prefix}/share/doc/nimbus"
cp "${custom_prefix}/bin/nimbus" "${linux_package_prefix}/bin/nimbus"
cp "${custom_prefix}/share/doc/nimbus/LICENSE" "${linux_package_prefix}/share/doc/nimbus/LICENSE"
cp "${custom_prefix}/share/doc/nimbus/README.md" "${linux_package_prefix}/share/doc/nimbus/README.md"
if NIMBUS_PREFIX="${linux_package_prefix}" bash -c '
    . "$1"
    [ "$(resolve_nimbus_release_document LICENSE)" = "$2/share/doc/nimbus/LICENSE" ]
    [ "$(resolve_nimbus_release_document README.md)" = "$2/share/doc/nimbus/README.md" ]
  ' bash "${testable_verify_install_sh}" "${linux_package_prefix}" \
    > "${output_dir}/linux-package-doc-layout.txt" 2>&1; then
  pass "standalone verification resolves the DEB/RPM release-document layout"
else
  fail "standalone verification resolves the DEB/RPM release-document layout"
fi

brew_root="${output_dir}/homebrew"
cask_root="${brew_root}/Caskroom/nimbus/0.1.46"
empty_prefix="${output_dir}/empty-prefix"
mkdir -p "${brew_root}/bin" "${cask_root}" "${empty_prefix}"
cask_root="$(cd "${cask_root}" && pwd)"
cp "${custom_prefix}/bin/nimbus" "${cask_root}/nimbus"
cp "${custom_prefix}/share/doc/nimbus/LICENSE" "${cask_root}/LICENSE"
cp "${custom_prefix}/share/doc/nimbus/README.md" "${cask_root}/README.md"
ln -s "../Caskroom/nimbus/0.1.46/nimbus" "${brew_root}/bin/nimbus"

if PATH="${brew_root}/bin:/usr/bin:/bin" NIMBUS_PREFIX="${empty_prefix}" bash -c '
    . "$1"
    [ "$(resolve_nimbus_release_document LICENSE)" = "$2/LICENSE" ]
    [ "$(resolve_nimbus_release_document README.md)" = "$2/README.md" ]
  ' bash "${testable_verify_install_sh}" "${cask_root}" \
    > "${output_dir}/homebrew-cask-doc-layout.txt" 2>&1; then
  pass "standalone verification resolves Homebrew Cask release documents"
else
  fail "standalone verification resolves Homebrew Cask release documents"
fi

if PATH="${brew_root}/bin:/usr/bin:/bin" NIMBUS_PREFIX="${empty_prefix}" sh -c '
    . "$1"
    NIMBUS_PREFIX="$3"
    [ "$(resolve_nimbus_release_document LICENSE)" = "$2/LICENSE" ]
    [ "$(resolve_nimbus_release_document README.md)" = "$2/README.md" ]
  ' sh "${testable_install_sh}" "${cask_root}" "${empty_prefix}" \
    > "${output_dir}/homebrew-cask-inline-doc-layout.txt" 2>&1; then
  pass "inline verification resolves Homebrew Cask release documents"
else
  fail "inline verification resolves Homebrew Cask release documents"
fi

stale_docs_prefix="${output_dir}/stale-docs-prefix"
mkdir -p "${stale_docs_prefix}/share/doc/nimbus"
printf 'stale license\n' > "${stale_docs_prefix}/share/doc/nimbus/LICENSE"
printf 'stale readme\n' > "${stale_docs_prefix}/share/doc/nimbus/README.md"

if PATH="${brew_root}/bin:/usr/bin:/bin" NIMBUS_PREFIX="${stale_docs_prefix}" bash -c '
    . "$1"
    [ "$(resolve_nimbus_release_document LICENSE)" = "$2/LICENSE" ]
    [ "$(resolve_nimbus_release_document README.md)" = "$2/README.md" ]
  ' bash "${testable_verify_install_sh}" "${cask_root}" \
    > "${output_dir}/stale-prefix-doc-layout.txt" 2>&1; then
  pass "standalone verification ignores documents outside the selected binary channel"
else
  fail "standalone verification ignores documents outside the selected binary channel"
fi

if PATH="${brew_root}/bin:/usr/bin:/bin" sh -c '
    . "$1"
    NIMBUS_PREFIX="$3"
    [ "$(resolve_nimbus_release_document LICENSE)" = "$2/LICENSE" ]
    [ "$(resolve_nimbus_release_document README.md)" = "$2/README.md" ]
  ' sh "${testable_install_sh}" "${cask_root}" "${stale_docs_prefix}" \
    > "${output_dir}/stale-prefix-inline-doc-layout.txt" 2>&1; then
  pass "inline verification ignores documents outside the selected binary channel"
else
  fail "inline verification ignores documents outside the selected binary channel"
fi

if PATH="${brew_root}/bin:/usr/bin:/bin" NIMBUS_PREFIX="${brew_root}" bash -c '
    . "$1"
    [ "$(resolve_nimbus_release_document LICENSE)" = "$2/LICENSE" ]
    [ "$(resolve_nimbus_release_document README.md)" = "$2/README.md" ]
  ' bash "${testable_verify_install_sh}" "${cask_root}" \
    > "${output_dir}/homebrew-prefix-symlink-doc-layout.txt" 2>&1 &&
   PATH="${brew_root}/bin:/usr/bin:/bin" sh -c '
    . "$1"
    NIMBUS_PREFIX="$2"
    [ -z "$(get_installed_nimbus_version)" ]
    [ "$(resolve_nimbus_release_document LICENSE)" = "$3/LICENSE" ]
    [ "$(resolve_nimbus_release_document README.md)" = "$3/README.md" ]
  ' sh "${testable_install_sh}" "${brew_root}" "${cask_root}"; then
  pass "Homebrew prefix symlink remains package-manager-owned"
else
  fail "Homebrew prefix symlink remains package-manager-owned"
fi

if PATH="${brew_root}/bin:/usr/bin:/bin" sh -c '
      . "$1"
      NIMBUS_PREFIX="$2"
      NIMBUS_VERSION="v0.1.46"
      DRY_RUN=1
      ! (download_and_install_nimbus)
    ' sh "${testable_install_sh}" "${brew_root}" \
    > "${output_dir}/homebrew-symlink-install-refusal.txt" 2>&1 &&
   grep -Fq "refusing to replace package-manager-owned symlink" \
     "${output_dir}/homebrew-symlink-install-refusal.txt"; then
  pass "direct installer refuses a package-manager-owned symlink"
else
  fail "direct installer refuses a package-manager-owned symlink"
fi

if PATH="${brew_root}/bin:/usr/bin:/bin" sh -c '
    . "$1"
    NIMBUS_PREFIX="$2"
    uninstall_macos
    [ -L "$NIMBUS_PREFIX/bin/nimbus" ]
  ' sh "${testable_install_sh}" "${brew_root}" \
    > "${output_dir}/homebrew-symlink-uninstall-preservation.txt" 2>&1; then
  pass "direct uninstaller preserves a package-manager-owned symlink"
else
  fail "direct uninstaller preserves a package-manager-owned symlink"
fi

linux_package_root="${output_dir}/linux-package-uninstall-prefix"
mkdir -p "${linux_package_root}/bin" "${linux_package_root}/share/doc/nimbus"
ln -s "/opt/nimbus/bin/nimbus" "${linux_package_root}/bin/nimbus"
printf 'package license\n' > "${linux_package_root}/share/doc/nimbus/LICENSE"
printf 'package readme\n' > "${linux_package_root}/share/doc/nimbus/README.md"

if PATH="/usr/bin:/bin" sh -c '
    . "$1"
    NIMBUS_PREFIX="$2"
    uninstall_linux
    [ -L "$NIMBUS_PREFIX/bin/nimbus" ]
    [ -f "$NIMBUS_PREFIX/share/doc/nimbus/LICENSE" ]
    [ -f "$NIMBUS_PREFIX/share/doc/nimbus/README.md" ]
  ' sh "${testable_install_sh}" "${linux_package_root}" \
    > "${output_dir}/linux-symlink-uninstall-preservation.txt" 2>&1 &&
   grep -Fq "Use the owning package manager" \
     "${output_dir}/linux-symlink-uninstall-preservation.txt"; then
  pass "Linux direct uninstaller preserves a package-manager-owned payload"
else
  fail "Linux direct uninstaller preserves a package-manager-owned payload"
fi

incomplete_prefix="${output_dir}/incomplete-prefix"
mkdir -p "${incomplete_prefix}/bin" "${incomplete_prefix}/libexec"
cp "${custom_prefix}/bin/nimbus" "${incomplete_prefix}/bin/nimbus"
cp "${custom_prefix}/libexec/gvproxy" "${incomplete_prefix}/libexec/gvproxy"
cp "${custom_prefix}/libexec/vfkit" "${incomplete_prefix}/libexec/vfkit"

if PATH="${mock_macos_bin}:/usr/bin:/bin" NIMBUS_PREFIX="${incomplete_prefix}" \
    bash "${repo_root}/scripts/verify-install.sh" \
    > "${output_dir}/missing-docs-standalone.txt" 2>&1; then
  fail "standalone verification rejects a prefix-owned binary without release documents"
elif grep -Fq "nimbus.LICENSE         missing" "${output_dir}/missing-docs-standalone.txt" &&
     grep -Fq "nimbus.README.md       missing" "${output_dir}/missing-docs-standalone.txt" &&
     grep -Fq "result                 unsupported" "${output_dir}/missing-docs-standalone.txt"; then
  pass "standalone verification rejects a prefix-owned binary without release documents"
else
  fail "standalone verification rejects a prefix-owned binary without release documents"
fi

if PATH="${mock_macos_bin}:/usr/bin:/bin" sh -c '
    . "$1"
    PLATFORM="darwin"
    NIMBUS_PREFIX="$2"
    ! verify_installation_inline
  ' sh "${testable_install_sh}" "${incomplete_prefix}" \
    > "${output_dir}/missing-docs-inline.txt" 2>&1 &&
   grep -Fq "nimbus.LICENSE         missing" "${output_dir}/missing-docs-inline.txt" &&
   grep -Fq "nimbus.README.md       missing" "${output_dir}/missing-docs-inline.txt" &&
   grep -Fq "result                 unsupported" "${output_dir}/missing-docs-inline.txt"; then
  pass "inline verification rejects a prefix-owned binary without release documents"
else
  fail "inline verification rejects a prefix-owned binary without release documents"
fi

if PATH="${brew_root}/bin:/usr/bin:/bin" NIMBUS_PREFIX="${incomplete_prefix}" bash -c '
    . "$1"
    ! resolve_nimbus_release_document LICENSE
    ! resolve_nimbus_release_document README.md
  ' bash "${testable_verify_install_sh}" &&
   PATH="${brew_root}/bin:/usr/bin:/bin" sh -c '
    . "$1"
    NIMBUS_PREFIX="$2"
    ! resolve_nimbus_release_document LICENSE
    ! resolve_nimbus_release_document README.md
  ' sh "${testable_install_sh}" "${incomplete_prefix}"; then
  pass "prefix-owned verification cannot borrow documents from a PATH channel"
else
  fail "prefix-owned verification cannot borrow documents from a PATH channel"
fi

foreign_path_bin="${output_dir}/foreign-path-bin"
mkdir -p "${foreign_path_bin}"
cp "${custom_prefix}/bin/nimbus" "${foreign_path_bin}/nimbus"
if PATH="${foreign_path_bin}:/usr/bin:/bin" sh -c '
    . "$1"
    NIMBUS_PREFIX="$2"
    [ -z "$(get_installed_nimbus_version)" ]
  ' sh "${testable_install_sh}" "${empty_prefix}"; then
  pass "direct installer ignores a foreign-channel nimbus on PATH"
else
  fail "direct installer ignores a foreign-channel nimbus on PATH"
fi

macos_uninstall_prefix="${output_dir}/macos-uninstall-prefix"
if sh -c '
    . "$1"
    NIMBUS_PREFIX="$2"
    DRY_RUN=""
    mkdir -p "${NIMBUS_PREFIX}/bin" "${NIMBUS_PREFIX}/libexec" "${NIMBUS_PREFIX}/share/doc/nimbus"
    printf "#!/bin/sh\n" > "${NIMBUS_PREFIX}/bin/nimbus"
    printf "#!/bin/sh\n" > "${NIMBUS_PREFIX}/libexec/gvproxy"
    printf "#!/bin/sh\n" > "${NIMBUS_PREFIX}/libexec/vfkit"
    chmod +x "${NIMBUS_PREFIX}/bin/nimbus" "${NIMBUS_PREFIX}/libexec/gvproxy" "${NIMBUS_PREFIX}/libexec/vfkit"
    printf "license\n" > "${NIMBUS_PREFIX}/share/doc/nimbus/LICENSE"
    printf "readme\n" > "${NIMBUS_PREFIX}/share/doc/nimbus/README.md"
    check_cmd() { return 1; }
    maybe_sudo() { "$@"; }
    uninstall_macos
    [ ! -e "${NIMBUS_PREFIX}/bin/nimbus" ] &&
      [ ! -e "${NIMBUS_PREFIX}/libexec/gvproxy" ] &&
      [ ! -e "${NIMBUS_PREFIX}/libexec/vfkit" ] &&
      [ ! -e "${NIMBUS_PREFIX}/share/doc/nimbus/LICENSE" ] &&
      [ ! -e "${NIMBUS_PREFIX}/share/doc/nimbus/README.md" ]
  ' sh "${testable_install_sh}" "${macos_uninstall_prefix}" \
    > "${output_dir}/macos-uninstall.txt" 2>&1; then
  pass "macOS uninstall removes direct-install bundled helpers and documents"
else
  fail "macOS uninstall removes direct-install bundled helpers and documents"
fi

# --- Dry run output ---------------------------------------------------------

echo ""
echo "Checking dry-run output..."

# Use a mock version to avoid GitHub API calls
if sh "${repo_root}/scripts/install.sh" --dry-run --version v0.1.14 --crun-version v1.29.1-nimbus.2 --libkrun-version v1.19.4-nimbus.3 \
    > "${output_dir}/dry-run.txt" 2>&1; then

  if grep -q "Install Plan" "${output_dir}/dry-run.txt"; then
    pass "dry-run shows install plan"
  else
    fail "dry-run shows install plan"
  fi

  if grep -q "Platform:" "${output_dir}/dry-run.txt"; then
    pass "dry-run shows platform"
  else
    fail "dry-run shows platform"
  fi

  if grep -q "nimbus:" "${output_dir}/dry-run.txt"; then
    pass "dry-run shows nimbus path"
  else
    fail "dry-run shows nimbus path"
  fi

  if grep -q "dry-run" "${output_dir}/dry-run.txt"; then
    pass "dry-run indicates no changes made"
  else
    fail "dry-run indicates no changes made"
  fi
else
  fail "dry-run exits successfully"
fi

# --- Platform-specific dry-run checks --------------------------------------

echo ""
echo "Checking platform-specific dry-run..."

os_name="$(uname -s)"

case "${os_name}" in
  Linux)
    if grep -q "nimbus-crun:" "${output_dir}/dry-run.txt"; then
      pass "Linux dry-run shows nimbus-crun"
    else
      fail "Linux dry-run shows nimbus-crun"
    fi

    if grep -q "nimbus-libkrun:" "${output_dir}/dry-run.txt"; then
      pass "Linux dry-run shows nimbus-libkrun"
    else
      fail "Linux dry-run shows nimbus-libkrun"
    fi

    if grep -q "/usr/libexec/nimbus/crun" "${output_dir}/dry-run.txt"; then
      pass "Linux dry-run shows crun install path"
    else
      fail "Linux dry-run shows crun install path"
    fi

    if grep -q "/usr/libexec/nimbus/lib" "${output_dir}/dry-run.txt"; then
      pass "Linux dry-run shows private libkrun path"
    else
      fail "Linux dry-run shows private libkrun path"
    fi

    if grep -q "manual build required" "${output_dir}/dry-run.txt"; then
      fail "Linux dry-run omits manual upstream libkrun instructions"
    else
      pass "Linux dry-run omits manual upstream libkrun instructions"
    fi
    ;;

  Darwin)
    if grep -q "Homebrew" "${output_dir}/dry-run.txt"; then
      pass "macOS dry-run mentions Homebrew"
    else
      fail "macOS dry-run mentions Homebrew"
    fi

    if grep -q "krunkit" "${output_dir}/dry-run.txt"; then
      pass "macOS dry-run mentions krunkit"
    else
      fail "macOS dry-run mentions krunkit"
    fi

    if grep -q "gvproxy" "${output_dir}/dry-run.txt"; then
      pass "macOS dry-run mentions gvproxy"
    else
      fail "macOS dry-run mentions gvproxy"
    fi
    ;;
esac

# --- Verification script checks ---------------------------------------------

echo ""
echo "Checking verification script..."

# The verification script should detect the current platform
if bash "${repo_root}/scripts/verify-install.sh" > "${output_dir}/verify.txt" 2>&1; then
  printf '  [info] verify-install.sh passed (components present)\n'
else
  printf '  [info] verify-install.sh reported issues (expected on fresh system)\n'
fi

if grep -q "host.os" "${output_dir}/verify.txt"; then
  pass "verify-install.sh reports host.os"
else
  fail "verify-install.sh reports host.os"
fi

if grep -q "host.arch" "${output_dir}/verify.txt"; then
  pass "verify-install.sh reports host.arch"
else
  fail "verify-install.sh reports host.arch"
fi

if grep -q "result" "${output_dir}/verify.txt"; then
  pass "verify-install.sh reports result"
else
  fail "verify-install.sh reports result"
fi

if grep -q "nimbus-bun-jsc" "${output_dir}/verify.txt"; then
  pass "verify-install.sh reports optional Bun/JSC adapter state"
else
  fail "verify-install.sh reports optional Bun/JSC adapter state"
fi

if grep -q "nimbus.LICENSE" "${output_dir}/verify.txt" &&
   grep -q "nimbus.README.md" "${output_dir}/verify.txt"; then
  pass "verify-install.sh reports required release documents"
else
  fail "verify-install.sh reports required release documents"
fi

# --- Uninstall dry-run ------------------------------------------------------

echo ""
echo "Checking uninstall dry-run..."

if sh "${repo_root}/scripts/install.sh" --dry-run --uninstall \
    > "${output_dir}/uninstall-dry-run.txt" 2>&1; then

  if grep -q "dry-run" "${output_dir}/uninstall-dry-run.txt"; then
    pass "uninstall dry-run indicates no changes"
  else
    fail "uninstall dry-run indicates no changes"
  fi

  if grep -q "remove" "${output_dir}/uninstall-dry-run.txt" || \
     grep -q "uninstall" "${output_dir}/uninstall-dry-run.txt"; then
    pass "uninstall dry-run describes removal"
  else
    fail "uninstall dry-run describes removal"
  fi
else
  fail "uninstall dry-run exits successfully"
fi

# --- Summary ----------------------------------------------------------------

echo ""
if [[ "${fail_count}" -eq 0 ]]; then
  printf 'verified: install script helper passed %d tests\n' "${test_count}"
  exit 0
else
  printf 'failed: %d of %d tests failed\n' "${fail_count}" "${test_count}" >&2
  exit 1
fi
