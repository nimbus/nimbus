#!/usr/bin/env bash
# Completion-gate verifier for
# docs/private/plans/crate-architecture-modularity-plan.md.
#
# CAM0 creates this verifier so the architecture plan is executable from the
# start. CAM0 should finish at exactly "Summary: 3 passed, 7 failed"; CAM7
# closes at exactly "Summary: 10 passed, 0 failed".

set -u
set -o pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}" || exit 2

PLAN_ACTIVE="docs/private/plans/crate-architecture-modularity-plan.md"
PLAN_ARCHIVED="docs/private/plans/archive/crate-architecture-modularity-plan.md"
if [ -f "${PLAN_ACTIVE}" ]; then
  PLAN="${PLAN_ACTIVE}"
else
  PLAN="${PLAN_ARCHIVED}"
fi
PLANS_README="docs/private/plans/README.md"
PROOF_DIR="docs/private/plans/proof/crate-architecture-modularity"
CAM0_PROOF="${PROOF_DIR}/cam0-baseline.md"
DEDICATED_WORKTREE="/Users/jack/.codex/worktrees/crate-architecture-modularity/nimbus"
BRANCH="codex/crate-architecture-modularity"

PASSED=0
FAILED=0
FAIL_DETAIL=()

pass() {
  PASSED=$((PASSED + 1))
  printf 'PASS: %s\n' "$1"
}

fail() {
  FAILED=$((FAILED + 1))
  printf 'FAIL: %s\n' "$1"
  if [ "$#" -ge 2 ]; then
    printf '      %s\n' "$2"
    FAIL_DETAIL+=("$1 - $2")
  else
    FAIL_DETAIL+=("$1")
  fi
}

has_file() {
  test -f "$1"
}

grep_file() {
  local pattern="$1"
  local file="$2"
  grep -Eiq "$pattern" "$file" 2>/dev/null
}

line_count() {
  wc -l < "$1" | tr -d ' '
}

package_has_workspace_dep() {
  local package="$1"
  local dependency="$2"
  cargo metadata --no-deps --format-version 1 2>/dev/null \
    | jq -e --arg package "$package" --arg dependency "$dependency" '
      .packages[]
      | select(.name == $package)
      | .dependencies[]?
      | select(.name == $dependency and .source == null)
    ' >/dev/null
}

runtime_workspace_deps() {
  cargo metadata --no-deps --format-version 1 2>/dev/null \
    | jq -r '
      .packages[]
      | select(.name == "nimbus-runtime")
      | .dependencies[]?
      | select(.source == null)
      | .name
    '
}

check_plan_and_routing() {
  local readme_route_ok=0
  if has_file "${PLANS_README}"; then
    if [ "${PLAN}" = "${PLAN_ACTIVE}" ]; then
      if grep_file "crate-architecture-modularity-plan\\.md" "${PLANS_README}" \
        && grep_file "${DEDICATED_WORKTREE}" "${PLANS_README}"; then
        readme_route_ok=1
      fi
    elif ! grep_file "crate-architecture-modularity-plan\\.md" "${PLANS_README}"; then
      readme_route_ok=1
    fi
  fi

  if has_file "${PLAN}" \
    && has_file "${PLANS_README}" \
    && [ "${readme_route_ok}" -eq 1 ] \
    && grep_file "verify-crate-architecture-modularity\\.sh" "${PLAN}" \
    && grep_file "${DEDICATED_WORKTREE}" "${PLAN}" \
    && grep_file "${BRANCH}" "${PLAN}"; then
    pass "plan, verifier, branch, dedicated worktree, and active-index routing contract are recorded"
  else
    fail "plan routing is incomplete" "expected ${PLAN}, ${PLANS_README}, verifier, ${BRANCH}, and ${DEDICATED_WORKTREE}"
  fi
}

