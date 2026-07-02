#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROOF_DIR="${ROOT}/docs/private/plans/proof/nimbus-sandbox-egress-launch-hardening"
PLAN="${ROOT}/docs/private/plans/nimbus-sandbox-egress-launch-hardening-plan.md"

passed=0
failed=0

pass() {
  printf 'PASS %s: %s\n' "$1" "$2"
  passed=$((passed + 1))
}

fail() {
  printf 'FAIL %s: %s\n' "$1" "$2"
  failed=$((failed + 1))
}

require_file() {
  local row="$1"
  local path="$2"
  local message="$3"
  if [ -f "${ROOT}/${path}" ]; then
    pass "$row" "$message"
  else
    fail "$row" "missing ${path}: ${message}"
  fi
}

require_dir() {
  local row="$1"
  local path="$2"
  local message="$3"
  if [ -d "${ROOT}/${path}" ]; then
    pass "$row" "$message"
  else
    fail "$row" "missing ${path}: ${message}"
  fi
}

require_grep() {
  local row="$1"
  local pattern="$2"
  local path="$3"
  local message="$4"
  if grep -Eq "$pattern" "${ROOT}/${path}" 2>/dev/null; then
    pass "$row" "$message"
  else
    fail "$row" "${message} (${path} lacks pattern: ${pattern})"
  fi
}

reject_grep() {
  local row="$1"
  local pattern="$2"
  local path="$3"
  local message="$4"
  if grep -Eq "$pattern" "${ROOT}/${path}" 2>/dev/null; then
    fail "$row" "${message} (${path} matches forbidden pattern: ${pattern})"
  else
    pass "$row" "$message"
  fi
}

check_selh0() {
  require_dir SELH0 "docs/private/plans/proof/nimbus-sandbox-egress-launch-hardening" "proof directory exists"
  require_file SELH0 "docs/private/plans/proof/nimbus-sandbox-egress-launch-hardening/selh0-baseline.md" "baseline proof exists"
  for row in SELH0 SELH1 SELH2 SELH3 SELH4 SELH5 SELH6 SELH7 SELH8; do
    require_grep SELH0 "${row}" "scripts/verify-nimbus-sandbox-egress-launch-hardening.sh" "verifier names ${row}"
  done
}

check_selh1() {
  require_file SELH1 "docs/private/plans/proof/nimbus-sandbox-egress-launch-hardening/selh1-decisions.md" "launch decision proof exists"
  require_grep SELH1 "GO-WITH-PUNCH-LIST" "docs/private/plans/proof/nimbus-sandbox-egress-launch-hardening/selh1-decisions.md" "D0 co-location decision recorded"
  require_grep SELH1 "file sink.*OCSF|OCSF.*file sink" "docs/private/plans/proof/nimbus-sandbox-egress-launch-hardening/selh1-decisions.md" "Q3 file sink now and OCSF later recorded"
  require_grep SELH1 "NIMBUS_CRUN_VERSION.*NIMBUS_LIBKRUN_VERSION|nimbus-crun.*nimbus-libkrun" "docs/private/plans/proof/nimbus-sandbox-egress-launch-hardening/selh1-decisions.md" "Q4 validated fork tuple source recorded"
  require_grep SELH1 "no blanket MITM|NO blanket MITM" "docs/private/plans/proof/nimbus-sandbox-egress-launch-hardening/selh1-decisions.md" "P2.14 no blanket MITM recorded"
  require_grep SELH1 "P2\\.14.*nimbus-proxy-pingora-plan\\.md.*nimbus-sandbox-egress-launch-hardening-plan\\.md" "docs/private/plans/nimbus-modernization-roadmap-plan-map.md" "P2.14 remains co-owned by K11P and SELH"
  reject_grep SELH1 "SELH (now |does |provides |supports ).{0,80}HTTPS credential injection|HTTPS credential injection (is |now )supported by SELH" "docs/private/plans/nimbus-sandbox-egress-launch-hardening-plan.md" "SELH does not claim HTTPS credential injection support"
  reject_grep SELH1 "active.*archive/nimbus-egress-gateway-extraction-plan|archive/nimbus-egress-gateway-extraction-plan.*active implementation" "docs/private/plans/README.md" "active routing does not point back to archived NEG"
}

