# Aggregate mutation and affected-slice self-tests for the network control-plane verifier.
# This file is sourced by verify-nimbus-network-control-plane.sh.
# shellcheck shell=bash

run_nnc61e_recovery_decision_self_tests() {
  script="$1"
  temporary="$2"
  nnc61e_fail=0
  nnc61e_mutations=(
    missing-tenant-cursor
    missing-cursor-ordering
    missing-quiescent-matrix
    missing-selector
    missing-action-row
    missing-cleanup-retention
    missing-successor-promotion
    missing-bounded-reader
    missing-kill-reap-proof
    snapshot-handoff
  )

  for mutation in "${nnc61e_mutations[@]}"; do
    fixture="${temporary}/nnc61e-${mutation}"
    mkdir -p "${fixture}"
    cp crates/nimbus-workloads/src/store.rs "${fixture}/store.rs"
    cp crates/nimbus-compute/src/workload_saga.rs "${fixture}/compute-root.rs"
    cp crates/nimbus-compute/src/workload_saga/recovery.rs "${fixture}/compute.rs"
    cp crates/nimbus-workloads/src/saga/state/teardown.rs "${fixture}/teardown.rs"
    cp crates/nimbus-server/src/workload_saga_store/tenant_enumeration.rs \
      "${fixture}/tenant-adapter.rs"
    cp crates/nimbus-server/src/workload_saga_store/tests/composition.rs \
      "${fixture}/process.rs"
    cp crates/nimbus-server/src/workload_saga_store/tests/recovery.rs \
      "${fixture}/matrix.rs"

    if ! node - "${mutation}" "${fixture}" <<'NODE'
const fs = require("fs");
const [mutation, root] = process.argv.slice(2);

function replaceOne(name, before, after) {
  const path = `${root}/${name}`;
  const source = fs.readFileSync(path, "utf8");
  const index = source.indexOf(before);
  if (index < 0) throw new Error(`${mutation}: missing mutation anchor ${before}`);
  fs.writeFileSync(path, source.slice(0, index) + after + source.slice(index + before.length));
}

switch (mutation) {
  case "missing-tenant-cursor":
    replaceOne("store.rs", "pub struct WorkloadSagaTenantCursor", "pub struct MissingWorkloadSagaTenantCursor");
    break;
  case "missing-cursor-ordering":
    replaceOne(
      "store.rs",
      "workload saga tenant page is duplicated, identity-unsorted, or cursor-regressing",
      "workload saga tenant page ordering guard removed",
    );
    break;
  case "missing-quiescent-matrix":
    {
      const path = `${root}/matrix.rs`;
      const source = fs.readFileSync(path, "utf8");
      if (!source.includes("process-recorded-quiescent")) {
        throw new Error(`${mutation}: missing quiescent matrix anchor`);
      }
      fs.writeFileSync(path, source.replaceAll("process-recorded-quiescent", "process-recorded-omitted"));
    }
    break;
  case "missing-selector":
    replaceOne("compute.rs", "pub enum WorkloadSagaAction", "pub enum MissingWorkloadSagaAction");
    break;
  case "missing-action-row":
    replaceOne(
      "compute.rs",
      "WorkloadSagaAction::Teardown(decision)",
      "WorkloadSagaAction::OmittedTeardown(decision)",
    );
    break;
  case "missing-cleanup-retention":
    replaceOne(
      "teardown.rs",
      "WorkloadTeardownDecision::CleanupPending {",
      "WorkloadTeardownDecision::MissingCleanupPending {",
    );
    break;
  case "missing-successor-promotion":
    replaceOne("compute.rs", "WorkloadSagaAction::PromoteSuccessor", "WorkloadSagaAction::OmittedSuccessorPromotion");
    break;
  case "missing-bounded-reader":
    replaceOne("compute.rs", "plan_recoverable_page", "omitted_recoverable_page");
    break;
  case "missing-kill-reap-proof":
    replaceOne("process.rs", "killed-at-boundary-and-reaped", "unbounded-child-cleanup");
    break;
  case "snapshot-handoff":
    fs.appendFileSync(`${root}/process.rs`, "\nconst SNAPSHOT_HANDOFF_PAYLOAD: &[u8] = b\"forbidden\";\n");
    break;
  default:
    throw new Error(`unknown NNC6.1e mutation ${mutation}`);
}
NODE
    then
      printf 'SELFTEST FAIL NNC6.1e mutation fixture %s could not be built\n' "${mutation}"
      nnc61e_fail=$((nnc61e_fail + 1))
      continue
    fi

    output="${temporary}/nnc61e-${mutation}.out"
    if NIMBUS_NETWORK_NNC65_AGGREGATE_SELF_TEST_BASELINE=1 \
      NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
      NIMBUS_NETWORK_VERIFY_RECOVERY_STORE_SOURCE="${fixture}/store.rs" \
      NIMBUS_NETWORK_VERIFY_RECOVERY_COMPUTE_ROOT_SOURCE="${fixture}/compute-root.rs" \
      NIMBUS_NETWORK_VERIFY_RECOVERY_COMPUTE_SOURCE="${fixture}/compute.rs" \
      NIMBUS_NETWORK_VERIFY_RECOVERY_TEARDOWN_SOURCE="${fixture}/teardown.rs" \
      NIMBUS_NETWORK_VERIFY_RECOVERY_TENANT_ADAPTER_SOURCE="${fixture}/tenant-adapter.rs" \
      NIMBUS_NETWORK_VERIFY_RECOVERY_PROCESS_SOURCE="${fixture}/process.rs" \
      NIMBUS_NETWORK_VERIFY_RECOVERY_MATRIX_SOURCE="${fixture}/matrix.rs" \
      "${script}" >"${output}" 2>&1; then
      printf 'SELFTEST FAIL NNC6.1e mutation %s unexpectedly exited zero\n' "${mutation}"
      nnc61e_fail=$((nnc61e_fail + 1))
    elif ! grep -q '^FAIL NNCV027 durable-workload-saga-authority' "${output}" ||
      grep -q '^PASS NNCV027 durable-workload-saga-authority' "${output}" ||
      [ "$(grep -c '^FAIL NNCV' "${output}")" -ne 1 ]; then
      printf 'SELFTEST FAIL NNC6.1e mutation %s did not fail exclusively as NNCV027\n' "${mutation}"
      nnc61e_fail=$((nnc61e_fail + 1))
    else
      printf 'SELFTEST PASS NNC6.1e mutation %s fails closed as NNCV027\n' "${mutation}"
    fi
  done

  return "${nnc61e_fail}"
}

