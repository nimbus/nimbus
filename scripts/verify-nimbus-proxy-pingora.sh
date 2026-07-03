#!/usr/bin/env bash
# Verifies the Nimbus proxy Pingora (K11P) control plane.

set -u
set -o pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}"

PLAN="docs/private/plans/nimbus-proxy-pingora-plan.md"
PROMPT="docs/private/plans/prompts/nimbus-proxy-pingora-goal.md"
PROOF_DIR="docs/private/plans/proof/nimbus-proxy-pingora"

passed=0
failed=0
failures=()

pass() {
  printf 'PASS %s: %s\n' "$1" "$2"
  passed=$((passed + 1))
}

fail() {
  printf 'FAIL %s: %s\n' "$1" "$2"
  failures+=("$1: $2")
  failed=$((failed + 1))
}

contains() {
  local path="$1"
  local pattern="$2"
  test -e "${path}" && grep -Eq "${pattern}" "${path}"
}

rejects() {
  local path="$1"
  local pattern="$2"
  ! grep -Eq "${pattern}" "${path}" 2>/dev/null
}

row_status() {
  local row="$1"
  awk -F'|' -v row="${row}" '
    $2 ~ (" " row " ") {
      gsub(/^[ \t]+|[ \t]+$/, "", $3);
      print $3;
      exit;
    }
  ' "${PLAN}" 2>/dev/null
}

require_file() {
  local row="$1"
  local path="$2"
  local message="$3"
  if [ -f "${path}" ]; then
    pass "${row}" "${message}"
  else
    fail "${row}" "missing ${path}: ${message}"
  fi
}

require_dir() {
  local row="$1"
  local path="$2"
  local message="$3"
  if [ -d "${path}" ]; then
    pass "${row}" "${message}"
  else
    fail "${row}" "missing ${path}: ${message}"
  fi
}

require_done() {
  local row="$1"
  local status
  status="$(row_status "${row}")"
  if [ "${status}" = "done" ]; then
    pass "${row}" "${row} ledger row is done"
  else
    fail "${row}" "${row} ledger row is ${status:-missing}, expected done"
    return 1
  fi
}

require_grep() {
  local row="$1"
  local pattern="$2"
  local path="$3"
  local message="$4"
  if contains "${path}" "${pattern}"; then
    pass "${row}" "${message}"
  else
    fail "${row}" "${message} (${path} lacks pattern: ${pattern})"
  fi
}

require_reject() {
  local row="$1"
  local pattern="$2"
  local path="$3"
  local message="$4"
  if rejects "${path}" "${pattern}"; then
    pass "${row}" "${message}"
  else
    fail "${row}" "${message} (${path} matches forbidden pattern: ${pattern})"
  fi
}

check_prompt_sync() {
  if [ ! -f "${PLAN}" ] || [ ! -f "${PROMPT}" ]; then
    fail K11P0 "plan and companion prompt both exist"
    return
  fi
  if diff -u \
    <(awk '/^```text$/{flag=1;next}/^```$/{flag=0}flag' "${PLAN}") \
    <(awk '/^```text$/{flag=1;next}/^```$/{flag=0}flag' "${PROMPT}") \
    >/dev/null; then
    pass K11P0 "embedded /goal prompt matches companion prompt"
  else
    fail K11P0 "embedded /goal prompt differs from companion prompt"
  fi
}

check_self_names_rows() {
  for row in K11P0 K11P1 K11P2 K11P3 K11P4 K11P5 K11P6 K11P7 K11P8 K11P9 K11P10 K11P11 K11P12 K11P13; do
    require_grep K11P0 "${row}" "scripts/verify-nimbus-proxy-pingora.sh" "verifier names ${row}"
  done
}

