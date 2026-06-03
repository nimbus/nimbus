#!/usr/bin/env bash
# Verifies Nimbus' Deno-family fork closure for the runtime crate.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

ALLOWLIST="docs/architecture/runtime/deno-fork-provenance-allowlist.tsv"
TREE_OUT="/tmp/nimbus-deno-fork-runtime-tree.out"
LOCK_TABLE="/tmp/nimbus-deno-fork-lock-table.tsv"
RUNTIME_CRATES="/tmp/nimbus-deno-fork-runtime-crates.txt"

EXPECTED_DENO_REPO="https://github.com/nimbus/deno"
EXPECTED_DENO_TAG="v2.8.2-nimbus.1"
EXPECTED_DENO_SHA="d7befe8b02b4334100e879ed5b8d46b1a9406313"
EXPECTED_DENO_SOURCE="git+${EXPECTED_DENO_REPO}?tag=${EXPECTED_DENO_TAG}#${EXPECTED_DENO_SHA}"

EXPECTED_V8_REPO="https://github.com/nimbus/rusty_v8"
EXPECTED_V8_TAG="v149.2.0-nimbus.1"
EXPECTED_V8_SHA="ce6663111a3ff8fde06bc04ba19bbbced60dbc8d"
EXPECTED_V8_SOURCE="git+${EXPECTED_V8_REPO}?tag=${EXPECTED_V8_TAG}#${EXPECTED_V8_SHA}"

PATCHED_DENO_CRATES="
deno_core
deno_crypto
deno_crypto_provider
deno_dotenv
deno_features
deno_fetch
deno_fs
deno_http
deno_inspector_server
deno_io
deno_maybe_sync
deno_napi
deno_net
deno_node
deno_node_crypto
deno_node_sqlite
deno_ops
deno_os
deno_package_json
deno_permissions
deno_process
deno_resolver
deno_signals
deno_subprocess_windows
deno_telemetry
deno_tls
deno_web
deno_webidl
deno_websocket
node_resolver
node_shim
serde_v8
"

PASS=0
FAIL=0
FAIL_DETAIL=()

pass() {
  PASS=$((PASS + 1))
  printf '  \033[32mPASS\033[0m  %s\n' "$1"
}