run_nnc81_process_harness_self_tests() {
  script="$1"
  temporary="$2"
  baseline="${temporary}/nnc81-metadata.json"
  nnc81_fail=0

  if ! cargo metadata --no-deps --format-version 1 >"${baseline}" 2>/dev/null; then
    printf 'SELFTEST FAIL NNCV036 baseline metadata could not be generated\n'
    return 1
  fi

  for mutation in extra-dev-dependency build-nimbus-dependency normal-runtime-dependency; do
    fixture="${temporary}/nnc81-${mutation}.json"
    if ! node - "${mutation}" "${baseline}" "${fixture}" <<'NODE'
const fs = require("fs");
const [mutation, baselinePath, fixturePath] = process.argv.slice(2);
const metadata = JSON.parse(fs.readFileSync(baselinePath, "utf8"));
const owner = metadata.packages.find(pkg => pkg.name === "nimbus-process-harness");
if (!owner) throw new Error("missing nimbus-process-harness metadata owner");

const edge = {
  name: "serde",
  source: "registry+self-test",
  req: "*",
  kind: "dev",
  rename: null,
  optional: false,
  uses_default_features: true,
  features: [],
  target: null,
  registry: null,
  path: null,
};
if (mutation === "build-nimbus-dependency") {
  edge.name = "nimbus-core";
  edge.source = null;
  edge.kind = "build";
} else if (mutation === "normal-runtime-dependency") {
  edge.name = "nimbus-runtime";
  edge.source = null;
  edge.kind = null;
} else if (mutation !== "extra-dev-dependency") {
  throw new Error(`unknown NNC8.1 mutation ${mutation}`);
}
owner.dependencies.push(edge);
fs.writeFileSync(fixturePath, JSON.stringify(metadata));
NODE
    then
      printf 'SELFTEST FAIL NNCV036 mutation fixture %s could not be built\n' "${mutation}"
      nnc81_fail=$((nnc81_fail + 1))
      continue
    fi

    output="${temporary}/nnc81-${mutation}.out"
    if NIMBUS_NETWORK_NNC63B_AGGREGATE_SELF_TEST_BASELINE=1 \
      NIMBUS_NETWORK_NNC64_AGGREGATE_SELF_TEST_BASELINE=1 \
      NIMBUS_NETWORK_NNC64A_AGGREGATE_SELF_TEST_BASELINE=1 \
      NIMBUS_NETWORK_NNC65_AGGREGATE_SELF_TEST_BASELINE=1 \
      NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
      NIMBUS_NETWORK_VERIFY_NNC81_METADATA="${fixture}" \
      "${script}" >"${output}" 2>&1; then
      printf 'SELFTEST FAIL NNCV036 mutation %s unexpectedly exited zero\n' "${mutation}"
      nnc81_fail=$((nnc81_fail + 1))
    elif ! grep -q '^FAIL NNCV036 shared-process-harness-owner' "${output}" ||
      grep -q '^PASS NNCV036 shared-process-harness-owner' "${output}" ||
      [ "$(grep -c '^FAIL NNCV' "${output}")" -ne 1 ] ||
      ! grep -q '^Summary: 37 passed, 1 failed$' "${output}"; then
      printf 'SELFTEST FAIL NNCV036 mutation %s did not fail exclusively\n' "${mutation}"
      nnc81_fail=$((nnc81_fail + 1))
    else
      printf 'SELFTEST PASS NNCV036 mutation %s fails closed exclusively\n' "${mutation}"
    fi
  done

  return "${nnc81_fail}"
}

run_self_test() {
  temporary="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-network-verifier-self-test.XXXXXX")" || {
    printf 'SELFTEST FAIL unable to create temporary directory\n'
    exit 1
  }
  trap 'rm -rf "${temporary}"' EXIT
  script="${REPO_ROOT}/scripts/verify-nimbus-network-control-plane.sh"
  self_fail=0

  run_parallel_self_test_batch() {
    launched_lanes=""
    for lane_spec in "$@"; do
      IFS='|' read -r lane_name lane_function missing_label <<<"${lane_spec}"
      if ! declare -F "${lane_function}" >/dev/null 2>&1; then
        printf 'SELFTEST FAIL %s contract helper is missing\n' "${missing_label}"
        self_fail=$((self_fail + 1))
        continue
      fi
      launched_lanes="${launched_lanes}${launched_lanes:+ }${lane_name}"
      (
        "${lane_function}" "${script}" "${temporary}"
        lane_status=$?
        printf '%d\n' "${lane_status}" >"${temporary}/${lane_name}.status"
      ) >"${temporary}/${lane_name}.out" 2>&1 &
    done

    wait
    for lane_name in ${launched_lanes}; do
      if [ ! -f "${temporary}/${lane_name}.status" ]; then
        printf 'SELFTEST FAIL %s lane did not record its result\n' "${lane_name}"
        self_fail=$((self_fail + 1))
        continue
      fi
      cat "${temporary}/${lane_name}.out"
      lane_status="$(cat "${temporary}/${lane_name}.status")"
      case "${lane_status}" in
        '' | *[!0-9]*)
          printf 'SELFTEST FAIL %s lane recorded an invalid result\n' "${lane_name}"
          self_fail=$((self_fail + 1))
          ;;
        *) self_fail=$((self_fail + lane_status)) ;;
      esac
    done
  }

  if NIMBUS_NETWORK_VERIFY_PLAN="${temporary}/missing-plan.md" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/missing-plan.out" 2>&1; then
    printf 'SELFTEST FAIL missing plan unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV001 plan-in-HEAD' "${temporary}/missing-plan.out" ||
    grep -q '^PASS NNCV001 plan-in-HEAD' "${temporary}/missing-plan.out"; then
    printf 'SELFTEST FAIL missing plan did not produce an exclusive NNCV001 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS missing plan fails closed as NNCV001\n'
  fi

  if NIMBUS_NETWORK_VERIFY_INVENTORY="${temporary}/missing-inventory.json" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/missing-inventory.out" 2>&1; then
    printf 'SELFTEST FAIL missing inventory unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV002 required-baseline-inputs' "${temporary}/missing-inventory.out" ||
    grep -q '^PASS NNCV002 required-baseline-inputs' "${temporary}/missing-inventory.out"; then
    printf 'SELFTEST FAIL missing inventory did not produce an exclusive NNCV002 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS missing inventory fails closed as NNCV002\n'
  fi

  if NIMBUS_NETWORK_VERIFY_DEPENDENCIES="${temporary}/missing-dependencies.json" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/missing-dependencies.out" 2>&1; then
    printf 'SELFTEST FAIL missing dependency baseline unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV002 required-baseline-inputs' "${temporary}/missing-dependencies.out" ||
    grep -q '^PASS NNCV002 required-baseline-inputs' "${temporary}/missing-dependencies.out"; then
    printf 'SELFTEST FAIL missing dependency baseline did not produce an exclusive NNCV002 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS missing dependency baseline fails closed as NNCV002\n'
  fi

  invalid_checkpoint_plan="${temporary}/invalid-checkpoint-plan.md"
  if ! node - "${PLAN}" "${invalid_checkpoint_plan}" <<'NODE'
