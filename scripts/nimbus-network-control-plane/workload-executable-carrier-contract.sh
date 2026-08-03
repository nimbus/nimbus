#!/usr/bin/env bash
# Static NNC6.3a contract for the strict executable carrier and closed digest.

set -u

REPO_ROOT="${NIMBUS_NETWORK_NNC63A_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
STARTING_CHECKPOINT="${NIMBUS_NETWORK_NNC63A_STARTING_CHECKPOINT:-81e4f2f9ca6c998973a67f0c58e4918998bcca5e}"
COMPLETION_CHECKPOINT="${NIMBUS_NETWORK_NNC63A_COMPLETION_CHECKPOINT:-ed0560b4e45f7ec571934624962de72d021a71a8}"
EXECUTABLE="crates/nimbus-workloads/src/saga/executable.rs"
SAGA="crates/nimbus-workloads/src/saga.rs"
SAGA_TESTS="crates/nimbus-workloads/src/saga/executable/tests.rs"
SAGA_RECORD_TESTS="crates/nimbus-workloads/src/saga/tests.rs"
WORKLOADS_MANIFEST="crates/nimbus-workloads/Cargo.toml"
COMPUTE_CODEC="crates/nimbus-compute/src/workload_executable.rs"
COMPUTE_TESTS="crates/nimbus-compute/src/workload_executable/tests.rs"
STORE_CODEC="crates/nimbus-server/src/workload_saga_store/codec.rs"
STORE_SCHEMA="crates/nimbus-server/src/workload_saga_store/schema.rs"
STORE_TEST_ROOT="crates/nimbus-server/src/workload_saga_store/tests/mod.rs"
PROCESS_PROOF="crates/nimbus-server/src/workload_saga_store/tests/executable_durability.rs"
OWNER_PLAN="docs/private/plans/nimbus-network-control-plane-plan.md"

NNC63A_ERRORS=()
NNC63A_CHECKS=0

add_error() {
  NNC63A_ERRORS+=("$1")
}

pass_check() {
  NNC63A_CHECKS=$((NNC63A_CHECKS + 1))
}

source_without_comments() {
  node - "$1" <<'NODE'
const fs = require("fs");
const path = process.argv[2];
const source = fs.existsSync(path) ? fs.readFileSync(path, "utf8") : "";
process.stdout.write(source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/.*$/gm, ""));
NODE
}

require_loaded_source() {
  value="$1"
  path="$2"
  label="$3"
  if [ -z "${value}" ]; then
    add_error "missing or empty ${label}: ${path}"
    return 1
  fi
  pass_check
  return 0
}

apply_test_mutation() {
  case "${NIMBUS_NETWORK_NNC63A_TEST_MUTATION:-}" in
    missing-carrier) executable_source="" ;;
    caller-digest) saga_source="${saga_source}"$'\n''fn new(desired_digest: WorkloadDesiredDigest) {}' ;;
    optional-physical) schema_source="${schema_source/field(\"executable\", FieldType::Object, true)/field(\"executable\", FieldType::Object, false)}" ;;
    sandbox-edge) manifest_source="${manifest_source}"$'\n''nimbus-sandbox = { path = "../nimbus-sandbox" }' ;;
    provider-effect) compute_source="${compute_source}"$'\n''fn leak() { SandboxBackend::start(); }' ;;
    missing-process-proof) process_source="" ;;
    snapshot-handoff) process_source="${process_source}"$'\n''command.env("EXECUTABLE_PAYLOAD", content);' ;;
    missing-noncanonical-gate) compute_source="${compute_source/decoded_canonical != intent.canonical_content()/decoded_canonical == intent.canonical_content()}" ;;
    legacy-format-compatibility) executable_source="${executable_source/self.format_version != WORKLOAD_EXECUTABLE_FORMAT_VERSION/self.format_version > WORKLOAD_EXECUTABLE_FORMAT_VERSION}" ;;
    debug-content-leak) executable_source="${executable_source/.field(\"content_digest\", &self.content_digest)/.field(\"content\", &self.content)}" ;;
    missing-admission-digest)
      saga_source="$(printf '%s' "${saga_source}" | node -e '
        let source = "";
        process.stdin.setEncoding("utf8");
        process.stdin.on("data", chunk => { source += chunk; });
        process.stdin.on("end", () => process.stdout.write(source.replace(
          "admission: &\u0027a WorkloadAdmissionEvidence",
          "removedEvidence: &\u0027a WorkloadAdmissionEvidence"
        )));
      ')"
      ;;
    missing-behavior-test) saga_tests_source="${saga_tests_source/crossed_content_digest_is_rejected/removed_crossed_content_digest_case}" ;;
    unexpected-path) paths="${paths}\ncrates/nimbus-services/src/manager/service_start.rs" ;;
  esac
}