check_k11p0() {
  require_done K11P0 || true
  require_dir K11P0 "${PROOF_DIR}" "proof directory exists"
  require_file K11P0 "${PROOF_DIR}/k11p0-baseline.md" "baseline proof exists"
  check_self_names_rows
  check_prompt_sync

  local proof="${PROOF_DIR}/k11p0-baseline.md"
  require_grep K11P0 'HEAD: `57a23c18b`' "${proof}" "baseline proof records current HEAD"
  require_grep K11P0 'TcpListener|TcpStream|thread::spawn|blocking worker' "${proof}" "baseline proof records blocking worker and per-client threads"
  require_grep K11P0 'manual HTTP parsing|parse_proxy_request|fresh upstream dial|connect_timeout' "${proof}" "baseline proof records manual parsing and fresh dials"
  require_grep K11P0 'HTTP credential injection|credential injection attaches secret' "${proof}" "baseline proof records HTTP credential injection tests"
  require_grep K11P0 'opaque CONNECT|CONNECT tunnel' "${proof}" "baseline proof records opaque CONNECT baseline"
  require_grep K11P0 'credential injection is unavailable for CONNECT tunnels' "${proof}" "baseline proof records CONNECT credential fail-closed behavior"
  require_grep K11P0 'DLP inspection input unavailable for CONNECT tunnels' "${proof}" "baseline proof records CONNECT DLP fail-closed behavior"
  require_grep K11P0 'DnsCacheConfig|min_ttl|max_ttl|alias_chain' "${proof}" "baseline proof records DNS TTL/cache and alias-chain gaps"
  require_grep K11P0 'EgressProxyPoolKey|pool-key completeness|not wired' "${proof}" "baseline proof records pool-key completeness as unwired"
  require_grep K11P0 'decision_log.rs|redaction.rs|response.rs|policy_state.rs|nimbus-egress/src/env.rs' "${proof}" "baseline proof records security-load-bearing modules"
  require_grep K11P0 'AppendOnlyDecisionLogSink|SELH' "${proof}" "baseline proof records SELH decision-log sink baseline"
  require_grep K11P0 'nimbus-proxy dependency posture|no Pingora dependency' "${proof}" "baseline proof records dependency posture"
  require_grep K11P0 'Pingora `e6e677f`|/Users/jack/src/github.com/cloudflare/pingora' "${proof}" "baseline proof records Pingora commit and path"
  require_grep K11P0 'Rama `adedfce9`|/Users/jack/src/github.com/plabayo/rama' "${proof}" "baseline proof records Rama commit and path"
  require_grep K11P0 'ClawPatrol|OpenShell|agent-sandbox|agent-vault|wardgate|Pipelock|iron-proxy|sandbox-runtime|onecli' "${proof}" "baseline proof records local reference matrix"
}

check_done_with_proof() {
  local row="$1"
  local proof="$2"
  local pattern="$3"
  require_done "${row}" || true
  require_file "${row}" "${proof}" "${row} proof exists"
  require_grep "${row}" "${pattern}" "${proof}" "${row} proof records required acceptance evidence"
}

check_k11p1() {
  check_done_with_proof K11P1 "${PROOF_DIR}/k11p1-pingora-spike.md" 'Pingora|EgressProxy|TLS backend|crypto-provider/FIPS|lifecycle|rollback'
  require_reject K11P1 '^pingora' "crates/nimbus-egress/Cargo.toml" "Pingora is not added to nimbus-egress"
  require_reject K11P1 '^pingora' "crates/nimbus-runtime/Cargo.toml" "Pingora is not added to nimbus-runtime"
  require_reject K11P1 '^pingora' "crates/nimbus-sandbox/Cargo.toml" "Pingora is not added to nimbus-sandbox"
}

check_k11p2() {
  check_done_with_proof K11P2 "${PROOF_DIR}/k11p2-http-forward-parity.md" 'HTTP forward|credential|DLP|redirect|readiness|reload|timeout'
}

check_k11p3() {
  check_done_with_proof K11P3 "${PROOF_DIR}/k11p3-phase-mapping.md" 'canonicalize|pre-DNS|resolved-IP|credential|DLP|terminal log'
}

check_k11p4() {
  check_done_with_proof K11P4 "${PROOF_DIR}/k11p4-peer-pool-identity.md" 'group_key|pool|tenant|policy generation|credential|ALPN|collision'
}

check_k11p5() {
  check_done_with_proof K11P5 "${PROOF_DIR}/k11p5-selective-https-interception.md" 'Rama crosswalk|CONNECT|CA|leaf|trust|upstream TLS|QUIC'
}

