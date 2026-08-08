import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

import { maskNonCode, walkRust } from "./source-contract-scanner.mjs";
import { greenFixture } from "./workload-restart-contract-fixture.mjs";

// Ownership reason: this deep NNCV034 verifier owns one production scan and
// one sole-diagnostic mutation contract. Its sibling owns only the green
// fixture and reuses the same lexical scanner.

const AUDIT_CHECKPOINT =
  process.env.NIMBUS_NETWORK_NNC64A_AUDIT_CHECKPOINT ??
  "8723bc9a8ac27abc8ecbbd59d5f8d8d159e98cc1";
const R1_START_CHECKPOINT =
  process.env.NIMBUS_NETWORK_NNC64A_R1_START_CHECKPOINT ??
  "6d8961bd6d4da819b2524128cb398e22e0a9382f";
const R1_COMPLETE_CHECKPOINT =
  process.env.NIMBUS_NETWORK_NNC64A_R1_COMPLETE_CHECKPOINT ??
  "d117ba369eaf5acc5ede9ec3edad32a11ddfbeb2";
const R2_START_CHECKPOINT =
  process.env.NIMBUS_NETWORK_NNC64A_R2_CHECKPOINT ?? R1_COMPLETE_CHECKPOINT;
const R2_COMPLETE_CHECKPOINT =
  process.env.NIMBUS_NETWORK_NNC64A_R2_COMPLETE_CHECKPOINT ??
  "73f53796392eae1b7c6df06e15450f272e228710";
const R3_CHECKPOINT =
  process.env.NIMBUS_NETWORK_NNC64A_R3_CHECKPOINT ?? R2_COMPLETE_CHECKPOINT;

const ALLOWED_EXACT_PATHS = new Set([
  "crates/nimbus-workloads/src/lib.rs",
  "crates/nimbus-workloads/src/saga.rs",
  "crates/nimbus-workloads/src/store.rs",
  "crates/nimbus-workloads/src/store/tests.rs",
  "crates/nimbus-workloads/src/store/tests/restart_candidates.rs",
  "crates/nimbus-compute/src/workload_saga.rs",
  "crates/nimbus-compute/src/resource_provision/tests.rs",
  "crates/nimbus-compute/src/state.rs",
  "crates/nimbus-compute/src/workload_projection.rs",
  "crates/nimbus-compute/src/workload_projection/tests.rs",
  "crates/nimbus-compute/src/workload_provisioner.rs",
  "crates/nimbus-compute/src/services.rs",
  "crates/nimbus-cli/src/network_composition.rs",
  "crates/nimbus-network/src/port_lease/lifetime.rs",
  "crates/nimbus-network/src/port_lease/rebind.rs",
  "crates/nimbus-sandbox/src/backends/container/runtime.rs",
  "crates/nimbus-sandbox/src/backends/krun/vm.rs",
  "crates/nimbus-sandbox/src/execution_attempt.rs",
  "crates/nimbus-sandbox/src/provision.rs",
  "crates/nimbus-sandbox/src/provision/tests.rs",
  "crates/nimbus-sandbox/src/provider_command/tests.rs",
  "crates/nimbus-sandbox/tests/production_network_composition.rs",
  "crates/nimbus-sandbox/tests/support/provision.rs",
  "crates/nimbus-server/src/listener_lease.rs",
  "crates/nimbus-server/src/listener_lease/restart_retain.rs",
  "crates/nimbus-server/src/listener_lease/restart_retain/tests.rs",
  "crates/nimbus-server/src/tests/managed_workload.rs",
  "crates/nimbus-server/src/workload_ingress.rs",
  "crates/nimbus-server/src/workload_ingress/tests.rs",
  "crates/nimbus-server/src/workload_saga_store.rs",
  "crates/nimbus-server/src/workload_composition/tests.rs",
  "crates/nimbus-server/src/http/services.rs",
  "crates/nimbus-server/src/http/mod.rs",
  "crates/nimbus-server/src/router.rs",
  "crates/nimbus-server/src/tests/service_manager.rs",
  "crates/nimbus-server/src/tests/service_manager/restart.rs",
  "crates/nimbus-server/src/workload_composition.rs",
  "crates/nimbus-server/src/state.rs",
  "crates/nimbus-sandbox/src/inspection.rs",
  "crates/nimbus-sandbox/src/lib.rs",
  "crates/nimbus-sandbox/src/provider_command.rs",
  "crates/nimbus-services/src/catalog.rs",
  "crates/nimbus-machine/src/api.rs",
  "crates/nimbus-node/src/host_lifecycle.rs",
  "crates/nimbus-node/src/reconciler.rs",
  "crates/nimbus-node/src/direct_process.rs",
  "crates/nimbus-node/src/systemd_transient.rs",
  "crates/nimbus-system/src/inventory.rs",
  "packages/nimbus/src/selftest.mjs",
  "packages/nimbus/src/capability_surface_contract.mjs",
  "packages/nimbus/src/control_plane_routes.ts",
  "packages/nimbus/README.md",
  "scripts/nimbus-root-sdk-artifact-policy.mjs",
  "scripts/verify-nimbus-network-control-plane.sh",
  "scripts/verify-nimbus-network-source-contract.mjs",
  "scripts/nimbus-network-control-plane/source-contract-scanner.mjs",
  "scripts/nimbus-network-control-plane/workload-network-plan-durability-contract.sh",
  "scripts/nimbus-network-control-plane/workload-restart-contract.sh",
  "scripts/nimbus-network-control-plane/workload-restart-contract-fixture.mjs",
  "scripts/nimbus-network-control-plane/workload-restart-source-contract.mjs",
  "docs/private/plans/nimbus-network-control-plane-plan.md",
  "docs/private/plans/README.md",
  "docs/private/plans/proof/nimbus-network-control-plane/nnc6.4a-fenced-restart-substitution-audit.md",
]);