verify_contract() {
  executable_source="$(source_without_comments "${REPO_ROOT}/${EXECUTABLE}")"
  saga_source="$(source_without_comments "${REPO_ROOT}/${SAGA}")"
  saga_tests_source="$(source_without_comments "${REPO_ROOT}/${SAGA_TESTS}")"
  saga_record_tests_source="$(source_without_comments "${REPO_ROOT}/${SAGA_RECORD_TESTS}")"
  manifest_source="$(source_without_comments "${REPO_ROOT}/${WORKLOADS_MANIFEST}")"
  compute_source="$(source_without_comments "${REPO_ROOT}/${COMPUTE_CODEC}")"
  compute_tests_source="$(source_without_comments "${REPO_ROOT}/${COMPUTE_TESTS}")"
  codec_source="$(source_without_comments "${REPO_ROOT}/${STORE_CODEC}")"
  schema_source="$(source_without_comments "${REPO_ROOT}/${STORE_SCHEMA}")"
  store_root_source="$(source_without_comments "${REPO_ROOT}/${STORE_TEST_ROOT}")"
  process_source="$(source_without_comments "${REPO_ROOT}/${PROCESS_PROOF}")"
  plan_source="$(source_without_comments "${REPO_ROOT}/${OWNER_PLAN}")"

  require_loaded_source "${executable_source}" "${EXECUTABLE}" "workloads-owned executable carrier" || true
  require_loaded_source "${saga_source}" "${SAGA}" "portable workload saga" || true
  require_loaded_source "${saga_tests_source}" "${SAGA_TESTS}" "portable executable behavior tests" || true
  require_loaded_source "${saga_record_tests_source}" "${SAGA_RECORD_TESTS}" "closed desired-digest behavior tests" || true
  require_loaded_source "${manifest_source}" "${WORKLOADS_MANIFEST}" "workloads manifest" || true
  require_loaded_source "${compute_source}" "${COMPUTE_CODEC}" "compute executable codec" || true
  require_loaded_source "${compute_tests_source}" "${COMPUTE_TESTS}" "compute executable codec tests" || true
  require_loaded_source "${codec_source}" "${STORE_CODEC}" "server saga codec" || true
  require_loaded_source "${schema_source}" "${STORE_SCHEMA}" "server saga schema" || true
  require_loaded_source "${store_root_source}" "${STORE_TEST_ROOT}" "server saga test root" || true
  require_loaded_source "${process_source}" "${PROCESS_PROOF}" "fresh-process executable proof" || true
  require_loaded_source "${plan_source}" "${OWNER_PLAN}" "canonical owner plan" || true

  if [ -n "${NIMBUS_NETWORK_NNC63A_TEST_CHANGED_PATHS:-}" ]; then
    paths="${NIMBUS_NETWORK_NNC63A_TEST_CHANGED_PATHS}"
  elif ! git -C "${REPO_ROOT}" cat-file -e "${STARTING_CHECKPOINT}^{commit}" 2>/dev/null; then
    add_error "NNC6.3a starting checkpoint is missing: ${STARTING_CHECKPOINT}"
    paths=""
  elif ! git -C "${REPO_ROOT}" cat-file -e "${COMPLETION_CHECKPOINT}^{commit}" 2>/dev/null; then
    add_error "NNC6.3a completion checkpoint is missing: ${COMPLETION_CHECKPOINT}"
    paths=""
  else
    committed="$(git -C "${REPO_ROOT}" diff --name-only \
      "${STARTING_CHECKPOINT}..${COMPLETION_CHECKPOINT}" 2>/dev/null)" || {
      add_error "NNC6.3a committed source range is unreadable"
      committed=""
    }
    paths="$(printf '%s\n' "${committed}" | sort -u)"
  fi

  apply_test_mutation

  carrier_errors="${#NNC63A_ERRORS[@]}"
  for seam in \
    'WORKLOAD_EXECUTABLE_FORMAT_VERSION: u32 = 1' \
    'MAX_WORKLOAD_EXECUTABLE_CONTENT_BYTES: usize = 1024 * 1024' \
    'pub enum WorkloadExecutableEncoding' \
    'SandboxSpecCanonicalJsonV1' \
    'pub struct WorkloadExecutableIntent' \
    'format_version: u32' \
    'encoding: WorkloadExecutableEncoding' \
    'content: String' \
    'content_digest: WorkloadExecutableContentDigest' \
    'self.format_version != WORKLOAD_EXECUTABLE_FORMAT_VERSION' \
    'impl fmt::Debug for WorkloadExecutableIntent'; do
    if ! printf '%s\n' "${executable_source}" | rg -q -F "${seam}"; then
      add_error "executable carrier lacks ${seam}"
    fi
  done
  if [ "${#NNC63A_ERRORS[@]}" -eq "${carrier_errors}" ]; then
    pass_check
  fi
  debug_impl_count="$(printf '%s\n' "${executable_source}" |
    rg -o 'impl[[:space:]]+fmt::Debug[[:space:]]+for[[:space:]]+WorkloadExecutableIntent' |
    awk 'END { print NR + 0 }')"
  if [ "${debug_impl_count}" -ne 1 ]; then
    add_error "expected one redacting executable Debug implementation, observed ${debug_impl_count}"
  else
    debug_impl_block="$(printf '%s' "${executable_source}" | node -e '
      let source = "";
      process.stdin.setEncoding("utf8");
      process.stdin.on("data", chunk => { source += chunk; });
      process.stdin.on("end", () => {
        const marker = "impl fmt::Debug for WorkloadExecutableIntent";
        const start = source.indexOf(marker);
        const open = source.indexOf("{", start);
        if (start < 0 || open < 0) return;
        let depth = 0;
        for (let index = open; index < source.length; index += 1) {
          if (source[index] === "{") depth += 1;
          if (source[index] === "}") depth -= 1;
          if (depth === 0) {
            process.stdout.write(source.slice(start, index + 1));
            return;
          }
        }
      });
    ')"
    debug_content_references="$(printf '%s\n' "${debug_impl_block}" |
      rg --pcre2 -n 'self\.content(?![._A-Za-z0-9])|\.field\("content"' || true)"
    if [ -n "${debug_content_references}" ]; then
      add_error "executable Debug exposes canonical content"
    else
      pass_check
    fi
  fi

  constructor_block="$(node - "${REPO_ROOT}/${SAGA}" <<'NODE'
