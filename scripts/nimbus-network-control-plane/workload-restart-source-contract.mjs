import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

import {
  maskNonCode,
  walkRust,
  withoutCfgTestItems,
} from "./source-contract-scanner.mjs";

const AUDIT_CHECKPOINT =
  process.env.NIMBUS_NETWORK_NNC64A_AUDIT_CHECKPOINT ??
  "8723bc9a8ac27abc8ecbbd59d5f8d8d159e98cc1";
const R1_CHECKPOINT =
  process.env.NIMBUS_NETWORK_NNC64A_R1_CHECKPOINT ??
  "6d8961bd6d4da819b2524128cb398e22e0a9382f";

const ALLOWED_EXACT_PATHS = new Set([
  "crates/nimbus-workloads/src/lib.rs",
  "crates/nimbus-workloads/src/saga.rs",
  "crates/nimbus-workloads/src/store.rs",
  "crates/nimbus-compute/src/workload_saga.rs",
  "crates/nimbus-compute/src/workload_projection.rs",
  "crates/nimbus-compute/src/workload_provisioner.rs",
  "crates/nimbus-compute/src/services.rs",
  "crates/nimbus-server/src/workload_saga_store.rs",
  "crates/nimbus-server/src/http/services.rs",
  "crates/nimbus-server/src/router.rs",
  "crates/nimbus-server/src/workload_composition.rs",
  "crates/nimbus-server/src/state.rs",
  "crates/nimbus-sandbox/src/inspection.rs",
  "crates/nimbus-machine/src/api.rs",
  "crates/nimbus-node/src/host_lifecycle.rs",
  "crates/nimbus-node/src/reconciler.rs",
  "crates/nimbus-node/src/direct_process.rs",
  "crates/nimbus-node/src/systemd_transient.rs",
  "crates/nimbus-system/src/inventory.rs",
  "packages/nimbus/src/selftest.mjs",
  "packages/nimbus/README.md",
  "scripts/verify-nimbus-network-control-plane.sh",
  "scripts/verify-nimbus-network-source-contract.mjs",
  "scripts/nimbus-network-control-plane/source-contract-scanner.mjs",
  "scripts/nimbus-network-control-plane/workload-network-plan-durability-contract.sh",
  "scripts/nimbus-network-control-plane/workload-restart-contract.sh",
  "scripts/nimbus-network-control-plane/workload-restart-source-contract.mjs",
  "docs/private/plans/nimbus-network-control-plane-plan.md",
  "docs/private/plans/README.md",
  "docs/private/plans/proof/nimbus-network-control-plane/nnc6.4a-fenced-restart-substitution-audit.md",
]);

const ALLOWED_PREFIXES = [
  "crates/nimbus-workloads/src/saga/",
  "crates/nimbus-compute/src/workload_saga/",
  "crates/nimbus-compute/src/workload_provisioner/",
  "crates/nimbus-server/src/workload_saga_store/",
  "crates/nimbus-server/src/http/services/",
  "crates/nimbus-sandbox/src/backends/container/runtime/",
  "crates/nimbus-sandbox/src/backends/krun/vm/",
  "crates/nimbus-sandbox/src/backends/oci/network/",
  "crates/nimbus-cli/src/compose/",
  "crates/nimbus-cli/src/machine/api/",
  "crates/nimbus-cli/src/machine/backend/",
  "crates/nimbus-cli/src/machine/stub/",
  "packages/nimbus/src/control-plane/",
  "packages/nimbus/tests/",
];

const R1_ALLOWED_EXACT_PATHS = new Set([
  "crates/nimbus-workloads/src/lib.rs",
  "crates/nimbus-workloads/src/saga.rs",
  "crates/nimbus-workloads/src/saga/network/tests.rs",
  "crates/nimbus-workloads/src/saga/provision/tests.rs",
  "crates/nimbus-workloads/src/saga/provision.rs",
  "crates/nimbus-workloads/src/saga/restart.rs",
  "crates/nimbus-workloads/src/saga/restart/tests.rs",
  "crates/nimbus-workloads/src/saga/state.rs",
  "crates/nimbus-workloads/src/saga/state/provision.rs",
  "crates/nimbus-workloads/src/saga/state/restart.rs",
  "crates/nimbus-workloads/src/saga/tests.rs",
  "crates/nimbus-workloads/src/saga/tests/restart_state.rs",
  "crates/nimbus-server/src/workload_saga_store/codec.rs",
  "crates/nimbus-server/src/workload_saga_store/schema.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/codec.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/composition.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/durability.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/mod.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/restart.rs",
  "crates/nimbus-compute/src/workload_saga/recovery.rs",
  "scripts/nimbus-network-control-plane/workload-restart-contract.sh",
  "scripts/nimbus-network-control-plane/workload-restart-source-contract.mjs",
  "scripts/nimbus-network-control-plane/workload-network-plan-durability-contract.sh",
  "scripts/verify-nimbus-network-control-plane.sh",
  "docs/private/plans/nimbus-network-control-plane-plan.md",
  "docs/private/plans/README.md",
  "docs/private/plans/proof/nimbus-network-control-plane/nnc6.4a-fenced-restart-substitution-audit.md",
]);

