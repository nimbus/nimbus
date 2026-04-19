#!/usr/bin/env bash
# Deterministic unit tests for the install script infrastructure.
#
# Tests platform detection, argument parsing, and verification logic
# without requiring actual installations or network access.
#
# See docs/plans/install-script-plan.md for the verification contract.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="$(mktemp -d "${TMPDIR:-/tmp}/neovex-install-helper.XXXXXX")"
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

  if grep -q "NEOVEX_REQUIRE_ATTESTATIONS" "${output_dir}/help.txt"; then
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

# --- Checksum enforcement ----------------------------------------------------

echo ""
echo "Checking checksum enforcement..."

printf 'neovex-test\n' > "${output_dir}/artifact.bin"
artifact_sha="$(sha256_of "${output_dir}/artifact.bin")"
printf '%s  artifact.bin\n' "${artifact_sha}" > "${output_dir}/checksums-ok.txt"
printf '%s  something-else.bin\n' "${artifact_sha}" > "${output_dir}/checksums-missing.txt"

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
  https://api.github.com/repos/agentstation/neovex/releases/latest)
    printf '{"tag_name":"v0.1.14"}'
    ;;
  https://api.github.com/repos/agentstation/neovex-crun/releases/latest)
    printf '{"tag_name":"v1.27-neovex.1"}'
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
    sh "${repo_root}/scripts/install.sh" --dry-run --version v0.1.14 --prefix /tmp/custom \
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
if sh "${repo_root}/scripts/install.sh" --dry-run --version v0.1.14 \
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

  if grep -q "neovex:" "${output_dir}/dry-run.txt"; then
    pass "dry-run shows neovex path"
  else
    fail "dry-run shows neovex path"
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
    if grep -q "neovex-crun:" "${output_dir}/dry-run.txt"; then
      pass "Linux dry-run shows neovex-crun"
    else
      fail "Linux dry-run shows neovex-crun"
    fi

    if grep -q "/usr/libexec/neovex/crun" "${output_dir}/dry-run.txt"; then
      pass "Linux dry-run shows crun install path"
    else
      fail "Linux dry-run shows crun install path"
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