check_selh2() {
  require_file SELH2 ".github/workflows/container-pep-egress.yml" "container PEP workflow exists"
  require_grep SELH2 "ubuntu-24\\.04" ".github/workflows/container-pep-egress.yml" "workflow runs on ubuntu-24.04"
  require_grep SELH2 "buildah.*conmon.*crun.*netavark.*aardvark-dns|buildah" ".github/workflows/container-pep-egress.yml" "workflow installs or verifies OCI host tools"
  require_grep SELH2 "cargo test -p nimbus-sandbox --test container_linux_egress -- --ignored --nocapture" ".github/workflows/container-pep-egress.yml" "workflow runs the ignored container egress proof"
  require_grep SELH2 "nft.*list ruleset|netavark.*--version|aardvark-dns.*--version" ".github/workflows/container-pep-egress.yml" "workflow publishes failure diagnostics"
  require_grep SELH2 "crates/nimbus-sandbox|crates/nimbus-egress|crates/nimbus-proxy|container-pep-egress" ".github/workflows/container-pep-egress.yml" "path filters cover sandbox, egress, proxy, and workflow edits"
  require_file SELH2 "docs/private/plans/proof/nimbus-sandbox-egress-launch-hardening/selh2-container-pep-ci.md" "container PEP CI proof exists"
}

check_selh3() {
  require_grep SELH3 "authorize_hostname_before_dns|pre_dns_authorize|PreDnsAuthorize" "crates/nimbus-proxy/src/worker.rs" "proxy performs hostname-only authorization before DNS"
  require_grep SELH3 "PreDnsAuthorize|PreDnsPolicy" "crates/nimbus-proxy/src/phase.rs" "phase order records pre-DNS authorization"
  require_grep SELH3 "authorize_hostname_without_resolved_ip|matches_l4_without_resolved_ip" "crates/nimbus-egress/src/policy.rs" "PDP exposes hostname-only precheck"
  require_grep SELH3 "denied_hostname_does_not_resolve|policy_denied_hostname.*before_dns|allowed_hostname_invokes.*resolver|resolved_internal.*denies" "crates/nimbus-proxy/src/tests.rs" "proxy tests cover DNS precheck cases"
  require_file SELH3 "docs/private/plans/proof/nimbus-sandbox-egress-launch-hardening/selh3-dns-precheck.md" "DNS precheck proof exists"
}

check_selh4() {
  require_grep SELH4 "AppendOnlyDecisionLogSink|DecisionLogFileSink|append-only" "crates/nimbus-proxy/src/decision_log.rs" "append-only decision log sink exists"
  require_grep SELH4 "with_decision_logger" "crates/nimbus-sandbox/src/backends/oci/egress.rs" "live OCI PEP build site injects a decision logger"
  reject_grep SELH4 "EgressProxyConfig::new\\(compiled\\)\\.with_bind_addr\\(bind_addr\\)" "crates/nimbus-sandbox/src/backends/oci/egress.rs" "live OCI PEP build site no longer uses the default noop logger"
  require_grep SELH4 "allow.*exactly one|deny.*exactly one|DLP.*exactly one|redact" "crates/nimbus-proxy/src/tests.rs" "proxy tests cover one terminal event and redaction"
  require_grep SELH4 "live.*noop|noop.*live|decision_logger" "crates/nimbus-sandbox/src/backends/oci/egress.rs" "sandbox tests or code prove live path does not use noop"
  require_file SELH4 "docs/private/plans/proof/nimbus-sandbox-egress-launch-hardening/selh4-decision-log-sink.md" "decision log proof exists"
}

check_selh5() {
  require_grep SELH5 "canonicalize_authority_host|canonical_host_result|HostAuthorityError" "crates/nimbus-egress/src/policy.rs" "policy path uses strict host authority canonicalization"
  require_grep SELH5 "canonicalize_authority_host|HostAuthorityError|nul|null" "crates/nimbus-proxy/src/request.rs" "proxy path rejects strict malformed authorities"
  require_grep SELH5 "null|percent|userinfo|non-canonical|CONNECT.*HTTP|trailing dot|resolved-IP" "crates/nimbus-proxy/src/tests.rs" "proxy tests cover malformed authority and DNS/IP anchor"
  require_grep SELH5 "host-plus-resolved-IP|host.*resolved IP|resolved-IP.*anchor|SNI.*not authority" "docs/private/operating/nimbus-egress-gateway.md" "operator docs state host plus resolved-IP authority"
  require_file SELH5 "docs/private/plans/proof/nimbus-sandbox-egress-launch-hardening/selh5-authority-anchor.md" "authority anchor proof exists"
}