const DIAGNOSTICS = {
  vocabulary:
    "restart-contract/vocabulary: portable restart vocabulary is missing or open",
  nestedState:
    "restart-contract/nested-state: same-generation restart state or attempt identity is incomplete",
  admissionIdentity:
    "restart-contract/admission-identity: restart admission does not bind every identity and fence",
  reducer:
    "restart-contract/reducer: compute is not the sole CAS restart admission authority",
  command:
    "restart-contract/command: confirmed restart commands are forgeable or incompletely fenced",
  ambiguity:
    "restart-contract/ambiguity: ambiguous restart effects do not inspect before exact-absence retry",
  schedule:
    "restart-contract/schedule: durable count, deadline, or deterministic-clock behavior is incomplete",
  withdrawal:
    "restart-contract/withdrawal: withdrawal or successor does not veto restart effects",
  readiness:
    "restart-contract/readiness: activation or callback fencing can bypass attachment and PEP readiness",
  capabilities:
    "restart-contract/capabilities: small Container and Krun restart substitutions are incomplete",
  service:
    "restart-contract/service: service or SDK restart lacks fenced idempotent submission",
  watch:
    "restart-contract/watch: automatic restart is not a bounded compute-owned durable watch",
  node: "restart-contract/node: tenant workload node providers do not enforce Restart=No",
  machine:
    "restart-contract/machine: forwarded restart command drops a saga or inspection fence",
  scheduler:
    "restart-contract/scheduler: provider-local restart scheduling or obsolete deadline state remains",
  behavior:
    "restart-contract/behavior: required restart behavior and recovery proofs are incomplete",
  network:
    "restart-contract/network: nimbus-network gained restart effects or a god provider",
  paths:
    "restart-contract/paths: NNC6.4a changed a path outside the frozen allowlist",
  ledger:
    "restart-contract/ledger: plan and proof do not retain the NNC6.4a acceptance and review tokens",
};

function joinSources(sources) {
  return sources.map((entry) => entry.source).join("\n");
}

function normalizeRustEntries(root, directory) {
  return walkRust(path.join(root, directory)).map((entry) => ({
    file: path.relative(root, entry.file).split(path.sep).join("/"),
    source: entry.source,
  }));
}

function readText(root, relativePath, { lexical = false } = {}) {
  const absolute = path.join(root, relativePath);
  if (!fs.existsSync(absolute) || !fs.statSync(absolute).isFile()) return "";
  const source = fs.readFileSync(absolute, "utf8");
  return lexical ? maskNonCode(source) : source;
}

function collectTestSources(root, directories) {
  const sources = [];
  const visit = (directory) => {
    if (!fs.existsSync(directory) || !fs.statSync(directory).isDirectory()) {
      return;
    }
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const absolute = path.join(directory, entry.name);
      if (entry.isDirectory()) {
        visit(absolute);
      } else if (entry.isFile() && entry.name.endsWith(".rs")) {
        const relative = path
          .relative(root, absolute)
          .split(path.sep)
          .join("/");
        const source = fs.readFileSync(absolute, "utf8");
        if (
          relative.includes("/tests/") ||
          entry.name === "tests.rs" ||
          /#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]/u.test(source)
        ) {
          sources.push(maskNonCode(source));
        }
      }
    }
  };
  for (const directory of directories) visit(path.join(root, directory));
  return sources.join("\n");
}

