#!/usr/bin/env bash
# Verifies the Nimbus egress gateway extraction control plane.

set -u
set -o pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

passed=0
failed=0
failures=()

pass() {
  printf '[PASS] %s\n' "$1"
  passed=$((passed + 1))
}

fail() {
  printf '[FAIL] %s\n' "$1"
  failures+=("$1")
  failed=$((failed + 1))
}

contains() {
  local file="$1"
  local pattern="$2"
  test -f "${file}" && grep -Eq "${pattern}" "${file}"
}

rg_contains() {
  local pattern="$1"
  shift
  rg -q "${pattern}" "$@"
}

plan_file() {
  if [ -f docs/private/plans/nimbus-egress-gateway-extraction-plan.md ]; then
    printf '%s\n' docs/private/plans/nimbus-egress-gateway-extraction-plan.md
    return 0
  fi
  if [ -f docs/private/plans/archive/nimbus-egress-gateway-extraction-plan.md ]; then
    printf '%s\n' docs/private/plans/archive/nimbus-egress-gateway-extraction-plan.md
    return 0
  fi
  return 1
}

printf 'Nimbus egress gateway extraction verifier\n'
printf 'Repo: %s\n\n' "${REPO_ROOT}"

if PLAN_PATH="$(plan_file)"; then
  pass "plan file exists at ${PLAN_PATH}"
else
  PLAN_PATH=""
  fail "plan file exists at active or archived path"
fi

if contains AGENTS.md 'nimbus-egress-gateway-extraction-plan\.md' \
  && contains CLAUDE.md 'nimbus-egress-gateway-extraction-plan\.md' \
  && contains docs/private/plans/README.md 'nimbus-egress-gateway-extraction-plan\.md' \
  && contains docs/private/plans/README.md 'nimbus-egress' \
  && contains docs/private/plans/README.md 'nimbus-proxy'; then
  pass "routing entries name the NEG plan and PDP/PEP split"
else
  fail "routing entries name the NEG plan in AGENTS.md, CLAUDE.md, and docs/private/plans/README.md"
fi

NEG0_PROOF=docs/private/plans/proof/nimbus-egress-gateway-extraction/neg0-baseline.md
if contains "${NEG0_PROOF}" 'coarse.*allow_net' \
  && contains "${NEG0_PROOF}" 'PDP/PEP' \
  && contains "${NEG0_PROOF}" 'OpenShell' \
  && contains "${NEG0_PROOF}" 'OPA/Envoy' \
  && contains "${NEG0_PROOF}" 'Cilium/Kubernetes' \
  && contains "${NEG0_PROOF}" 'Deno/Supabase/Workerd' \
  && contains "${NEG0_PROOF}" 'E2B' \
  && contains "${NEG0_PROOF}" 'Firecracker' \
  && contains "${NEG0_PROOF}" 'ClawPatrol' \
  && contains "${NEG0_PROOF}" 'Agentgateway' \
  && contains "${NEG0_PROOF}" 'SPIFFE/SPIRE' \
  && contains "${NEG0_PROOF}" 'Linkerd' \
  && contains "${NEG0_PROOF}" 'Pingora' \
  && contains "${NEG0_PROOF}" 'Istio ztunnel' \
  && contains "${NEG0_PROOF}" 'DYNAMIC_DNS' \
  && contains "${NEG0_PROOF}" 'WHATWG/WPT' \
  && contains "${NEG0_PROOF}" 'OWASP SSRF' \
  && contains "${NEG0_PROOF}" 'NIST zero trust' \
  && contains "${NEG0_PROOF}" 'OpenTelemetry/OCSF' \
  && contains "${NEG0_PROOF}" 'HTTP/MASQUE RFCs' \
  && contains "${NEG0_PROOF}" 'workload identity provenance' \
  && contains "${NEG0_PROOF}" 'canonical URL/IP parsing' \
  && contains "${NEG0_PROOF}" 'policy distribution/readiness/invalid updates' \
  && contains "${NEG0_PROOF}" 'bounded dynamic DNS/FQDN state' \
  && contains "${NEG0_PROOF}" 'DNS alias-chain handling' \
  && contains "${NEG0_PROOF}" 'request-phase ordering' \
  && contains "${NEG0_PROOF}" 'connection reuse and coalescing' \
  && contains "${NEG0_PROOF}" 'bounded DLP' \
  && contains "${NEG0_PROOF}" 'credential-header ownership' \
  && contains "${NEG0_PROOF}" 'telemetry redaction' \
  && contains "${NEG0_PROOF}" 'direct dynamic-DNS fast-path assumptions' \
  && contains "${NEG0_PROOF}" 'protocol-compliance negative tests' \
  && contains "${NEG0_PROOF}" 'Band success criteria' \
  && contains "${NEG0_PROOF}" 'Recovery loop' \
  && contains "${NEG0_PROOF}" '/goal'; then
  pass "NEG0 baseline proof records starting state, exemplars, residual risks, and control rules"
