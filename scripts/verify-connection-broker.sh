#!/usr/bin/env bash
# Focused verifier for the Connection Broker / WebSocket egress regression
# scaffold. This gate stays green: it verifies the current PR's canaries and
# policy test, not future connection-broker plan bands.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

RUNTIME_TESTS="crates/nimbus-runtime/src/runtime_capabilities/tests.rs"
PACKAGE_WIRING="crates/nimbus-runtime/src/runtime/tests/basic_invocation/package_resolution.rs"
SUPPORT_WIRING="crates/nimbus-runtime/src/runtime/tests/basic_invocation/support.rs"
NETWORKING_CANARY="tests/runtime/node/networking-canaries/bundles/ws-echo.mjs"
HOST_HEAVY_CANARY="tests/runtime/node/host-heavy-canaries/bundles/ws-server-listen.mjs"
NETWORKING_PACKAGE="tests/runtime/node/networking-canaries/package.json"
NETWORKING_LOCK="tests/runtime/node/networking-canaries/package-lock.json"
HOST_HEAVY_PACKAGE="tests/runtime/node/host-heavy-canaries/package.json"
HOST_HEAVY_LOCK="tests/runtime/node/host-heavy-canaries/package-lock.json"

PASS=0
FAIL=0
FAILURES=()

pass() {
  PASS=$((PASS + 1))
  printf '  PASS  %s\n' "$1"
}

fail() {
  FAIL=$((FAIL + 1))
  printf '  FAIL  %s\n' "$1"
  if [ "$#" -gt 1 ]; then
    printf '        %s\n' "$2"
    FAILURES+=("$1: $2")
  else
    FAILURES+=("$1")
  fi
}

has_text() {
  local file="$1"
  local pattern="$2"
  [ -f "${file}" ] && grep -qE "${pattern}" "${file}"
}

contains_json_dependency() {
  local file="$1"
  local package="$2"
  local version="$3"
  has_text "${file}" "\"${package}\"[[:space:]]*:[[:space:]]*\"${version}\""
}

printf 'Connection Broker / WebSocket egress regression verifier\n'
printf 'Repo: %s\n\n' "${REPO_ROOT}"

if has_text "${RUNTIME_TESTS}" 'fn node_isolates_deny_public_host_egress_in_every_profile' \
  && has_text "${RUNTIME_TESTS}" 'echo\.example\.com' \
  && has_text "${RUNTIME_TESTS}" 'application_node22_local_development'; then
  pass "runtime profiles deny public-host WebSocket/fetch egress"
else
  fail "runtime public-host denial test missing" "${RUNTIME_TESTS}"
fi

if has_text "${PACKAGE_WIRING}" 'ws-echo\.mjs' \
  && has_text "${PACKAGE_WIRING}" 'ws-server-listen\.mjs'; then
  pass "WebSocket canaries are wired into runtime canary batches"
else
  fail "WebSocket canaries are not wired into package_resolution.rs" "${PACKAGE_WIRING}"
fi

if has_text "${SUPPORT_WIRING}" '"ws-echo\.mjs"' \
  && has_text "${SUPPORT_WIRING}" '"ws-server-listen\.mjs"' \
  && has_text "${SUPPORT_WIRING}" 'ws_server_listen' \
  && has_text "${SUPPORT_WIRING}" 'Requires net access'; then
  pass "expected payloads and deny assertions cover ws canaries"
else
  fail "support assertions missing ws canary expectations" "${SUPPORT_WIRING}"
fi

if has_text "${NETWORKING_CANARY}" 'WebSocketServer' \
  && has_text "${NETWORKING_CANARY}" 'new WebSocket' \
  && has_text "${NETWORKING_CANARY}" 'ws://127\.0\.0\.1' \
  && has_text "${NETWORKING_CANARY}" 'socket\.send' \
  && has_text "${NETWORKING_CANARY}" 'hello-ws'; then
  pass "networking ws-echo canary exercises loopback WebSocket round trip"
else
  fail "networking ws-echo canary incomplete" "${NETWORKING_CANARY}"
fi

if has_text "${HOST_HEAVY_CANARY}" 'WebSocketServer' \
  && has_text "${HOST_HEAVY_CANARY}" '127\.0\.0\.1' \
  && has_text "${HOST_HEAVY_CANARY}" 'port: 0' \
  && has_text "${HOST_HEAVY_CANARY}" 'NIMBUS_NODE_HOST_HEAVY_SERVICE_ROUTE_REQUIRED'; then
  pass "host-heavy ws-server-listen canary pins production listen denial"
else
  fail "host-heavy ws-server-listen canary incomplete" "${HOST_HEAVY_CANARY}"
fi

if contains_json_dependency "${NETWORKING_PACKAGE}" "ws" "8.17.1" \
  && contains_json_dependency "${NETWORKING_LOCK}" "ws" "8.17.1"; then
  pass "networking canary package declares pinned ws dependency"
else
  fail "networking canary package metadata missing pinned ws dependency" "${NETWORKING_PACKAGE} / ${NETWORKING_LOCK}"
fi

if contains_json_dependency "${HOST_HEAVY_PACKAGE}" "ws" "8.17.1" \
  && contains_json_dependency "${HOST_HEAVY_LOCK}" "ws" "8.17.1"; then
  pass "host-heavy canary package declares pinned ws dependency"
else
  fail "host-heavy canary package metadata missing pinned ws dependency" "${HOST_HEAVY_PACKAGE} / ${HOST_HEAVY_LOCK}"
fi

printf '\n%d passed, %d failed\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -gt 0 ]; then
  printf '\nFailures:\n'
  for failure in "${FAILURES[@]}"; do
    printf '  - %s\n' "${failure}"
  done
  exit 1
fi
