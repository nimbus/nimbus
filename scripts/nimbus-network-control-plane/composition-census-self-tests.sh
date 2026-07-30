#!/usr/bin/env bash

# NNC4.6f mutation cases for the production network-authority census. This file
# is sourced by the aggregate verifier so that its composition root stays thin.

nnc46f_only_nncv015_failed() {
  local output="$1"
  [ "$(grep -c '^FAIL ' "${output}" || true)" -eq 1 ] &&
    grep -q '^FAIL NNCV015 local-network-composition-census' "${output}" &&
    ! grep -q '^PASS NNCV015 local-network-composition-census' "${output}"
}

run_nnc46f_composition_census_self_tests() {
  local script="$1"
  local temporary="$2"
  local failures=0

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_CASE=1 \
    NIMBUS_NETWORK_VERIFY_COMPOSITION_CENSUS="${temporary}/missing-composition-census.json" \
    "${script}" >"${temporary}/missing-composition-census.out" 2>&1; then
    printf 'SELFTEST FAIL missing composition census unexpectedly exited zero\n'
    failures=$((failures + 1))
  elif ! nnc46f_only_nncv015_failed "${temporary}/missing-composition-census.out" ||
    ! grep -q 'composition authority census missing:' "${temporary}/missing-composition-census.out"; then
    printf 'SELFTEST FAIL missing composition census did not fail NNCV015 precisely\n'
    failures=$((failures + 1))
  else
    printf 'SELFTEST PASS missing composition census fails closed as NNCV015\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_CASE=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE='fn second_manager_constructor() { LocalNetworkManager::open("shadow", registry()); }' \
    "${script}" >"${temporary}/second-manager-constructor.out" 2>&1; then
    printf 'SELFTEST FAIL second manager constructor unexpectedly exited zero\n'
    failures=$((failures + 1))
  elif ! nnc46f_only_nncv015_failed "${temporary}/second-manager-constructor.out" ||
    ! grep -q 'manager-direct-open|second_manager_constructor' "${temporary}/second-manager-constructor.out"; then
    printf 'SELFTEST FAIL second manager constructor did not fail NNCV015 precisely\n'
    failures=$((failures + 1))
  else
    printf 'SELFTEST PASS second manager constructor fails closed as NNCV015\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_CASE=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE='fn divergent_root_resolver() { LocalNodeNetworkRoot::resolve_for_current_platform(None); }' \
    "${script}" >"${temporary}/divergent-root-resolver.out" 2>&1; then
    printf 'SELFTEST FAIL divergent root resolver unexpectedly exited zero\n'
    failures=$((failures + 1))
  elif ! nnc46f_only_nncv015_failed "${temporary}/divergent-root-resolver.out" ||
    ! grep -q 'local-node-root-resolver|divergent_root_resolver' "${temporary}/divergent-root-resolver.out"; then
    printf 'SELFTEST FAIL divergent root resolver did not fail NNCV015 precisely\n'
    failures=$((failures + 1))
  else
    printf 'SELFTEST PASS divergent root resolver fails closed as NNCV015\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_CASE=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_MUTATION='wrong-os-node-realm' \
    "${script}" >"${temporary}/wrong-os-node-realm.out" 2>&1; then
    printf 'SELFTEST FAIL wrong OS-node realm unexpectedly exited zero\n'
    failures=$((failures + 1))
  elif ! nnc46f_only_nncv015_failed "${temporary}/wrong-os-node-realm.out" ||
    ! grep -q 'composition OS-node realm mismatch: .*expected=guest-node:observed=parent-host' "${temporary}/wrong-os-node-realm.out"; then
    printf 'SELFTEST FAIL wrong OS-node realm did not fail NNCV015 precisely\n'
    failures=$((failures + 1))
  else
    printf 'SELFTEST PASS wrong OS-node realm fails closed as NNCV015\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_CASE=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE='fn guest_mint_parent() { MachineForwarderAuthority::new(provider(), generation()); }' \
    "${script}" >"${temporary}/guest-minted-parent-identity.out" 2>&1; then
    printf 'SELFTEST FAIL guest-minted parent identity unexpectedly exited zero\n'
    failures=$((failures + 1))
  elif ! nnc46f_only_nncv015_failed "${temporary}/guest-minted-parent-identity.out" ||
    ! grep -q 'guest-minted parent identity is forbidden: .*machine-forwarder-authority-mint|guest_mint_parent' "${temporary}/guest-minted-parent-identity.out"; then
    printf 'SELFTEST FAIL guest-minted parent identity did not fail NNCV015 precisely\n'
    failures=$((failures + 1))
  else
    printf 'SELFTEST PASS guest-minted parent identity fails closed as NNCV015\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_CASE=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_MUTATION='false-runtime-proof' \
    "${script}" >"${temporary}/false-runtime-proof.out" 2>&1; then
    printf 'SELFTEST FAIL false runtime-proof claim unexpectedly exited zero\n'
    failures=$((failures + 1))
  elif ! nnc46f_only_nncv015_failed "${temporary}/false-runtime-proof.out" ||
    ! grep -q 'test fixture cannot claim runtime proof:' "${temporary}/false-runtime-proof.out" ||
    ! grep -q 'source-only future network seam cannot claim runtime proof:' "${temporary}/false-runtime-proof.out"; then
    printf 'SELFTEST FAIL false runtime-proof claim did not fail NNCV015 precisely\n'
    failures=$((failures + 1))
  else
    printf 'SELFTEST PASS false runtime-proof claim fails closed as NNCV015\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_CASE=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_MUTATION='bless-unapproved-direct' \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn unapproved_direct(config: Config) { KrunSandboxBackend::new(config); }\nfn unapproved_segment(root: &Path) { ConfiguredSegmentAllocator::reconstruct_from_state_root(root); }' \
    "${script}" >"${temporary}/blessed-unapproved-direct.out" 2>&1; then
    printf 'SELFTEST FAIL census-blessed unapproved direct/raw-root occurrences unexpectedly exited zero\n'
    failures=$((failures + 1))
  elif ! nnc46f_only_nncv015_failed "${temporary}/blessed-unapproved-direct.out" ||
    ! grep -q 'composition occurrence kind has no authority policy: crates/nimbus-server/src/listener_lease.rs|server-internal-direct-reconstruction-declaration|reconstruct_direct|2' "${temporary}/blessed-unapproved-direct.out" ||
    ! grep -q 'composition occurrence kind has no authority policy: .*segment-primitive-reconstruction|unapproved_segment' "${temporary}/blessed-unapproved-direct.out"; then
    printf 'SELFTEST FAIL census-blessed unapproved direct/raw-root occurrences did not fail NNCV015 precisely\n'
    failures=$((failures + 1))
  else
    printf 'SELFTEST PASS census cannot bless unapproved direct or raw-root occurrences\n'
  fi

  return "${failures}"
}