else
  fail "NEG0 baseline proof records required starting state, exemplars, residual risks, and control rules"
fi

if [ -d crates/nimbus-egress ] \
  && contains crates/nimbus-egress/Cargo.toml '^nimbus-core[[:space:]]*=' \
  && ! rg -q 'nimbus-runtime|nimbus-proxy|nimbus-sandbox|deno_|tokio|hyper|rustls|reqwest|hickory' crates/nimbus-egress/Cargo.toml \
  && rg_contains '\b(pub struct|pub enum) EgressPolicy\b|\bpub struct CompiledEgressPolicy\b|\bpub struct EgressRule\b|\bpub enum EgressEnforcementMode\b' crates/nimbus-egress/src \
  && ! rg -q '\b(CompiledSandboxEgressPolicy|SandboxEgressPolicy|SandboxEgressRule|SandboxEgressRequest|SandboxEgressAuthorization|SandboxEgressEnforcement)' crates/nimbus-sandbox crates/nimbus-tenant crates/nimbus-services crates/nimbus-bin crates/nimbus; then
  pass "NEG1 PDP crate exists, is pure, and SandboxEgress type names are gone from production consumers"
else
  fail "NEG1 PDP crate exists, is pure, and SandboxEgress type names are gone from production consumers"
fi

if [ -d crates/nimbus-proxy ] \
  && contains crates/nimbus-proxy/Cargo.toml '^nimbus-egress[[:space:]]*=' \
  && contains crates/nimbus-proxy/Cargo.toml '^nimbus-core[[:space:]]*=' \
  && ! rg -q '^nimbus-proxy[[:space:]]*=' crates/nimbus-egress/Cargo.toml 2>/dev/null \
  && rg_contains '\bEgressProxy\b' crates/nimbus-proxy/src \
  && ! rg -q '\bSandboxEgressProxy\b' crates scripts \
  && rg_contains 'policy_generation|PolicyGeneration' crates/nimbus-proxy/src \
  && rg_contains 'last_known_good|LastKnownGood' crates/nimbus-proxy/src \
  && rg_contains 'Dns|DNS|alias_chain|AliasChain' crates/nimbus-proxy/src \
  && rg_contains 'pool.*key|PoolKey' crates/nimbus-proxy/src \
  && rg_contains 'canonical|Canonical' crates/nimbus-proxy/src; then
  pass "NEG2 PEP crate owns EgressProxy, readiness/reload, DNS state, canonicalization, and pool identity"
else
  fail "NEG2 PEP crate owns EgressProxy, readiness/reload, DNS state, canonicalization, and pool identity"
fi

if contains crates/nimbus-runtime/src/egress.rs '\btrait EgressGateway\b' \
  && contains crates/nimbus-runtime/src/egress.rs '\bstruct EgressRequest\b' \
  && contains crates/nimbus-runtime/src/egress.rs '\bstruct EgressAuthorization\b' \
  && ! rg -q 'nimbus-(egress|proxy|sandbox|core)[[:space:]]*=' crates/nimbus-runtime/Cargo.toml \
  && ! rg -q 'deno_fetch::deno_fetch::init\(Default::default\(\)\)' crates/nimbus-runtime/src/runtime/bootstrap/extensions.rs \
  && rg_contains 'egress.*fetch|fetch.*egress|EgressGateway' crates/nimbus-runtime/src tests crates/nimbus-runtime/tests; then
  pass "NEG3 runtime EgressGateway seam and isolate fetch binding exist and preserve zero workspace deps"