function greenFixture() {
  return {
    workloads: withoutCfgTestItems(`
pub enum WorkloadRestartPolicy {
    Never,
    OnFailure { max_restarts: u32 },
    Always { max_restarts: u32 },
}
pub enum WorkloadRestartTrigger { Automatic, Explicit }
pub struct WorkloadRestartEpoch(u64);
pub struct WorkloadRestartRequestId(String);
pub struct WorkloadExecutionAttemptId(String);
pub enum WorkloadRestartPhase {
    Idle,
    Requested,
    PublicationWithdrawalPending,
    ExecutionQuiescencePending,
    Scheduled,
    PreparationPending,
    AttachmentPending,
    ActivationPrerequisitePending,
    ActivationPending,
    ReadinessPending,
    PublicationPending,
    ObservationPending,
}
pub enum WorkloadRestartDisposition {
    Ready,
    DispatchPending,
    InspectionRequired,
    DefiniteFailure,
}
pub struct WorkloadRestartAdmission {
    saga_id: WorkloadSagaId,
    source: WorkloadProvisionSourceEvidence,
    generation: WorkloadGeneration,
    desired_digest: WorkloadDesiredDigest,
    revision: WorkloadSagaRevision,
    trigger: WorkloadRestartTrigger,
    inspection_version: Option<WorkloadInspectionVersion>,
    provider_selection: WorkloadExecutionProviderId,
    restart_epoch: WorkloadRestartEpoch,
    policy_attempt_count: u32,
    request_id: WorkloadRestartRequestId,
    attempt_id: WorkloadExecutionAttemptId,
}
pub struct WorkloadRestartState {
    current_execution_attempt_id: WorkloadExecutionAttemptId,
    phase: WorkloadRestartPhase,
    not_before_unix_millis: u64,
    completed_automatic_restart_count: u32,
}
pub struct WorkloadSagaRecord {
    restart: WorkloadRestartState,
}
struct TransitionIdentityPayload { restart: &'a WorkloadRestartState }
`),
    compute: withoutCfgTestItems(`
fn compare_and_swap_restart_admission() {}
fn automatic_and_explicit_restart_use_same_reducer() {}
fn claim_restart_command() {}
fn inspect_ambiguous_restart() {}
fn retry_after_authenticated_absence() {}
pub(crate) struct ConfirmedWorkloadRestartCommand {
    saga_id: WorkloadSagaId,
    generation: WorkloadGeneration,
    attempt_id: WorkloadExecutionAttemptId,
    restart_epoch: WorkloadRestartEpoch,
    dispatch_epoch: WorkloadRestartDispatchEpoch,
    issuing_revision: WorkloadSagaRevision,
    inspection_version: SandboxInspectionVersion,
    provider_selection: WorkloadExecutionProviderId,
}
impl ConfirmedWorkloadRestartCommand {
    pub(crate) fn from_confirmation() -> Self { unreachable!() }
}
trait RestartPublicationWithdrawalCapability {}
trait WorkloadExecutionQuiescenceCapability {}
trait WorkloadRestartPreparationCapability {}
impl WorkloadExecutionQuiescenceCapability for ContainerRestartAdapter {}
impl WorkloadRestartPreparationCapability for ContainerRestartAdapter {}
impl WorkloadExecutionQuiescenceCapability for KrunRestartAdapter {}
impl WorkloadRestartPreparationCapability for KrunRestartAdapter {}
fn bounded_restart_watch() {}
fn load_durable_restart_page() {}
fn read_only_exit_hint() {}
`),
    providers: withoutCfgTestItems(`
enum AttachmentDisposition { RestartRetained, Terminal }
fn retain_network_allocation() {}
fn retain_port_lease() {}
fn retain_attachment_identity() {}
fn retain_pep_authority() {}
`),
    server: withoutCfgTestItems(`
pub struct ServiceRestartRequest {
    source_generation: WorkloadGeneration,
    request_id: WorkloadRestartRequestId,
}
fn submit_service_restart() {}
`),
    sdk: `services.restart({ sourceGeneration, requestId });\n/api/tenants/:tenant/services/:service/restart`,
    codec: `
const REQUIRED_FIELDS: [&str; 2] = ["restartPolicy", "restartState"];
fn validate_physical_shape() {}
`,
    node: withoutCfgTestItems(`
fn into_host_lifecycle_request() { HostLifecycleProperty::Restart(HostRestartPolicy::No); }
fn ensure_external_restart_disabled() { policy != HostRestartPolicy::No; }
`),
    machine: withoutCfgTestItems(`
pub struct MachineRestartCommand {
    saga_id: WorkloadSagaId,
    generation: WorkloadGeneration,
    attempt_id: WorkloadExecutionAttemptId,
    restart_epoch: WorkloadRestartEpoch,
    dispatch_epoch: WorkloadRestartDispatchEpoch,
    inspection_version: SandboxInspectionVersion,
    provider_selection: WorkloadExecutionProviderId,
}
`),
    network: withoutCfgTestItems("pub struct NetworkAttachmentId(String);"),
    tests: maskNonCode(`
fn same_generation_restart_keeps_desired_generation() {}
fn restart_recovery_eligibility_is_exhaustive() {}
fn explicit_restart_does_not_consume_automatic_count() {}
fn concurrent_triggers_admit_one_restart_epoch() {}
fn deadline_survives_clock_rollback_without_early_admission() {}
fn deadline_survives_engine_reopen() {}
fn count_survives_engine_reopen() {}
fn withdrawal_vetoes_unissued_restart() {}
fn successor_vetoes_restart_before_admission() {}
fn activation_waits_for_same_generation_attachment_and_pep() {}
fn old_attempt_callback_is_rejected() {}
fn duplicate_service_request_returns_same_restart_epoch() {}
fn get_and_name_resolution_make_zero_restart_effects() {}
fn automatic_watch_is_bounded_and_does_not_busy_spin() {}
fn reconciler_rejects_provider_restart_and_duplicates_before_backend_validation() {}
fn machine_restart_wire_rejects_crossed_fences() {}
fn fresh_process_restart_reopens_engine() {}
fn crash_after_restart_effect_inspects_before_retry() {}
fn cancellation_after_submission_preserves_durable_work() {}
fn compose_local_and_forwarded_restart_use_compute() {}
`),
    plan: "NNC6.4a A1-A20 NNCV034 candidate-frozen Sol/xhigh/fast",
    changedPaths: [
      "scripts/verify-nimbus-network-source-contract.mjs",
      "scripts/nimbus-network-control-plane/source-contract-scanner.mjs",
      "scripts/nimbus-network-control-plane/workload-restart-contract.sh",
      "scripts/nimbus-network-control-plane/workload-restart-source-contract.mjs",
      "scripts/verify-nimbus-network-control-plane.sh",
      "docs/private/plans/nimbus-network-control-plane-plan.md",
      "docs/private/plans/README.md",
      "docs/private/plans/proof/nimbus-network-control-plane/nnc6.4a-fenced-restart-substitution-audit.md",
    ],
    r1ChangedPaths: [
      "crates/nimbus-workloads/src/saga/restart.rs",
      "crates/nimbus-workloads/src/saga/state/restart.rs",
      "crates/nimbus-server/src/workload_saga_store/codec.rs",
    ],
  };
}

