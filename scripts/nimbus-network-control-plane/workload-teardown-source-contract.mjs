// Source-derived NNCV035 contract for the compute-owned workload teardown.
// The product scan and green mutation fixture share the repository scanner and
// the NNCV034 attributed-test assertion implementation.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

import { maskNonCode, walkRust } from "./source-contract-scanner.mjs";
import {
  BEHAVIOR_TESTS,
  FORWARDED_MACHINE_TESTS,
  NATIVE_SOURCE_RETIREMENT_TESTS,
  greenTeardownFixture,
} from "./workload-teardown-contract-fixture.mjs";
import {
  createTeardownAttributedTestChecker,
  remaskTeardownTestSources,
} from "./workload-teardown-test-assertion.mjs";

export const workloadTeardownDiagnostics = {
  vocabulary:
    "teardown-contract/vocabulary: portable teardown phase and reference vocabulary is incomplete or open",
  reducer:
    "teardown-contract/reducer: compute is not the sole fenced teardown CAS authority",
  command:
    "teardown-contract/command: confirmed teardown commands are forgeable or incompletely fenced",
  order:
    "teardown-contract/order: durable teardown does not enforce withdrawal then drain then stop then detach then release then record",
  service:
    "teardown-contract/service: service or sandbox stop retains direct provider-effect authority",
  definitionDelete:
    "teardown-contract/definition-delete: definition removal can cross unresolved or late lifecycle work",
  nativeSourceRetirement:
    "teardown-contract/native-source-retirement: native stop or definition deletion bypasses the exact compute teardown, source fence, generation split, or attributed proof",
  compose:
    "teardown-contract/compose: Compose down bypasses the canonical Engine-backed compute saga",
  machine:
    "teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences",
  forwardedMachineRegistry:
    "teardown-contract/forwarded-machine-registry: forwarded teardown does not expose the exact five-phase compute registry substitution",
  forwardedMachineLifecycle:
    "teardown-contract/forwarded-machine-lifecycle: parent and guest lifecycle authority is incomplete or not batch fenced",
  forwardedMachineRecovery:
    "teardown-contract/forwarded-machine-recovery: request-loss and two-realm crash recovery proofs are incomplete",
  ingress:
    "teardown-contract/ingress: final ingress withdrawal cannot prove exact worker, route, and lease settlement",
  tenant:
    "teardown-contract/tenant: tenant deletion bypasses durable child-saga teardown",
  compensation:
    "teardown-contract/compensation: provision or restart handoff lacks exact durable settlement and ambiguity handling",
  behavior:
    "teardown-contract/behavior: required teardown behavior proofs are incomplete or non-assertive",
  network:
    "teardown-contract/network: nimbus-network gained teardown effects or a god provider",
  paths:
    "teardown-contract/paths: NNC6.5 changed a path outside the frozen audit allowlist",
  ledger:
    "teardown-contract/ledger: plan and proof do not retain the NNC6.5 expected-red acceptance tokens",
};

const ALLOWED_PATHS = new Set([
  "docs/private/plans/README.md",
  "docs/private/plans/nimbus-network-control-plane-plan.md",
  "docs/private/plans/proof/nimbus-network-control-plane/nnc6.5-teardown-choreography-substitution-audit.md",
  "scripts/nimbus-network-control-plane/workload-teardown-contract-fixture.mjs",
  "scripts/nimbus-network-control-plane/workload-teardown-contract.sh",
  "scripts/nimbus-network-control-plane/workload-teardown-source-contract.mjs",
  "scripts/nimbus-network-control-plane/workload-teardown-test-assertion.mjs",
  "scripts/nimbus-network-control-plane/workload-restart-source-contract.mjs",
  "scripts/verify-nimbus-network-control-plane.sh",
  "scripts/verify-nimbus-network-source-contract.mjs",
]);

const AUDIT_START_CHECKPOINT = "26a02363c96af5204061c6a0d2c6f9311ffc9b49";

const REQUIRED_ORDER = [
  "WorkloadSagaPhase::WithdrawalCommitted",
  "WorkloadTeardownStep::WithdrawPublication",
  "WorkloadSagaPhase::Withdrawn",
  "WorkloadTeardownStep::DrainExecution",
  "WorkloadSagaPhase::Drained",
  "WorkloadTeardownStep::StopExecution",
  "WorkloadSagaPhase::WorkloadStopped",
  "WorkloadTeardownStep::DetachNetwork",
  "WorkloadSagaPhase::NetworkDetached",
  "WorkloadTeardownStep::ReleaseNetwork",
];

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

function currentChangedPaths(root) {
  const tracked = execFileSync("git", ["diff", "--name-only", "HEAD", "--"], {
    cwd: root,
    encoding: "utf8",
  });
  const untracked = execFileSync(
    "git",
    ["ls-files", "--others", "--exclude-standard"],
    { cwd: root, encoding: "utf8" },
  );
  return [...new Set(`${tracked}\n${untracked}`.split("\n").filter(Boolean))];
}

function completedItemCheckpoint(plan) {
  const row = plan
    .split("\n")
    .find((line) => line.startsWith("| NNC6.5 | `done` |"));
  return {
    complete: row !== undefined,
    checkpoint:
      row?.match(/\*\*Item commit:\*\* `([0-9a-f]{40})`/u)?.[1] ?? null,
  };
}

function changedPathsBetween(root, startCheckpoint, endCheckpoint) {
  try {
    return execFileSync(
      "git",
      ["diff", "--name-only", startCheckpoint, endCheckpoint, "--"],
      { cwd: root, encoding: "utf8" },
    )
      .split("\n")
      .filter(Boolean);
  } catch {
    return ["__invalid_nnc65_item_checkpoint__"];
  }
}

function auditPathSources(root, plan) {
  const item = completedItemCheckpoint(plan);
  return {
    auditItemComplete: item.complete,
    auditItemCompleteCheckpoint: item.checkpoint,
    currentChangedPaths: currentChangedPaths(root),
    historicalAuditChangedPaths: item.complete
      ? item.checkpoint
        ? changedPathsBetween(root, AUDIT_START_CHECKPOINT, item.checkpoint)
        : ["__invalid_nnc65_item_checkpoint__"]
      : [],
  };
}