else
  fail "NEG3 runtime EgressGateway seam and isolate fetch binding exist and preserve zero workspace deps"
fi

if rg_contains 'impl[^{]+EgressGateway' crates/nimbus-server/src crates/nimbus-server/tests 2>/dev/null \
  && rg_contains 'egress_gateway|cross.*substrate|substrate.*parity' crates/nimbus-server/src crates/nimbus-server/tests 2>/dev/null \
  && rg_contains 'no_policy|default.*deny|ready|readiness' crates/nimbus-server/src crates/nimbus-server/tests 2>/dev/null; then
  pass "NEG4 server gateway impl and cross-substrate parity tests exist"
else
  fail "NEG4 server gateway impl and cross-substrate parity tests exist"
fi

if rg_contains 'Credential|credential' crates/nimbus-egress/src crates/nimbus-proxy/src 2>/dev/null \
  && rg_contains 'Dlp|DLP|dlp' crates/nimbus-egress/src crates/nimbus-proxy/src 2>/dev/null \
  && rg_contains 'strip|redact|redirect|truncat|Authorization|Cookie' crates/nimbus-proxy/src 2>/dev/null \
  && ! rg -q 'SecretValue|SecretStore|plaintext|secret_material|ResolvedSecret' crates/nimbus-egress/src 2>/dev/null \
  && rg_contains 'query.*redact|redact.*query|bearer|cookie|userinfo' crates/nimbus-proxy/src crates/nimbus-proxy/tests 2>/dev/null; then
  pass "NEG5 credential injection, DLP enforcement, fail-closed truncation, and redaction gates exist"
else
  fail "NEG5 credential injection, DLP enforcement, fail-closed truncation, and redaction gates exist"
fi

if rg_contains 'wasm.*EgressGateway|EgressGateway.*wasm|wasi.*http|http-client' crates docs/private/operating 2>/dev/null \
  && rg_contains 'three.*substrate|substrate.*consistency|wasm.*deny|deny.*wasm' crates/nimbus-runtime crates/nimbus-server crates/nimbus-proxy tests 2>/dev/null; then
  pass "NEG6 wasm EgressGateway binding seam and three-substrate consistency tests exist"
else
  fail "NEG6 wasm EgressGateway binding seam and three-substrate consistency tests exist"
fi

if [ -n "${PLAN_PATH}" ] \
  && [ -f docs/private/operating/nimbus-egress-gateway.md ] \
  && contains docs/private/operating/nimbus-egress-gateway.md 'three substrates, one decision' \
  && contains docs/private/architecture/runtime/adapter-boundary.md 'nimbus-egress' \
  && contains docs/private/architecture/server/auth-runtime-trust.md 'nimbus-proxy' \
  && [ "$(grep -Ec '\| NEG[0-7] \|.*\| done \|' "${PLAN_PATH}")" -eq 8 ] \
  && [ -f docs/private/plans/proof/nimbus-egress-gateway-extraction/neg7-closeout.md ] \
  && contains docs/private/plans/proof/nimbus-egress-gateway-extraction/neg7-closeout.md 'branch CI green'; then
  pass "NEG7 closeout docs, done ledger, final proof, and CI evidence exist"
else
  fail "NEG7 closeout docs, done ledger, final proof, and CI evidence exist"
fi

printf '\nSummary: %d passed, %d failed\n' "${passed}" "${failed}"

if [ "${failed}" -ne 0 ]; then
  printf '\nFailed conditions:\n'
  for failure in "${failures[@]}"; do
    printf -- '- %s\n' "${failure}"
  done
  exit 1
fi