const fs = require("fs");
const path = process.argv[2];
const source = fs.existsSync(path) ? fs.readFileSync(path, "utf8") : "";
const start = source.indexOf("pub fn new(\n        kind: DesiredWorkloadKind");
const end = source.indexOf("    pub(super) fn validate", start);
process.stdout.write(start >= 0 && end > start ? source.slice(start, end) : "");
NODE
)"
  if printf '%s\n%s\n' "${constructor_block}" "${saga_source}" |
    rg -q 'pub fn new\([^)]*desired_digest:[[:space:]]*WorkloadDesiredDigest|fn new\(desired_digest:[[:space:]]*WorkloadDesiredDigest'; then
    add_error "WorkloadSagaIntent still accepts caller-supplied desired digest"
  else
    pass_check
  fi
  digest_payload_block="$(printf '%s' "${saga_source}" | node -e '
    let source = "";
    process.stdin.setEncoding("utf8");
    process.stdin.on("data", chunk => { source += chunk; });
    process.stdin.on("end", () => {
      const start = source.indexOf("struct WorkloadDesiredDigestPayload");
      const end = source.indexOf("\n}\n", start);
      process.stdout.write(start >= 0 && end > start ? source.slice(start, end + 2) : "");
    });
  ')"
  digest_errors="${#NNC63A_ERRORS[@]}"
  for seam in \
    "executable: &'a WorkloadExecutableIntent" \
    "admission: &'a WorkloadAdmissionEvidence"; do
    if ! printf '%s\n' "${digest_payload_block}" | rg -q -F "${seam}"; then
      add_error "closed desired digest lacks ${seam}"
    fi
  done
  for seam in \
    'derive_desired_digest' \
    'workload desired digest does not match complete desired intent'; do
    if ! printf '%s\n' "${saga_source}" | rg -q -F "${seam}"; then
      add_error "closed desired digest lacks ${seam}"
    fi
  done
  if [ "${#NNC63A_ERRORS[@]}" -eq "${digest_errors}" ]; then
    pass_check
  fi

  codec_errors="${#NNC63A_ERRORS[@]}"
  for seam in \
    'encode_sandbox_spec' \
    'decode_sandbox_spec' \
    'serde_json::to_vec' \
    'serde_json::from_slice' \
    'decoded_canonical != intent.canonical_content()'; do
    if ! printf '%s\n' "${compute_source}" | rg -q -F "${seam}"; then
      add_error "compute executable codec lacks ${seam}"
    fi
  done
  if [ "${#NNC63A_ERRORS[@]}" -eq "${codec_errors}" ]; then
    pass_check
  fi

  if printf '%s\n' "${manifest_source}" | rg -q '^nimbus-sandbox[[:space:]]*='; then
    add_error "nimbus-workloads depends on nimbus-sandbox"
  else
    pass_check
  fi
  forbidden_effects="$(printf '%s\n%s\n' "${executable_source}" "${compute_source}" |
    rg -n 'SandboxBackend|LocalNetworkManager|NetworkAttachmentProvider|IngressProvider|ForwardingProvider|TcpListener|UdpSocket|start_service|apply_network_plan' || true)"
  if [ -n "${forbidden_effects}" ]; then
    add_error "NNC6.3a executable path imports or calls provider effects: ${forbidden_effects}"
  else
    pass_check
  fi

  physical_source="${codec_source}\n${schema_source}"
  if ! printf '%s\n' "${physical_source}" | rg -q 'executable'; then
    add_error "physical saga codec/schema lacks executable"
  elif ! printf '%s\n' "${schema_source}" |
    rg -q 'field\("executable",[[:space:]]*FieldType::Object,[[:space:]]*true\)'; then
    add_error "executable is not one required physical object"
  else
    pass_check
  fi

  process_errors="${#NNC63A_ERRORS[@]}"
  for seam in \
    'mod executable_durability;' \
    'SubprocessCrashCutHarness' \
    'workload-saga.executable-durable' \
    'decode_sandbox_spec' \
    'IntentCommitted'; do
    if ! printf '%s\n%s\n' "${store_root_source}" "${process_source}" | rg -q -F "${seam}"; then
      add_error "fresh-process executable proof lacks ${seam}"
    fi
  done
  if [ "${#NNC63A_ERRORS[@]}" -eq "${process_errors}" ]; then
    pass_check
  fi
  snapshot_handoff="$(printf '%s\n' "${process_source}" |
    rg -n '(^|[^A-Z0-9_])(EXECUTABLE_(PAYLOAD|CONTENT|SPEC)|RECORD_JSON|SNAPSHOT)([^A-Z0-9_]|$)|stdin\(Stdio::piped' || true)"
  if [ -n "${snapshot_handoff}" ]; then
    add_error "fresh-process executable proof permits snapshot handoff: ${snapshot_handoff}"
  else
    pass_check
  fi

  behavior_errors="${#NNC63A_ERRORS[@]}"
  for seam in \
    missing_executable_field_is_rejected \
    unknown_executable_field_is_rejected \
    duplicate_executable_field_is_rejected \
    crossed_content_digest_is_rejected \
    oversized_executable_content_is_rejected \
    debug_redacts_executable_content \
    desired_digest_binds_complete_intent \
    exact_successor_retains_executable \
    noncanonical_sandbox_spec_is_rejected \
    sandbox_spec_round_trip_is_exact \
    malformed_executable_does_not_mutate_store; do
    if ! printf '%s\n%s\n%s\n%s\n' "${saga_tests_source}" "${saga_record_tests_source}" "${compute_tests_source}" "${process_source}" |
      rg -q -F "${seam}"; then
      add_error "NNC6.3a behavioral matrix lacks ${seam}"
    fi
  done
  if [ "${#NNC63A_ERRORS[@]}" -eq "${behavior_errors}" ]; then
    pass_check
  fi

  if ! printf '%s\n' "${plan_source}" |
    rg -q '^\| NNC6\.3a \| Persist one strict workloads-owned executable carrier and derive the closed desired digest\.'; then
    add_error "canonical plan does not route NNC6.3a to the strict executable carrier"
  else
    pass_check
  fi

  unexpected="$(printf '%b\n' "${paths}" | awk '
    NF == 0 { next }
    $0 == "crates/nimbus-workloads/src/lib.rs" { next }
    $0 == "crates/nimbus-workloads/src/saga.rs" { next }
    $0 == "crates/nimbus-workloads/src/saga/executable.rs" { next }
    $0 == "crates/nimbus-workloads/src/saga/executable/tests.rs" { next }
    $0 == "crates/nimbus-workloads/src/saga/tests.rs" { next }
    $0 == "crates/nimbus-workloads/src/saga/network/tests.rs" { next }
    $0 == "crates/nimbus-workloads/src/store/tests.rs" { next }
    $0 == "crates/nimbus-compute/src/lib.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_executable.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_executable/tests.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_saga/tests.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_saga/ingress/tests.rs" { next }
    $0 == "crates/nimbus-compute/src/workload_saga/recovery/tests.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/codec.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/schema.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/tests/mod.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/tests/composition.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/tests/codec.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/tests/durability.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/tests/recovery.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/tests/tenant_enumeration.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/tests/compiled_plan_durability.rs" { next }
    $0 == "crates/nimbus-server/src/workload_saga_store/tests/executable_durability.rs" { next }
    $0 == "crates/nimbus-server/src/router.rs" { next }
    $0 == "scripts/nimbus-network-control-plane/workload-saga-ingress-contract.sh" { next }
    $0 == "scripts/nimbus-network-control-plane/workload-executable-carrier-contract.sh" { next }
    $0 == "scripts/verify-nimbus-network-control-plane.sh" { next }
    $0 == "docs/private/plans/nimbus-network-control-plane-plan.md" { next }
    $0 == "docs/private/plans/README.md" { next }
    $0 ~ /^docs\/private\/plans\/proof\/nimbus-network-control-plane\/nnc6\.3a-[0-9A-Za-z._-]*\.md$/ { next }
    { print }
  ')"
  if [ -n "${unexpected}" ]; then
    add_error "NNC6.3a source diff escapes the frozen allowlist: ${unexpected}"
  else
    pass_check
  fi
}