const fs = require("fs");
const [source, target] = process.argv.slice(2);
const text = fs.readFileSync(source, "utf8");
const invalid = text.replace(
  /^(\| Last checkpoint commit \|.*?`)[0-9a-f]{40}(`.*\|)$/m,
  "$1" + "0".repeat(40) + "$2",
);
if (invalid === text) process.exit(1);
fs.writeFileSync(target, invalid);
NODE
  then
    printf 'SELFTEST FAIL nonexistent checkpoint fixture could not be built\n'
    self_fail=$((self_fail + 1))
  elif NIMBUS_NETWORK_VERIFY_PLAN="${invalid_checkpoint_plan}" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/invalid-checkpoint.out" 2>&1; then
    printf 'SELFTEST FAIL nonexistent checkpoint unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV008 checkpoint-ledger-recoverable' "${temporary}/invalid-checkpoint.out" ||
    grep -q '^PASS NNCV008 checkpoint-ledger-recoverable' "${temporary}/invalid-checkpoint.out" ||
    ! grep -q 'Last checkpoint commit does not resolve: 0000000000000000000000000000000000000000' "${temporary}/invalid-checkpoint.out"; then
    printf 'SELFTEST FAIL nonexistent checkpoint did not produce the exact exclusive NNCV008 diagnostic\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS nonexistent checkpoint fails closed as NNCV008\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_LEGACY_PORT_AUTHORITY='synthetic.rs:1:struct PortManager' \
    "${script}" >"${temporary}/legacy-port-authority.out" 2>&1; then
    printf 'SELFTEST FAIL injected legacy port authority unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV005 no-duplicate-port-allocation-authority' "${temporary}/legacy-port-authority.out" ||
    grep -q '^PASS NNCV005 no-duplicate-port-allocation-authority' "${temporary}/legacy-port-authority.out" ||
    [ "$(grep -c '^FAIL ' "${temporary}/legacy-port-authority.out")" -ne 1 ] ||
    ! grep -q '^Summary: 37 passed, 1 failed$' "${temporary}/legacy-port-authority.out"; then
    printf 'SELFTEST FAIL legacy port authority did not produce an exclusive NNCV005 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS legacy port authority fails closed as NNCV005\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_UNCLASSIFIED="synthetic-unclassified.rs:1:TcpListener::bind" \
    "${script}" >"${temporary}/unclassified.out" 2>&1; then
    printf 'SELFTEST FAIL injected unclassified bind unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/unclassified.out" ||
    grep -q '^PASS NNCV006 unclassified-production-bind' "${temporary}/unclassified.out"; then
    printf 'SELFTEST FAIL injected bind did not produce an exclusive NNCV006 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS injected bind fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'#[cfg(test)]\nuse std::net::TcpListener;\nfn production_authority() { TcpListener::bind("127.0.0.1:0"); }\n' \
    "${script}" >"${temporary}/production-after-test-cfg.out" 2>&1; then
    printf 'SELFTEST FAIL production bind after test-only item unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/production-after-test-cfg.out" ||
    grep -q '^PASS NNCV006 unclassified-production-bind' "${temporary}/production-after-test-cfg.out"; then
    printf 'SELFTEST FAIL production bind after test-only item escaped NNCV006\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS production bind after test-only item fails as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'struct Fixture {\n    #[cfg(test)]\n    test_only: bool,\n}\nimpl Fixture {\n    fn production_authority() { TcpListener::bind("127.0.0.1:0"); }\n}\n' \
    "${script}" >"${temporary}/production-after-test-field.out" 2>&1; then
    printf 'SELFTEST FAIL production bind after cfg(test) field unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/production-after-test-field.out" ||
    grep -q '^PASS NNCV006 unclassified-production-bind' "${temporary}/production-after-test-field.out"; then
    printf 'SELFTEST FAIL cfg(test) field hid a later production bind from NNCV006\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS cfg(test) field cannot hide a later production bind from NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn production_authority(\n    #[cfg(test)]\n    test_only: bool\n) { TcpListener::bind("127.0.0.1:0"); }\n' \
    "${script}" >"${temporary}/production-after-test-parameter.out" 2>&1; then
    printf 'SELFTEST FAIL production bind after cfg(test) parameter unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/production-after-test-parameter.out" ||
    grep -q '^PASS NNCV006 unclassified-production-bind' "${temporary}/production-after-test-parameter.out"; then
    printf 'SELFTEST FAIL cfg(test) parameter hid its production function from NNCV006\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS cfg(test) parameter cannot hide its production function from NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'#[cfg(test)]\nmacro_rules! listener_fixture { () => {}; }\nfn production_authority() { TcpListener::bind("127.0.0.1:0"); }\n' \
    "${script}" >"${temporary}/production-after-test-macro.out" 2>&1; then
    printf 'SELFTEST FAIL production bind after cfg(test) macro unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/production-after-test-macro.out" ||
    ! grep -q 'tcp-bind|production_authority' "${temporary}/production-after-test-macro.out"; then
    printf 'SELFTEST FAIL cfg(test) macro hid the following production bind from NNCV006\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS cfg(test) macro cannot hide the following production bind from NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'#[cfg(test)]\nfixture! {\n    fn hidden() { TcpListener::bind("127.0.0.1:0"); }\n}\nfn production_authority() { TcpListener::bind("127.0.0.1:0"); }\n' \
    "${script}" >"${temporary}/production-after-test-macro-invocation.out" 2>&1; then
    printf 'SELFTEST FAIL production bind after cfg(test) brace macro unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/production-after-test-macro-invocation.out" ||
    ! grep -q 'tcp-bind|production_authority' "${temporary}/production-after-test-macro-invocation.out" ||
    grep -q 'tcp-bind|hidden' "${temporary}/production-after-test-macro-invocation.out"; then
    printf 'SELFTEST FAIL cfg(test) brace macro hid or leaked authority across its AST item boundary\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS cfg(test) brace macro cannot hide the following production bind from NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'use std::net::TcpListener as Listener;\nfn production_authority() { Listener::bind("127.0.0.1:0"); }\n' \
    "${script}" >"${temporary}/aliased-listener-bind.out" 2>&1; then
    printf 'SELFTEST FAIL aliased listener bind unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/aliased-listener-bind.out" ||
    ! grep -q 'ambiguous socket authority alias is forbidden:' "${temporary}/aliased-listener-bind.out"; then
    printf 'SELFTEST FAIL aliased listener bind did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS aliased listener bind fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'type Listener = TcpListener;\nfn production_authority() { Listener::bind("127.0.0.1:0"); }\n' \
    "${script}" >"${temporary}/type-aliased-listener-bind.out" 2>&1; then
    printf 'SELFTEST FAIL type-aliased listener bind unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/type-aliased-listener-bind.out" ||
    ! grep -q 'socket authority type alias is forbidden:' "${temporary}/type-aliased-listener-bind.out"; then
    printf 'SELFTEST FAIL type-aliased listener bind did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS type-aliased listener bind fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn production_authority() { unsafe { UdpSocket::from_raw_socket(1); } }\n' \
    "${script}" >"${temporary}/udp-raw-socket-adoption.out" 2>&1; then
    printf 'SELFTEST FAIL UDP raw-socket adoption unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/udp-raw-socket-adoption.out" ||
    ! grep -q 'udp-from-raw-socket' "${temporary}/udp-raw-socket-adoption.out"; then
    printf 'SELFTEST FAIL UDP raw-socket adoption did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS UDP raw-socket adoption fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn production_authority(listener: &Socket) { listener.bind("127.0.0.1:0"); }\n' \
    "${script}" >"${temporary}/instance-listener-bind.out" 2>&1; then
    printf 'SELFTEST FAIL instance listener bind unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/instance-listener-bind.out" ||
    ! grep -q 'unclassified ambiguous bind/adoption operation:' "${temporary}/instance-listener-bind.out"; then
    printf 'SELFTEST FAIL instance listener bind did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS instance listener bind fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn production_authority(sockets: &[Socket], index: usize) { sockets[index].bind("127.0.0.1:0"); }\n' \
    "${script}" >"${temporary}/postfix-listener-bind.out" 2>&1; then
    printf 'SELFTEST FAIL postfix listener bind unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/postfix-listener-bind.out" ||
    ! grep -q 'ambiguous-instance-bind' "${temporary}/postfix-listener-bind.out"; then
    printf 'SELFTEST FAIL postfix listener bind did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS postfix listener bind fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn production_authority() { UnixDatagram::bind("/tmp/nimbus-verifier.sock"); }\n' \
    "${script}" >"${temporary}/unix-datagram-bind.out" 2>&1; then
    printf 'SELFTEST FAIL Unix datagram bind unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/unix-datagram-bind.out" ||
    ! grep -q 'unix-datagram-bind|production_authority' "${temporary}/unix-datagram-bind.out"; then
    printf 'SELFTEST FAIL Unix datagram bind did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS Unix datagram bind fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'struct Held(pub TcpListener);\n' \
    "${script}" >"${temporary}/tuple-listener-ownership.out" 2>&1; then
    printf 'SELFTEST FAIL tuple listener ownership unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/tuple-listener-ownership.out" ||
    ! grep -q 'listener-ownership-slot|<module>' "${temporary}/tuple-listener-ownership.out"; then
    printf 'SELFTEST FAIL tuple listener ownership did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS tuple listener ownership fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn handoff()\n    -> Result<\n        TcpListener,\n        std::io::Error,\n    >\n{\n    unreachable!()\n}\n' \
    "${script}" >"${temporary}/multiline-listener-return.out" 2>&1; then
    printf 'SELFTEST FAIL multiline listener return unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/multiline-listener-return.out" ||
    ! grep -q 'listener-return-handoff|handoff' "${temporary}/multiline-listener-return.out"; then
    printf 'SELFTEST FAIL multiline listener return did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS multiline listener return fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'#[path = "tests/fixture.rs"]\nmod fixture;\n' \
    "${script}" >"${temporary}/production-test-path-inclusion.out" 2>&1; then
    printf 'SELFTEST FAIL production inclusion of a test-exempt path unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/production-test-path-inclusion.out" ||
    ! grep -q 'production module/include references test-exempt source:' "${temporary}/production-test-path-inclusion.out"; then
    printf 'SELFTEST FAIL production inclusion of a test-exempt path did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS production inclusion of a test-exempt path fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn indirect() { let bind_listener = TcpListener::bind; let _ = bind_listener("127.0.0.1:0"); }\n' \
    "${script}" >"${temporary}/bind-function-value.out" 2>&1; then
    printf 'SELFTEST FAIL bind function value unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/bind-function-value.out" ||
    ! grep -q 'tcp-bind|indirect' "${temporary}/bind-function-value.out"; then
    printf 'SELFTEST FAIL bind function value did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS bind function value fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'use libc::bind;\nfn bare() { unsafe { bind(0, std::ptr::null(), 0); } }\n' \
    "${script}" >"${temporary}/bare-bind-import.out" 2>&1; then
    printf 'SELFTEST FAIL bare imported bind unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/bare-bind-import.out" ||
    ! grep -q 'ambiguous-bind-function-import' "${temporary}/bare-bind-import.out" ||
    ! grep -q 'ambiguous-bare-bind-call' "${temporary}/bare-bind-import.out"; then
    printf 'SELFTEST FAIL bare imported bind did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS bare imported bind fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'trait Kind { type Listener; }\nstruct Host;\nimpl Kind for Host { type Listener = TcpListener; }\n' \
    "${script}" >"${temporary}/associated-listener-alias.out" 2>&1; then
    printf 'SELFTEST FAIL associated listener alias unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/associated-listener-alias.out" ||
    ! grep -q 'associated socket authority type alias is forbidden:' "${temporary}/associated-listener-alias.out"; then
    printf 'SELFTEST FAIL associated listener alias did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS associated listener alias fails closed as NNCV006\n'
  fi

  NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'struct Fixture;\nimpl Fixture {\n    #[cfg(test)]\n    fixture! { TcpListener host_port }\n}\nfn production_without_bind() {\n    #[cfg(test)]\n    fixture! { TcpListener host_port }\n    let _ = match 1 {\n        #[cfg(test)]\n        0 => TcpListener::bind("127.0.0.1:0"),\n        _ => 1,\n    };\n}\n' \
    "${script}" >"${temporary}/cfg-associated-statement-nodes.out" 2>&1 || true
  if ! grep -q '^PASS NNCV006 unclassified-production-bind' "${temporary}/cfg-associated-statement-nodes.out" ||
    grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/cfg-associated-statement-nodes.out"; then
    printf 'SELFTEST FAIL cfg(test) associated/statement nodes were misclassified as production\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS cfg(test) associated/statement nodes remain test-only for NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn consume_prebound(listener: TcpListener) { drop(listener); }\n' \
    "${script}" >"${temporary}/prebound-listener-consumer.out" 2>&1; then
    printf 'SELFTEST FAIL pre-bound listener consumer unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/prebound-listener-consumer.out" ||
    ! grep -q 'listener-ownership-slot|consume_prebound' "${temporary}/prebound-listener-consumer.out"; then
    printf 'SELFTEST FAIL pre-bound listener consumer did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS pre-bound listener consumer fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn find_available_listener_port() -> u16 { 7000 }\n' \
    "${script}" >"${temporary}/suspicious-port-allocation.out" 2>&1; then
    printf 'SELFTEST FAIL suspicious port allocator unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/suspicious-port-allocation.out" ||
    ! grep -q 'suspicious-port-allocation-definition' "${temporary}/suspicious-port-allocation.out"; then
    printf 'SELFTEST FAIL suspicious port allocator did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS suspicious port allocator fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn reserve_port() -> u16 { 7000 }\n' \
    "${script}" >"${temporary}/reserve-port-allocation.out" 2>&1; then
    printf 'SELFTEST FAIL reserve-port allocator unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/reserve-port-allocation.out" ||
    ! grep -q 'suspicious-port-allocation-definition|reserve_port' "${temporary}/reserve-port-allocation.out"; then
    printf 'SELFTEST FAIL reserve-port allocator did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS reserve-port allocator fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn configure_gvproxy() { let args = ["-ssh-port"]; }\n' \
    "${script}" >"${temporary}/provider-port-request.out" 2>&1; then
    printf 'SELFTEST FAIL provider port request unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/provider-port-request.out" ||
    ! grep -q 'gvproxy-ssh-port-request' "${temporary}/provider-port-request.out"; then
    printf 'SELFTEST FAIL provider port request did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS provider port request fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn configure_provider() {\n    let generic = ProviderRequest { host_port: 7000 };\n    let forward = crate::MachinePortForwardRequest { local: "127.0.0.1:7000" };\n    let netavark = crate::NetavarkRequest { port_mappings: mappings };\n}\n' \
    "${script}" >"${temporary}/generic-provider-port-request.out" 2>&1; then
    printf 'SELFTEST FAIL generic provider port request unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/generic-provider-port-request.out" ||
    ! grep -q 'provider-port-request|configure_provider' "${temporary}/generic-provider-port-request.out" ||
    ! grep -q 'machine-forwarder-port-request|configure_provider' "${temporary}/generic-provider-port-request.out" ||
    ! grep -q 'netavark-port-mapping-request|configure_provider' "${temporary}/generic-provider-port-request.out"; then
    printf 'SELFTEST FAIL generic provider port request did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS generic provider port request fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn configure_provider(host_port: u16) { let request = ProviderRequest { host_port }; }\n' \
    "${script}" >"${temporary}/shorthand-provider-port-request.out" 2>&1; then
    printf 'SELFTEST FAIL shorthand provider port request unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/shorthand-provider-port-request.out" ||
    ! grep -q 'provider-port-request|configure_provider' "${temporary}/shorthand-provider-port-request.out"; then
    printf 'SELFTEST FAIL shorthand provider port request did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS shorthand provider port request fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_SWAP_SITE_IDS='cli-main-direct-listener,cli-main-systemd-listener' \
    "${script}" >"${temporary}/same-path-site-swap.out" 2>&1; then
    printf 'SELFTEST FAIL same-path site swap unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/same-path-site-swap.out" ||
    ! grep -Eq 'authority (kind|symbol) .* is invalid for site' "${temporary}/same-path-site-swap.out"; then
    printf 'SELFTEST FAIL same-path site swap did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS same-path site swap fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_CORRUPT_SITE_DECLARATION='sandbox-pep-port-allocation' \
    "${script}" >"${temporary}/stale-site-declaration.out" 2>&1; then
    printf 'SELFTEST FAIL stale site declaration unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/stale-site-declaration.out" ||
    ! grep -q 'active site declaration missing or stale: sandbox-pep-port-allocation' "${temporary}/stale-site-declaration.out"; then
    printf 'SELFTEST FAIL stale site declaration did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS stale site declaration fails closed as NNCV006\n'
  fi

  bind_exemption_self_fail=0
  if [ ! -f "${BIND_EXEMPTION_SELF_TESTS}" ]; then
    printf 'SELFTEST FAIL bind-exemption self-test helper is missing: %s\n' "${BIND_EXEMPTION_SELF_TESTS}"
    self_fail=$((self_fail + 1))
  else
    # shellcheck source=scripts/nimbus-network-control-plane/bind-exemption-self-tests.sh
    . "${BIND_EXEMPTION_SELF_TESTS}"
    # INVENTORY is set by the verifier before this module is sourced.
    # shellcheck disable=SC2153
    run_nnc46f_bind_exemption_self_tests "${script}" "${temporary}" "${INVENTORY}" ||
      bind_exemption_self_fail=$?
    self_fail=$((self_fail + bind_exemption_self_fail))
  fi

  NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_CLASSIFIED_OCCURRENCE="__nimbus_network_verifier_self_test__/cfg-test-followed-by-production.rs|tcp-bind|first_authority|1" \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn first_authority() { TcpListener::bind("127.0.0.1:0"); }\nfn second_authority() { TcpListener::bind("127.0.0.1:0"); }\n' \
    "${script}" >"${temporary}/second-bind-in-classified-file.out" 2>&1 || true
  if grep -q '^PASS NNCV006 unclassified-production-bind' "${temporary}/second-bind-in-classified-file.out" ||
    ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/second-bind-in-classified-file.out"; then
    printf 'SELFTEST FAIL second bind in a classified file escaped NNCV006\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS second bind in a classified file fails as NNCV006\n'
  fi

  NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_CLASSIFIED_OCCURRENCE="__nimbus_network_verifier_self_test__/cfg-test-followed-by-production.rs|tcp-bind|deleted_authority|1" \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'fn production_without_bind() {}\n' \
    "${script}" >"${temporary}/stale-bind-classification.out" 2>&1 || true
  if ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/stale-bind-classification.out" ||
    grep -q '^PASS NNCV006 unclassified-production-bind' "${temporary}/stale-bind-classification.out" ||
    ! grep -q 'stale authority occurrence classification: .*deleted_authority' "${temporary}/stale-bind-classification.out"; then
    printf 'SELFTEST FAIL stale bind classification did not fail NNCV006 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS stale bind classification fails as NNCV006\n'
  fi

  NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'#[cfg(test)]\nmod tests { fn listener_fixture() { TcpListener::bind("127.0.0.1:0"); } }\nfn production_without_bind() {}\n' \
    "${script}" >"${temporary}/test-module-bind.out" 2>&1 || true
  if ! grep -q '^PASS NNCV006 unclassified-production-bind' "${temporary}/test-module-bind.out" ||
    grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/test-module-bind.out"; then
    printf 'SELFTEST FAIL cfg(test) module bind was misclassified as production\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS cfg(test) module bind remains test-only for NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_CORE_SCAN_ROOT="${temporary}/missing-core-source" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/missing-core-source.out" 2>&1; then
    printf 'SELFTEST FAIL missing core source unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV010 core-runtime-foundation-invariants' "${temporary}/missing-core-source.out" ||
    grep -q '^PASS NNCV010 core-runtime-foundation-invariants' "${temporary}/missing-core-source.out"; then
    printf 'SELFTEST FAIL missing core source did not produce an exclusive NNCV010 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS missing core source fails closed as NNCV010\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_FORBIDDEN_DEPENDENCY='nimbus-tenant' \
    "${script}" >"${temporary}/forbidden-workspace-dependency.out" 2>&1; then
    printf 'SELFTEST FAIL injected upper workspace dependency unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-workspace-dependency.out" ||
    grep -q '^PASS NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-workspace-dependency.out"; then
    printf 'SELFTEST FAIL upper workspace dependency did not produce an exclusive NNCV012 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS upper workspace dependency fails closed as NNCV012\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_FORBIDDEN_DEPENDENCY='axum' \
    "${script}" >"${temporary}/forbidden-transport-dependency.out" 2>&1; then
    printf 'SELFTEST FAIL injected transport dependency unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-transport-dependency.out" ||
    grep -q '^PASS NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-transport-dependency.out"; then
    printf 'SELFTEST FAIL transport dependency did not produce an exclusive NNCV012 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS transport dependency fails closed as NNCV012\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_FORBIDDEN_DEPENDENCY='aws-sdk-ec2' \
    "${script}" >"${temporary}/forbidden-cloud-dependency.out" 2>&1; then
    printf 'SELFTEST FAIL injected cloud SDK dependency unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-cloud-dependency.out" ||
    grep -q '^PASS NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-cloud-dependency.out"; then
    printf 'SELFTEST FAIL cloud SDK dependency did not produce an exclusive NNCV012 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS cloud SDK dependency fails closed as NNCV012\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_FORBIDDEN_EFFECT='fn bind_effect() { std::net::TcpListener::bind("127.0.0.1:0"); }' \
    "${script}" >"${temporary}/forbidden-effect.out" 2>&1; then
    printf 'SELFTEST FAIL injected provider effect unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-effect.out" ||
    grep -q '^PASS NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-effect.out"; then
    printf 'SELFTEST FAIL injected provider effect did not produce an exclusive NNCV012 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS injected provider effect fails closed as NNCV012\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_DUPLICATE_DEFINITION='pub struct NetworkPlan;' \
    "${script}" >"${temporary}/duplicate-definition.out" 2>&1; then
    printf 'SELFTEST FAIL injected duplicate definition unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV013 single-network-definition-owner' "${temporary}/duplicate-definition.out" ||
    grep -q '^PASS NNCV013 single-network-definition-owner' "${temporary}/duplicate-definition.out"; then
    printf 'SELFTEST FAIL injected duplicate definition did not produce an exclusive NNCV013 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS injected duplicate definition fails closed as NNCV013\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_ADDRESS_IDENTITY='pub struct NetworkSegmentId(Cidr);' \
    "${script}" >"${temporary}/address-identity.out" 2>&1; then
    printf 'SELFTEST FAIL injected address identity unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV014 address-is-not-network-identity' "${temporary}/address-identity.out" ||
    grep -q '^PASS NNCV014 address-is-not-network-identity' "${temporary}/address-identity.out"; then
    printf 'SELFTEST FAIL injected address identity did not produce an exclusive NNCV014 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS injected address identity fails closed as NNCV014\n'
  fi

  NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_FORBIDDEN_EFFECT=$'// TcpListener::bind is documentation only.\nconst NOTE: &str = "std::net::TcpListener::bind";\nfn pure_contract() {}\n' \
    "${script}" >"${temporary}/non-code-effect-terms.out" 2>&1 || true
  if ! grep -q '^PASS NNCV012 forbidden-network-dependencies-effects' "${temporary}/non-code-effect-terms.out" ||
    grep -q '^FAIL NNCV012 forbidden-network-dependencies-effects' "${temporary}/non-code-effect-terms.out"; then
    printf 'SELFTEST FAIL comment/string provider terms were misclassified as effects\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS comment/string provider terms remain non-effects for NNCV012\n'
  fi

  if NIMBUS_NETWORK_VERIFY_NETWORK_SCAN_ROOT="${temporary}/missing-network-source" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/missing-network-source.out" 2>&1; then
    printf 'SELFTEST FAIL missing network source unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV012 forbidden-network-dependencies-effects' "${temporary}/missing-network-source.out" ||
    ! grep -q '^FAIL NNCV013 single-network-definition-owner' "${temporary}/missing-network-source.out" ||
    ! grep -q '^FAIL NNCV014 address-is-not-network-identity' "${temporary}/missing-network-source.out"; then
    printf 'SELFTEST FAIL missing network source did not fail all source-contract conditions\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS missing network source fails closed for NNCV012-NNCV014\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_CASE=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE='fn bad() { LocalPortLeaseAuthority::open("foreign"); }' \
    "${script}" >"${temporary}/composition-primitive-open.out" 2>&1; then
    printf 'SELFTEST FAIL unclassified primitive composition open unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV015 local-network-composition-census' "${temporary}/composition-primitive-open.out" ||
    grep -q '^PASS NNCV015 local-network-composition-census' "${temporary}/composition-primitive-open.out" ||
    ! grep -q 'primitive-port-authority-open|bad' "${temporary}/composition-primitive-open.out"; then
    printf 'SELFTEST FAIL primitive composition open did not fail NNCV015 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS primitive composition open fails closed as NNCV015\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_CASE=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE='fn bad(config: Config) { KrunSandboxBackend::new(config); }' \
    "${script}" >"${temporary}/composition-direct-backend.out" 2>&1; then
    printf 'SELFTEST FAIL unclassified direct backend construction unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV015 local-network-composition-census' "${temporary}/composition-direct-backend.out" ||
    ! grep -q 'direct-krun-backend-construction|bad' "${temporary}/composition-direct-backend.out"; then
    printf 'SELFTEST FAIL direct backend construction did not fail NNCV015 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS direct backend construction fails closed as NNCV015\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_CASE=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE='fn bad() { NetworkAttachmentProviderRegistration::new(provider(), endpoints(), lifecycle(), sovereignty()); }' \
    "${script}" >"${temporary}/composition-fabricated-registration.out" 2>&1; then
    printf 'SELFTEST FAIL fabricated attachment registration unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV015 local-network-composition-census' "${temporary}/composition-fabricated-registration.out" ||
    ! grep -q 'attachment-registration-construction|bad' "${temporary}/composition-fabricated-registration.out"; then
    printf 'SELFTEST FAIL fabricated attachment registration did not fail NNCV015 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS fabricated attachment registration fails closed as NNCV015\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_CASE=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE='use nimbus_network::LocalNetworkManager as HiddenManager;' \
    "${script}" >"${temporary}/composition-import-alias.out" 2>&1; then
    printf 'SELFTEST FAIL composition authority import alias unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV015 local-network-composition-census' "${temporary}/composition-import-alias.out" ||
    ! grep -q 'composition-type-import-alias|<module>' "${temporary}/composition-import-alias.out"; then
    printf 'SELFTEST FAIL composition import alias did not fail NNCV015 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS composition import alias fails closed as NNCV015\n'
  fi

  NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_CASE=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'#[cfg(test)]\nfn hidden() { LocalPortLeaseAuthority::open("test-only"); }\nfn production_without_composition() {}\n' \
    "${script}" >"${temporary}/composition-cfg-test.out" 2>&1 || true
  if ! grep -q '^PASS NNCV015 local-network-composition-census' "${temporary}/composition-cfg-test.out" ||
    grep -q '^FAIL NNCV015 local-network-composition-census' "${temporary}/composition-cfg-test.out"; then
    printf 'SELFTEST FAIL cfg(test) composition call was misclassified as production\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS cfg(test) composition call remains test-only for NNCV015\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_CASE=1 \
    NIMBUS_NETWORK_VERIFY_TEST_DROP_COMPOSITION_KEY='crates/nimbus-cli/src/network_composition.rs|manager-bootstrap|claim|1' \
    "${script}" >"${temporary}/composition-stale-classification.out" 2>&1; then
    printf 'SELFTEST FAIL stale composition classification unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV015 local-network-composition-census' "${temporary}/composition-stale-classification.out" ||
    ! grep -q 'stale production network authority classification: crates/nimbus-cli/src/network_composition.rs|manager-bootstrap|claim|1' "${temporary}/composition-stale-classification.out"; then
    printf 'SELFTEST FAIL stale composition classification did not fail NNCV015 precisely\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS stale composition classification fails closed as NNCV015\n'
  fi

  composition_self_fail=0
  if [ ! -f "${COMPOSITION_CENSUS_SELF_TESTS}" ]; then
    printf 'SELFTEST FAIL composition-census self-test helper is missing: %s\n' "${COMPOSITION_CENSUS_SELF_TESTS}"
    self_fail=$((self_fail + 1))
  else
    # shellcheck source=scripts/nimbus-network-control-plane/composition-census-self-tests.sh
    . "${COMPOSITION_CENSUS_SELF_TESTS}"
    run_nnc46f_composition_census_self_tests "${script}" "${temporary}" ||
      composition_self_fail=$?
    self_fail=$((self_fail + composition_self_fail))
  fi

  if NIMBUS_NETWORK_VERIFY_SOVEREIGNTY_HELPER="${temporary}/missing-sovereignty-helper.py" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/missing-sovereignty-helper.out" 2>&1; then
    printf 'SELFTEST FAIL missing sovereignty helper did not fail closed\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV016 sovereignty-tripwire-contract' "${temporary}/missing-sovereignty-helper.out" ||
    grep -q '^PASS NNCV016 sovereignty-tripwire-contract' "${temporary}/missing-sovereignty-helper.out"; then
    printf 'SELFTEST FAIL missing sovereignty helper did not fail NNCV016 exclusively\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS missing sovereignty helper fails closed as NNCV016\n'
  fi

  if [ ! -f "${SOVEREIGNTY_TRIPWIRE_SELF_TESTS}" ]; then
    printf 'SELFTEST FAIL sovereignty-tripwire self-test helper is missing: %s\n' "${SOVEREIGNTY_TRIPWIRE_SELF_TESTS}"
    self_fail=$((self_fail + 1))
  elif ! timeout 120 bash "${SOVEREIGNTY_TRIPWIRE_SELF_TESTS}" \
    >"${temporary}/sovereignty-tripwire-self-tests.out" 2>&1; then
    printf 'SELFTEST FAIL sovereignty-tripwire mutation suite failed\n'
    sed -n '1,200p' "${temporary}/sovereignty-tripwire-self-tests.out"
    self_fail=$((self_fail + 1))
  elif ! grep -q '^Ran 70 tests' "${temporary}/sovereignty-tripwire-self-tests.out" ||
    ! grep -q '^OK$' "${temporary}/sovereignty-tripwire-self-tests.out"; then
    printf 'SELFTEST FAIL sovereignty-tripwire mutation suite count/result is not exact\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS sovereignty-tripwire mutation suite passes 70 tests\n'
  fi

  # Each lane invokes the same read-only aggregate child with disjoint
  # mutation variables and output paths. Batches cap concurrent repository
  # scans at two so the bind scanner retains enough process and memory headroom.
  # The overlap does not change any exclusive failure assertion.
  run_parallel_self_test_batch \
    'nnc52a|run_nnc52a_attachment_ordering_self_tests|attachment-ordering' \
    'nnc52d|run_nnc52d_startup_orphan_self_tests|startup-orphan'
  run_parallel_self_test_batch \
    'nnc53|run_nnc53_attachment_readiness_self_tests|attachment-readiness' \
    'nnc54|run_nnc54_attachment_crash_self_tests|attachment-crash'
  run_parallel_self_test_batch \
    'nnc54a|run_nnc54a_machine_forwarded_batch_self_tests|machine-forwarded-batch' \
    'nnc55|run_nnc55_effect_locality_self_tests|effect-locality'
  run_parallel_self_test_batch \
    'nnc56|run_nnc56_side_effect_free_inspection_self_tests|side-effect-free inspection' \
    'nnc61|run_nnc61_compute_network_manager_self_tests|compute-network-manager'
  run_parallel_self_test_batch \
    'nnc61a|run_nnc61a_compute_node_workload_coordinator_self_tests|compute-node-workload-coordinator' \
    'nnc61e|run_nnc61e_recovery_decision_self_tests|recovery-decision'

  if [ ! -f "${NNC62_WORKLOAD_NETWORK_PLAN_COMPILER_CONTRACT}" ]; then
    printf 'SELFTEST FAIL NNCV028 workload-network-plan compiler helper is missing\n'
    self_fail=$((self_fail + 1))
  elif ! bash "${NNC62_WORKLOAD_NETWORK_PLAN_COMPILER_CONTRACT}" --self-test \
    >"${temporary}/nnc62-contract-self-test.out" 2>&1; then
    printf 'SELFTEST FAIL NNCV028 workload-network-plan compiler mutation suite failed\n'
    sed -n '1,120p' "${temporary}/nnc62-contract-self-test.out"
    self_fail=$((self_fail + 1))
  elif ! grep -q '^NNC6\.2 contract self-test: 7 passed, 0 failed$' \
    "${temporary}/nnc62-contract-self-test.out"; then
    printf 'SELFTEST FAIL NNCV028 workload-network-plan compiler mutation count is not exact\n'
    self_fail=$((self_fail + 1))
  else
    sed -n '1,120p' "${temporary}/nnc62-contract-self-test.out"
  fi

  if [ ! -f "${NNC62A_WORKLOAD_NETWORK_PLAN_DURABILITY_CONTRACT}" ]; then
    printf 'SELFTEST FAIL NNCV029 workload-network-plan durability helper is missing\n'
    self_fail=$((self_fail + 1))
  elif ! bash "${NNC62A_WORKLOAD_NETWORK_PLAN_DURABILITY_CONTRACT}" --self-test \
    >"${temporary}/nnc62a-contract-self-test.out" 2>&1; then
    printf 'SELFTEST FAIL NNCV029 workload-network-plan durability mutation suite failed\n'
    sed -n '1,160p' "${temporary}/nnc62a-contract-self-test.out"
    self_fail=$((self_fail + 1))
  elif ! grep -q '^NNC6\.2a contract self-test: 10 passed, 0 failed$' \
    "${temporary}/nnc62a-contract-self-test.out"; then
    printf 'SELFTEST FAIL NNCV029 workload-network-plan durability mutation count is not exact\n'
    self_fail=$((self_fail + 1))
  else
    sed -n '1,160p' "${temporary}/nnc62a-contract-self-test.out"
  fi

  if [ ! -f "${NNC61E1_WORKLOAD_SAGA_INGRESS_CONTRACT}" ]; then
    printf 'SELFTEST FAIL NNCV030 workload-saga ingress helper is missing\n'
    self_fail=$((self_fail + 1))
  elif ! bash "${NNC61E1_WORKLOAD_SAGA_INGRESS_CONTRACT}" --self-test \
    >"${temporary}/nnc61e1-contract-self-test.out" 2>&1; then
    printf 'SELFTEST FAIL NNCV030 workload-saga ingress mutation suite failed\n'
    sed -n '1,180p' "${temporary}/nnc61e1-contract-self-test.out"
    self_fail=$((self_fail + 1))
  elif ! grep -q '^NNC6\.1e1 ingress contract self-test: 13 passed, 0 failed$' \
    "${temporary}/nnc61e1-contract-self-test.out"; then
    printf 'SELFTEST FAIL NNCV030 workload-saga ingress mutation count is not exact\n'
    self_fail=$((self_fail + 1))
  else
    sed -n '1,180p' "${temporary}/nnc61e1-contract-self-test.out"
  fi

  if [ ! -f "${NNC63A_WORKLOAD_EXECUTABLE_CONTRACT}" ]; then
    printf 'SELFTEST FAIL NNCV031 workload executable helper is missing\n'
    self_fail=$((self_fail + 1))
  elif ! bash "${NNC63A_WORKLOAD_EXECUTABLE_CONTRACT}" --self-test \
    >"${temporary}/nnc63a-contract-self-test.out" 2>&1; then
    printf 'SELFTEST FAIL NNCV031 workload executable mutation suite failed\n'
    sed -n '1,180p' "${temporary}/nnc63a-contract-self-test.out"
    self_fail=$((self_fail + 1))
  elif ! grep -q '^NNC6\.3a executable contract self-test: 13 passed, 0 failed$' \
    "${temporary}/nnc63a-contract-self-test.out"; then
    printf 'SELFTEST FAIL NNCV031 workload executable mutation count is not exact\n'
    self_fail=$((self_fail + 1))
  else
    sed -n '1,180p' "${temporary}/nnc63a-contract-self-test.out"
  fi

  if [ ! -f "${NNC63B_WORKLOAD_PROVISION_DECISION_CONTRACT}" ]; then
    printf 'SELFTEST FAIL NNCV032 workload provision decision helper is missing\n'
    self_fail=$((self_fail + 1))
  elif ! bash "${NNC63B_WORKLOAD_PROVISION_DECISION_CONTRACT}" --self-test \
    >"${temporary}/nnc63b-contract-self-test.out" 2>&1; then
    printf 'SELFTEST FAIL NNCV032 workload provision decision mutation suite failed\n'
    sed -n '1,220p' "${temporary}/nnc63b-contract-self-test.out"
    self_fail=$((self_fail + 1))
  elif ! rg -q '^NNC6\.3b provision contract self-test: 36 passed, 0 failed$' \
    "${temporary}/nnc63b-contract-self-test.out"; then
    printf 'SELFTEST FAIL NNCV032 workload provision decision mutation count is not exact\n'
    self_fail=$((self_fail + 1))
  else
    sed -n '1,220p' "${temporary}/nnc63b-contract-self-test.out"
  fi

  if [ ! -f "${NNC64_WORKLOAD_PROVISION_DISPATCH_CONTRACT}" ]; then
    printf 'SELFTEST FAIL NNCV033 workload provision dispatch helper is missing\n'
    self_fail=$((self_fail + 1))
  elif ! bash "${NNC64_WORKLOAD_PROVISION_DISPATCH_CONTRACT}" --self-test \
    >"${temporary}/nnc64-contract-self-test.out" 2>&1; then
    printf 'SELFTEST FAIL NNCV033 workload provision dispatch mutation suite failed\n'
    sed -n '1,260p' "${temporary}/nnc64-contract-self-test.out"
    self_fail=$((self_fail + 1))
  elif ! rg -q '^NNC6\.4 provider dispatch contract self-test: 50 passed, 0 failed$' \
    "${temporary}/nnc64-contract-self-test.out"; then
    printf 'SELFTEST FAIL NNCV033 workload provision dispatch mutation count is not exact\n'
    self_fail=$((self_fail + 1))
  else
    sed -n '1,260p' "${temporary}/nnc64-contract-self-test.out"
  fi

  if [ ! -f "${NNC64A_WORKLOAD_RESTART_CONTRACT}" ]; then
    printf 'SELFTEST FAIL NNCV034 workload restart helper is missing\n'
    self_fail=$((self_fail + 1))
  elif ! bash "${NNC64A_WORKLOAD_RESTART_CONTRACT}" --self-test \
    >"${temporary}/nnc64a-contract-self-test.out" 2>&1; then
    printf 'SELFTEST FAIL NNCV034 workload restart mutation suite failed\n'
    sed -n '1,220p' "${temporary}/nnc64a-contract-self-test.out"
    self_fail=$((self_fail + 1))
  elif ! rg -q '^NNC6\.4a restart contract self-test: 86 passed, 0 failed$' \
    "${temporary}/nnc64a-contract-self-test.out"; then
    printf 'SELFTEST FAIL NNCV034 workload restart mutation count is not exact\n'
    self_fail=$((self_fail + 1))
  else
    sed -n '1,220p' "${temporary}/nnc64a-contract-self-test.out"
  fi

  if [ ! -f "${NNC65_WORKLOAD_TEARDOWN_CONTRACT}" ]; then
    printf 'SELFTEST FAIL NNCV035 workload teardown helper is missing\n'
    self_fail=$((self_fail + 1))
  elif ! bash "${NNC65_WORKLOAD_TEARDOWN_CONTRACT}" --self-test \
    >"${temporary}/nnc65-contract-self-test.out" 2>&1; then
    printf 'SELFTEST FAIL NNCV035 workload teardown mutation suite failed\n'
    sed -n '1,220p' "${temporary}/nnc65-contract-self-test.out"
    self_fail=$((self_fail + 1))
  elif ! rg -q '^NNC6\.5 teardown contract self-test: 180 passed, 0 failed$' \
    "${temporary}/nnc65-contract-self-test.out"; then
    printf 'SELFTEST FAIL NNCV035 workload teardown mutation count is not exact\n'
    self_fail=$((self_fail + 1))
  else
    sed -n '1,220p' "${temporary}/nnc65-contract-self-test.out"
  fi

  if NIMBUS_NETWORK_NNC65_AGGREGATE_SELF_TEST_BASELINE=0 \
    NIMBUS_NETWORK_VERIFY_TEARDOWN_FIXTURE=1 \
    NIMBUS_NETWORK_VERIFY_TEARDOWN_MUTATION=missing-phase \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/nnc65-aggregate-mutation.out" 2>&1; then
    printf 'SELFTEST FAIL NNCV035 aggregate teardown mutation unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV035 fenced-workload-teardown' \
    "${temporary}/nnc65-aggregate-mutation.out" ||
    grep -q '^PASS NNCV035 fenced-workload-teardown' \
      "${temporary}/nnc65-aggregate-mutation.out" ||
    [ "$(grep -c '^FAIL NNCV' "${temporary}/nnc65-aggregate-mutation.out")" -ne 1 ] ||
    ! grep -q '^Summary: 37 passed, 1 failed$' \
      "${temporary}/nnc65-aggregate-mutation.out"; then
    printf 'SELFTEST FAIL NNCV035 aggregate mutation did not fail exclusively\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS NNCV035 aggregate mutation fails closed exclusively\n'
  fi

  run_nnc81_process_harness_self_tests "${script}" "${temporary}"
  self_fail=$((self_fail + $?))

  if ! bash "${NNC82_PROVIDER_CURRENT_CLAIM_CONTRACT}" --self-test \
    >"${temporary}/nnc82-contract-self-test.out" 2>&1; then
    printf 'SELFTEST FAIL NNCV037 current-claim mutation suite failed\n'
    self_fail=$((self_fail + 1))
  elif ! rg -q '^NNC8\.2 current-claim contract self-test: 9 passed, 0 failed$' \
    "${temporary}/nnc82-contract-self-test.out"; then
    printf 'SELFTEST FAIL NNCV037 current-claim mutation count is not exact\n'
    self_fail=$((self_fail + 1))
  else
    sed -n '1,180p' "${temporary}/nnc82-contract-self-test.out"
  fi

  if ! node "${COMPILER_AUTHORITY_CONTRACT}" --self-test \
    --inventory "${INVENTORY}" \
    >"${temporary}/nnc91-compiler-authority-self-test.out" 2>&1; then
    printf 'SELFTEST FAIL NNCV038 compiler authority mutation suite failed\n'
    sed -n '1,160p' "${temporary}/nnc91-compiler-authority-self-test.out"
    self_fail=$((self_fail + 1))
  elif ! rg -q '^compiler authority self-test: 18 passed, 0 failed$' \
    "${temporary}/nnc91-compiler-authority-self-test.out"; then
    printf 'SELFTEST FAIL NNCV038 compiler authority mutation count is not exact\n'
    self_fail=$((self_fail + 1))
  else
    sed -n '1,160p' "${temporary}/nnc91-compiler-authority-self-test.out"
  fi

  if NIMBUS_NETWORK_VERIFY_COMPILER_BASELINE="${temporary}/missing-compiler-baseline.json" \
    NIMBUS_NETWORK_VERIFY_COMPILER_SELF_TEST_FORCE=1 \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/nnc91-missing-baseline.out" 2>&1; then
    printf 'SELFTEST FAIL NNCV038 missing baseline unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV038 compiler-generated-authority-closure' \
    "${temporary}/nnc91-missing-baseline.out" ||
    grep -q '^PASS NNCV038 compiler-generated-authority-closure' \
      "${temporary}/nnc91-missing-baseline.out" ||
    [ "$(grep -c '^FAIL NNCV' "${temporary}/nnc91-missing-baseline.out")" -ne 1 ] ||
    ! grep -q '^Summary: 38 passed, 1 failed$' \
      "${temporary}/nnc91-missing-baseline.out"; then
    printf 'SELFTEST FAIL NNCV038 missing baseline did not fail exclusively\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS NNCV038 missing baseline fails closed exclusively\n'
  fi

  if [ "${self_fail}" -ne 0 ]; then
    printf 'self-test: %d failed\n' "${self_fail}"
    exit 1
  fi
  printf 'self-test: 607 passed, 0 failed\n'
}