check_cam0_proof() {
  if has_file "${CAM0_PROOF}" \
    && grep_file "${DEDICATED_WORKTREE}" "${CAM0_PROOF}" \
    && grep_file "git worktree list --porcelain" "${CAM0_PROOF}" \
    && grep_file "${BRANCH}" "${CAM0_PROOF}" \
    && grep_file "Crate inventory" "${CAM0_PROOF}" \
    && grep_file "Dependency graph" "${CAM0_PROOF}" \
    && grep_file "Large-file inventory" "${CAM0_PROOF}" \
    && grep_file "nimbus-core zero-I/O" "${CAM0_PROOF}" \
    && grep_file "nimbus-runtime zero-workspace-dep" "${CAM0_PROOF}" \
    && grep_file "NOS" "${CAM0_PROOF}" \
    && grep_file "KME" "${CAM0_PROOF}"; then
    pass "CAM0 baseline proof records worktree, graph, inventory, invariants, and NOS/KME baselines"
  else
    fail "CAM0 baseline proof is missing required evidence" "expected ${CAM0_PROOF}"
  fi
}

check_core_runtime_invariants() {
  local core_io_pattern='std::fs|tokio::fs|async_std::fs|std::process|Command::new|TcpStream|TcpListener|UdpSocket|UnixStream|UnixListener|File::|OpenOptions|read_to_string|create_dir|remove_file|reqwest|hyper::Client|ureq|tokio::net'
  local runtime_deps
  runtime_deps="$(runtime_workspace_deps)"

  if ! rg -q "${core_io_pattern}" crates/nimbus-core/src --glob '*.rs' \
    && test -z "${runtime_deps}"; then
    pass "nimbus-core has no real I/O hits and nimbus-runtime has zero workspace dependencies"
  else
    fail "core/runtime invariant violation" "nimbus-core I/O hits or nimbus-runtime workspace deps: ${runtime_deps:-none listed}"
  fi
}

check_firestore_provider_family_seam() {
  local shared_seam=0
  if rg -q 'firestore[_-](family|common|provider)|nimbus[_-]firestore|FirestoreProviderFamily|FirestorePathSemantics' \
    crates/nimbus-cloud-functions crates/nimbus-firebase crates -g Cargo.toml 2>/dev/null; then
    shared_seam=1
  fi

  if ! package_has_workspace_dep "nimbus-cloud-functions" "nimbus-firebase" \
    && ! rg -q 'nimbus_firebase|nimbus-firebase' crates/nimbus-cloud-functions \
    && test "${shared_seam}" -eq 1; then
    pass "Cloud Functions and Firebase consume a shared Firestore provider-family seam"
  else
    fail "Firestore provider-family seam incomplete" "nimbus-cloud-functions still depends on/imports nimbus-firebase, or no shared seam evidence was found"
  fi
}

check_public_interfaces() {
  if ! rg -q 'pub mod adapters_(mongodb|dynamodb)|Test-only re-export|LocalAdminTokenRecord|LocalServerSecurityState|OperatorPolicyDocument|TenantIsolationDecision' crates/nimbus-server/src/lib.rs \
    && ! rg -q 'ConvexRegistry|LocalServerSecurityState|LocalAdminTokenRecord' crates/nimbus/src/lib.rs; then
    pass "nimbus and nimbus-server public exports are narrowed to intentional surfaces"
  else
    fail "public interface surface remains broad" "server/facade still expose test-only adapter internals or local/policy internals"
  fi
}

check_storage_traits() {
  local traits_file="crates/nimbus-storage/src/traits/mod.rs"
  if has_file "${traits_file}" \
    && test "$(line_count "${traits_file}")" -lt 500 \
    && ! rg -q 'trait ObjectMetaStore|macro_rules!' "${traits_file}" \
    && has_file "crates/nimbus-storage/src/traits/object_metadata.rs" \
    && has_file "crates/nimbus-storage/src/traits/provider_impls.rs"; then
    pass "storage trait modules are concept-owned and ObjectMetaStore has a metadata-plane home"
  else
    fail "storage trait modularity incomplete" "expected thin traits/mod.rs plus object_metadata.rs and provider_impls.rs"
  fi
}