function frozenAuditChangedPaths(sources) {
  return sources.auditItemComplete
    ? sources.historicalAuditChangedPaths
    : sources.currentChangedPaths;
}

function productionSources(root) {
  const crate = (name) => normalizeRustEntries(root, `crates/${name}/src`);
  const workloadEntries = crate("nimbus-workloads");
  const computeEntries = crate("nimbus-compute");
  const serviceEntries = crate("nimbus-services");
  const serverEntries = crate("nimbus-server");
  const cliEntries = crate("nimbus-cli");
  const machineEntries = crate("nimbus-machine");
  const sandboxEntries = crate("nimbus-sandbox");
  const networkEntries = crate("nimbus-network");
  const testEntries = collectTestSources(root, [
    "crates/nimbus-workloads/src",
    "crates/nimbus-workloads/tests",
    "crates/nimbus-compute/src",
    "crates/nimbus-compute/tests",
    "crates/nimbus-services/src",
    "crates/nimbus-server/src",
    "crates/nimbus-server/tests",
    "crates/nimbus-sandbox/src",
    "crates/nimbus-sandbox/tests",
    "crates/nimbus-network/src",
    "crates/nimbus-node/src",
    "crates/nimbus-machine/src",
    "crates/nimbus-cli/src",
  ]);
  const plan = [
    readText(root, "docs/private/plans/nimbus-network-control-plane-plan.md"),
    readText(
      root,
      "docs/private/plans/proof/nimbus-network-control-plane/nnc6.5-teardown-choreography-substitution-audit.md",
    ),
  ].join("\n");
  const nativeCallerPaths = [
    "crates/nimbus-compute/src/resource_retirement.rs",
    "crates/nimbus-compute/src/services.rs",
    "crates/nimbus-compute/src/sandboxes.rs",
    "crates/nimbus-services/src/manager/definitions.rs",
    "crates/nimbus-services/src/manager/definition_mutation.rs",
    "crates/nimbus-services/src/manager/source.rs",
    "crates/nimbus-services/src/manager/source_retirement.rs",
    "crates/nimbus-services/src/manager/sandboxes.rs",
    "crates/nimbus-server/src/http/services.rs",
    "crates/nimbus-server/src/http/sandboxes.rs",
  ];
  return {
    workloads: joinSources(workloadEntries),
    compute: joinSources(computeEntries),
    services: joinSources(serviceEntries),
    server: joinSources(serverEntries),
    cli: joinSources([...cliEntries, ...machineEntries]),
    sandbox: joinSources(sandboxEntries),
    network: joinSources(networkEntries),
    tests: joinSources(testEntries),
    testEntries,
    plan,
    nativeCallers: nativeCallerPaths
      .map((candidate) => readText(root, candidate, { lexical: true }))
      .join("\n"),
    nativeSourceRetirement: readText(
      root,
      "crates/nimbus-services/src/manager/source_retirement.rs",
      { lexical: true },
    ),
    provisionSettlementTests: readText(
      root,
      "crates/nimbus-compute/src/resource_retirement/tests/provision_settlement.rs",
      { lexical: true },
    ),
    provisionSettlementSupport: readText(
      root,
      "crates/nimbus-compute/src/resource_retirement/tests/support.rs",
      { lexical: true },
    ),
    computeState: readText(root, "crates/nimbus-compute/src/state.rs", {
      lexical: true,
    }),
    serverComposition: readText(
      root,
      "crates/nimbus-server/src/workload_composition.rs",
      { lexical: true },
    ),
    localComposition: readText(
      root,
      "crates/nimbus-cli/src/network_composition.rs",
      { lexical: true },
    ),
    httpServices: readText(root, "crates/nimbus-server/src/http/services.rs", {
      lexical: true,
    }),
    httpSandboxes: readText(
      root,
      "crates/nimbus-server/src/http/sandboxes.rs",
      { lexical: true },
    ),
    computeServices: readText(root, "crates/nimbus-compute/src/services.rs", {
      lexical: true,
    }),
    computeSandboxes: readText(root, "crates/nimbus-compute/src/sandboxes.rs", {
      lexical: true,
    }),
    resourceRetirement: readText(
      root,
      "crates/nimbus-compute/src/resource_retirement.rs",
      { lexical: true },
    ),
    serviceDefinitions: [
      "crates/nimbus-services/src/manager/definitions.rs",
      "crates/nimbus-services/src/manager/source_retirement.rs",
    ]
      .map((candidate) => readText(root, candidate, { lexical: true }))
      .join("\n"),
    serviceProjections: [
      "crates/nimbus-services/src/manager/handles.rs",
      "crates/nimbus-services/src/manager/sandboxes.rs",
      "crates/nimbus-services/src/manager/source_retirement.rs",
    ]
      .map((candidate) => readText(root, candidate, { lexical: true }))
      .join("\n"),
    ...auditPathSources(root, plan),
  };
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

const hasTestsAt = createTeardownAttributedTestChecker(extractItem);

function hasAll(source, tokens) {
  return tokens.every((token) => source.includes(token));
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

function enumVariants(source, name) {
  return extractItem(source, `enum ${name}`)
    .split("\n")
    .map((line) => line.match(/^\s*([A-Z][A-Za-z0-9_]*)\s*(?:[,({])/u)?.[1])
    .filter(Boolean);
}

function behaviorTestsPass(sources) {
  return nativeSourceRetirementTestsPass(sources) && [...new Set([
    ...BEHAVIOR_TESTS,
    ...NATIVE_SOURCE_RETIREMENT_TESTS,
  ])].every((name) =>
    sources.testEntries.some((entry) =>
      hasTestsAt(sources, entry.file, [name]),
    ),
  );
}

function nativeSourceRetirementTestsPass(sources) {
  const definitionTests = new Set(NATIVE_SOURCE_RETIREMENT_TESTS.slice(6, 12));
  const sessionTest =
    "session_binding_rejects_a_later_execution_with_the_same_source_generation";
  const contenderTest = "concurrent_start_and_stop_linearize_at_the_source_fence";
  return NATIVE_SOURCE_RETIREMENT_TESTS.every((name) => {
    const owns = (file) => {
      if (definitionTests.has(name)) {
        return file ===
          "crates/nimbus-server/src/tests/service_manager/definition_retirement.rs";
      }
      if (name === sessionTest) {
        return file === "crates/nimbus-services/src/manager/tests/sessions.rs";
      }
      if (name === contenderTest) {
        return file === "crates/nimbus-compute/src/workload_provisioner/tests.rs";
      }
      return (
        file === "crates/nimbus-compute/src/resource_retirement/tests.rs" ||
        file.startsWith("crates/nimbus-compute/src/resource_retirement/tests/")
      );
    };
    return sources.testEntries.some(
      (entry) => owns(entry.file) && hasTestsAt(sources, entry.file, [name]),
    );
  });
}

function forwardedMachineTestsPass(sources, names) {
  return names.every((name) =>
    sources.testEntries.some((entry) => hasTestsAt(sources, entry.file, [name])),
  );
}

function replaceOnce(sources, area, before, after) {
  if (!sources[area].includes(before)) {
    throw new Error(`teardown mutation target missing: ${area}:${before}`);
  }
  sources[area] = sources[area].replace(before, after);
}

function replaceOnceInTest(sources, before, after) {
  const entry = sources.testEntries.find((candidate) =>
    candidate.source.includes(before),
  );
  if (!entry)
    throw new Error(`teardown test mutation target missing: ${before}`);
  entry.source = entry.source.replace(before, after);
  remaskTeardownTestSources(sources);
}

function replaceOnceInNamedTest(sources, name, before, after) {
  const entry = sources.testEntries.find((candidate) =>
    candidate.source.includes(`fn ${name}`),
  );
  if (!entry) throw new Error(`teardown named test mutation target missing: ${name}`);
  if (!entry.source.includes(before)) {
    throw new Error(`teardown named test body mutation target missing: ${name}:${before}`);
  }
  entry.source = entry.source.replace(before, after);
  remaskTeardownTestSources(sources);
}

function applyFixtureMutation(sources, mutation) {
  const replacements = {
    "missing-phase": ["workloads", "    NetworkReleased,\n", ""],
    "missing-reference-set": [
      "workloads",
      "pub struct WorkloadEffectReferences;",
      "",
    ],
    "missing-attempt-id": [
      "workloads",
      "pub struct WorkloadTeardownAttemptId(String);",
      "",
    ],
    "missing-claim": ["workloads", "pub struct WorkloadTeardownClaim;", ""],
    "missing-reducer": [
      "compute",
      "fn materialize_teardown_candidate()",
      "fn omitted_teardown_candidate()",
    ],
    "missing-revision-fence": [
      "compute",
      "    confirm_transition();\n",
      "",
    ],
    "missing-commit-loaded": [
      "compute",
      "    confirm_teardown_transition();\n",
      "",
    ],
    "missing-command": [
      "compute",
      "struct ConfirmedWorkloadTeardownCommand",
      "struct UnconfirmedWorkloadTeardownCommand",
    ],
    "missing-command-transition": [
      "compute",
      "    confirmed_transition_id: WorkloadSagaTransitionId,\n",
      "",
    ],
    "missing-command-attempt": [
      "compute",
      "    fn attempt_id() -> WorkloadTeardownAttemptId {}\n",
      "",
    ],
    "missing-command-epoch": [
      "compute",
      "    fn dispatch_epoch() -> WorkloadTeardownDispatchEpoch {}\n",
      "",
    ],
    "forgeable-command": [
      "compute",
      "    fn from_confirmation()",
      "    pub fn from_confirmation()",
    ],
    "stop-before-withdraw": [
      "workloads",
      "    WorkloadSagaPhase::WithdrawalCommitted; WorkloadTeardownStep::WithdrawPublication;\n    WorkloadSagaPhase::Withdrawn; WorkloadTeardownStep::DrainExecution;\n    WorkloadSagaPhase::Drained; WorkloadTeardownStep::StopExecution;",
      "    WorkloadSagaPhase::Drained; WorkloadTeardownStep::StopExecution;\n    WorkloadSagaPhase::WithdrawalCommitted; WorkloadTeardownStep::WithdrawPublication;\n    WorkloadSagaPhase::Withdrawn; WorkloadTeardownStep::DrainExecution;",
    ],
    "detach-before-stop": [
      "workloads",
      "    WorkloadSagaPhase::Drained; WorkloadTeardownStep::StopExecution;\n    WorkloadSagaPhase::WorkloadStopped; WorkloadTeardownStep::DetachNetwork;",
      "    WorkloadSagaPhase::WorkloadStopped; WorkloadTeardownStep::DetachNetwork;\n    WorkloadSagaPhase::Drained; WorkloadTeardownStep::StopExecution;",
    ],
    "release-before-detach": [
      "workloads",
      "    WorkloadSagaPhase::WorkloadStopped; WorkloadTeardownStep::DetachNetwork;\n    WorkloadSagaPhase::NetworkDetached; WorkloadTeardownStep::ReleaseNetwork;",
      "    WorkloadSagaPhase::NetworkDetached; WorkloadTeardownStep::ReleaseNetwork;\n    WorkloadSagaPhase::WorkloadStopped; WorkloadTeardownStep::DetachNetwork;",
    ],
    "record-before-release": [
      "workloads",
      "WorkloadSagaPhase::NetworkReleased; ProposedWorkloadTeardownTransition::RecordTerminal;",
      "WorkloadSagaPhase::NetworkReleased; ProposedWorkloadTeardownTransition::PrematureTerminal;",
    ],
    "missing-service-submit": [
      "compute",
      "fn submit_service_teardown() {}",
      "",
    ],
    "missing-sandbox-submit": [
      "compute",
      "fn submit_sandbox_teardown() {}",
      "",
    ],
    "missing-service-projection": [
      "services",
      "fn project_recorded_service_teardown(source_generation: SourceGeneration, observed_execution_generation: WorkloadGeneration, execution: WorkloadExecutionReference) {}",
      "",
    ],
    "missing-sandbox-projection": [
      "services",
      "fn project_recorded_sandbox_teardown(source_generation: SourceGeneration, observed_execution_generation: WorkloadGeneration, execution: WorkloadExecutionReference) {}",
      "",
    ],
    "missing-definition-claim": [
      "services",
      "fn claim_service_definition_retirement() {}",
      "",
    ],
    "missing-provision-join": [
      "compute",
      "fn fence_and_join_inflight_provision() {}",
      "",
    ],
    "missing-late-result-drain": [
      "compute",
      "fn retire_late_provision_result() {}",
      "",
    ],
    "missing-definition-finalize": [
      "services",
      "fn finalize_service_definition_after_recorded() {}",
      "",
    ],
    "missing-native-runtime": [
      "computeState",
      "WorkloadTeardownRuntime::new(capabilities)",
      "RemovedTeardownRuntime::build(capabilities)",
    ],
    "missing-native-local-registry": [
      "localComposition",
      "    ServerWorkloadProviders::new().with_teardown_capabilities(teardown);\n",
      "",
    ],
    "missing-native-restart-settlement": [
      "resourceRetirement",
      "fn settle_issued_restart_before_native_teardown() {}",
      "",
    ],
    "missing-native-execution-reference": [
      "serviceProjections",
      "WorkloadExecutionReference",
      "OmittedExecutionReference",
    ],
    "source-execution-generation-conflated": [
      "serviceProjections",
      "observed_execution_generation",
      "source_generation",
    ],
    "native-direct-effect": [
      "nativeCallers",
      "fn submit_service_teardown() {}",
      "fn submit_service_teardown() { retire_service_for_decision_async(); }",
    ],
    "native-direct-sandbox-retirement": [
      "nativeCallers",
      "fn submit_sandbox_teardown() {}",
      "fn submit_sandbox_teardown() { retire_sandbox_resource_async(); }",
    ],
    "native-direct-backend-stop": [
      "nativeCallers",
      "fn finalize_service_definition_after_recorded() {}",
      "fn finalize_service_definition_after_recorded() { sandbox_backend.stop(); }",
    ],
    "native-source-finalizer-direct-backend-stop": [
      "nativeSourceRetirement",
      "fn finalize_unstarted_source_stop() {}",
      "fn finalize_unstarted_source_stop() { sandbox_backend.stop(); }",
    ],
    "native-source-finalizer-aliased-backend-stop": [
      "nativeSourceRetirement",
      "fn finalize_unstarted_source_stop() {}",
      "fn finalize_unstarted_source_stop() { let backend = &self.sandbox_backend; backend.stop(); }",
    ],
    "native-source-finalizer-ufcs-backend-stop": [
      "nativeSourceRetirement",
      "fn finalize_unstarted_source_stop() {}",
      "fn finalize_unstarted_source_stop() { SandboxBackend::stop(self.sandbox_backend.as_ref()); }",
    ],
    "managed-teardown-raw-registry-field": [
      "serverComposition",
      "teardown_capabilities: Option<ExactWorkloadTeardownCapabilityRealm>",
      "teardown_capabilities: Option<WorkloadTeardownCapabilityRegistry>",
    ],
    "managed-teardown-unused-exact-realm": [
      "serverComposition",
      "teardown_capabilities: self.teardown_capabilities.map(Box::new)",
      "teardown_capabilities: raw_teardown_capabilities.map(Box::new)",
    ],
    "source-claim-helper-hidden-yield-poll": [
      "provisionSettlementSupport",
      "self.wait_for_signal(",
      "tokio::task::yield_now(); self.wait_for_signal(",
    ],
    "service-source-claim-yield-poll": [
      "provisionSettlementTests",
      "wait_for_source_claim(&service_source_claim",
      "tokio::task::yield_now(&service_source_claim",
    ],
    "sandbox-source-claim-yield-poll": [
      "provisionSettlementTests",
      "wait_for_source_claim(&sandbox_source_claim",
      "tokio::task::yield_now(&sandbox_source_claim",
    ],
    "sandbox-context-downgrade": [
      "httpSandboxes",
      "stop_sandbox(&authorization.tenant_context)",
      "stop_sandbox(authorization.tenant_context.tenant_id())",
    ],
    "definition-context-downgrade": [
      "httpServices",
      "delete_service_definition(&tenant_context)",
      "delete_service_definition(tenant_context.tenant_id())",
    ],
    "missing-compose-store": [
      "cli",
      "fn compose_down_engine_workload_saga_store() { EngineWorkloadSagaStore; }",
      "",
    ],
    "missing-compose-submit": [
      "compute",
      "fn submit_compose_teardown() {}",
      "",
    ],
    "missing-compose-wait": [
      "compute",
      "fn wait_for_teardown_outcome() {}",
      "",
    ],
    "missing-machine-envelope": [
      "cli",
      "struct MachineApiWorkloadTeardownCommandEnvelope;",
      "",
    ],
    "missing-machine-phase": [
      "cli",
      "fn dispatch_machine_teardown_phase() {}",
      "",
    ],
    "missing-machine-fence": [
      "cli",
      "fn authenticate_machine_teardown_attempt_and_epoch() {}",
      "",
    ],
    "parent-release-before-absence": [
      "cli",
      "fn release_parent_publication_after_guest_absence() {}",
      "",
    ],
    "missing-machine-active-fence": [
      "cli",
      "fn ensure_no_active_workload_sagas_before_machine_stop() {}",
      "",
    ],
    "missing-ingress-capability": [
      "server",
      "trait FinalIngressWithdrawalCapability",
      "trait FinalRemovalCapability",
    ],
    "missing-ingress-join": [
      "server",
      "fn cancel_and_join_ingress_workers() {}",
      "",
    ],
    "missing-ingress-settlement": [
      "server",
      "fn settle_exact_listener_leases() {}",
      "",
    ],
    "swallowed-ingress-failure": [
      "server",
      "fn propagate_listener_settlement_failure() {}",
      "",
    ],
    "missing-tenant-enumeration": ["compute", "fn list_tenant_sagas() {}", ""],
    "missing-tenant-driver": ["compute", "fn drive_tenant_teardown() {}", ""],
    "finish-tenant-delete-early": [
      "compute",
      "fn require_all_recorded_before_finish_tenant_delete() {}",
      "",
    ],
    "missing-failed-provision-cause": [
      "workloads",
      "FailedProvision",
      "OmittedProvisionCause",
    ],
    "missing-failed-provision-compensation": [
      "compute",
      "fn compensate_definite_provision_failure()",
      "fn omit_definite_provision_failure()",
    ],
    "ambiguous-provision-stops": [
      "compute",
      "fn inspect_ambiguous_provision_before_compensation() {}",
      "",
    ],
    "missing-restart-handoff": [
      "workloads",
      "    WorkloadTeardownDecision::RestartSettlementPending;\n",
      "",
    ],
    "network-effect": [
      "network",
      "pub struct NetworkAttachmentId(String);",
      "pub struct NetworkAttachmentId(String); fn teardown() { TcpListener::bind(addr); }",
    ],
    "god-provider": [
      "network",
      "pub struct NetworkAttachmentId(String);",
      "pub struct NetworkAttachmentId(String); trait TeardownProvider {}",
    ],
    "missing-forwarded-registry-registrations": [
      "cli",
      "struct ForwardedMachineTeardownRegistrations;",
      "",
    ],
    "missing-forwarded-registry-capability": [
      "cli",
      "    NetworkAttachmentTeardownCapabilities::new();\n",
      "",
    ],
    "missing-forwarded-lifecycle-prepared-start": [
      "sandbox",
      "fn claim_dispatch_epoch_started() {}",
      "",
    ],
    "missing-forwarded-lifecycle-absence-retry": [
      "sandbox",
      "fn claim_dispatch_epoch_after_inspected_absence_started() {}",
      "",
    ],
    "missing-forwarded-lifecycle-batch-retain": [
      "network",
      "fn retain_provider_managed_batch_after_confirmed_absence() {}",
      "",
    ],
    "missing-forwarded-lifecycle-batch-release": [
      "network",
      "fn release_retained_provider_managed_batch_after_confirmed_absence() {}",
      "",
    ],
    "missing-forwarded-recovery-request-start": [
      "cli",
      "    claim_execute_started();\n",
      "",
    ],
    "missing-forwarded-recovery-inspect": [
      "cli",
      "    inspect_current_claim_async_and_publish();\n",
      "",
    ],
  };

  if (mutation === "open-phase-enum") {
    replaceOnce(
      sources,
      "workloads",
      "    Recorded,\n",
      "    Recorded,\n    Unknown,\n",
    );
  } else if (mutation === "cli-local-saga-store") {
    sources.cli += "\nstruct CliWorkloadSagaStore;\n";
  } else if (mutation === "missing-attributed-tests") {
    replaceOnceInNamedTest(
      sources,
      "compose_down_local_uses_engine_saga_and_compute_teardown",
      "fn compose_down_local_uses_engine_saga_and_compute_teardown",
      "fn missing_teardown_behavior_test",
    );
  } else if (mutation === "empty-test-body") {
    replaceOnceInNamedTest(
      sources,
      "compose_down_local_uses_engine_saga_and_compute_teardown",
      "    let observed = teardown_trace();\n    let expected = expected_teardown_trace();\n    assert_eq!(observed, expected);",
      "",
    );
  } else if (mutation === "helper-only-test-body") {
    replaceOnceInNamedTest(
      sources,
      "compose_down_local_uses_engine_saga_and_compute_teardown",
      "    let observed = teardown_trace();\n    let expected = expected_teardown_trace();\n    assert_eq!(observed, expected);",
      "    run_teardown_fixture();",
    );
  } else if (mutation === "declaration-only-test-body") {
    replaceOnceInNamedTest(
      sources,
      "compose_down_local_uses_engine_saga_and_compute_teardown",
      "    let observed = teardown_trace();\n    let expected = expected_teardown_trace();\n    assert_eq!(observed, expected);",
      "    let observed = teardown_trace();",
    );
  } else if (mutation === "tautological-test-assertion") {
    replaceOnceInNamedTest(
      sources,
      "compose_down_local_uses_engine_saga_and_compute_teardown",
      "    assert_eq!(observed, expected);",
      "    assert_eq!(observed, observed);",
    );
  } else if (mutation === "unexpected-path") {
    sources.currentChangedPaths.push(
      "crates/nimbus-compute/src/workload_saga/teardown.rs",
    );
  } else if (mutation === "future-product-path") {
    sources.auditItemComplete = true;
    sources.auditItemCompleteCheckpoint = "a".repeat(40);
    sources.currentChangedPaths.push(
      "crates/nimbus-compute/src/workload_saga/teardown.rs",
    );
  } else if (mutation === "invalid-completed-item-checkpoint") {
    sources.auditItemComplete = true;
    sources.auditItemCompleteCheckpoint = null;
    sources.historicalAuditChangedPaths = ["__invalid_nnc65_item_checkpoint__"];
  } else if (mutation === "missing-ledger-token") {
    replaceOnce(sources, "plan", "candidate-frozen", "candidate-open");
  } else if (mutation === "missing-forwarded-registry-test") {
    replaceOnceInTest(
      sources,
      `fn ${FORWARDED_MACHINE_TESTS.registry[0]}`,
      "fn missing_forwarded_registry_test",
    );
  } else if (mutation === "missing-forwarded-registry-inspect-test") {
    replaceOnceInTest(
      sources,
      `fn ${FORWARDED_MACHINE_TESTS.registry[1]}`,
      "fn missing_forwarded_registry_inspect_test",
    );
  } else if (mutation === "missing-forwarded-recovery-response-loss-test") {
    replaceOnceInTest(
      sources,
      `fn ${FORWARDED_MACHINE_TESTS.recovery[0]}`,
      "fn missing_forwarded_response_loss_test",
    );
  } else if (mutation === "missing-forwarded-recovery-process-test") {
    replaceOnceInTest(
      sources,
      `fn ${FORWARDED_MACHINE_TESTS.recovery[1]}`,
      "fn missing_forwarded_process_test",
    );
  } else if (mutation === "missing-provision-join") {
    replaceOnce(
      sources,
      "compute",
      "fn fence_and_join_inflight_provision() {}",
      "",
    );
    replaceOnce(
      sources,
      "resourceRetirement",
      "fn fence_and_join_inflight_provision() {}",
      "",
    );
  } else if (mutation === "missing-late-result-drain") {
    replaceOnce(
      sources,
      "compute",
      "fn retire_late_provision_result() {}",
      "",
    );
    replaceOnce(
      sources,
      "resourceRetirement",
      "fn retire_late_provision_result() {}",
      "",
    );
  } else if (mutation === "missing-native-execution-reference") {
    sources.serviceProjections = sources.serviceProjections.replaceAll(
      "WorkloadExecutionReference",
      "OmittedExecutionReference",
    );
  } else if (mutation === "source-execution-generation-conflated") {
    sources.serviceProjections = sources.serviceProjections.replaceAll(
      "observed_execution_generation",
      "source_generation",
    );
  } else if (mutation === "missing-native-test") {
    replaceOnceInTest(
      sources,
      `fn ${NATIVE_SOURCE_RETIREMENT_TESTS.at(-1)}`,
      "fn missing_native_source_retirement_test",
    );
  } else if (mutation in replacements) {
    replaceOnce(sources, ...replacements[mutation]);
  } else if (mutation) {
    throw new Error(`unknown teardown contract mutation: ${mutation}`);
  }
}

export function verifyWorkloadTeardownContract() {
  const fixture = process.env.NIMBUS_NETWORK_VERIFY_TEARDOWN_FIXTURE === "1";
  const stage = process.env.NIMBUS_NETWORK_VERIFY_TEARDOWN_STAGE ?? "aggregate";
  const root = path.resolve(
    process.env.NIMBUS_NETWORK_VERIFY_TEARDOWN_SCAN_ROOT ?? ".",
  );
  const sources = fixture ? greenTeardownFixture() : productionSources(root);
  if (fixture) {
    applyFixtureMutation(
      sources,
      process.env.NIMBUS_NETWORK_VERIFY_TEARDOWN_MUTATION ?? "",
    );
  }

  const errors = [];
  const nativeErrors = [];
  const requireContract = (condition, diagnostic) => {
    if (!condition) errors.push(diagnostic);
  };
  const requireNativeContract = (condition) => {
    if (!condition) {
      nativeErrors.push(workloadTeardownDiagnostics.nativeSourceRetirement);
    }
  };

  const phases = enumVariants(sources.workloads, "WorkloadSagaPhase");
  const teardownPhases = phases.filter((phase) =>
    [
      "WithdrawalCommitted",
      "Withdrawn",
      "Drained",
      "WorkloadStopped",
      "NetworkDetached",
      "NetworkReleased",
      "Recorded",
      "Unknown",
    ].includes(phase),
  );
  requireContract(
    teardownPhases.join(" ") ===
      "WithdrawalCommitted Withdrawn Drained WorkloadStopped NetworkDetached NetworkReleased Recorded" &&
      hasAll(sources.workloads, [
        "WorkloadEffectReferences",
        "PublicationAbsent",
        "ExecutionDrained",
        "ExecutionStopped",
        "NetworkDetached",
        "NetworkReleased",
      ]),
    workloadTeardownDiagnostics.vocabulary,
  );

  const materializer = extractItem(
    sources.compute,
    "fn materialize_teardown_candidate",
  );
  const confirmation = extractItem(
    sources.compute,
    "fn confirm_teardown_transition",
  );
  const teardownDriver = extractItem(sources.compute, "impl WorkloadTeardownDriver");
  requireContract(
    hasAll(sources.workloads, [
      "WorkloadTeardownAttemptId",
      "WorkloadTeardownClaim",
      "WorkloadTeardownDisposition",
    ]) &&
      hasAll(materializer, [
        "claim_teardown",
        "record_resource_free_teardown_step",
        "record_terminal_teardown",
      ]) &&
      hasAll(confirmation, ["confirm_transition", "WorkloadSagaConfirmation"]) &&
      hasAll(teardownDriver, [
        "decide_teardown",
        "materialize_teardown_candidate",
        "confirm_teardown_transition",
      ]),
    workloadTeardownDiagnostics.reducer,
  );

  const command = extractItem(
    sources.compute,
    "struct ConfirmedWorkloadTeardownCommand",
  );
  const commandConstructor = extractItem(
    sources.compute,
    "impl ConfirmedWorkloadTeardownCommand",
  );
  const result = extractItem(sources.compute, "fn apply_teardown_result");
  requireContract(
    hasAll(command, [
      "command_id: WorkloadTeardownCommandId",
      "confirmed_revision: WorkloadSagaRevision",
      "confirmed_transition_id: WorkloadSagaTransitionId",
      "source: WorkloadProvisionSourceEvidence",
      "mode: WorkloadTeardownCommandMode",
      "claim: WorkloadTeardownClaim",
    ]) &&
      hasAll(commandConstructor, [
        "fn from_confirmation",
        "WorkloadSagaConfirmation::AppliedByThisCall",
        "WorkloadTeardownCommandMode::Execute",
        "fn attempt_id",
        "fn dispatch_epoch",
        "fn provider_target",
        "fn subjects",
      ]) &&
      !/\bpub(?:\([^)]*\))?\s+fn\s+from_confirmation\b/u.test(
        commandConstructor,
      ) &&
      hasAll(result, [
        "authenticate_confirmed_record",
        "authenticate_command_result",
        "apply_teardown_effect_result",
        "apply_teardown_inspection_result",
      ]),
    workloadTeardownDiagnostics.command,
  );

  const order = extractItem(sources.workloads, "fn teardown_step_for_phase");
  const portableReducer = extractItem(sources.workloads, "fn decide_teardown");
  requireContract(
    appearsInOrder(order, REQUIRED_ORDER) &&
      hasAll(portableReducer, [
        "WorkloadTeardownDecision::RestartSettlementPending",
        "WorkloadSagaPhase::NetworkReleased",
        "ProposedWorkloadTeardownTransition::RecordTerminal",
      ]) &&
      hasTestsAt(sources, "crates/nimbus-compute/src/workload_saga/teardown_driver/tests.rs", [
        "teardown_driver_records_exact_five_step_order",
      ]),
    workloadTeardownDiagnostics.order,
  );

  requireContract(
    hasAll(sources.compute, [
      "submit_service_teardown",
      "submit_sandbox_teardown",
    ]) &&
      hasAll(sources.services, [
        "project_recorded_service_teardown",
        "project_recorded_sandbox_teardown",
      ]) &&
      !/\b(?:retire_service_for_decision_async|retire_sandbox_resource_async|TenantServiceRetirement)\b/u.test(
        sources.services,
      ) &&
      !/sandbox_backend\s*\.\s*stop\s*\(/u.test(sources.services),
    workloadTeardownDiagnostics.service,
  );

  requireContract(
    hasAll(sources.services, [
      "claim_service_definition_retirement",
      "finalize_service_definition_after_recorded",
    ]) &&
      hasAll(sources.compute, [
        "fence_and_join_inflight_provision",
        "retire_late_provision_result",
      ]),
    workloadTeardownDiagnostics.definitionDelete,
  );

  const providerStruct = extractItem(
    sources.serverComposition,
    "pub struct ServerWorkloadProviders",
  );
  const providerImpl = extractItem(
    sources.serverComposition,
    "impl ServerWorkloadProviders",
  ) || extractItem(
    sources.serverComposition,
    "impl<Attachment, Execution, Ingress> ServerWorkloadProviders",
  );
  const intoManagedCompute = extractItem(
    sources.serverComposition,
    "fn into_managed_compute",
  );
  const computeComposition = extractItem(
    sources.computeState,
    "pub enum ComputeWorkloadComposition",
  );
  const computeFromConfig = extractItem(sources.computeState, "pub fn from_config");
  const localComposition = extractItem(
    sources.localComposition,
    "fn into_workload_composition",
  );
  const serviceRoute = extractItem(sources.httpServices, "fn service_lifecycle_route");
  const definitionRoute = extractItem(
    sources.httpServices,
    "fn delete_service_definition",
  );
  const sandboxRoute = extractItem(sources.httpSandboxes, "fn stop_sandbox");
  const computeServiceLifecycle = extractItem(
    sources.computeServices,
    "fn service_lifecycle",
  );
  const computeDefinitionDelete = extractItem(
    sources.computeServices,
    "fn delete_service_definition",
  );
  const computeSandboxStop = extractItem(sources.computeSandboxes, "fn stop_sandbox");
  const serviceProvisionSettlement = extractItem(
    sources.provisionSettlementTests,
    "fn service_stop_joins_inflight_provision_and_retires_late_success",
  );
  const sandboxProvisionSettlement = extractItem(
    sources.provisionSettlementTests,
    "fn sandbox_stop_joins_inflight_provision_and_retires_late_success",
  );
  const sourceClaimWait = extractItem(
    sources.provisionSettlementSupport,
    "fn wait_for_source_claim",
  );
  const sourceSignalWait = extractItem(
    sources.provisionSettlementSupport,
    "fn wait_for_signal",
  );
  const serverWorkloadComposition = extractItem(
    sources.serverComposition,
    "struct ServerWorkloadComposition",
  );
  requireNativeContract(
    hasAll(providerStruct, [
      "teardown_capabilities",
      "Option",
      "WorkloadTeardownCapabilityRegistry",
    ]) &&
      hasAll(providerImpl, [
        "teardown_capabilities: None",
        "with_teardown_capabilities",
        "Some(teardown_capabilities)",
      ]) &&
      hasAll(intoManagedCompute, ["teardown_capabilities", "ComputeWorkloadComposition::Managed"]) &&
      hasAll(computeComposition, [
        "teardown_capabilities",
        "ExactWorkloadTeardownCapabilityRealm",
        "execution_provider_id",
      ]) &&
      /teardown_capabilities\s*:\s*Option\s*<\s*Box\s*<\s*ExactWorkloadTeardownCapabilityRealm\s*>\s*>/u.test(
        computeComposition,
      ) &&
      /teardown_capabilities\s*:\s*Option\s*<\s*ExactWorkloadTeardownCapabilityRealm\s*>/u.test(
        serverWorkloadComposition,
      ) &&
      sources.serverComposition.includes("ExactWorkloadTeardownCapabilityRealm::new") &&
      hasAll(computeFromConfig, [
        "teardown_capabilities",
        "WorkloadTeardownRuntime::new",
        "into_registry_for",
        "&capability_selection",
        "&execution_provider_id",
      ]) &&
      intoManagedCompute.includes(
        "teardown_capabilities: self.teardown_capabilities.map(Box::new)",
      ) &&
      intoManagedCompute.includes(
        "execution_provider_id: self.execution_provider_id",
      ) &&
      hasAll(localComposition, [
        "KrunTeardownAdapter::new",
        "KrunAttachmentTeardownAdapter::new",
        "IngressTeardownCapabilities::new",
        "ServerIngressPublicationAdapter",
        "WorkloadTeardownCapabilityRegistry::new",
        "with_teardown_capabilities",
      ]) &&
      hasAll(sources.resourceRetirement, [
        "fence_and_join_inflight_provision",
        "retire_late_provision_result",
        "settle_issued_restart_before_native_teardown",
      ]) &&
      hasAll(sources.serviceDefinitions, [
        "claim_service_definition_retirement",
        "finalize_service_definition_after_recorded",
      ]) &&
      hasAll(sources.serviceProjections, [
        "project_recorded_service_teardown",
        "project_recorded_sandbox_teardown",
        "source_generation",
        "observed_execution_generation",
        "WorkloadExecutionReference",
      ]) &&
      hasAll(serviceRoute, ["tenant_context", "service_lifecycle"]) &&
      hasAll(definitionRoute, ["tenant_context", "delete_service_definition", "&tenant_context"]) &&
      hasAll(sandboxRoute, ["tenant_context", "stop_sandbox", "&authorization.tenant_context"]) &&
      hasAll(computeServiceLifecycle, ["TenantIsolationContext", "submit_service_teardown"]) &&
      hasAll(computeDefinitionDelete, ["TenantIsolationContext", "submit_definition_teardown"]) &&
      hasAll(computeSandboxStop, ["TenantIsolationContext", "submit_sandbox_teardown"]) &&
      !/\b(?:retire_service_for_decision_async|retire_sandbox_resource_async)\b/u.test(
        sources.nativeCallers,
      ) &&
      !/sandbox_backend\s*\.\s*stop\s*\(/u.test(sources.nativeCallers) &&
      hasAll(sources.nativeSourceRetirement, [
        "finalize_unstarted_source_stop",
        "finalize_unstarted_service_definition_deletion",
        "finalize_service_definition_after_recorded",
      ]) &&
      !/(?:\.|::)\s*stop\s*\(/u.test(sources.nativeSourceRetirement) &&
      hasAll(serviceProvisionSettlement, [
        "install_source_claim_signal",
        "wait_for_source_claim",
        "service_source_is_fenced",
      ]) &&
      !/yield_now\s*\(/u.test(serviceProvisionSettlement) &&
      hasAll(sandboxProvisionSettlement, [
        "install_source_claim_signal",
        "wait_for_source_claim",
        "sandbox_source_is_fenced",
      ]) &&
      !/yield_now\s*\(/u.test(sandboxProvisionSettlement) &&
      hasAll(sourceClaimWait, ["wait_for_signal", "entered", "source"]) &&
      hasAll(sourceSignalWait, [
        "tokio::time::timeout",
        "entered.acquire",
      ]) &&
      !/yield_now\s*\(/u.test(`${sourceClaimWait}\n${sourceSignalWait}`) &&
      nativeSourceRetirementTestsPass(sources),
  );

  requireContract(
    hasAll(sources.cli, [
      "compose_down_engine_workload_saga_store",
      "EngineWorkloadSagaStore",
    ]) &&
      hasAll(sources.compute, [
        "submit_compose_teardown",
        "wait_for_teardown_outcome",
      ]) &&
      !/\bCliWorkloadSagaStore\b/u.test(sources.cli) &&
      !/\bstop_service_target\b/u.test(sources.cli),
    workloadTeardownDiagnostics.compose,
  );

  requireContract(
    hasAll(sources.cli, [
      "MachineApiWorkloadTeardownCommandEnvelope",
      "dispatch_machine_teardown_phase",
      "authenticate_machine_teardown_attempt_and_epoch",
      "withdraw_parent_publication_before_guest_stop",
      "release_parent_publication_after_guest_absence",
      "ensure_no_active_workload_sagas_before_machine_stop",
    ]) && !/\bstop_service_sandbox\b/u.test(sources.cli),
    workloadTeardownDiagnostics.machine,
  );

  requireContract(
    hasAll(sources.cli, [
      "ForwardedMachineTeardownRegistrations",
      "PROVIDER_JOURNAL_NAMESPACE",
      "NetworkAttachmentTeardownCapabilities::new",
      "WorkloadExecutionTeardownCapabilities::new",
      "IngressTeardownCapabilities::new",
    ]) &&
      forwardedMachineTestsPass(sources, FORWARDED_MACHINE_TESTS.registry),
    workloadTeardownDiagnostics.forwardedMachineRegistry,
  );

  requireContract(
    hasAll(sources.sandbox, [
      "ProviderCommandCurrentExecution",
      "fn authenticates",
      "claim_dispatch_epoch_started",
      "claim_dispatch_epoch_after_inspected_absence_started",
      "execute_started_claim_async",
    ]) &&
      hasAll(sources.network, [
        "retain_provider_managed_batch_after_confirmed_absence",
        "release_retained_provider_managed_batch_after_confirmed_absence",
      ]) &&
      hasAll(sources.cli, [
        "begin_parent_publication_withdrawal",
        "record_parent_publication_withdrawn_retained",
        "begin_parent_publication_release",
        "record_parent_publication_released",
      ]) &&
      forwardedMachineTestsPass(sources, FORWARDED_MACHINE_TESTS.lifecycle),
    workloadTeardownDiagnostics.forwardedMachineLifecycle,
  );

  requireContract(
    hasAll(sources.cli, [
      "claim_execute_started",
      "execute_started_claim_async",
      "adopt_inspect",
      "inspect_current_claim_async_and_publish",
    ]) &&
      forwardedMachineTestsPass(sources, FORWARDED_MACHINE_TESTS.recovery),
    workloadTeardownDiagnostics.forwardedMachineRecovery,
  );

  requireContract(
    hasAll(sources.server, [
      "FinalIngressWithdrawalCapability",
      "execute_exact_final_withdrawal",
      "inspect_exact_final_withdrawal",
      "cancel_and_join_ingress_workers",
      "close_exact_ingress_routes",
      "settle_exact_listener_leases",
      "prove_exact_ingress_absence",
      "propagate_listener_settlement_failure",
    ]),
    workloadTeardownDiagnostics.ingress,
  );

  requireContract(
    hasAll(sources.compute, [
      "list_tenant_sagas",
      "drive_tenant_teardown",
      "require_all_recorded_before_finish_tenant_delete",
    ]) && !/\bTenantServiceRetirement\b/u.test(sources.services),
    workloadTeardownDiagnostics.tenant,
  );

  requireContract(
    hasAll(sources.workloads, ["WorkloadTeardownCause", "FailedProvision"]) &&
      hasAll(sources.compute, [
        "compensate_definite_provision_failure",
        "WorkloadTeardownCause::FailedProvision",
        "inspect_ambiguous_provision_before_compensation",
        "retain_cancellation_after_submission",
        "settle_issued_restart_before_teardown",
        "retain_late_restart_result",
        "enter_withdrawal_committed_after_restart_settlement",
      ]),
    workloadTeardownDiagnostics.compensation,
  );

  requireContract(
    behaviorTestsPass(sources),
    workloadTeardownDiagnostics.behavior,
  );

  requireContract(
    !/\b(?:TcpListener|TcpStream|UdpSocket|SandboxBackend|TeardownProvider|NetworkProvider)\b/u.test(
      sources.network,
    ),
    workloadTeardownDiagnostics.network,
  );

  requireContract(
    frozenAuditChangedPaths(sources).every((candidate) =>
      ALLOWED_PATHS.has(candidate),
    ),
    workloadTeardownDiagnostics.paths,
  );

  requireContract(
    hasAll(sources.plan, [
      "NNC6.5",
      "A1-A24",
      "NNCV035",
      "persist withdrawal -> withdraw -> drain -> stop -> detach -> release -> record",
      "zero stop effects",
      "Definition/source/session removal waits",
      "candidate-frozen",
      "Sol/xhigh/fast",
    ]),
    workloadTeardownDiagnostics.ledger,
  );

  if (stage === "native") return [...new Set(nativeErrors)];
  if (stage !== "aggregate") {
    return [`unknown workload teardown contract stage: ${stage}`];
  }
  return errors;
}
