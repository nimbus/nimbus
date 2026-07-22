#!/usr/bin/env bash
# Deterministic unit tests for scripts/verify-third-party-attribution.sh.
#
# Tests cover: pre-existence pass, missing LICENSE-MIT-muvm, missing
# THIRD_PARTY.md, listed-file-missing, missing provenance header,
# success path with valid headers, and vendored-patch NOTICE enforcement.
#
# See docs/private/plans/nimbus-sandbox-plan.md "Fork-Health Guardrails" §G4.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
script_under_test="${repo_root}/scripts/verify-third-party-attribution.sh"

if [ ! -x "${script_under_test}" ]; then
  printf '  [FAIL] %s is not executable\n' "${script_under_test}" >&2
  exit 1
fi

output_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-attribution-helper.XXXXXX")"
trap 'rm -rf "${output_dir}"' EXIT

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

# Each fixture lives under output_dir/<case>/{scripts,crates}/ and is run by
# invoking the copied script from inside that fixture root.
new_fixture() {
  local name="$1"
  local fixture="${output_dir}/${name}"
  mkdir -p "${fixture}/scripts"
  cp "${script_under_test}" "${fixture}/scripts/verify-third-party-attribution.sh"
  chmod +x "${fixture}/scripts/verify-third-party-attribution.sh"
  printf '%s\n' "${fixture}"
}

run_gate() {
  local fixture="$1"
  ( cd "${fixture}" && bash "${fixture}/scripts/verify-third-party-attribution.sh" 2>&1 )
}

assert_pass() {
  local name="$1"
  local fixture="$2"
  local output exit_code
  set +e
  output="$(run_gate "${fixture}")"
  exit_code=$?
  set -e
  if [ "${exit_code}" -eq 0 ]; then
    pass "${name}"
  else
    fail "${name} (expected pass, got exit ${exit_code}; output: ${output})"
  fi
}

assert_fail() {
  local name="$1"
  local fixture="$2"
  local expected_substr="$3"
  local output exit_code
  set +e
  output="$(run_gate "${fixture}")"
  exit_code=$?
  set -e
  if [ "${exit_code}" -eq 0 ]; then
    fail "${name} (expected fail, got pass; output: ${output})"
    return
  fi
  if printf '%s' "${output}" | grep -Fq "${expected_substr}"; then
    pass "${name}"
  else
    fail "${name} (failed correctly but expected substring '${expected_substr}' not found; output: ${output})"
  fi
}

# --- Case 1: pre-existence pass (no guarded crates) -------------------------

case1="$(new_fixture case1-pre-existence)"
mkdir -p "${case1}/crates/some-other-crate"
assert_pass "pre-existence: no guarded crates -> pass" "${case1}"

# --- Case 2: nimbus-guest exists but no LICENSE-MIT-muvm --------------------

case2="$(new_fixture case2-missing-license)"
mkdir -p "${case2}/crates/nimbus-guest/src"
cat > "${case2}/crates/nimbus-guest/THIRD_PARTY.md" <<'EOF'
| path | provenance |
| --- | --- |
EOF
assert_fail "nimbus-guest missing LICENSE-MIT-muvm fails" "${case2}" "missing LICENSE-MIT-muvm"

# --- Case 3: nimbus-guest exists with license but no THIRD_PARTY.md ---------

case3="$(new_fixture case3-missing-manifest)"
mkdir -p "${case3}/crates/nimbus-guest/src"
printf 'MIT License (muvm)\n' > "${case3}/crates/nimbus-guest/LICENSE-MIT-muvm"
assert_fail "nimbus-guest missing THIRD_PARTY.md fails" "${case3}" "missing THIRD_PARTY.md"

# --- Case 4: THIRD_PARTY.md lists a file that doesn't exist -----------------

case4="$(new_fixture case4-listed-missing)"
mkdir -p "${case4}/crates/nimbus-guest/src"
printf 'MIT License (muvm)\n' > "${case4}/crates/nimbus-guest/LICENSE-MIT-muvm"
cat > "${case4}/crates/nimbus-guest/THIRD_PARTY.md" <<'EOF'
| path | provenance |
| --- | --- |
| `src/ghost.rs` | Lifted from AsahiLinux/muvm@abc1234 |
EOF
assert_fail "listed-but-missing file fails" "${case4}" "but file is missing"

# --- Case 5: listed file exists but lacks provenance header -----------------