run_nnc81_affected_self_test() {
  temporary="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-network-nnc81-self-test.XXXXXX")" || {
    printf 'NNC8.1 affected self-test: unable to create temporary directory\n'
    exit 1
  }
  trap 'rm -rf "${temporary}"' EXIT
  script="${REPO_ROOT}/scripts/verify-nimbus-network-control-plane.sh"
  affected_fail=0

  run_nnc81_process_harness_self_tests "${script}" "${temporary}"
  affected_fail=$((affected_fail + $?))

  if NIMBUS_NETWORK_NNC65_AGGREGATE_SELF_TEST_BASELINE=0 \
    NIMBUS_NETWORK_VERIFY_TEARDOWN_FIXTURE=1 \
    NIMBUS_NETWORK_VERIFY_TEARDOWN_MUTATION=missing-phase \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/nnc65-aggregate-mutation.out" 2>&1; then
    printf 'SELFTEST FAIL NNCV035 affected aggregate mutation unexpectedly exited zero\n'
    affected_fail=$((affected_fail + 1))
  elif ! grep -q '^FAIL NNCV035 fenced-workload-teardown' \
    "${temporary}/nnc65-aggregate-mutation.out" ||
    grep -q '^PASS NNCV035 fenced-workload-teardown' \
      "${temporary}/nnc65-aggregate-mutation.out" ||
    [ "$(grep -c '^FAIL NNCV' "${temporary}/nnc65-aggregate-mutation.out")" -ne 1 ] ||
    ! grep -q '^Summary: 37 passed, 1 failed$' \
      "${temporary}/nnc65-aggregate-mutation.out"; then
    printf 'SELFTEST FAIL NNCV035 affected aggregate mutation did not fail exclusively\n'
    affected_fail=$((affected_fail + 1))
  else
    printf 'SELFTEST PASS NNCV035 affected aggregate mutation fails closed exclusively\n'
  fi

  if [ "${affected_fail}" -ne 0 ]; then
    printf 'NNC8.1 affected self-test: %d failed\n' "${affected_fail}"
    exit 1
  fi
  printf 'NNC8.1 affected self-test: 4 passed, 0 failed\n'
}