const ALLOWED_PREFIXES = [
  "crates/nimbus-workloads/src/saga/",
  "crates/nimbus-compute/src/workload_saga/",
  "crates/nimbus-compute/src/workload_provisioner/",
  "crates/nimbus-network/src/port_lease/rebind/",
  "crates/nimbus-services/src/manager/",
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

const R2_ALLOWED_EXACT_PATHS = new Set([
  "crates/nimbus-workloads/src/lib.rs",
  "crates/nimbus-workloads/src/saga.rs",
  "crates/nimbus-workloads/src/saga/restart.rs",
  "crates/nimbus-workloads/src/saga/restart/tests.rs",
  "crates/nimbus-workloads/src/saga/state.rs",
  "crates/nimbus-workloads/src/saga/state/restart.rs",
  "crates/nimbus-workloads/src/saga/tests.rs",
  "crates/nimbus-workloads/src/saga/tests/restart_state.rs",
  "crates/nimbus-workloads/src/store.rs",
  "crates/nimbus-workloads/src/store/tests.rs",
  "crates/nimbus-workloads/src/store/tests/restart_candidates.rs",
  "crates/nimbus-compute/src/workload_saga.rs",
  "crates/nimbus-compute/src/resource_provision/tests.rs",
  "crates/nimbus-compute/src/state.rs",
  "crates/nimbus-compute/src/workload_projection.rs",
  "crates/nimbus-compute/src/workload_projection/tests.rs",
  "crates/nimbus-compute/src/workload_provisioner/tests.rs",
  "crates/nimbus-cli/src/compose/tests/lifecycle.rs",
  "crates/nimbus-cli/src/machine/api/service_workloads/provision.rs",
  "crates/nimbus-cli/src/machine/api/service_workloads/provision/tests.rs",
  "crates/nimbus-cli/src/machine/backend/provision.rs",
  "crates/nimbus-cli/src/network_composition.rs",
  "crates/nimbus-sandbox/src/backends/container/runtime.rs",
  "crates/nimbus-sandbox/src/backends/krun/vm.rs",
  "crates/nimbus-sandbox/src/lib.rs",
  "crates/nimbus-sandbox/src/execution_attempt.rs",
  "crates/nimbus-sandbox/src/inspection.rs",
  "crates/nimbus-sandbox/src/provider_command.rs",
  "crates/nimbus-sandbox/src/provider_command/tests.rs",
  "crates/nimbus-sandbox/src/provision.rs",
  "crates/nimbus-sandbox/src/provision/tests.rs",
  "crates/nimbus-sandbox/tests/production_network_composition.rs",
  "crates/nimbus-sandbox/tests/support/provision.rs",
  "crates/nimbus-network/src/port_lease/lifetime.rs",
  "crates/nimbus-network/src/port_lease/rebind.rs",
  "crates/nimbus-server/src/listener_lease.rs",
  "crates/nimbus-server/src/listener_lease/restart_retain.rs",
  "crates/nimbus-server/src/listener_lease/restart_retain/tests.rs",
  "crates/nimbus-server/src/tests/managed_workload.rs",
  "crates/nimbus-server/src/workload_composition.rs",
  "crates/nimbus-server/src/workload_ingress.rs",
  "crates/nimbus-server/src/workload_ingress/tests.rs",
  "crates/nimbus-services/src/catalog.rs",
  "crates/nimbus-services/src/manager/handles.rs",
  "crates/nimbus-services/src/manager/retirement.rs",
  "crates/nimbus-services/src/manager/sandboxes.rs",
  "crates/nimbus-cli/src/machine/backend/provision/tests.rs",
  "crates/nimbus-server/src/workload_composition/tests.rs",
  "crates/nimbus-server/src/workload_saga_store.rs",
  "crates/nimbus-server/src/workload_saga_store/codec.rs",
  "crates/nimbus-server/src/workload_saga_store/restart_candidates.rs",
  "crates/nimbus-server/src/workload_saga_store/schema.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/codec.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/durability.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/ingress.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/mod.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/provision_driver_process.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/restart_candidates.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/restart.rs",
  "crates/nimbus-server/src/workload_saga_store/tests/store.rs",
  "scripts/nimbus-network-control-plane/workload-restart-contract.sh",
  "scripts/nimbus-network-control-plane/workload-restart-source-contract.mjs",
  "scripts/verify-nimbus-network-control-plane.sh",
  "docs/private/plans/nimbus-network-control-plane-plan.md",
  "docs/private/plans/README.md",
  "docs/private/plans/proof/nimbus-network-control-plane/nnc6.4a-fenced-restart-substitution-audit.md",
]);

const R2_ALLOWED_PREFIXES = [
  "crates/nimbus-compute/src/workload_saga/",
  "crates/nimbus-sandbox/src/backends/container/runtime/",
  "crates/nimbus-sandbox/src/backends/krun/vm/",
  "crates/nimbus-network/src/port_lease/rebind/",
  "crates/nimbus-services/src/manager/tests/",
];

const R3_ALLOWED_EXACT_PATHS = new Set([
  "crates/nimbus-compute/src/services.rs",
  "crates/nimbus-compute/src/state.rs",
  "crates/nimbus-compute/src/workload_saga.rs",
  "crates/nimbus-compute/src/workload_saga/restart_runtime.rs",
  "crates/nimbus-compute/src/workload_saga/restart_submission.rs",
  "crates/nimbus-compute/src/workload_saga/restart_submission/tests.rs",
  "crates/nimbus-server/src/http/mod.rs",
  "crates/nimbus-server/src/http/services.rs",
  "crates/nimbus-server/src/router.rs",
  "crates/nimbus-server/src/tests/managed_workload.rs",
  "crates/nimbus-server/src/tests/service_manager.rs",
  "crates/nimbus-server/src/tests/service_manager/restart.rs",
  "crates/nimbus-workloads/src/saga/state/restart.rs",
  "crates/nimbus-workloads/src/saga/tests/restart_state.rs",
  "docs/private/plans/README.md",
  "docs/private/plans/nimbus-network-control-plane-plan.md",
  "docs/private/plans/proof/nimbus-network-control-plane/nnc6.4a-fenced-restart-substitution-audit.md",
  "packages/nimbus/src/capability_surface_contract.mjs",
  "packages/nimbus/src/control-plane/client.ts",
  "packages/nimbus/src/control-plane/types.ts",
  "packages/nimbus/src/control_plane_routes.ts",
  "packages/nimbus/src/selftest.mjs",
  "scripts/nimbus-root-sdk-artifact-policy.mjs",
  "scripts/nimbus-network-control-plane/workload-restart-contract-fixture.mjs",
  "scripts/nimbus-network-control-plane/workload-restart-source-contract.mjs",
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
          sources.push({ file: relative, source: maskNonCode(source) });
        }
      }
    }
  };
  for (const directory of directories) visit(path.join(root, directory));
  return sources;
}