run_contract() {
  cd "${REPO_ROOT}" || return 1
  NNC63A_ERRORS=()
  NNC63A_CHECKS=0
  for tool in git node rg awk; do
    command -v "${tool}" >/dev/null 2>&1 || add_error "missing required verifier tool ${tool}"
  done
  verify_contract
  if [ "${#NNC63A_ERRORS[@]}" -ne 0 ]; then
    for error in "${NNC63A_ERRORS[@]}"; do
      printf 'NNC6.3a executable contract failure: %s\n' "${error}" >&2
    done
    return 1
  fi
  printf 'NNC6.3a executable contract: %d checks passed\n' "${NNC63A_CHECKS}"
}

run_self_test() {
  if ! run_contract >/dev/null 2>&1; then
    printf 'NNC6.3a executable contract self-test: baseline contract is not green\n' >&2
    return 1
  fi
  temporary="$(mktemp -d "${TMPDIR:-/tmp}/nnc63a-contract-self-test.XXXXXX")" || return 1
  trap 'rm -rf "${temporary}"' EXIT
  failures=0
  for mutation in \
    missing-carrier caller-digest optional-physical sandbox-edge provider-effect \
    missing-process-proof snapshot-handoff missing-noncanonical-gate legacy-format-compatibility \
    debug-content-leak \
    missing-admission-digest missing-behavior-test unexpected-path; do
    output="${temporary}/${mutation}.out"
    case "${mutation}" in
      missing-carrier) expected="executable carrier lacks WORKLOAD_EXECUTABLE_FORMAT_VERSION: u32 = 1" ;;
      caller-digest) expected="WorkloadSagaIntent still accepts caller-supplied desired digest" ;;
      optional-physical) expected="executable is not one required physical object" ;;
      sandbox-edge) expected="nimbus-workloads depends on nimbus-sandbox" ;;
      provider-effect) expected="imports or calls provider effects" ;;
      missing-process-proof) expected="fresh-process executable proof lacks SubprocessCrashCutHarness" ;;
      snapshot-handoff) expected="fresh-process executable proof permits snapshot handoff" ;;
      missing-noncanonical-gate) expected="compute executable codec lacks decoded_canonical != intent.canonical_content()" ;;
      legacy-format-compatibility) expected="executable carrier lacks self.format_version != WORKLOAD_EXECUTABLE_FORMAT_VERSION" ;;
      debug-content-leak) expected="executable Debug exposes canonical content" ;;
      missing-admission-digest) expected="closed desired digest lacks admission: &'a WorkloadAdmissionEvidence" ;;
      missing-behavior-test) expected="behavioral matrix lacks crossed_content_digest_is_rejected" ;;
      unexpected-path) expected="source diff escapes the frozen allowlist" ;;
    esac
    if NIMBUS_NETWORK_NNC63A_TEST_MUTATION="${mutation}" \
      NIMBUS_NETWORK_NNC63A_TEST_CHANGED_PATHS="scripts/nimbus-network-control-plane/workload-executable-carrier-contract.sh" \
      bash "$0" --check >"${output}" 2>&1; then
      printf 'SELFTEST FAIL NNCV031 %s unexpectedly passed\n' "${mutation}"
      failures=$((failures + 1))
    elif ! grep -Fq "${expected}" "${output}"; then
      printf 'SELFTEST FAIL NNCV031 %s missed expected diagnostic: %s\n' \
        "${mutation}" "${expected}"
      sed -n '1,80p' "${output}"
      failures=$((failures + 1))
    else
      printf 'SELFTEST PASS NNCV031 %s fails closed with expected diagnostic\n' "${mutation}"
    fi
  done
  if [ "${failures}" -ne 0 ]; then
    printf 'NNC6.3a executable contract self-test: %d failed\n' "${failures}"
    return 1
  fi
  printf 'NNC6.3a executable contract self-test: 13 passed, 0 failed\n'
}

case "${1:-}" in
  '' | --check) run_contract ;;
  --self-test) run_self_test ;;
  *)
    printf 'usage: %s [--check|--self-test]\n' "$0" >&2
    exit 2
    ;;
esac