check_k11p6() {
  check_done_with_proof K11P6 "${PROOF_DIR}/k11p6-https-credential-dlp-parity.md" 'HTTPS credential|DLP|caller-supplied|redirect|redacted|authorized'
}

check_k11p7() {
  check_done_with_proof K11P7 "${PROOF_DIR}/k11p7-sandbox-runtime-wiring.md" 'container|krun|trust|proxy wiring|isolate|wasm|fail-closed'
}

check_k11p8() {
  check_done_with_proof K11P8 "${PROOF_DIR}/k11p8-closeout.md" 'cargo fmt --all --check|make check|make clippy|make deny|verify-third-party-attribution|hosted CI'
  for row in K11P0 K11P1 K11P2 K11P3 K11P4 K11P5 K11P6 K11P7 K11P8; do
    require_grep K11P8 "\\| ${row} \\| done \\|" "${PLAN}" "${row} ledger row is done at closeout"
  done
}

# --- Reopen-wave rows (K11P9-K11P13): substance gates, not just proof greps.
# These fail red if the crate regresses to a feature-gated substrate, a
# blocking accept loop, per-sandbox runtimes, or unbounded buffer sites.

check_k11p9() {
  check_done_with_proof K11P9 "${PROOF_DIR}/k11p9-substrate-default.md" 'substrate|accept loop|ProxySubstrate|process_new_http|JoinSet'
  require_reject K11P9 'pingora-substrate' "crates/nimbus-proxy/Cargo.toml" "the pingora-substrate feature gate is deleted"
  if ! grep -Rq 'cfg(feature' crates/nimbus-proxy/src; then
    pass K11P9 "nimbus-proxy src has no feature-gated code paths"
  else
    fail K11P9 "nimbus-proxy src reintroduced cfg(feature ...) gating"
  fi
  require_file K11P9 "crates/nimbus-proxy/src/substrate.rs" "substrate module exists"
  require_grep K11P9 'OnceLock' "crates/nimbus-proxy/src/substrate.rs" "shared substrate is a process-wide singleton"
  local runtime_hits
  runtime_hits="$(grep -REl 'Runtime::new|new_multi_thread|new_current_thread' crates/nimbus-proxy/src/ 2>/dev/null)"
  if [ "${runtime_hits}" = "crates/nimbus-proxy/src/substrate.rs" ]; then
    pass K11P9 "tokio runtime construction is confined to substrate.rs (no per-sandbox runtimes)"
  else
    fail K11P9 "runtime construction outside substrate.rs: ${runtime_hits:-none-found}"
  fi
  require_grep K11P9 'process_new_http' "crates/nimbus-proxy/src/worker.rs" "Pingora ProxyHttp is the production forward data plane"
  require_grep K11P9 'JoinSet' "crates/nimbus-proxy/src/worker.rs" "per-sandbox connection tasks are tracked for token-scoped shutdown"
  require_reject K11P9 'thread::spawn|thread::Builder' "crates/nimbus-proxy/src/worker.rs" "no per-client or per-proxy OS threads remain"
  if [ ! -f "crates/nimbus-proxy/src/pingora_forward.rs" ] && [ ! -f "crates/nimbus-proxy/src/pingora_substrate.rs" ]; then
    pass K11P9 "per-request connector helper and test-gated spike are deleted"
  else
    fail K11P9 "old pingora_forward.rs / pingora_substrate.rs spike files still present"
  fi
}

check_k11p10() {
  check_done_with_proof K11P10 "${PROOF_DIR}/k11p10-streaming-bounds.md" 'stream|cap|clamp|leaf cache|max_inspection_bytes'
  require_grep K11P10 'BODY_PREALLOC_CLAMP_BYTES' "crates/nimbus-proxy/src/body.rs" "declared-length preallocation is clamped"
  require_grep K11P10 'LEAF_CACHE_CAP' "crates/nimbus-proxy/src/tls_authority.rs" "TLS leaf cache is bounded"
  require_reject K11P10 'read_to_end' "crates/nimbus-proxy/src/https_intercept.rs" "intercepted HTTPS streams instead of buffering to EOF"
  require_grep K11P10 'stream_content_length_body|copy_until_eof' "crates/nimbus-proxy/src/https_intercept.rs" "intercept path uses streaming body helpers"
}