function productionSources(root) {
  const crate = (name) =>
    joinSources(normalizeRustEntries(root, `crates/${name}/src`));
  return {
    workloads: crate("nimbus-workloads"),
    compute: crate("nimbus-compute"),
    providers: crate("nimbus-sandbox"),
    server: crate("nimbus-server"),
    codec: readText(
      root,
      "crates/nimbus-server/src/workload_saga_store/codec.rs",
    ),
    sdk: [
      readText(root, "packages/nimbus/src/control-plane/client.ts"),
      readText(root, "packages/nimbus/src/control-plane/routes.ts"),
      readText(root, "packages/nimbus/src/selftest.mjs"),
      readText(root, "packages/nimbus/README.md"),
    ].join("\n"),
    node: crate("nimbus-node"),
    machine: [crate("nimbus-machine"), crate("nimbus-cli")].join("\n"),
    network: crate("nimbus-network"),
    tests: collectTestSources(root, [
      "crates/nimbus-workloads/src",
      "crates/nimbus-workloads/tests",
      "crates/nimbus-compute/src",
      "crates/nimbus-compute/tests",
      "crates/nimbus-server/src",
      "crates/nimbus-server/tests",
      "crates/nimbus-sandbox/src",
      "crates/nimbus-sandbox/tests",
      "crates/nimbus-machine/src",
      "crates/nimbus-cli/src",
      "crates/nimbus-node/src",
    ]),
    plan: [
      readText(root, "docs/private/plans/nimbus-network-control-plane-plan.md"),
      readText(
        root,
        "docs/private/plans/proof/nimbus-network-control-plane/nnc6.4a-fenced-restart-substitution-audit.md",
      ),
    ].join("\n"),
    changedPaths: changedPathsSince(root, AUDIT_CHECKPOINT),
    r1ChangedPaths: changedPathsSince(root, R1_CHECKPOINT),
  };
}