case5="$(new_fixture case5-missing-header)"
mkdir -p "${case5}/crates/nimbus-guest/src"
printf 'MIT License (muvm)\n' > "${case5}/crates/nimbus-guest/LICENSE-MIT-muvm"
cat > "${case5}/crates/nimbus-guest/THIRD_PARTY.md" <<'EOF'
| path | provenance |
| --- | --- |
| `src/lifted.rs` | Lifted from AsahiLinux/muvm@abc1234 |
EOF
cat > "${case5}/crates/nimbus-guest/src/lifted.rs" <<'EOF'
// Some Rust file without provenance.
fn lifted() {}
EOF
assert_fail "listed file without provenance header fails" "${case5}" "missing 'Lifted from"

# --- Case 6: full happy path ------------------------------------------------

case6="$(new_fixture case6-happy-path)"
mkdir -p "${case6}/crates/nimbus-guest/src"
printf 'MIT License (muvm)\n' > "${case6}/crates/nimbus-guest/LICENSE-MIT-muvm"
cat > "${case6}/crates/nimbus-guest/THIRD_PARTY.md" <<'EOF'
| path | provenance |
| --- | --- |
| `src/lifted.rs` | Lifted from AsahiLinux/muvm@abc1234 |
| `src/adapted.rs` | Adapted from AsahiLinux/muvm@def5678 |
EOF
cat > "${case6}/crates/nimbus-guest/src/lifted.rs" <<'EOF'
// Lifted from AsahiLinux/muvm@abc1234 — guest PID-1 entry point, stripped
// of Asahi defaults.
fn lifted() {}
EOF
cat > "${case6}/crates/nimbus-guest/src/adapted.rs" <<'EOF'
// Adapted from AsahiLinux/muvm@def5678 — reworked for Nimbus vsock control
// protocol.
fn adapted() {}
EOF
assert_pass "happy path: all checks green" "${case6}"

# --- Case 7: nimbus-libkrun-snapshot crate also enforced --------------------

case7="$(new_fixture case7-libkrun-snapshot)"
mkdir -p "${case7}/crates/nimbus-libkrun-snapshot/src"
assert_fail "nimbus-libkrun-snapshot missing LICENSE-APACHE-firecracker fails" "${case7}" "missing LICENSE-APACHE-firecracker"

# --- Case 8: nimbus-libkrun-* glob matches arbitrary suffixes ---------------

case8="$(new_fixture case8-libkrun-glob)"
mkdir -p "${case8}/crates/nimbus-libkrun-misc/src"
assert_fail "nimbus-libkrun-misc fails on missing manifest" "${case8}" "missing THIRD_PARTY.md"

# --- Case 9: vendored patch missing from root NOTICE -----------------------

case9="$(new_fixture case9-vendored-patch-missing-notice)"
mkdir -p "${case9}/third_party/object_store-0.14.0"
printf 'Apache-2.0\n' > "${case9}/third_party/object_store-0.14.0/LICENSE.txt"
cat > "${case9}/Cargo.toml" <<'EOF'
[patch.crates-io]
object_store = { path = "third_party/object_store-0.14.0" }
EOF
printf 'Nimbus\n' > "${case9}/NOTICE"
assert_fail "vendored patch missing from root NOTICE fails" "${case9}" "root NOTICE does not name vendored patch"

# --- Case 10: vendored patches with retained legal text pass ---------------

case10="$(new_fixture case10-vendored-patch-happy-path)"
mkdir -p "${case10}/third_party/object_store-0.14.0" \
  "${case10}/third_party/brotli-3.5.0"
printf 'Apache-2.0\n' > "${case10}/third_party/object_store-0.14.0/LICENSE.txt"
printf 'BSD-3-Clause\n' > "${case10}/third_party/brotli-3.5.0/LICENSE"
cat > "${case10}/Cargo.toml" <<'EOF'
[patch.crates-io]
object_store = { path = "third_party/object_store-0.14.0" }
brotli = { path = "third_party/brotli-3.5.0" }
EOF
cat > "${case10}/NOTICE" <<'EOF'
object_store
Apache Arrow Object Store
The Apache Software Foundation (http://www.apache.org/).
brotli
Copyright (c) 2016 Dropbox, Inc.
Neither the name of the copyright holder nor the names of its contributors
EOF
assert_pass "vendored patches with retained legal text pass" "${case10}"

# --- Syntax check -----------------------------------------------------------

if bash -n "${script_under_test}" 2>/dev/null; then
  pass "verify-third-party-attribution.sh bash syntax"
else
  fail "verify-third-party-attribution.sh bash syntax"
fi

# --- Summary ----------------------------------------------------------------

printf '\n'
printf 'tests run: %d\n' "${test_count}"
if [ "${fail_count}" -gt 0 ]; then
  printf 'verify-third-party-attribution-helper.sh: FAIL (%d/%d)\n' \
    "${fail_count}" "${test_count}" >&2
  exit 1
fi
printf 'verify-third-party-attribution-helper.sh: pass (%d/%d)\n' \
  "${test_count}" "${test_count}"
