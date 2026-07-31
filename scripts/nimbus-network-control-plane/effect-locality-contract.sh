# shellcheck shell=bash

verify_nnc55_sandbox_effect_locality() {
  error="$(node "${SOURCE_CONTRACT_HELPER}" sandbox-effect-locality 2>&1)"
  nnc_locality_status=$?
  if [ "${nnc_locality_status}" -eq 0 ]; then
    pass "NNCV022" "sandbox-provider-effect-locality"
  else
    fail "NNCV022" "sandbox-provider-effect-locality" "${error}"
  fi
}

verify_nnc55_sealed_effect_capabilities() {
  error="$(node "${SOURCE_CONTRACT_HELPER}" sealed-effect-capabilities 2>&1)"
  nnc_seal_status=$?
  if [ "${nnc_seal_status}" -eq 0 ]; then
    pass "NNCV023" "sealed-provider-effect-capabilities"
  else
    fail "NNCV023" "sealed-provider-effect-capabilities" "${error}"
  fi
}

run_nnc55_effect_locality_self_tests() {
  script="$1"
  temporary="$2"
  nnc55_fail=0

  for mutation in \
    core-dev core-feature core-no-default serde-no-default tokio windows-networking; do
    output="${temporary}/dependency-contract-${mutation}.out"
    if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
      NIMBUS_NETWORK_VERIFY_TEST_DEPENDENCY_CONTRACT_CASE="${mutation}" \
      "${script}" >"${output}" 2>&1; then
      printf 'SELFTEST FAIL dependency mutation %s unexpectedly exited zero\n' "${mutation}"
      nnc55_fail=$((nnc55_fail + 1))
    elif ! grep -q '^FAIL NNCV004 network-dependency-contract' "${output}" ||
      grep -q '^PASS NNCV004 network-dependency-contract' "${output}" ||
      [ "$(grep -c '^FAIL ' "${output}")" -ne 1 ]; then
      printf 'SELFTEST FAIL dependency mutation %s did not fail exclusively as NNCV004\n' "${mutation}"
      nnc55_fail=$((nnc55_fail + 1))
    else
      printf 'SELFTEST PASS dependency mutation %s fails closed as NNCV004\n' "${mutation}"
    fi
  done

  nnc12_names=(
    grouped-tcp-connect-timeout
    grouped-command
    mount
    umount2
    upper-crate-import
    portable-forwarding-trait
  )
  nnc12_fixtures=(
    $'use std::net::{SocketAddr, TcpStream};\nfn probe(address: SocketAddr) { let _ = TcpStream::connect_timeout(&address, std::time::Duration::from_secs(1)); }\n'
    $'use std::process::{Command, Stdio};\nfn apply() { let _ = Command::new("nsenter").arg("nft").stdout(Stdio::piped()); }\n'
    $'fn mount_effect() { unsafe { libc::mount(std::ptr::null(), std::ptr::null(), std::ptr::null(), 0, std::ptr::null()); } }\n'
    $'fn unmount_effect() { unsafe { libc::umount2(std::ptr::null(), 0); } }\n'
    $'use nimbus_sandbox::backends::oci::network::OciNetworkProcess;\n'
    $'pub trait ForwardingProvider { fn expose(&self); fn withdraw(&self); }\n'
  )
  for index in "${!nnc12_names[@]}"; do
    mutation="${nnc12_names[${index}]}"
    output="${temporary}/source-boundary-${mutation}.out"
    if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
      NIMBUS_NETWORK_VERIFY_TEST_FORBIDDEN_EFFECT="${nnc12_fixtures[${index}]}" \
      "${script}" >"${output}" 2>&1; then
      printf 'SELFTEST FAIL source-boundary mutation %s unexpectedly exited zero\n' "${mutation}"
      nnc55_fail=$((nnc55_fail + 1))
    elif ! grep -q '^FAIL NNCV012 forbidden-network-dependencies-effects' "${output}" ||
      grep -q '^PASS NNCV012 forbidden-network-dependencies-effects' "${output}" ||
      [ "$(grep -c '^FAIL ' "${output}")" -ne 1 ]; then
      printf 'SELFTEST FAIL source-boundary mutation %s did not fail exclusively as NNCV012\n' "${mutation}"
      nnc55_fail=$((nnc55_fail + 1))
    else
      printf 'SELFTEST PASS source-boundary mutation %s fails closed as NNCV012\n' "${mutation}"
    fi
  done

  nnc22_names=(
    moved-namespace-syscall
    moved-prepared-netavark-call
    second-readiness-socket
    moved-gvproxy-request
    moved-proxy-listener
    moved-proxy-target-connect
    moved-forwarding-mutation
    duplicate-readiness-provider
    command-outside-owner
  )
  nnc22_fixtures=(
    $'fn moved_namespace() { unsafe { libc::mount(std::ptr::null(), std::ptr::null(), std::ptr::null(), 0, std::ptr::null()); } }\n'
    $'fn moved_provider(ipam: &OciIpamAuthority, operation: &OciNetavarkOperation) { let _ = prepare_container_network_setup(ipam, operation); }\n'
    $'fn moved_probe(address: std::net::SocketAddr, timeout: std::time::Duration) { let _ = TcpStream::connect_timeout(&address, timeout); }\n'
    $'fn moved_gvproxy() { send_machine_forwarder_request(provider, "POST", "/expose", request, deadline); }\n'
    $'fn moved_proxy(guest_listener_addr: std::net::SocketAddr) { let _ = TcpListener::bind(guest_listener_addr); }\n'
    $'fn moved_proxy_connect(target_addr: std::net::SocketAddr) { let _ = TcpStream::connect_timeout(&target_addr, MACHINE_PORT_PROXY_CONNECT_TIMEOUT); }\n'
    $'fn moved_mutation(provider: &dyn MachinePortForwardingProvider, binding: &SandboxPortBinding) { let _ = provider.expose_one(binding); }\n'
    $'trait ReadinessProbeProvider {}\nimpl ReadinessProbeProvider for SocketReadinessProbeProvider {}\n'
    $'fn moved_command() { let _ = Command::new("nsenter").arg("nft"); }\n'
  )
  nnc22_paths=(
    "" "" "" "" "" "" "" ""
    "crates/nimbus-sandbox/src/backends/container/runtime/status.rs"
  )
  for index in "${!nnc22_names[@]}"; do
    mutation="${nnc22_names[${index}]}"
    output="${temporary}/effect-locality-${mutation}.out"
    if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
      NIMBUS_NETWORK_VERIFY_TEST_SANDBOX_EFFECT_LOCALITY="${nnc22_fixtures[${index}]}" \
      NIMBUS_NETWORK_VERIFY_TEST_SANDBOX_EFFECT_LOCALITY_PATH="${nnc22_paths[${index}]}" \
      "${script}" >"${output}" 2>&1; then
      printf 'SELFTEST FAIL effect-locality mutation %s unexpectedly exited zero\n' "${mutation}"
      nnc55_fail=$((nnc55_fail + 1))
    elif ! grep -q '^FAIL NNCV022 sandbox-provider-effect-locality' "${output}" ||
      grep -q '^PASS NNCV022 sandbox-provider-effect-locality' "${output}" ||
      [ "$(grep -c '^FAIL ' "${output}")" -ne 1 ]; then
      printf 'SELFTEST FAIL effect-locality mutation %s did not fail exclusively as NNCV022\n' "${mutation}"
      nnc55_fail=$((nnc55_fail + 1))
    else
      printf 'SELFTEST PASS effect-locality mutation %s fails closed as NNCV022\n' "${mutation}"
    fi
  done

  nnc23_names=(
    widened-host-effects
    namespace-reexport
    readiness-apply-authority
    readiness-wrapper-apply-authority
    public-forwarding-provider
  )
  nnc23_fixtures=(
    $'pub(crate) trait AttachmentHostEffects {}\n'
    $'pub(crate) use netns::create_persistent_network_namespace;\n'
    $'fn inspect(pin: &dyn OciEgressPinProvider) {}\n'
    $'fn inspect_wrapper(pin: &dyn OciEgressPinProvider) { let _ = pin; }\n'
    $'pub trait MachinePortForwardingProvider {}\n'
  )
  nnc23_paths=(
    "" "" ""
    "crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle.rs"
    ""
  )
  for index in "${!nnc23_names[@]}"; do
    mutation="${nnc23_names[${index}]}"
    output="${temporary}/capability-seal-${mutation}.out"
    if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
      NIMBUS_NETWORK_VERIFY_TEST_SEALED_EFFECT_CAPABILITY="${nnc23_fixtures[${index}]}" \
      NIMBUS_NETWORK_VERIFY_TEST_SEALED_EFFECT_CAPABILITY_PATH="${nnc23_paths[${index}]}" \
      "${script}" >"${output}" 2>&1; then
      printf 'SELFTEST FAIL capability mutation %s unexpectedly exited zero\n' "${mutation}"
      nnc55_fail=$((nnc55_fail + 1))
    elif ! grep -q '^FAIL NNCV023 sealed-provider-effect-capabilities' "${output}" ||
      grep -q '^PASS NNCV023 sealed-provider-effect-capabilities' "${output}" ||
      [ "$(grep -c '^FAIL ' "${output}")" -ne 1 ]; then
      printf 'SELFTEST FAIL capability mutation %s did not fail exclusively as NNCV023\n' "${mutation}"
      nnc55_fail=$((nnc55_fail + 1))
    else
      printf 'SELFTEST PASS capability mutation %s fails closed as NNCV023\n' "${mutation}"
    fi
  done

  output="${temporary}/capability-seal-portable-effect.out"
  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_PORTABLE_EFFECT_CAPABILITY=$'pub trait NetworkProvider { fn effect_callback(&self); }\n' \
    "${script}" >"${output}" 2>&1; then
    printf 'SELFTEST FAIL portable effect capability unexpectedly exited zero\n'
    nnc55_fail=$((nnc55_fail + 1))
  elif ! grep -q '^FAIL NNCV023 sealed-provider-effect-capabilities' "${output}" ||
    grep -q '^PASS NNCV023 sealed-provider-effect-capabilities' "${output}" ||
    [ "$(grep -c '^FAIL ' "${output}")" -ne 1 ]; then
    printf 'SELFTEST FAIL portable effect capability did not fail exclusively as NNCV023\n'
    nnc55_fail=$((nnc55_fail + 1))
  else
    printf 'SELFTEST PASS portable effect capability fails closed as NNCV023\n'
  fi

  return "${nnc55_fail}"
}