function productionSources(root) {
  const entries = new Map();
  const crateEntries = (name) => {
    const found = normalizeRustEntries(root, `crates/${name}/src`);
    for (const entry of found) entries.set(entry.file, entry.source);
    return found;
  };
  const workloads = crateEntries("nimbus-workloads");
  const compute = crateEntries("nimbus-compute");
  const providers = crateEntries("nimbus-sandbox");
  const server = crateEntries("nimbus-server");
  const node = crateEntries("nimbus-node");
  const machine = [
    ...crateEntries("nimbus-machine"),
    ...crateEntries("nimbus-cli"),
  ];
  const network = crateEntries("nimbus-network");
  const testEntries = collectTestSources(root, [
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
  ]);
  return {
    workloads: joinSources(workloads),
    compute: joinSources(compute),
    providers: joinSources(providers),
    server: joinSources(server),
    codec: readText(
      root,
      "crates/nimbus-server/src/workload_saga_store/codec.rs",
    ),
    sdk: [
      readText(root, "packages/nimbus/src/control-plane/client.ts"),
      readText(root, "packages/nimbus/src/control_plane_routes.ts"),
      readText(root, "packages/nimbus/src/selftest.mjs"),
      readText(root, "packages/nimbus/README.md"),
    ].join("\n"),
    node: joinSources(node),
    machine: joinSources(machine),
    network: joinSources(network),
    tests: joinSources(testEntries),
    testEntries,
    files: Object.fromEntries(entries),
    plan: [
      readText(root, "docs/private/plans/nimbus-network-control-plane-plan.md"),
      readText(
        root,
        "docs/private/plans/proof/nimbus-network-control-plane/nnc6.4a-fenced-restart-substitution-audit.md",
      ),
    ].join("\n"),
    changedPaths: changedPathsSince(root, AUDIT_CHECKPOINT),
    r1ChangedPaths: changedPathsBetween(
      root,
      R1_START_CHECKPOINT,
      R1_COMPLETE_CHECKPOINT,
    ),
    r2ChangedPaths: changedPathsBetween(
      root,
      R2_START_CHECKPOINT,
      R2_COMPLETE_CHECKPOINT,
    ),
    r3ChangedPaths: changedPathsSince(root, R3_CHECKPOINT),
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

function changedPathsBetween(root, startCheckpoint, endCheckpoint) {
  const tracked = execFileSync(
    "git",
    ["diff", "--name-only", `${startCheckpoint}..${endCheckpoint}`, "--"],
    { cwd: root, encoding: "utf8" },
  );
  return [...new Set(tracked.split("\n").filter(Boolean))];
}

function replaceOnce(sources, area, before, after) {
  if (!sources[area].includes(before)) {
    throw new Error(`restart mutation target missing: ${area}:${before}`);
  }
  sources[area] = sources[area].replace(before, after);
  if (area === "tests") {
    const owner = sources.testEntries.find((entry) =>
      entry.source.includes(before),
    );
    if (!owner) {
      throw new Error(`restart test mutation owner missing: ${before}`);
    }
    owner.source = owner.source.replace(before, after);
  }
}

function replaceOnceInFile(sources, file, before, after) {
  const source = sources.files[file] ?? "";
  if (!source.includes(before)) {
    throw new Error(`restart file mutation target missing: ${file}:${before}`);
  }
  sources.files[file] = source.replace(before, after);
  sources.compute = Object.entries(sources.files)
    .filter(([candidate]) => candidate.startsWith("crates/nimbus-compute/"))
    .map(([, candidate]) => candidate)
    .join("\n");
}

function sourceAt(sources, file) {
  return sources.files[file] ?? "";
}

function testSourceAt(sources, file) {
  return sources.testEntries.find((entry) => entry.file === file)?.source ?? "";
}

function hasTestsAt(sources, file, testNames) {
  return hasAll(testSourceAt(sources, file), testNames);
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
  const decisionFile =
    "crates/nimbus-compute/src/workload_saga/restart_decision.rs";
  const dispatchFile =
    "crates/nimbus-compute/src/workload_saga/restart_dispatch.rs";
  const restartStateFile = "crates/nimbus-workloads/src/saga/state/restart.rs";
  const providerFile =
    "crates/nimbus-compute/src/workload_saga/restart_provider.rs";
  const sandboxFile =
    "crates/nimbus-compute/src/workload_saga/restart_sandbox.rs";
  const watchFile = "crates/nimbus-compute/src/workload_saga/restart_watch.rs";
  const serviceFacadeFile = "crates/nimbus-compute/src/services.rs";
  const serviceRouteFile = "crates/nimbus-server/src/http/services.rs";

  const fileMutations = {
    "local-stop-start": [
      serviceFacadeFile,
      "async fn submit_service_restart() {",
      "async fn submit_service_restart() { stop_service(); start_service();",
    ],
    "missing-api-idempotency": [serviceRouteFile, "request_id: String,", ""],
    "separate-explicit-reducer": [
      decisionFile,
      "admit_explicit_restart(record, request)",
      "decide_explicit_restart_separately(record, request)",
    ],
    "stale-admission-revision": [
      decisionFile,
      "require_exact_revision(record.revision(), request.source_revision())?;",
      "let _stale_revision = request.source_revision();",
    ],
    "double-admission-winner": [
      decisionFile,
      "self.commit_loaded(Some(&current), candidate.clone()).await?;",
      "self.write_without_compare(Some(&current), candidate.clone()).await?;",
    ],
    "withdrawal-race-after-read": [
      decisionFile,
      "reject_withdrawal_or_successor(record)?;",
      "allow_withdrawal_race_after_read(record)?;",
    ],
    "missing-command-transition-id": [
      dispatchFile,
      "transition_id: WorkloadSagaTransitionId,",
      "",
    ],
    "missing-command-desired-digest": [
      dispatchFile,
      "desired_digest: WorkloadDesiredDigest,",
      "",
    ],
    "missing-command-request-id": [
      dispatchFile,
      "request_id: WorkloadRestartRequestId,",
      "",
    ],
    "missing-command-source-execution": [
      dispatchFile,
      "source_execution: WorkloadExecutionReference,",
      "",
    ],
    "missing-command-target-execution": [
      dispatchFile,
      "execution: WorkloadExecutionReference,",
      "",
    ],
    "crossed-command-result": [
      dispatchFile,
      "authenticate_result_transition(record, command, &result)?;",
      "accept_crossed_result_transition(record, command, &result)?;",
    ],
    "execute-on-confirmed-replay": [
      dispatchFile,
      "WorkloadSagaConfirmation::ConfirmedAfterAmbiguity | WorkloadSagaConfirmation::ConfirmedReplay",
      "WorkloadSagaConfirmation::ConfirmedAfterAmbiguity",
    ],
    "ambiguity-infers-absence": [
      dispatchFile,
      "WorkloadRestartCommandOutcome::AuthenticatedAbsent { evidence } => retry_after_authenticated_absence(record, command, evidence)",
      "WorkloadRestartCommandOutcome::AuthenticatedAbsent { evidence } => retry_without_inspection(record, command, evidence)",
    ],
    "absence-retry-changes-attempt": [
      dispatchFile,
      "record.restart_inspection_to_retry(command.claim(), absence)?;",
      "record.restart_with_new_attempt(command.claim(), absence)?;",
    ],
    "absence-retry-reuses-dispatch-epoch": [
      dispatchFile,
      "record.restart_inspection_to_retry(command.claim(), absence)?;",
      "record.restart_retry_same_epoch(command.claim(), absence)?;",
    ],
    "absence-retry-skips-dispatch-epoch": [
      dispatchFile,
      "record.restart_inspection_to_retry(command.claim(), absence)?;",
      "record.restart_retry_skipped_epoch(command.claim(), absence)?;",
    ],
    "definite-failure-continues": [
      dispatchFile,
      "stop_restart_dispatch(candidate)",
      "retry_after_authenticated_absence(command)",
    ],
    "quiesce-before-publication-withdrawal": [
      restartStateFile,
      "WorkloadRestartStep::WithdrawPublication => Some(WorkloadRestartPhase::ExecutionQuiescencePending)",
      "WorkloadRestartStep::WithdrawPublication => Some(WorkloadRestartPhase::Scheduled)",
    ],
    "restart-detach-releases-authority": [
      restartStateFile,
      "WorkloadRestartStep::PrepareExecution => Some(WorkloadRestartPhase::AttachmentPending)",
      "WorkloadRestartStep::PrepareExecution => Some(WorkloadRestartPhase::ActivationPending)",
    ],
    "attachment-drops-attempt-fence": [
      restartStateFile,
      "WorkloadRestartStep::AttachNetwork => Some(WorkloadRestartPhase::ActivationPrerequisitePending)",
      "WorkloadRestartStep::AttachNetwork => Some(WorkloadRestartPhase::ActivationPending)",
    ],
    "publish-before-new-attempt-ready": [
      restartStateFile,
      "WorkloadRestartStep::InspectReadiness => Some(WorkloadRestartPhase::PublicationPending)",
      "WorkloadRestartStep::InspectReadiness => Some(WorkloadRestartPhase::ObservationPending)",
    ],
    "missing-container-restart-adapter": [
      sandboxFile,
      "impl_sandbox_restart_capabilities!(ContainerProvisionAdapter);",
      "",
    ],
    "missing-krun-restart-adapter": [
      sandboxFile,
      "impl_sandbox_restart_capabilities!(KrunProvisionAdapter);",
      "",
    ],
    "restart-registry-first-available-fallback": [
      providerFile,
      "self.providers.get(&realm).ok_or_else",
      "self.providers.values().next().ok_or_else",
    ],
    "duplicate-restart-capability-registration": [
      providerFile,
      "if self.providers.insert(realm.clone(), capabilities).is_some()",
      "if self.providers.contains_key(&realm)",
    ],
    "unbounded-watch-page": [
      watchFile,
      "page_size: NonZeroUsize,",
      "page_size: usize,",
    ],
    "watch-busy-spin": [
      watchFile,
      "self.clock.wait_until(deadline, &self.cancellation).await;",
      "continue;",
    ],
    "watch-uses-system-clock": [
      watchFile,
      "self.clock.now_unix_millis()",
      "SystemTime::now()",
    ],
    "watch-effects-from-read-only-hint": [
      watchFile,
      "RestartHint::ReadOnly",
      "execute_provider(); RestartHint::ReadOnly",
    ],
    "get-starts-restart-watch": [
      watchFile,
      "fn read_only_exit_hint()",
      "fn get_service() { bounded_restart_watch(); }\nfn read_only_exit_hint()",
    ],
    "watch-cancellation-drops-durable-work": [
      watchFile,
      "if self.cancellation.is_cancelled() { break; }",
      "if self.cancellation.is_cancelled() { store.delete_restart()?; break; }",
    ],
    "unbounded-watch-sweep": [
      watchFile,
      "while pages < MAX_RESTART_PAGES_PER_SWEEP {",
      "loop {",
    ],
  };
  if (mutation in fileMutations) {
    replaceOnceInFile(sources, ...fileMutations[mutation]);
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
    "pep-drops-attempt-fence": [
      "tests",
      "activation_waits_for_same_generation_attachment_and_pep",
      "activation_ignores_pep_readiness",
    ],
    "old-attempt-callback": [
      "tests",
      "old_attempt_provider_observation_is_rejected_before_result",
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
  } else if (mutation === "forgeable-constructor") {
    replaceOnceInFile(
      sources,
      dispatchFile,
      "fn from_confirmation",
      "pub fn new",
    );
  } else if (mutation === "bypass-admission-cas") {
    replaceOnceInFile(
      sources,
      decisionFile,
      "compare_and_swap_restart_admission",
      "restart_without_admission",
    );
  } else if (mutation === "direct-ambiguity-retry") {
    replaceOnceInFile(
      sources,
      dispatchFile,
      "fn inspect_ambiguous_restart",
      "retry_ambiguous_restart",
    );
  } else if (mutation === "god-provider") {
    replaceOnceInFile(
      sources,
      providerFile,
      "trait RestartPublicationWithdrawalCapability",
      "trait RestartProvider",
    );
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

function hasField(source, name, type) {
  const escapedName = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const escapedType = type.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(
    `^\\s*${escapedName}\\s*:\\s*${escapedType}\\s*,\\s*$`,
    "mu",
  ).test(source);
}

function appearsInOrder(source, tokens) {
  let cursor = 0;
  for (const token of tokens) {
    const found = source.indexOf(token, cursor);
    if (found < 0) return false;
    cursor = found + token.length;
  }
  return true;
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

function isR2AllowedPath(candidate) {
  return (
    R2_ALLOWED_EXACT_PATHS.has(candidate) ||
    R2_ALLOWED_PREFIXES.some((prefix) => candidate.startsWith(prefix))
  );
}

function isR3AllowedPath(candidate) {
  return R3_ALLOWED_EXACT_PATHS.has(candidate);
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

  const restartDecisionSource = sourceAt(
    sources,
    "crates/nimbus-compute/src/workload_saga/restart_decision.rs",
  );
  const restartDecision = extractItem(
    restartDecisionSource,
    "fn decide_restart_admission",
  );
  const restartAdmissionCas = extractItem(
    restartDecisionSource,
    "fn compare_and_swap_restart_admission",
  );
  requireContract(
    hasAll(restartDecision, [
      "require_exact_revision",
      "require_exact_generation",
      "require_exact_desired_digest",
      "require_exact_inspection_version",
      "require_exact_provider_selection",
      "reject_withdrawal_or_successor",
      "WorkloadRestartTrigger::Automatic",
      "WorkloadRestartTrigger::Explicit",
      "admit_automatic_restart",
      "admit_explicit_restart",
    ]) &&
      hasAll(restartAdmissionCas, [
        "decide_restart_admission",
        "commit_loaded",
      ]) &&
      hasTestsAt(
        sources,
        "crates/nimbus-compute/src/workload_saga/restart_decision/tests.rs",
        [
          "automatic_and_explicit_restart_use_same_reducer",
          "concurrent_triggers_admit_one_restart_epoch",
          "crossed_admission_fences_fail_before_cas",
          "withdrawal_winning_before_admission_vetoes_cas",
          "successor_winning_before_admission_vetoes_cas",
          "explicit_restart_does_not_increment_automatic_count",
          "deadline_not_due_returns_wait_without_effect",
          "cancellation_before_submission_makes_zero_store_and_provider_calls",
        ],
      ),
    DIAGNOSTICS.reducer,
  );

  const restartDispatchSource = sourceAt(
    sources,
    "crates/nimbus-compute/src/workload_saga/restart_dispatch.rs",
  );
  const command = extractItem(
    restartDispatchSource,
    "struct ConfirmedWorkloadRestartCommand",
  );
  const commandImpl = extractItem(
    restartDispatchSource,
    "impl ConfirmedWorkloadRestartCommand",
  );
  const claimRestartCommand = extractItem(
    restartDispatchSource,
    "fn claim_restart_command",
  );
  const applyRestartResult = extractItem(
    restartDispatchSource,
    "fn apply_restart_result",
  );
  const compareAndSwapRestartResult = extractItem(
    restartDispatchSource,
    "fn compare_and_swap_restart_result",
  );
  const commandFields = [
    ["command_id", "WorkloadRestartCommandId"],
    ["key", "WorkloadSagaKey"],
    ["saga_id", "WorkloadSagaId"],
    ["transition_id", "WorkloadSagaTransitionId"],
    ["generation", "WorkloadGeneration"],
    ["desired_digest", "WorkloadDesiredDigest"],
    ["source", "WorkloadProvisionSourceEvidence"],
    ["source_execution", "WorkloadExecutionReference"],
    ["execution", "WorkloadExecutionReference"],
    ["restart_epoch", "WorkloadRestartEpoch"],
    ["dispatch_epoch", "WorkloadRestartDispatchEpoch"],
    ["request_id", "WorkloadRestartRequestId"],
    ["issuing_revision", "WorkloadSagaRevision"],
    ["confirmed_revision", "WorkloadSagaRevision"],
    ["inspection_version", "Option<WorkloadInspectionVersion>"],
    ["provider_selection", "WorkloadExecutionProviderId"],
    ["step", "WorkloadRestartStep"],
    ["mode", "WorkloadRestartCommandMode"],
    ["claim", "WorkloadRestartCommandClaim"],
    ["executable", "WorkloadExecutableIntent"],
    ["compiled_network_plan", "CompiledWorkloadNetworkPlan"],
  ];
  requireContract(
    commandFields.every(([name, type]) => hasField(command, name, type)) &&
      hasAll(commandImpl, [
        "fn from_confirmation",
        "authenticate_exact_restart_confirmation",
        "WorkloadSagaConfirmation::AppliedByThisCall",
        "WorkloadRestartCommandMode::Execute",
        "fn source_attempt_id",
        "self.source_execution.attempt_id()",
        "fn attempt_id",
        "self.execution.attempt_id()",
      ]) &&
      !/\bpub(?:\([^)]*\))?\s+fn\s+(?:new|from_confirmation)\b/u.test(
        commandImpl,
      ) &&
      hasAll(claimRestartCommand, [
        "confirm_transition",
        "WorkloadSagaConfirmation::ConfirmedAfterAmbiguity",
        "WorkloadSagaConfirmation::ConfirmedReplay",
        "inspect_ambiguous_restart",
        "WorkloadRestartSymbolicAction::StartExactAttempt",
      ]) &&
      hasAll(applyRestartResult, [
        "authenticate_result_transition",
        "authenticate_result_attempt",
        "authenticate_result_dispatch_epoch",
      ]) &&
      hasAll(compareAndSwapRestartResult, [
        "confirm_transition",
        "proposed.candidate()",
      ]) &&
      hasTestsAt(
        sources,
        "crates/nimbus-compute/src/workload_saga/restart_dispatch/tests.rs",
        ["confirmed_restart_command_is_private_and_complete"],
      ),
    DIAGNOSTICS.command,
  );

  const inspectAmbiguousRestart = extractItem(
    restartDispatchSource,
    "fn inspect_ambiguous_restart",
  );
  const retryAfterAbsence = extractItem(
    restartDispatchSource,
    "fn retry_after_authenticated_absence",
  );
  requireContract(
    hasAll(claimRestartCommand, [
      "confirm_transition",
      "inspect_ambiguous_restart",
    ]) &&
      hasAll(inspectAmbiguousRestart, [
        "restart_dispatch_to_inspection",
        "confirm_transition",
        "InspectExactAttempt",
        "from_confirmation",
      ]) &&
      hasAll(applyRestartResult, [
        "WorkloadRestartCommandOutcome::AuthenticatedAbsent",
        "WorkloadRestartCommandOutcome::Ambiguous",
        "WorkloadRestartCommandOutcome::InProgress",
        "WorkloadRestartCommandOutcome::Succeeded",
        "WorkloadRestartCommandOutcome::DefiniteFailure",
        "retry_after_authenticated_absence",
        "stop_restart_dispatch",
      ]) &&
      hasAll(retryAfterAbsence, [
        "WorkloadRestartAbsenceEvidence::for_inspection",
        "restart_inspection_to_retry",
        "StartExactAttempt",
      ]) &&
      !/restart_(?:with_new_attempt|retry_same_epoch|retry_skipped_epoch)/u.test(
        retryAfterAbsence,
      ) &&
      hasTestsAt(
        sources,
        "crates/nimbus-compute/src/workload_saga/restart_dispatch/tests.rs",
        [
          "direct_claim_cas_winner_alone_executes",
          "confirmed_replay_does_not_execute",
          "ambiguous_claim_cas_fresh_reads_before_effect",
          "crash_after_restart_effect_inspects_before_retry",
          "authenticated_absence_retries_same_attempt_at_next_dispatch_epoch",
          "in_progress_never_retries",
          "definite_failure_stops_later_commands",
          "crossed_restart_result_is_rejected",
          "reused_skipped_and_crossed_dispatch_epochs_fail_closed",
        ],
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

  const restartStateSource = sourceAt(
    sources,
    "crates/nimbus-workloads/src/saga/state/restart.rs",
  );
  const restartStepForPhase = extractItem(
    restartStateSource,
    "fn restart_step_for_phase",
  );
  const restartTargetForStep = extractItem(
    restartStateSource,
    "fn restart_target_for_step",
  );
  const restartDriverSource = sourceAt(
    sources,
    "crates/nimbus-compute/src/workload_saga/restart_driver.rs",
  );
  const driveConfirmedRestart = extractItem(
    restartDriverSource,
    "fn drive_confirmed_restart",
  );
  const restartDispatcherSource = sourceAt(
    sources,
    "crates/nimbus-compute/src/workload_saga/restart_dispatcher.rs",
  );
  const dispatchConfirmed = extractItem(
    restartDispatcherSource,
    "fn dispatch_confirmed",
  );
  requireContract(
    hasAll(restartStepForPhase, [
      "PublicationWithdrawalPending =>",
      "WorkloadRestartStep::WithdrawPublication",
      "ExecutionQuiescencePending =>",
      "WorkloadRestartStep::QuiesceExecution",
      "PreparationPending =>",
      "WorkloadRestartStep::PrepareExecution",
      "AttachmentPending =>",
      "WorkloadRestartStep::AttachNetwork",
      "ActivationPrerequisitePending =>",
      "WorkloadRestartStep::InspectActivationPrerequisites",
      "ActivationPending =>",
      "WorkloadRestartStep::ActivateExecution",
      "ReadinessPending =>",
      "WorkloadRestartStep::InspectReadiness",
      "PublicationPending =>",
      "WorkloadRestartStep::Publish",
      "ObservationPending =>",
      "WorkloadRestartStep::ObservePublication",
    ]) &&
      hasAll(restartTargetForStep, [
        "WorkloadRestartStep::WithdrawPublication =>",
        "Some(WorkloadRestartPhase::ExecutionQuiescencePending)",
        "WorkloadRestartStep::QuiesceExecution => Some(WorkloadRestartPhase::Scheduled)",
        "WorkloadRestartStep::PrepareExecution => Some(WorkloadRestartPhase::AttachmentPending)",
        "WorkloadRestartStep::AttachNetwork =>",
        "Some(WorkloadRestartPhase::ActivationPrerequisitePending)",
        "WorkloadRestartStep::InspectActivationPrerequisites =>",
        "Some(WorkloadRestartPhase::ActivationPending)",
        "WorkloadRestartStep::ActivateExecution => Some(WorkloadRestartPhase::ReadinessPending)",
        "WorkloadRestartStep::InspectReadiness => Some(WorkloadRestartPhase::PublicationPending)",
        "WorkloadRestartStep::Publish => Some(WorkloadRestartPhase::ObservationPending)",
        "WorkloadRestartStep::ObservePublication => None",
      ]) &&
      hasAll(driveConfirmedRestart, [
        "decide_restart_progress",
        "confirm_transition",
        "dispatch_confirmed",
        "apply_restart_result",
        "compare_and_swap_restart_result",
      ]) &&
      hasAll(dispatchConfirmed, [
        "capabilities.invoke(command)",
        "observation.matches_command(command)",
        "CrossedProviderObservation",
      ]) &&
      sources.tests.includes("restart_legal_transition_matrix_is_exhaustive") &&
      hasTestsAt(
        sources,
        "crates/nimbus-compute/src/workload_saga/restart_driver/tests.rs",
        [
          "publication_withdrawal_precedes_execution_quiescence",
          "restart_retained_detach_precedes_attachment",
          "activation_waits_for_same_generation_attachment_and_pep",
          "readiness_binds_the_new_execution_attempt",
          "publication_waits_for_new_attempt_readiness",
          "withdrawal_after_admission_vetoes_unissued_command",
          "withdrawal_after_ambiguous_effect_requires_inspection",
        ],
      ) &&
      hasTestsAt(
        sources,
        "crates/nimbus-compute/src/workload_saga/restart_dispatcher/tests.rs",
        ["old_attempt_provider_observation_is_rejected_before_result"],
      ),
    DIAGNOSTICS.readiness,
  );

  const restartProviderSource = sourceAt(
    sources,
    "crates/nimbus-compute/src/workload_saga/restart_provider.rs",
  );
  const restartSandboxSource = sourceAt(
    sources,
    "crates/nimbus-compute/src/workload_saga/restart_sandbox.rs",
  );
  const restartIngressSource = sourceAt(
    sources,
    "crates/nimbus-server/src/workload_ingress.rs",
  );
  const restartCapabilityNames = [
    "RestartPublicationWithdrawalCapability",
    "WorkloadExecutionQuiescenceCapability",
    "WorkloadRestartPreparationCapability",
    "NetworkRestartAttachmentCapability",
    "WorkloadRestartActivationPrerequisiteCapability",
    "WorkloadRestartActivationCapability",
    "WorkloadRestartReadinessCapability",
    "RestartPublicationCapability",
    "RestartPublicationObservationCapability",
  ];
  const restartCapabilitiesAreObjectSafe = restartCapabilityNames.every(
    (name) => {
      const trait = extractItem(restartProviderSource, `trait ${name}`);
      return (
        hasAll(trait, [
          "Send + Sync",
          "&self",
          "&ConfirmedWorkloadRestartCommand",
        ]) && !/\bfn\s+\w+\s*</u.test(trait)
      );
    },
  );
  const registerRestartCapabilities = extractItem(
    restartProviderSource,
    "fn register_restart_capabilities",
  );
  const resolveRestartCapabilities = extractItem(
    restartProviderSource,
    "fn resolve_restart_capabilities",
  );
  requireContract(
    restartCapabilitiesAreObjectSafe &&
      hasAll(restartSandboxSource, [
        "macro_rules! impl_sandbox_restart_capabilities",
        "impl WorkloadExecutionQuiescenceCapability for $adapter",
        "impl WorkloadRestartPreparationCapability for $adapter",
        "impl NetworkRestartAttachmentCapability for $adapter",
        "impl WorkloadRestartActivationPrerequisiteCapability for $adapter",
        "impl WorkloadRestartActivationCapability for $adapter",
        "impl WorkloadRestartReadinessCapability for $adapter",
        "impl_sandbox_restart_capabilities!(ContainerProvisionAdapter)",
        "impl_sandbox_restart_capabilities!(KrunProvisionAdapter)",
      ]) &&
      hasAll(restartIngressSource, [
        "impl RestartPublicationWithdrawalCapability for ServerIngressPublicationAdapter",
        "impl RestartPublicationCapability for ServerIngressPublicationAdapter",
        "impl RestartPublicationObservationCapability for ServerIngressPublicationAdapter",
      ]) &&
      hasAll(registerRestartCapabilities, [
        "insert(realm.clone(), capabilities).is_some()",
        "DuplicateProviderSelection",
      ]) &&
      hasAll(resolveRestartCapabilities, [
        "providers.get(&realm)",
        "matches_command(command)",
        "MissingProviderSelection",
        "CrossedProviderRealm",
      ]) &&
      !/\b(?:values|iter)\s*\(\s*\)\s*\.\s*(?:next|find)\b/u.test(
        resolveRestartCapabilities,
      ) &&
      hasTestsAt(
        sources,
        "crates/nimbus-compute/src/workload_saga/restart_provider/tests.rs",
        [
          "restart_registry_rejects_duplicate_provider_selection",
          "restart_registry_has_no_first_available_fallback",
        ],
      ) &&
      hasTestsAt(
        sources,
        "crates/nimbus-compute/src/workload_saga/restart_sandbox/tests.rs",
        [
          "container_restart_quiescence_capability_authenticates_command",
          "container_restart_preparation_retains_authority_and_binds_attempt",
          "krun_restart_quiescence_capability_authenticates_command",
          "krun_restart_preparation_retains_authority_and_binds_attempt",
          "real_restart_adapters_reject_crossed_provider_attempt_and_inspection",
          "concurrent_restart_dispatch_produces_one_provider_effect",
        ],
      ) &&
      !/\b(?:trait|struct|enum)\s+(?:God)?RestartProvider\b/u.test(
        sources.compute,
      ),
    DIAGNOSTICS.capabilities,
  );

  const explicitSubmissionSource = sourceAt(
    sources,
    "crates/nimbus-compute/src/workload_saga/restart_submission.rs",
  );
  const submitExplicitRestart = extractItem(
    explicitSubmissionSource,
    "async fn submit",
  );
  const serviceFacadeSource = sourceAt(
    sources,
    "crates/nimbus-compute/src/services.rs",
  );
  const submitServiceRestart = extractItem(
    serviceFacadeSource,
    "async fn submit_service_restart",
  );
  const serviceRouteSource = sourceAt(
    sources,
    "crates/nimbus-server/src/http/services.rs",
  );
  requireContract(
    hasAll(serviceRouteSource, [
      "source_generation: u64",
      "request_id: String",
      "StatusCode::ACCEPTED",
    ]) &&
      hasAll(submitExplicitRestart, [
        "compare_and_swap_restart_admission",
        ".track(",
      ]) &&
      hasAll(submitServiceRestart, [
        "WorkloadProvisionSourceGeneration",
        "WorkloadProvisionSourceIdentity::sandbox_backed_service",
        "WorkloadSagaKey::new",
        "ExplicitWorkloadRestartRequest::new",
        "submit_explicit",
        "submit_service_restart",
      ]) &&
      hasAll(sources.sdk, [
        "services.restart",
        "/restart",
        "sourceGeneration",
        "requestId",
      ]) &&
      hasAll(sources.tests, [
        "completed_explicit_request_replay_returns_the_same_restart_epoch",
        "completed_explicit_request_rejects_crossed_admission_content",
        "duplicate_service_request_returns_same_restart_epoch",
      ]) &&
      !/\bstop_service\b|\bstart_service\b/u.test(submitServiceRestart),
    DIAGNOSTICS.service,
  );

  const restartWatchSource = sourceAt(
    sources,
    "crates/nimbus-compute/src/workload_saga/restart_watch.rs",
  );
  const restartClock = extractItem(restartWatchSource, "trait RestartClock");
  const restartWatch = extractItem(
    restartWatchSource,
    "struct DurableRestartWatch",
  );
  const loadRestartPage = extractItem(
    restartWatchSource,
    "fn load_durable_restart_page",
  );
  const dispatchRestartSweep = extractItem(
    restartWatchSource,
    "fn dispatch_each_due_epoch_once",
  );
  const boundedRestartWatch = extractItem(
    restartWatchSource,
    "fn bounded_restart_watch",
  );
  const readOnlyExitHint = extractItem(
    restartWatchSource,
    "fn read_only_exit_hint",
  );
  requireContract(
    hasAll(restartClock, [
      "now_unix_millis",
      "wait_until",
      "WorkloadRestartCancellationToken",
    ]) &&
      hasAll(restartWatch, [
        "page_size: NonZeroUsize",
        "clock: Arc<dyn RestartClock>",
        "cancellation: WorkloadRestartCancellationToken",
        "sweep_cursor: Mutex<Option<WorkloadRestartCandidateCursor>>",
      ]) &&
      restartWatchSource.includes("const MAX_RESTART_PAGES_PER_SWEEP: usize") &&
      hasAll(loadRestartPage, ["list_restart_candidates", "page_size"]) &&
      hasAll(dispatchRestartSweep, [
        "self.sweep_cursor.lock().await",
        "pages < MAX_RESTART_PAGES_PER_SWEEP",
        "self.load_durable_restart_page",
        "self.supervisor",
        ".track(record.clone())",
        "*retained_cursor = cursor",
      ]) &&
      hasAll(boundedRestartWatch, [
        "dispatch_each_due_epoch_once",
        "self.clock.now_unix_millis",
        "self.clock.wait_until",
        "self.cancellation.is_cancelled",
      ]) &&
      hasAll(readOnlyExitHint, ["RestartHint::ReadOnly"]) &&
      !/\b(?:SystemTime|Utc)::now\b|\b(?:execute|publish|attach|quiesce)_provider\b|\bdelete_restart\b|\bfn\s+(?:get|resolve_name)\w*\b[\s\S]{0,160}\bbounded_restart_watch\b/u.test(
        restartWatchSource,
      ) &&
      hasTestsAt(
        sources,
        "crates/nimbus-compute/src/workload_saga/restart_watch/tests.rs",
        [
          "automatic_watch_loads_one_bounded_durable_page",
          "automatic_watch_caps_each_sweep_and_rotates_cursor",
          "automatic_watch_does_not_busy_spin_before_deadline",
          "automatic_watch_dispatches_each_due_epoch_once",
          "read_only_exit_hint_cannot_submit_or_execute_restart",
          "watch_cancellation_cancels_waiter_not_durable_work",
          "get_and_name_resolution_make_zero_restart_effects",
        ],
      ),
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
      sources.r1ChangedPaths.every(isR1AllowedPath) &&
      sources.r2ChangedPaths.every(isR2AllowedPath) &&
      sources.r3ChangedPaths.every(isR3AllowedPath),
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
