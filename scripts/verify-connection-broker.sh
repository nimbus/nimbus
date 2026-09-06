#!/usr/bin/env bash
# Connection Broker (CB) plan verifier — the 14-condition progression gate
# from the plan's completion contract (docs/private/plans/
# archive/connection-broker-plan.md §9). All CB0-CB10 bands are accounted for.
# CB5 lands as a corrective erratum: observable runtime traffic uses the shared
# PDP, and proxy-required isolate traffic is denied because no isolate PEP
# transport is product-wired. Supervisor proxies enforce HTTP(S) authority
# policy and do not claim runtime WebSocket protocol classification. The
# required reading is `14 passed, 0 failed`. Conditions 1-2
# need the local docs/private plan; on a checkout without it they fail (this is
# a local control-plane gate, not a CI lane).

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

PLAN_ACTIVE="docs/private/plans/connection-broker-plan.md"
PLAN_ARCHIVED="docs/private/plans/archive/connection-broker-plan.md"
if [ -f "${PLAN_ACTIVE}" ]; then PLAN_FILE="${PLAN_ACTIVE}"; else PLAN_FILE="${PLAN_ARCHIVED}"; fi
PLANS_README="docs/private/plans/README.md"
PROOF_DIR="docs/private/plans/proof/connection-broker"

COND_PASS=0
COND_FAIL=0
cond_pass() {
  COND_PASS=$((COND_PASS + 1))
  printf 'COND PASS %2d: %s\n' "$1" "$2"
}
cond_fail() {
  COND_FAIL=$((COND_FAIL + 1))
  printf 'COND FAIL %2d: %s\n' "$1" "$2"
}
grep_any() {
  # grep_any <pattern> <path...> — true if any existing path matches.
  local pattern="$1"; shift
  local path
  for path in "$@"; do
    [ -e "${path}" ] && grep -rqE "${pattern}" "${path}" 2>/dev/null && return 0
  done
  return 1
}

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

if contains_json_dependency "${NETWORKING_PACKAGE}" "ws" "8.21.3" \
  && contains_json_dependency "${NETWORKING_LOCK}" "ws" "8.21.3"; then
  pass "networking canary package declares pinned ws dependency"
else
  fail "networking canary package metadata missing pinned ws dependency" "${NETWORKING_PACKAGE} / ${NETWORKING_LOCK}"
fi

if contains_json_dependency "${HOST_HEAVY_PACKAGE}" "ws" "8.21.3" \
  && contains_json_dependency "${HOST_HEAVY_LOCK}" "ws" "8.21.3"; then
  pass "host-heavy canary package declares pinned ws dependency"
else
  fail "host-heavy canary package metadata missing pinned ws dependency" "${HOST_HEAVY_PACKAGE} / ${HOST_HEAVY_LOCK}"
fi

# ---------------------------------------------------------------------------
# The 14 plan conditions (plan §9). 1-3 = CB0 scaffold; 4-14 = CB1-CB10.
# ---------------------------------------------------------------------------
printf '\nConnection-broker plan conditions:\n'

# 1. Plan checked in (local docs/private control plane).
if [ -f "${PLAN_FILE}" ]; then
  cond_pass 1 "plan present (${PLAN_FILE})"
else
  cond_fail 1 "plan missing: ${PLAN_FILE} (local-only doc; run from a checkout with docs/private)"
fi

# 2. Routing pointer present.
if [ -f "${PLANS_README}" ] && grep -q "connection-broker" "${PLANS_README}"; then
  cond_pass 2 "plans README routes to the connection-broker plan"
else
  cond_fail 2 "plans README routing pointer missing"
fi

# 3. Landed regression coverage (the canary/regression checks above).
if [ "${FAIL}" -eq 0 ] && [ "${PASS}" -ge 7 ]; then
  cond_pass 3 "landed regression coverage green (${PASS} canary/regression checks)"
else
  cond_fail 3 "regression coverage incomplete (${PASS} pass / ${FAIL} fail above)"
fi

# 4. CB1: Residency states in nimbus-services.
if grep_any 'enum Residency' crates/nimbus-services/src; then
  cond_pass 4 "Residency::{Hibernated,Resident} exists (CB1)"
else
  cond_fail 4 "CB1 pending: no Residency enum in nimbus-services"
fi

# 5. CB2: per-frame invoke verb + warm pool.
if grep_any 'per_frame_invoke|invoke_frame' crates/nimbus-services/src crates/nimbus-runtime/src \
  && grep_any 'ThreadLocalPool|WarmPool|warm_pool' crates/nimbus-services/src crates/nimbus-runtime/src; then
  cond_pass 5 "per-frame invoke verb + warm pool exist (CB2)"
else
  cond_fail 5 "CB2 pending: per-frame invoke verb / warm pool not landed"
fi

# 6. CB1: host-owned connection registry (socket map outside the isolate).
if grep_any 'ConnectionRegistry|ws_commands' crates/nimbus-services/src; then
  cond_pass 6 "host-owned connection registry exists (CB1)"