check_k11p11() {
  check_done_with_proof K11P11 "${PROOF_DIR}/k11p11-intercept-error-semantics.md" 'dial|write|read|502|fail-closed|terminal'
  require_grep K11P11 'upstream dial failed' "crates/nimbus-proxy/src/https_intercept.rs" "upstream dial failure returns a structured response"
  require_grep K11P11 'upstream write failed' "crates/nimbus-proxy/src/https_intercept.rs" "upstream write failure returns a structured response"
  require_grep K11P11 'upstream response read failed' "crates/nimbus-proxy/src/https_intercept.rs" "upstream read failure returns a structured response"
}

check_k11p12() {
  check_done_with_proof K11P12 "${PROOF_DIR}/k11p12-trust-anchor-persistence.md" 'atomic|rename|fsync|0644|rooted|lock'
  local egress="crates/nimbus-sandbox/src/backends/oci/egress.rs"
  require_grep K11P12 'validate_trust_anchor_path' "${egress}" "trust-anchor writes are rooted-path validated"
  require_grep K11P12 'create_new' "${egress}" "trust-anchor writer uses an exclusive temp file"
  require_grep K11P12 'rename' "${egress}" "trust-anchor writer publishes via atomic rename"
  require_grep K11P12 'sync_all' "${egress}" "trust-anchor writer fsyncs before and after rename"
  require_reject K11P12 'lock_trust_anchor_paths' "${egress}" "the dual-mutex registry shape is gone"
}

check_k11p13() {
  check_done_with_proof K11P13 "${PROOF_DIR}/k11p13-recloseout.md" 'code review|architectural review|verifier|closeout'
  for row in K11P2 K11P4 K11P8; do
    require_grep K11P13 "\\| ${row} \\| done \\|" "${PLAN}" "${row} ledger row is re-closed"
  done
}

# Reuse-safety wiring invariants (adversarial review 2026-07-02). These lock in
# the properties that make cross-tenant upstream reuse structurally impossible
# and preserve the hard-won reuse contract so a future pooling change cannot
# silently reintroduce the credential-crossing / connection-bound-auth holes.
check_reuse_safety() {
  local proxy_src="crates/nimbus-proxy/src"
  # The per-sandbox ephemeral CA is an explicit, tested isolation invariant.
  require_grep REUSE 'ephemeral_authorities_are_distinct_and_export_only_public_material' \
    "${proxy_src}/tls_authority.rs" "ephemeral per-sandbox CA distinctness is tested"
  require_grep REUSE 'distinct_sandboxes_receive_distinct_ephemeral_cas' \
    "crates/nimbus-sandbox/src/backends/oci/egress.rs" "distinct-CA-per-sandbox invariant is tested"
  # The reuse contract (credential fields stay in the key; connection-oriented
  # auth eviction; per-PEP connector) must remain documented in pool.rs.
  require_grep REUSE 'Reuse contract' "${proxy_src}/pool.rs" "reuse-safety contract is documented in pool.rs"
  require_grep REUSE 'connection-oriented auth|Connection-oriented auth' "${proxy_src}/pool.rs" \
    "reuse contract records the connection-bound-auth hole"
  # Cross-tenant impossibility rests on NO shared/node-wide upstream connector.
  # ProxySubstrate is capacity-only; it must not grow an upstream connector/pool.
  require_reject REUSE 'Connector|connection_pool' "${proxy_src}/substrate.rs" \
    "ProxySubstrate holds no shared upstream connector (cross-tenant reuse stays impossible)"
}

printf 'Nimbus proxy Pingora verifier\n'
printf 'Repo: %s\n\n' "${ROOT}"

check_k11p0
check_k11p1
check_k11p2
check_k11p3
check_k11p4
check_k11p5
check_k11p6
check_k11p7
check_k11p8
check_k11p9
check_k11p10
check_k11p11
check_k11p12
check_k11p13
check_reuse_safety

printf '\nSummary: %d passed, %d failed\n' "${passed}" "${failed}"

if [ "${failed}" -ne 0 ]; then
  printf '\nFailed conditions:\n'
  for failure in "${failures[@]}"; do
    printf -- '- %s\n' "${failure}"
  done
  exit 1
fi
