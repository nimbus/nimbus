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
    "bun_source_ref": "nimbus-bun-jsc-proof-main-20260525",
    "bun_source_revision": "ad0e1d2bbc6690651e04f10eaf1dcdf8a6c0de57",
    "target_triple": "x86_64-unknown-linux-gnu",
    "platform": "linux",
    "library": "libnimbus_bun_jsc_embedder.so",
    "library_sha256": library_sha,
    "abi": {
        "name": "nimbus-bun-jsc-embedder",
        "version": 1,
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
        {"name": "bun", "version": "nimbus-bun-jsc-proof-main-20260525"},
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
    printf '{"tag_name":"v1.27.1-nimbus.2"}'
    ;;
  https://api.github.com/repos/nimbus/nimbus-libkrun/releases/latest)
    printf '{"tag_name":"v1.18.1-nimbus.1"}'
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
    pass "dry-run uses GITHUB_TOKEN for GitHub API lookups"
  else
    fail "dry-run uses GITHUB_TOKEN for GitHub API lookups"
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
    sh "${repo_root}/scripts/install.sh" --dry-run --version v0.1.14 --libkrun-version v1.18.1-nimbus.1 --prefix /tmp/custom \
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

# --- Dry run output ---------------------------------------------------------

echo ""
echo "Checking dry-run output..."

# Use a mock version to avoid GitHub API calls
if sh "${repo_root}/scripts/install.sh" --dry-run --version v0.1.14 --crun-version v1.27.1-nimbus.2 --libkrun-version v1.18.1-nimbus.1 \
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