fail() {
  FAIL=$((FAIL + 1))
  printf '  \033[31mFAIL\033[0m  %s\n' "$1"
  if [ $# -ge 2 ]; then
    printf '        %s\n' "$2"
    FAIL_DETAIL+=("$1 - $2")
  else
    FAIL_DETAIL+=("$1")
  fi
}

step() {
  printf '\n\033[1m[%s]\033[0m %s\n' "$1" "$2"
}

lock_package_table() {
  awk '
    function emit() {
      if (name != "") {
        print name "\t" version "\t" source
      }
    }
    $0 == "[[package]]" {
      emit()
      name = ""
      version = ""
      source = ""
      next
    }
    /^name = / {
      name = $3
      gsub(/"/, "", name)
      next
    }
    /^version = / {
      version = $3
      gsub(/"/, "", version)
      next
    }
    /^source = / {
      source = $0
      sub(/^source = "/, "", source)
      sub(/"$/, "", source)
      next
    }
    END {
      emit()
    }
  ' Cargo.lock
}

lock_field() {
  local crate="$1"
  local field="$2"
  awk -F '\t' -v crate="${crate}" -v field="${field}" '
    $1 == crate {
      if (field == "version") {
        print $2
      } else if (field == "source") {
        print $3
      }
      found = 1
      exit
    }
    END {
      if (!found) {
        exit 1
      }
    }
  ' "${LOCK_TABLE}"
}

allowlist_reason() {
  local crate="$1"
  awk -F '\t' -v crate="${crate}" '
    $0 ~ /^#/ || $0 == "" { next }
    $1 == crate && $2 == "crates.io" && length($3) > 20 {
      print $3
      found = 1
      exit
    }
    END {
      if (!found) {
        exit 1
      }
    }
  ' "${ALLOWLIST}"
}

workspace_patch_entry_matches() {
  local crate="$1"
  local repo="$2"
  local tag="$3"
  awk -v crate="${crate}" -v repo="${repo}" -v tag="${tag}" '
    $0 ~ "^" crate " = " && index($0, "git = \"" repo "\"") && index($0, "tag = \"" tag "\"") {
      found = 1
    }
    END {
      exit(found ? 0 : 1)
    }
  ' Cargo.toml
}

runtime_deno_family_crates() {
  awk '
    /^(deno_|denort_helper|napi_sym|node_|serde_v8|sys_traits|urlpattern|v8 )/ {
      print $1
    }
  ' "${TREE_OUT}" | sort -u
}

printf '\033[1mDeno fork provenance verifier\033[0m\n'
printf 'Repo: %s\n' "${REPO_ROOT}"
printf 'Expected nimbus/deno: %s#%s\n' "${EXPECTED_DENO_TAG}" "${EXPECTED_DENO_SHA}"
printf 'Expected nimbus/rusty_v8: %s#%s\n' "${EXPECTED_V8_TAG}" "${EXPECTED_V8_SHA}"

cargo tree -p nimbus-runtime --prefix none --charset ascii >"${TREE_OUT}" 2>/tmp/nimbus-deno-fork-cargo-tree.err
TREE_STATUS=$?
lock_package_table >"${LOCK_TABLE}"
runtime_deno_family_crates >"${RUNTIME_CRATES}"

step 1 "Cargo tree for nimbus-runtime is available"
if [ "${TREE_STATUS}" -eq 0 ] && [ -s "${TREE_OUT}" ] && grep -q '^deno_core ' "${TREE_OUT}"; then
  pass "cargo tree -p nimbus-runtime completed and includes Deno runtime crates"
else
  fail "cargo tree unavailable" "Expected cargo tree -p nimbus-runtime to succeed and include deno_core"
fi

step 2 "Workspace patch table pins the expected fork tags"
PATCH_MISSING=0
for crate in ${PATCHED_DENO_CRATES}; do
  if ! workspace_patch_entry_matches "${crate}" "${EXPECTED_DENO_REPO}" "${EXPECTED_DENO_TAG}"; then
    PATCH_MISSING=1
    printf '        missing/incorrect Deno patch entry: %s\n' "${crate}"
  fi
done
if ! workspace_patch_entry_matches "v8" "${EXPECTED_V8_REPO}" "${EXPECTED_V8_TAG}"; then
  PATCH_MISSING=1
  printf '        missing/incorrect rusty_v8 patch entry: v8\n'
fi
if [ "${PATCH_MISSING}" -eq 0 ]; then
  pass "Cargo.toml patch entries match expected nimbus/deno and nimbus/rusty_v8 tags"
else
  fail "Patch table mismatch" "Expected all patch-sensitive crates to use the pinned Nimbus fork tags"
fi

step 3 "Patch-sensitive lockfile sources match expected tags and SHAs"
LOCK_MISMATCH=0
for crate in ${PATCHED_DENO_CRATES}; do
  source="$(lock_field "${crate}" source || true)"
  if [ "${source}" != "${EXPECTED_DENO_SOURCE}" ]; then
    LOCK_MISMATCH=1
    printf '        %s source mismatch: %s\n' "${crate}" "${source:-missing}"
  fi
done
v8_source="$(lock_field "v8" source || true)"
if [ "${v8_source}" != "${EXPECTED_V8_SOURCE}" ]; then
  LOCK_MISMATCH=1
  printf '        v8 source mismatch: %s\n' "${v8_source:-missing}"
fi
if [ "${LOCK_MISMATCH}" -eq 0 ]; then
  pass "Cargo.lock resolves patch-sensitive crates to expected fork revisions"
else
  fail "Lockfile source mismatch" "Expected nimbus/deno ${EXPECTED_DENO_SHA} and nimbus/rusty_v8 ${EXPECTED_V8_SHA}"
fi

step 4 "Runtime Deno-family crates are forked or allowlisted"
UNKNOWN=0
FORKED=0
ALLOWLISTED=0
while IFS= read -r crate; do
  [ -z "${crate}" ] && continue
  version="$(lock_field "${crate}" version || true)"
  source="$(lock_field "${crate}" source || true)"
  if [ -z "${source}" ]; then
    UNKNOWN=1
    printf '        missing lockfile source for %s\n' "${crate}"
    continue
  fi

  if [ "${source}" = "${EXPECTED_DENO_SOURCE}" ]; then
    FORKED=$((FORKED + 1))
    printf '        forked     %-24s v%-10s %s\n' "${crate}" "${version}" "${source}"
  elif [ "${crate}" = "v8" ] && [ "${source}" = "${EXPECTED_V8_SOURCE}" ]; then
    FORKED=$((FORKED + 1))
    printf '        forked     %-24s v%-10s %s\n' "${crate}" "${version}" "${source}"
  elif printf '%s' "${source}" | grep -q '^registry+https://github.com/rust-lang/crates.io-index$' \
       && reason="$(allowlist_reason "${crate}")"; then
    ALLOWLISTED=$((ALLOWLISTED + 1))
    printf '        allowlist  %-24s v%-10s crates.io - %s\n' "${crate}" "${version}" "${reason}"
  else
    UNKNOWN=1
    printf '        unknown    %-24s v%-10s %s\n' "${crate}" "${version:-missing}" "${source}"
  fi
done <"${RUNTIME_CRATES}"

if [ "${UNKNOWN}" -eq 0 ]; then
  pass "All runtime Deno-family crates are on expected forks or allowlisted with reasons (${FORKED} forked, ${ALLOWLISTED} allowlisted)"
else
  fail "Unclassified Deno-family crate source" "Expected every runtime Deno-family crate to be forked or listed in ${ALLOWLIST}"
fi

step 5 "Verifier inputs are self-describing"
if [ -f "${ALLOWLIST}" ] \
   && grep -q '^# Crate' "${ALLOWLIST}" \
   && grep -q '^deno_ast[[:space:]]' "${ALLOWLIST}" \
   && grep -q '^sys_traits[[:space:]]' "${ALLOWLIST}" \
   && grep -q '^urlpattern[[:space:]]' "${ALLOWLIST}"; then
  pass "Allowlist exists with reasons for crates.io Deno-family exceptions"
else
  fail "Allowlist incomplete" "Expected ${ALLOWLIST} to include crates, sources, and reasons"
fi

printf '\n\033[1mSummary:\033[0m %s passed, %s failed\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -ne 0 ]; then
  printf '\nFailures:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${detail}"
  done
  exit 1
fi