check_postgres_modularity() {
  local backend="crates/nimbus-storage/src/postgres/backend.rs"
  local proof="${PROOF_DIR}/cam3-storage.md"
  if has_file "${backend}" \
    && { test "$(line_count "${backend}")" -lt 1500 \
      || { has_file "${proof}" && grep_file "Postgres backend ownership exception" "${proof}"; }; }; then
    pass "Postgres backend root is below threshold or has a strong ownership exception"
  else
    fail "Postgres backend root remains over threshold without exception" "expected ${backend} < 1500 lines or ${proof} exception"
  fi
}

check_sandbox_modularity() {
  local oci_network="crates/nimbus-sandbox/src/backends/oci/network.rs"
  local container_runtime="crates/nimbus-sandbox/src/backends/container/runtime.rs"
  local oci_modules=0
  local container_modules=0

  if find crates/nimbus-sandbox/src/backends/oci -maxdepth 3 -type f \
    | grep -Eq 'network/(layout|netns|netavark|ipam|forwarding|proxy|dto)\.rs'; then
    oci_modules=1
  fi
  if find crates/nimbus-sandbox/src/backends/container -maxdepth 2 -type f \
    | grep -Eq '(runner|manifest|readiness|status|launch|config)\.rs'; then
    container_modules=1
  fi

  if has_file "${oci_network}" \
    && has_file "${container_runtime}" \
    && test "$(line_count "${oci_network}")" -lt 1500 \
    && test "$(line_count "${container_runtime}")" -lt 1500 \
    && test "${oci_modules}" -eq 1 \
    && test "${container_modules}" -eq 1; then
    pass "OCI/container sandbox roots are concept-owned"
  else
    fail "OCI/container sandbox modularity incomplete" "expected thin roots plus network and container concept modules"
  fi
}

check_runtime_bootstrap_js_assets() {
  local source="crates/nimbus-runtime/src/runtime/bootstrap/source.rs"
  if has_file "${source}" \
    && test "$(line_count "${source}")" -lt 700 \
    && ! rg -q 'const [A-Z0-9_]+_SOURCE: &str = r#"' "${source}" \
    && test "$(find crates/nimbus-runtime/src/runtime/bootstrap/js -maxdepth 1 -name '*.js' | wc -l | tr -d ' ')" -ge 8; then
    pass "runtime bootstrap JavaScript lives as named assets with Rust as registry/executor"
  else
    fail "runtime bootstrap source split incomplete" "expected source.rs to stop embedding large JS programs"
  fi
}

check_final_closeout() {
  if has_file "${PLAN}" \
    && ! grep -Eq '\| CAM[0-7] \| `(todo|in_progress|blocked)`' "${PLAN}" \
    && has_file "${PROOF_DIR}/cam7-closeout.md" \
    && grep_file "cargo fmt --all --check.*pass|pass.*cargo fmt --all --check" "${PROOF_DIR}/cam7-closeout.md" \
    && grep_file "make check.*pass|pass.*make check" "${PROOF_DIR}/cam7-closeout.md" \
    && grep_file "make clippy.*pass|pass.*make clippy" "${PROOF_DIR}/cam7-closeout.md" \
    && grep_file "make deny.*pass|pass.*make deny" "${PROOF_DIR}/cam7-closeout.md" \
    && grep_file "branch CI.*green|green.*branch CI" "${PROOF_DIR}/cam7-closeout.md" \
    && grep_file "PR.*open|open.*PR" "${PROOF_DIR}/cam7-closeout.md"; then
    pass "CAM0-CAM7 are closed with local gates, green branch CI, and an open PR"
  else
    fail "final closeout incomplete" "expected all CAM rows done plus ${PROOF_DIR}/cam7-closeout.md with local gates, CI, and PR"
  fi
}

check_plan_and_routing
check_cam0_proof
check_core_runtime_invariants
check_firestore_provider_family_seam
check_public_interfaces
check_storage_traits
check_postgres_modularity
check_sandbox_modularity
check_runtime_bootstrap_js_assets
check_final_closeout

printf '\nSummary: %d passed, %d failed\n' "${PASSED}" "${FAILED}"

if [ "${FAILED}" -gt 0 ]; then
  printf '\nOutstanding:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf ' - %s\n' "${detail}"
  done
  exit 1
fi

exit 0