function changedPathsSince(root, checkpoint) {
  const tracked = execFileSync(
    "git",
    ["diff", "--name-only", checkpoint, "--"],
    { cwd: root, encoding: "utf8" },
  );
  const untracked = execFileSync(
    "git",
    ["ls-files", "--others", "--exclude-standard"],
    { cwd: root, encoding: "utf8" },
  );
  return [...new Set(`${tracked}\n${untracked}`.split("\n").filter(Boolean))];
}

function replaceOnce(sources, area, before, after) {
  if (!sources[area].includes(before)) {
    throw new Error(`restart mutation target missing: ${area}:${before}`);
  }
  sources[area] = sources[area].replace(before, after);
}

function applyFixtureMutation(sources, mutation) {
  const admissionFields = {
    "missing-saga-id": "saga_id: WorkloadSagaId,",
    "missing-source": "source: WorkloadProvisionSourceEvidence,",
    "missing-generation": "generation: WorkloadGeneration,",
    "missing-desired-digest": "desired_digest: WorkloadDesiredDigest,",
    "missing-revision": "revision: WorkloadSagaRevision,",
    "missing-trigger": "trigger: WorkloadRestartTrigger,",
    "missing-inspection-version":
      "inspection_version: Option<WorkloadInspectionVersion>,",
    "missing-provider-selection":
      "provider_selection: WorkloadExecutionProviderId,",
    "missing-restart-epoch": "restart_epoch: WorkloadRestartEpoch,",
    "missing-policy-count": "policy_attempt_count: u32,",
    "missing-request-id": "request_id: WorkloadRestartRequestId,",
    "missing-attempt-id": "attempt_id: WorkloadExecutionAttemptId,",
  };
  if (mutation in admissionFields) {
    replaceOnce(sources, "workloads", admissionFields[mutation], "");
    return;
  }
  const replacements = {
    "crossed-attempt-id": [
      "workloads",
      "attempt_id: WorkloadExecutionAttemptId,",
      "attempt_id: WorkloadExecutionId,",
    ],
    "synthetic-generation": [
      "tests",
      "same_generation_restart_keeps_desired_generation",
      "restart_increments_desired_generation",
    ],
    "unknown-variant": [
      "workloads",
      "    ObservationPending,",
      "    ObservationPending,\n    ProviderManaged,",
    ],
    "forgeable-constructor": [
      "compute",
      "pub(crate) fn from_confirmation",
      "pub fn new",
    ],
    "bypass-admission-cas": [
      "compute",
      "compare_and_swap_restart_admission",
      "restart_without_admission",
    ],
    "direct-ambiguity-retry": [
      "compute",
      "inspect_ambiguous_restart",
      "retry_ambiguous_restart",
    ],
    "reset-count": [
      "tests",
      "count_survives_engine_reopen",
      "count_resets_after_process_restart",
    ],
    "reset-deadline": [
      "tests",
      "deadline_survives_engine_reopen",
      "deadline_recomputed_from_process_start",
    ],
    "withdrawal-loses": [
      "tests",
      "withdrawal_vetoes_unissued_restart",
      "restart_ignores_withdrawal",
    ],
    "activate-before-readiness": [
      "tests",
      "activation_waits_for_same_generation_attachment_and_pep",
      "activation_precedes_attachment",
    ],
    "old-attempt-callback": [
      "tests",
      "old_attempt_callback_is_rejected",
      "old_attempt_callback_updates_projection",
    ],
    "god-provider": [
      "compute",
      "trait RestartPublicationWithdrawalCapability {}",
      "trait RestartProvider {}",
    ],
    "network-effect": [
      "network",
      "pub struct NetworkAttachmentId(String);",
      "pub struct NetworkAttachmentId(String); fn restart() { TcpListener::bind(); }",
    ],
    "local-stop-start": [
      "server",
      "fn submit_service_restart() {}",
      "fn submit_service_restart() { stop_service(); start_service(); }",
    ],
    "missing-api-idempotency": [
      "server",
      "request_id: WorkloadRestartRequestId,",
      "",
    ],
    "node-restart": [
      "node",
      "HostRestartPolicy::No",
      "HostRestartPolicy::OnFailure",
    ],
    "machine-fence-discard": [
      "machine",
      "inspection_version: SandboxInspectionVersion,",
      "",
    ],
    "backend-local-scheduler": [
      "providers",
      "fn retain_pep_authority() {}",
      "fn retain_pep_authority() {} struct Manifest { next_restart_at_millis: u64 }",
    ],
    "missing-behavior-proof": [
      "tests",
      "fresh_process_restart_reopens_engine",
      "fresh_process_restart_uses_handoff",
    ],
    "missing-ledger-token": ["plan", "A1-A20", "acceptance-pending"],
  };
  if (mutation === "unexpected-path") {
    sources.changedPaths.push("crates/nimbus-tenant/src/restart.rs");
  } else if (mutation === "missing-restart-codec-field") {
    replaceOnce(sources, "codec", '"restartState"', '"removedRestartState"');
  } else if (mutation === "accept-unknown-restart-codec-field") {
    replaceOnce(
      sources,
      "codec",
      "validate_physical_shape",
      "accept_unknown_physical_shape",
    );
  } else if (mutation === "restart-transition-id-omits-state") {
    replaceOnce(
      sources,
      "workloads",
      "struct TransitionIdentityPayload { restart: &'a WorkloadRestartState }",
      "struct TransitionIdentityPayload { omitted_restart: () }",
    );
  } else if (mutation === "restart-phase-not-recoverable") {
    replaceOnce(
      sources,
      "tests",
      "restart_recovery_eligibility_is_exhaustive",
      "restart_phase_is_not_recoverable",
    );
  } else if (mutation === "explicit-consumes-automatic-count") {
    replaceOnce(
      sources,
      "tests",
      "explicit_restart_does_not_consume_automatic_count",
      "explicit_restart_consumes_automatic_count",
    );
  } else if (mutation === "r1-scope-broadening") {
    sources.r1ChangedPaths.push(
      "crates/nimbus-compute/src/workload_saga/restart.rs",
    );
  } else if (mutation in replacements) {
    replaceOnce(sources, ...replacements[mutation]);
  } else if (mutation) {
    throw new Error(`unknown restart contract mutation: ${mutation}`);
  }
}