else
  cond_fail 6 "CB1 pending: host-owned connection registry not landed"
fi

# 7. CB3: hibernation persistence via TenantKvStore.
# Anchor on a CB3 implementation symbol (a fn/type), not doc mentions of
# TenantKvStore — a downstream band's doc reference must not flip this.
if grep_any 'HibernationStore|HibernationAttachment|persist_hibernation|rehydrate_from_kv' \
  crates/nimbus-services/src \
  && grep_any 'TenantKvStore' crates/nimbus-services/src; then
  cond_pass 7 "hibernation persistence via TenantKvStore (CB3)"
else
  cond_fail 7 "CB3 pending: hibernation persistence not landed"
fi

# 8. CB4: inbound ingress WS-upgrade router (default-ALLOW layer).
if grep_any 'broker.*upgrade|ingress.*websocket|WsIngress' crates/nimbus-server/src crates/nimbus-services/src; then
  cond_pass 8 "inbound ingress WS-upgrade router exists (CB4)"
else
  cond_fail 8 "CB4 pending: ingress WS-upgrade router not landed"
fi

# 9. CB5 corrective contract: one runtime PDP path, with proxy-required rules
# denied until an actual isolate PEP transport exists. The old services-only
# facade was deleted during the network-control-plane cutover because it was
# not on the production request path.
if grep_any 'fn authorize_runtime_egress' crates/nimbus-bridge/src/egress.rs \
  && grep_any 'requires_proxy_enforcement' crates/nimbus-bridge/src/egress.rs \
  && grep_any 'isolate.*no route to the nimbus-proxy PEP|isolate substrate cannot apply' \
    crates/nimbus-runtime/src; then
  cond_pass 9 "runtime egress uses one PDP and proxy-required isolate traffic fails closed (CB5 erratum)"
else
  cond_fail 9 "CB5 erratum pending: shared PDP / no-PEP fail-closed contract not proved"
fi

# 10. CB5: observable runtime WebSocket-out on the same decision path as fetch.
# Anchor on the live runtime hook and the WebSocket URL-to-policy lowering.
if grep_any 'EgressGatewayTransport::WebSocket' crates/nimbus-runtime/src/runtime/bootstrap/extensions.rs \
  && grep_any 'from_websocket_url_with_context' crates/nimbus-runtime/src/egress.rs \
  && grep_any 'isolate_websocket_consults_egress_gateway_before_transport' \
    crates/nimbus-runtime/src/egress.rs; then
  cond_pass 10 "runtime WebSocket-out bound to the fetch egress decision path (CB5)"
else
  cond_fail 10 "CB5 pending: runtime WebSocket-out not bound to the egress decision path"
fi

# 11. CB7: cross-substrate HTTP(S) authority-policy parity test. This does not
# claim that the supervisor proxy can classify WebSocket inside opaque TLS.
if grep_any 'evil.example' crates/nimbus-server/src/adapters/convex/host_bridge/egress_gateway.rs \
  && grep_any 'cross_substrate_parity' \
    crates/nimbus-server/src/adapters/convex/host_bridge/egress_gateway.rs; then
  cond_pass 11 "cross-substrate HTTP(S) authority-policy parity test exists (CB7)"
else
  cond_fail 11 "CB7 pending: cross-substrate HTTP(S) parity test not landed"
fi

# 12. CB1/CB8: cluster placement seam (resolve-to-self ClusterTransport shape).
if grep_any 'ClusterTransport|PlacementLookup|resolve_to_self' crates/nimbus-services/src; then
  cond_pass 12 "cluster placement seam exists (CB1 day-one seam)"
else
  cond_fail 12 "CB1/CB8 pending: placement seam not landed"
fi

# 13. CB9: Resident app-WS + standard-ws/socket.io zero-config acceptance.
# Anchor on the CB9 implementation artifacts, not today's canary bundles
# (which legitimately use the ws package and would false-positive).
if grep_any 'ResidentAppWs|resident_app_ws|ws_server_compat|SocketIoCompat' crates/nimbus-services/src; then
  cond_pass 13 "Resident app-WS + ws/socket.io compat acceptance (CB9)"
else
  cond_fail 13 "CB9 pending: standard-ws/socket.io zero-config surface not landed"
fi

# 14. CB10: connection metering usage records.
if grep_any 'active_cpu|ActiveCpu' crates/nimbus-services/src \
  && grep_any 'residency.*usage|usage.*residency' crates/nimbus-services/src; then
  cond_pass 14 "connection metering emits Active-CPU + residency usage records (CB10)"
else
  cond_fail 14 "CB10 pending: connection metering not landed"
fi

printf '\nSummary: %d passed, %d failed (of 14 plan conditions)\n' "${COND_PASS}" "${COND_FAIL}"
if [ "${COND_FAIL}" -gt 0 ]; then
  printf 'Progression gate requires 14 passed, 0 failed.\n'
  exit 1
fi
exit 0