check_selh6() {
  require_file SELH6 "packaging/linux-distribution-contract.env" "validated tuple manifest exists"
  require_grep SELH6 "NIMBUS_CRUN_VERSION=.*nimbus|NIMBUS_LIBKRUN_VERSION=.*nimbus" "packaging/linux-distribution-contract.env" "tuple manifest carries Nimbus fork versions"
  reject_grep SELH6 "NIMBUS_CRUN_RELEASES_API}/latest|NIMBUS_LIBKRUN_RELEASES_API}/latest" "scripts/install.sh" "installer does not default fork artifacts through latest"
  require_grep SELH6 "linux-distribution-contract.env|NIMBUS_CRUN_UPSTREAM_VERSION|NIMBUS_LIBKRUN_UPSTREAM_VERSION" "scripts/install.sh" "installer consumes the tuple source"
  require_grep SELH6 "EXPECTED_NIMBUS_CRUN_VERSION|EXPECTED_NIMBUS_LIBKRUN_VERSION|EXPECTED_LIBKRUN_SONAME|EXPECTED_LIBKRUNFW_SONAME|EXPECTED_LIBKRUN_ABI_SYMBOL" "scripts/check-vmm-host.sh" "ABI doctor reports expected tuple and ABI"
  require_grep SELH6 "install.*tuple|ABI mismatch|latest" "scripts/verify-nimbus-sandbox-egress-launch-hardening.sh" "verifier checks install tuple and latest rejection"
  require_file SELH6 "docs/private/plans/proof/nimbus-sandbox-egress-launch-hardening/selh6-fork-tuple-abi.md" "fork tuple proof exists"
}

check_selh7() {
  require_grep SELH7 "Result<.*RuntimeAffinityKey|RuntimeAffinityError|MissingTenant" "crates/nimbus-runtime/src/affinity.rs" "runtime affinity returns an error when a tenant label is required"
  require_grep SELH7 "tenant.*absent|missing tenant|requires a tenant label" "crates/nimbus-bridge/src/egress.rs" "runtime egress denies absent tenant labels"
  require_grep SELH7 "missing.*tenant.*affinity|function.*tenant|present.*tenant" "crates/nimbus-runtime/src/executor/tests/router_affinity.rs" "runtime tests cover missing and present tenant affinity"
  require_grep SELH7 "absent.*tenant|mismatched.*tenant|matching.*tenant" "crates/nimbus-bridge/src/egress.rs" "bridge tests cover absent, mismatched, and matching tenant egress"
  require_file SELH7 "docs/private/plans/proof/nimbus-sandbox-egress-launch-hardening/selh7-runtime-tenant-guards.md" "runtime tenant guard proof exists"
}

check_selh8() {
  for row in SELH0 SELH1 SELH2 SELH3 SELH4 SELH5 SELH6 SELH7 SELH8; do
    require_grep SELH8 "\\| ${row} \\| done \\|" "docs/private/plans/nimbus-sandbox-egress-launch-hardening-plan.md" "${row} ledger row is done"
  done
  require_file SELH8 "docs/private/plans/proof/nimbus-sandbox-egress-launch-hardening/selh8-closeout.md" "closeout proof exists"
  require_grep SELH8 "cargo fmt --all --check|make check|make clippy|make deny|verify-third-party-attribution" "docs/private/plans/proof/nimbus-sandbox-egress-launch-hardening/selh8-closeout.md" "closeout proof records required commands"
}

check_selh0
check_selh1
check_selh2
check_selh3
check_selh4
check_selh5
check_selh6
check_selh7
check_selh8

printf 'SELH verifier summary: %d passed, %d failed\n' "$passed" "$failed"

if [ "$failed" -ne 0 ]; then
  exit 1
fi