function extractItem(source, marker) {
  const start = source.indexOf(marker);
  const open = source.indexOf("{", start);
  if (start < 0 || open < 0) return "";
  let depth = 0;
  for (let cursor = open; cursor < source.length; cursor += 1) {
    if (source[cursor] === "{") depth += 1;
    else if (source[cursor] === "}") depth -= 1;
    if (depth === 0) return source.slice(start, cursor + 1);
  }
  return "";
}

function enumVariants(source, name) {
  return extractItem(source, `enum ${name}`)
    .split("\n")
    .map((line) => line.match(/^\s*([A-Z][A-Za-z0-9_]*)\s*(?:[,({])/u)?.[1])
    .filter(Boolean);
}

function hasAll(source, tokens) {
  return tokens.every((token) => source.includes(token));
}

function isAllowedPath(candidate) {
  return (
    ALLOWED_EXACT_PATHS.has(candidate) ||
    ALLOWED_PREFIXES.some((prefix) => candidate.startsWith(prefix))
  );
}

function isR1AllowedPath(candidate) {
  return R1_ALLOWED_EXACT_PATHS.has(candidate);
}

export function verifyWorkloadRestartContract() {
  const fixture = process.env.NIMBUS_NETWORK_VERIFY_RESTART_FIXTURE === "1";
  const root = path.resolve(
    process.env.NIMBUS_NETWORK_VERIFY_RESTART_SCAN_ROOT ?? ".",
  );
  const sources = fixture ? greenFixture() : productionSources(root);
  if (fixture) {
    applyFixtureMutation(
      sources,
      process.env.NIMBUS_NETWORK_VERIFY_RESTART_MUTATION ?? "",
    );
  }

  const errors = [];
  const requireContract = (condition, diagnostic) => {
    if (!condition) errors.push(diagnostic);
  };

  const policyVariants = enumVariants(
    sources.workloads,
    "WorkloadRestartPolicy",
  );
  const phaseVariants = enumVariants(sources.workloads, "WorkloadRestartPhase");
  requireContract(
    hasAll(sources.workloads, [
      "WorkloadRestartTrigger",
      "WorkloadRestartEpoch",
      "WorkloadRestartRequestId",
      "WorkloadExecutionAttemptId",
      "WorkloadRestartDisposition",
    ]) &&
      policyVariants.join(" ") === "Never OnFailure Always" &&
      phaseVariants.join(" ") ===
        "Idle Requested PublicationWithdrawalPending ExecutionQuiescencePending Scheduled PreparationPending AttachmentPending ActivationPrerequisitePending ActivationPending ReadinessPending PublicationPending ObservationPending" &&
      hasAll(sources.codec, [
        '"restartPolicy"',
        '"restartState"',
        "validate_physical_shape",
      ]),
    DIAGNOSTICS.vocabulary,
  );

  requireContract(
    hasAll(sources.workloads, [
      "restart: WorkloadRestartState",
      "current_execution_attempt_id: WorkloadExecutionAttemptId",
    ]) &&
      hasAll(
        extractItem(sources.workloads, "struct TransitionIdentityPayload"),
        ["restart:", "WorkloadRestartState"],
      ) &&
      sources.tests.includes(
        "same_generation_restart_keeps_desired_generation",
      ) &&
      sources.tests.includes("restart_recovery_eligibility_is_exhaustive"),
    DIAGNOSTICS.nestedState,
  );

  const admission = extractItem(
    sources.workloads,
    "struct WorkloadRestartAdmission",
  );
  requireContract(
    hasAll(admission, [
      "saga_id: WorkloadSagaId",
      "source: WorkloadProvisionSourceEvidence",
      "generation: WorkloadGeneration",
      "desired_digest: WorkloadDesiredDigest",
      "revision: WorkloadSagaRevision",
      "trigger: WorkloadRestartTrigger",
      "inspection_version: Option<WorkloadInspectionVersion>",
      "provider_selection: WorkloadExecutionProviderId",
      "restart_epoch: WorkloadRestartEpoch",
      "policy_attempt_count: u32",
      "request_id: WorkloadRestartRequestId",
      "attempt_id: WorkloadExecutionAttemptId",
    ]),
    DIAGNOSTICS.admissionIdentity,
  );

  requireContract(
    hasAll(sources.compute, [
      "compare_and_swap_restart_admission",
      "automatic_and_explicit_restart_use_same_reducer",
    ]) && sources.tests.includes("concurrent_triggers_admit_one_restart_epoch"),
    DIAGNOSTICS.reducer,
  );

  const command = extractItem(
    sources.compute,
    "struct ConfirmedWorkloadRestartCommand",
  );
  requireContract(
    hasAll(command, [
      "saga_id: WorkloadSagaId",
      "generation: WorkloadGeneration",
      "attempt_id: WorkloadExecutionAttemptId",
      "restart_epoch: WorkloadRestartEpoch",
      "dispatch_epoch: WorkloadRestartDispatchEpoch",
      "issuing_revision: WorkloadSagaRevision",
      "inspection_version: SandboxInspectionVersion",
      "provider_selection: WorkloadExecutionProviderId",
    ]) &&
      sources.compute.includes("pub(crate) fn from_confirmation") &&
      !sources.compute.includes("pub fn new"),
    DIAGNOSTICS.command,
  );

  requireContract(
    hasAll(sources.compute, [
      "claim_restart_command",
      "inspect_ambiguous_restart",
      "retry_after_authenticated_absence",
    ]) &&
      sources.tests.includes(
        "crash_after_restart_effect_inspects_before_retry",
      ),
    DIAGNOSTICS.ambiguity,
  );

  requireContract(
    hasAll(sources.workloads, [
      "not_before_unix_millis",
      "completed_automatic_restart_count",
    ]) &&
      hasAll(sources.tests, [
        "explicit_restart_does_not_consume_automatic_count",
        "deadline_survives_clock_rollback_without_early_admission",
        "deadline_survives_engine_reopen",
        "count_survives_engine_reopen",
      ]),
    DIAGNOSTICS.schedule,
  );

  requireContract(
    hasAll(sources.tests, [
      "withdrawal_vetoes_unissued_restart",
      "successor_vetoes_restart_before_admission",
    ]),
    DIAGNOSTICS.withdrawal,
  );

  requireContract(
    hasAll(sources.tests, [
      "activation_waits_for_same_generation_attachment_and_pep",
      "old_attempt_callback_is_rejected",
    ]),
    DIAGNOSTICS.readiness,
  );

  requireContract(
    hasAll(sources.compute, [
      "RestartPublicationWithdrawalCapability",
      "WorkloadExecutionQuiescenceCapability",
      "WorkloadRestartPreparationCapability",
      "for ContainerRestartAdapter",
      "for KrunRestartAdapter",
    ]) &&
      !/\b(?:trait|struct|enum)\s+(?:God)?RestartProvider\b/u.test(
        sources.compute,
      ),
    DIAGNOSTICS.capabilities,
  );

  requireContract(
    hasAll(sources.server, [
      "source_generation: WorkloadGeneration",
      "request_id: WorkloadRestartRequestId",
      "submit_service_restart",
    ]) &&
      hasAll(sources.sdk, [
        "services.restart",
        "/restart",
        "sourceGeneration",
        "requestId",
      ]) &&
      sources.tests.includes(
        "duplicate_service_request_returns_same_restart_epoch",
      ) &&
      !/submit_service_restart[\s\S]{0,300}\bstop_service\b[\s\S]{0,200}\bstart_service\b/u.test(
        sources.server,
      ),
    DIAGNOSTICS.service,
  );

  requireContract(
    hasAll(sources.compute, [
      "bounded_restart_watch",
      "load_durable_restart_page",
      "read_only_exit_hint",
    ]) &&
      hasAll(sources.tests, [
        "get_and_name_resolution_make_zero_restart_effects",
        "automatic_watch_is_bounded_and_does_not_busy_spin",
      ]),
    DIAGNOSTICS.watch,
  );

  const nodeLowering = extractItem(
    sources.node,
    "fn into_host_lifecycle_request",
  );
  const nodeRestartGuard = extractItem(
    sources.node,
    "fn ensure_external_restart_disabled",
  );
  requireContract(
    nodeLowering.includes("HostRestartPolicy::No") &&
      nodeRestartGuard.includes("!= HostRestartPolicy::No") &&
      `${sources.tests}\n${sources.node}`.includes(
        "reconciler_rejects_provider_restart_and_duplicates_before_backend_validation",
      ),
    DIAGNOSTICS.node,
  );

  const machineCommand = extractItem(
    sources.machine,
    "struct MachineRestartCommand",
  );
  requireContract(
    hasAll(machineCommand, [
      "saga_id: WorkloadSagaId",
      "generation: WorkloadGeneration",
      "attempt_id: WorkloadExecutionAttemptId",
      "restart_epoch: WorkloadRestartEpoch",
      "dispatch_epoch: WorkloadRestartDispatchEpoch",
      "inspection_version: SandboxInspectionVersion",
      "provider_selection: WorkloadExecutionProviderId",
    ]) && sources.tests.includes("machine_restart_wire_rejects_crossed_fences"),
    DIAGNOSTICS.machine,
  );

  requireContract(
    hasAll(sources.providers, [
      "RestartRetained",
      "retain_network_allocation",
      "retain_port_lease",
      "retain_attachment_identity",
      "retain_pep_authority",
    ]) &&
      !/\bnext_restart_at_millis\b|\bprovider_local_restart_scheduler\b/u.test(
        sources.providers,
      ),
    DIAGNOSTICS.scheduler,
  );

  requireContract(
    hasAll(sources.tests, [
      "fresh_process_restart_reopens_engine",
      "crash_after_restart_effect_inspects_before_retry",
      "cancellation_after_submission_preserves_durable_work",
      "compose_local_and_forwarded_restart_use_compute",
    ]),
    DIAGNOSTICS.behavior,
  );

  requireContract(
    !/\b(?:TcpListener|TcpStream|UdpSocket|SandboxBackend|RestartProvider)\b/u.test(
      sources.network,
    ),
    DIAGNOSTICS.network,
  );

  requireContract(
    sources.changedPaths.every(isAllowedPath) &&
      sources.r1ChangedPaths.every(isR1AllowedPath),
    DIAGNOSTICS.paths,
  );

  requireContract(
    hasAll(sources.plan, [
      "NNC6.4a",
      "A1-A20",
      "NNCV034",
      "candidate-frozen",
      "Sol/xhigh/fast",
    ]),
    DIAGNOSTICS.ledger,
  );

  return errors;
}

export const workloadRestartDiagnostics = DIAGNOSTICS;
