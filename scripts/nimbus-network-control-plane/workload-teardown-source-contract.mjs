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
  PHYSICAL_MACHINE_STOP_TESTS,
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
          /#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]/u.test(source) ||
          /#\s*\[\s*(?:(?:tokio|rstest)\s*::\s*)?test\b/u.test(source)
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
    readText(
      root,
      "docs/private/plans/proof/nimbus-network-control-plane/nnc6.5f-compose-machine-caller-substitution-audit.md",
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
    composeCommand: readText(root, "crates/nimbus-cli/src/compose/mod.rs", {
      lexical: true,
    }),
    composeRetirement: [
      "crates/nimbus-cli/src/compose/lifecycle.rs",
      "crates/nimbus-cli/src/compose/retirement.rs",
    ]
      .map((candidate) => readText(root, candidate, { lexical: true }))
      .join("\n"),
    forwardedServerComposition: readText(
      root,
      "crates/nimbus-cli/src/network_composition/forwarded.rs",
      { lexical: true },
    ),
    forwardedComposeComposition: readText(
      root,
      "crates/nimbus-cli/src/compose/provision.rs",
      { lexical: true },
    ),
    forwardedCanonicalComposition: readText(
      root,
      "crates/nimbus-cli/src/network_composition/forwarded/profile.rs",
      { lexical: true },
    ),
    exactGuestTeardown: [
      "crates/nimbus-cli/src/machine/backend/teardown.rs",
      "crates/nimbus-cli/src/machine/api/service_workloads/teardown.rs",
      "crates/nimbus-machine/src/api/teardown.rs",
    ]
      .map((candidate) => readText(root, candidate, { lexical: true }))
      .join("\n"),
    coarseGuestApi: [
      "crates/nimbus-cli/src/machine/backend.rs",
      "crates/nimbus-cli/src/machine/client.rs",
      "crates/nimbus-cli/src/machine/api/capabilities.rs",
      "crates/nimbus-cli/src/machine/api/routes.rs",
      "crates/nimbus-cli/src/machine/api/service_workloads.rs",
      "crates/nimbus-machine/src/api.rs",
    ]
      .map((candidate) => readText(root, candidate, { lexical: true }))
      .join("\n"),
    physicalDesireAdmissions: [
      "crates/nimbus-compute/src/workload_saga/ingress.rs",
      "crates/nimbus-compute/src/workload_saga/restart_decision.rs",
    ]
      .map((candidate) => readText(root, candidate, { lexical: true }))
      .join("\n"),
    physicalStopDecision: readText(
      root,
      "crates/nimbus-compute/src/machine_stop_authority.rs",
      { lexical: true },
    ),
    physicalStopDecisionStoreAdapter: readText(
      root,
      "crates/nimbus-server/src/workload_saga_store/machine_authority.rs",
      { lexical: true },
    ),
    physicalStopProvider: [
      "crates/nimbus-cli/src/machine/publication_authority/confirmed/stop_barrier.rs",
      "crates/nimbus-cli/src/machine/publication_authority/confirmed.rs",
    ]
      .map((candidate) => readText(root, candidate, { lexical: true }))
      .join("\n"),
    physicalProviderAdmissions: [
      "crates/nimbus-cli/src/machine/backend/provision.rs",
      "crates/nimbus-cli/src/machine/backend/provision/restart.rs",
    ]
      .map((candidate) => readText(root, candidate, { lexical: true }))
      .join("\n"),
    physicalStopEffects: readText(
      root,
      "crates/nimbus-cli/src/machine/manager/stop.rs",
      { lexical: true },
    ),
    physicalStopStandalone: readText(
      root,
      "crates/nimbus-cli/src/machine/handlers.rs",
      { lexical: true },
    ),
    physicalStopServer: readText(
      root,
      "crates/nimbus-cli/src/machine/server_control.rs",
      { lexical: true },
    ),
    physicalStopOs: readText(
      root,
      "crates/nimbus-cli/src/machine/handlers/os.rs",
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

function countOccurrences(source, token) {
  if (token.length === 0) return 0;
  return source.split(token).length - 1;
}

function itemBody(item) {
  const open = item.indexOf("{");
  return open >= 0 && item.endsWith("}") ? item.slice(open + 1, -1).trim() : "";
}

function extractCall(source, marker) {
  const start = source.indexOf(marker);
  const open = source.indexOf("(", start);
  if (start < 0 || open < 0) return "";
  let depth = 0;
  for (let cursor = open; cursor < source.length; cursor += 1) {
    if (source[cursor] === "(") depth += 1;
    else if (source[cursor] === ")") depth -= 1;
    if (depth === 0) return source.slice(start, cursor + 1);
  }
  return "";
}

function escapedRegex(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
}

function returnsCallResult(item, callee) {
  const escaped = escapedRegex(callee);
  return new RegExp(
    `(?:return\\s+${escaped}\\s*\\([^;{}]*\\)\\s*;|${escaped}\\s*\\([^;{}]*\\))\\s*$`,
    "u",
  ).test(itemBody(item));
}

function enumVariants(source, name) {
  return extractItem(source, `enum ${name}`)
    .split("\n")
    .map((line) => line.match(/^\s*([A-Z][A-Za-z0-9_]*)\s*(?:[,({])/u)?.[1])
    .filter(Boolean);
}

function behaviorTestsPass(sources) {
  return (
    nativeSourceRetirementTestsPass(sources) &&
    [...new Set([...BEHAVIOR_TESTS, ...NATIVE_SOURCE_RETIREMENT_TESTS])].every(
      (name) =>
        sources.testEntries.some((entry) =>
          hasTestsAt(sources, entry.file, [name]),
        ),
    )
  );
}

function physicalMachineStopTestsPass(sources) {
  return PHYSICAL_MACHINE_STOP_TESTS.every((name) =>
    sources.testEntries.some((entry) =>
      hasTestsAt(sources, entry.file, [name]),
    ),
  );
}

function nativeSourceRetirementTestsPass(sources) {
  const definitionTests = new Set(NATIVE_SOURCE_RETIREMENT_TESTS.slice(6, 12));
  const sessionTest =
    "session_binding_rejects_a_later_execution_with_the_same_source_generation";
  const contenderTest =
    "concurrent_start_and_stop_linearize_at_the_source_fence";
  return NATIVE_SOURCE_RETIREMENT_TESTS.every((name) => {
    const owns = (file) => {
      if (definitionTests.has(name)) {
        return (
          file ===
          "crates/nimbus-server/src/tests/service_manager/definition_retirement.rs"
        );
      }
      if (name === sessionTest) {
        return file === "crates/nimbus-services/src/manager/tests/sessions.rs";
      }
      if (name === contenderTest) {
        return (
          file === "crates/nimbus-compute/src/workload_provisioner/tests.rs"
        );
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
    sources.testEntries.some((entry) =>
      hasTestsAt(sources, entry.file, [name]),
    ),
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
  if (!entry)
    throw new Error(`teardown named test mutation target missing: ${name}`);
  if (!entry.source.includes(before)) {
    throw new Error(
      `teardown named test body mutation target missing: ${name}:${before}`,
    );
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
    "missing-revision-fence": ["compute", "    confirm_transition();\n", ""],
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
    "missing-compose-persistence": [
      "composeCommand",
      "run_compose_down(command, persistence_config).await;",
      "run_compose_down(command).await;",
    ],
    "missing-compose-engine": [
      "composeCommand",
      "    let engine = Engine::new_with_persistence_config(persistence_config.clone()).await;\n",
      "",
    ],
    "missing-compose-store": [
      "composeRetirement",
      "    let saga_store = Arc::new(EngineWorkloadSagaStore::new(Arc::clone(&engine)));\n",
      "",
    ],
    "compose-store-not-wired": [
      "composeRetirement",
      "    let runtime = prepared.activate(Arc::clone(&engine), Arc::clone(&saga_store));\n",
      "    let runtime = prepared.activate(Arc::clone(&engine), Arc::new(UnrelatedSagaStore));\n",
    ],
    "compose-wired-activation-discarded": [
      "composeRetirement",
      "    let runtime = prepared.activate(Arc::clone(&engine), Arc::clone(&saga_store));\n",
      "    prepared.activate(Arc::clone(&engine), Arc::clone(&saga_store));\n    let runtime = prepared.activate(Arc::clone(&engine), Arc::new(UnrelatedSagaStore));\n",
    ],
    "missing-compose-retirer": [
      "composeRetirement",
      "    let retirer = runtime.resource_retirer();\n",
      "",
    ],
    "missing-compose-submit": [
      "composeRetirement",
      "    let outcome = retirer.submit_service_teardown(tenant_context, service_name).await;\n",
      "",
    ],
    "missing-compose-recorded": [
      "composeRetirement",
      "            return ComposeServiceRetirementOutcome::recorded(\n                outcome.disposition(),\n                execution,\n            );",
      "            return Err(ComposeRetirementIncomplete);",
    ],
    "compose-recorded-result-discarded": [
      "composeRetirement",
      "            return ComposeServiceRetirementOutcome::recorded(\n                outcome.disposition(),\n                execution,\n            );",
      "            ComposeServiceRetirementOutcome::recorded(\n                outcome.disposition(),\n                execution,\n            );\n            return ComposeServiceRetirementOutcome::source_finalized();",
    ],
    "compose-terminal-reference-discarded": [
      "composeRetirement",
      "            let execution = outcome.terminal_execution_reference();",
      "            outcome.terminal_execution_reference();\n            let execution = None;",
    ],
    "compose-recorded-omits-terminal-binding": [
      "composeRetirement",
      "                execution,\n",
      "                None,\n",
    ],
    "missing-machine-envelope": [
      "exactGuestTeardown",
      "struct MachineApiWorkloadTeardownCommandEnvelope;",
      "",
    ],
    "missing-machine-phase": [
      "exactGuestTeardown",
      "fn build_remote_request() {}",
      "",
    ],
    "missing-machine-fence": [
      "exactGuestTeardown",
      "    validate_source_and_target();\n    validate_subjects();\n    validate_retirement_order();",
      "",
    ],
    "guest-dispatch-skips-validation": [
      "exactGuestTeardown",
      "    let validated = self.validate();\n    self.execute(validated).await;",
      "    self.execute(ValidatedForwardedMachineTeardown::unchecked()).await;",
    ],
    "guest-dispatch-discards-validation": [
      "exactGuestTeardown",
      "    let validated = self.validate();\n    self.execute(validated).await;",
      "    let validated = self.validate();\n    self.execute(ValidatedForwardedMachineTeardown::unchecked()).await;",
    ],
    "guest-remote-before-journal-claim": [
      "exactGuestTeardown",
      "    claim_execute_started(validated);\n",
      "    remote_result(&unchecked_request);\n    claim_execute_started(validated);\n",
    ],
    "guest-aliased-remote-before-journal-claim": [
      "exactGuestTeardown",
      "    claim_execute_started(validated);\n",
      "    client.teardown_workload_phase_prepared(&unchecked_request);\n    claim_execute_started(validated);\n",
    ],
    "parent-release-before-absence": [
      "exactGuestTeardown",
      "release_parent_batch_after_guest_release",
      "release_parent_batch_before_guest_release",
    ],
    "missing-machine-active-fence": [
      "physicalStopDecision",
      "    EmptyWithFence(ConfirmedMachineStopAuthorization),\n",
      "",
    ],
    "missing-machine-rescan": [
      "physicalStopDecision",
      "workloads.list_machine_workload_authority_from_engine().await",
      "WorkloadAuthoritySnapshot::unavailable()",
    ],
    "untyped-machine-active-conflict": [
      "physicalStopDecision",
      "    ActiveWorkloadTeardownRequired,\n",
      "    Other(String),\n",
    ],
    "projection-address-machine-authority": [
      "physicalStopDecision",
      "list_machine_workload_authority_from_engine",
      "list_machine_workload_authority_from_system_projection_and_ip_address",
    ],
    "unavailable-machine-authority-allows-stop": [
      "physicalStopDecision",
      "    AuthorityUnavailable,\n",
      "    AuthorityUnavailableButProceed,\n",
    ],
    "crossed-machine-authority-allows-stop": [
      "physicalStopDecision",
      "    Crossed,\n",
      "    CrossedButProceed,\n",
    ],
    "machine-barrier-after-publication": [
      "physicalStopEffects",
      "    let stop_barrier = stop_authority.begin_physical_stop(&authorization);\n    withdraw_machine_publications();",
      "    withdraw_machine_publications();\n    let stop_barrier = stop_authority.begin_physical_stop(&authorization);",
    ],
    "machine-standalone-bypass": [
      "physicalStopStandalone",
      "async fn run_machine_stop() {\n    let stop_authority = HostMachineStopAuthority::new();\n    let authorization = stop_authority.authorize().await;\n    stop_machine_with_layout_authorized(stop_authority, authorization);\n}",
      "async fn run_machine_stop() {\n    raw_machine_stop();\n}",
    ],
    "machine-server-bypass": [
      "physicalStopServer",
      "fn stop_machine<'a>() {\n    let authorization = stop_authority.authorize().await;\n    stop_machine_with_layout_authorized(stop_authority, authorization);\n}",
      "fn stop_machine<'a>() {\n    raw_machine_stop();\n}",
    ],
    "machine-restart-bypass": [
      "physicalStopServer",
      "fn restart_machine<'a>() {\n    let authorization = stop_authority.authorize().await;\n    restart_machine_with_layout_authorized(stop_authority, authorization);\n}",
      "fn restart_machine<'a>() {\n    raw_machine_stop();\n}",
    ],
    "machine-bootc-restart-bypass": [
      "physicalStopOs",
      "fn restart_bootc_machine(authorized: &mut Option<AuthorizedMachineStop>) {\n    let authorization = authorized.take().ok_or_else(missing_machine_stop_authority);\n    authorization.stop();\n    start_machine();\n}",
      "fn restart_bootc_machine() {\n    raw_machine_stop();\n}",
    ],
    "machine-os-apply-restart-bypass": [
      "physicalStopOs",
      "fn apply_machine_os_change(authorized: &mut Option<AuthorizedMachineStop>) {\n    let authorization = authorized.take().ok_or_else(missing_machine_stop_authority);\n    authorization.stop();\n    config.guest.image_source = target_source;\n}",
      "fn apply_machine_os_change() {\n    raw_machine_stop();\n}",
    ],
    "machine-admission-after-empty-scan": [
      "physicalStopDecision",
      "    let claim = barriers.claim_effect_free_barrier();\n    let sagas = workloads.list_machine_workload_authority_from_engine().await;",
      "    let sagas = workloads.list_machine_workload_authority_from_engine().await;\n    allow_forwarded_workload_admission();\n    let claim = barriers.claim_effect_free_barrier();",
    ],
    "missing-machine-active-barrier-clear": [
      "physicalStopDecision",
      "            barriers.clear_effect_free_barrier().await;\n",
      "",
    ],
    "missing-initial-desire-admission-guard": [
      "physicalDesireAdmissions",
      "async fn submit_intent() {\n    let _permit = match &self.desire_admission_guard {\n        Some(guard) => Some(guard.acquire(&admission).await),\n        None => None,\n    };\n    let disposition = self.commit_loaded(loaded.as_ref(), next.clone()).await;\n}",
      "async fn submit_intent() {\n    self.commit_loaded(loaded.as_ref(), next.clone()).await;\n}",
    ],
    "initial-desire-guard-released-before-cas": [
      "physicalDesireAdmissions",
      "    let disposition = self.commit_loaded(loaded.as_ref(), next.clone()).await;",
      "    drop(_permit);\n    let disposition = self.commit_loaded(loaded.as_ref(), next.clone()).await;",
    ],
    "missing-restart-desire-admission-guard": [
      "physicalDesireAdmissions",
      "async fn compare_and_swap_restart_admission() {\n    let _permit = match &self.desire_admission_guard {\n        Some(guard) => Some(guard.acquire(&admission).await),\n        None => None,\n    };\n    let result = self.commit_loaded(Some(&current), candidate.clone()).await;\n    drop(_permit);\n}",
      "async fn compare_and_swap_restart_admission() {\n    self.commit_loaded(Some(&current), candidate.clone()).await;\n}",
    ],
    "restart-desire-guard-released-before-cas": [
      "physicalDesireAdmissions",
      "    let result = self.commit_loaded(Some(&current), candidate.clone()).await;\n    drop(_permit);",
      "    drop(_permit);\n    let result = self.commit_loaded(Some(&current), candidate.clone()).await;",
    ],
    "missing-machine-barrier-digest-machine-name": [
      "physicalStopProvider",
      "        machine_name: &self.machine_name,\n",
      "",
    ],
    "missing-machine-barrier-digest-authority": [
      "physicalStopProvider",
      "        forwarder_authority: &self.forwarder_authority,\n",
      "",
    ],
    "missing-machine-barrier-digest-epoch": [
      "physicalStopProvider",
      "        epoch: self.epoch,\n",
      "",
    ],
    "missing-machine-barrier-digest-state": [
      "physicalStopProvider",
      "        state: self.state,\n",
      "",
    ],
    "machine-barrier-digest-disconnected": [
      "physicalStopProvider",
      "Sha256::digest(bytes)",
      'Sha256::digest(b"constant")',
    ],
    "missing-machine-admission-barrier-traversal": [
      "physicalStopProvider",
      "    let barrier = body.stop_barriers.iter()\n",
      "    let barrier = unrelated_barriers.iter()\n",
    ],
    "missing-machine-admission-provider-comparison": [
      "physicalStopProvider",
      "    if barrier.forwarder_authority.provider_instance() != forwarder_authority.provider_instance() {\n",
      "    if false {\n",
    ],
    "missing-machine-admission-generation-comparison": [
      "physicalStopProvider",
      "    if barrier.forwarder_authority.generation() != forwarder_authority.generation() {\n",
      "    if false {\n",
    ],
    "machine-admission-provider-fails-open": [
      "physicalStopProvider",
      "return Err(Error::Crossed);",
      "return Ok(());",
    ],
    "machine-admission-generation-fails-open": [
      "physicalStopProvider",
      "return Err(Error::Stale);",
      "return Ok(());",
    ],
    "machine-admission-fence-fails-open": [
      "physicalStopProvider",
      "Err(Error::Fenced)",
      "Ok(())",
    ],
    "missing-machine-admission-authentication": [
      "physicalStopProvider",
      "fn authenticate_workload_admission_absence(body, machine_name, forwarder_authority) {\n    let barrier = body.stop_barriers.iter()\n        .filter(|barrier| barrier.machine_name == machine_name)\n        .max_by_key(|barrier| barrier.epoch)\n        .filter(|barrier| !barrier.state.is_terminal());\n    if barrier.forwarder_authority.provider_instance() != forwarder_authority.provider_instance() {\n        return Err(Error::Crossed);\n    }\n    if barrier.forwarder_authority.generation() != forwarder_authority.generation() {\n        return Err(Error::Stale);\n    }\n    Err(Error::Fenced)\n}",
      "",
    ],
    "machine-desire-guard-does-not-hold-lock": [
      "physicalStopProvider",
      "struct ConfirmedMachineDesireAdmissionPermit {\n    _lock: ConfirmedMachinePublicationLock,\n}",
      "struct ConfirmedMachineDesireAdmissionPermit;",
    ],
    "machine-barrier-claim-outside-provider-lock": [
      "physicalStopProvider",
      "fn claim_machine_stop_barrier() {\n    self.mutate_with_error(|body| {\n        provider_instance();\n        generation();\n        body.stop_barriers.push(barrier);\n        provider_witnesses(body, forwarder_authority);\n    });\n}",
      "fn claim_machine_stop_barrier() {\n    persist_machine_stop_barrier_without_lock();\n}",
    ],
    "machine-provider-auth-outside-provider-lock": [
      "physicalStopProvider",
      "fn authenticate_retirement_witness() {\n    self.mutate(|body| {\n        authenticate_workload_admission_absence(body, machine_name, authority);\n        body.retirement_witnesses.push(candidate);\n    });\n}",
      "fn authenticate_retirement_witness() {\n    authenticate_workload_admission_absence_without_lock();\n    self.mutate(|body| body.retirement_witnesses.push(candidate));\n}",
    ],
    "machine-provision-admission-bypasses-barrier": [
      "physicalProviderAdmissions",
      "fn validate_exact_phase() {\n    self.publication_journal.authenticate_retirement_witness();\n}",
      "fn validate_exact_phase() {}",
    ],
    "machine-provision-effect-before-barrier-auth": [
      "physicalProviderAdmissions",
      "fn execute_exact_phase() {\n    let validated = self.validate_exact_phase();\n    self.phases.execute();\n}",
      "fn execute_exact_phase() {\n    self.phases.execute();\n    let validated = self.validate_exact_phase();\n}",
    ],
    "machine-publication-admission-bypasses-barrier": [
      "physicalProviderAdmissions",
      "    self.authenticate_parent(&validated);\n",
      "",
    ],
    "machine-publication-effect-before-barrier-auth": [
      "physicalProviderAdmissions",
      "    self.authenticate_parent(&validated);\n    self.phases.execute(command, || self.publish(&validated));",
      "    self.phases.execute(command, || self.publish(&validated));\n    self.authenticate_parent(&validated);",
    ],
    "machine-restart-admission-bypasses-barrier": [
      "physicalProviderAdmissions",
      "fn validate_restart_phase() {\n    self.publication_journal.authenticate_or_stage_restart_witness();\n}",
      "fn validate_restart_phase() {}",
    ],
    "machine-restart-effect-before-barrier-auth": [
      "physicalProviderAdmissions",
      "fn execute_restart_phase() {\n    let validated = self.validate_restart_phase();\n    self.restart_phases.execute();\n}",
      "fn execute_restart_phase() {\n    self.restart_phases.execute();\n    let validated = self.validate_restart_phase();\n}",
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
    "missing-forwarded-canonical-registry": [
      "forwardedCanonicalComposition",
      "    let teardown = WorkloadTeardownCapabilityRegistry::new(\n",
      "    let teardown = IncompleteTeardownRegistry::new(\n",
    ],
    "missing-forwarded-server-registry": [
      "forwardedServerComposition",
      "    prepare_forwarded_workload_profile(backend)\n",
      "    prepare_forwarded_workload_without_teardown(backend)\n",
    ],
    "forwarded-server-discards-canonical-result": [
      "forwardedServerComposition",
      "    prepare_forwarded_workload_profile(backend)\n",
      "    prepare_forwarded_workload_profile(backend);\n    prepare_forwarded_workload_without_teardown(backend)\n",
    ],
    "missing-forwarded-compose-registry": [
      "forwardedComposeComposition",
      "    prepare_forwarded_workload_profile(backend)\n",
      "    prepare_forwarded_workload_without_teardown(backend)\n",
    ],
    "forwarded-compose-discards-canonical-result": [
      "forwardedComposeComposition",
      "    prepare_forwarded_workload_profile(backend)\n",
      "    prepare_forwarded_workload_profile(backend);\n    prepare_forwarded_workload_without_teardown(backend)\n",
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
  } else if (mutation === "compose-direct-stop") {
    sources.composeRetirement +=
      "\nasync fn coarse_compose_stop(backend: &dyn SandboxBackend) { backend.stop(sandbox_id).await; }\n";
  } else if (mutation === "coarse-guest-route-survives") {
    sources.coarseGuestApi +=
      '\nfn stop_service_sandbox() {}\nconst COARSE_OPERATION: &str = "service-sandboxes.stop";\n';
  } else if (mutation === "coarse-guest-wire-survives") {
    sources.coarseGuestApi +=
      "\nstruct MachineApiServiceSandboxStopResponseWire;\nstruct MachineApiServiceSandboxStopResponse;\n";
  } else if (mutation === "coarse-guest-capability-survives") {
    sources.coarseGuestApi +=
      "\nfn machine_api_capabilities() {\n    machine_api_operation_status(MACHINE_API_STOP_OPERATION, Vec::new());\n}\n";
  } else if (mutation === "physical-effect-authentication-outside-stop-body") {
    replaceOnce(
      sources,
      "physicalStopEffects",
      "    let stop_barrier = stop_authority.begin_physical_stop(&authorization);\n",
      "",
    );
    sources.physicalStopEffects +=
      "\nfn unrelated_barrier(stop_authority: &HostMachineStopAuthority, authorization: &ConfirmedMachineStopAuthorization) {\n    stop_authority.begin_physical_stop(authorization);\n}\n";
  } else if (mutation === "machine-stop-policy-moved-to-server-adapter") {
    sources.physicalStopDecisionStoreAdapter +=
      "\nenum MachineWorkloadStopDecision { Crossed }\nfn classify_machine_stop_authority() {}\n";
  } else if (mutation === "machine-barrier-persistence-moved-to-backend") {
    sources.physicalProviderAdmissions +=
      "\nstruct DurableMachineStopBarrier;\n";
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
    replaceOnce(sources, "compute", "fn retire_late_provision_result() {}", "");
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
  const teardownDriver = extractItem(
    sources.compute,
    "impl WorkloadTeardownDriver",
  );
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
      hasAll(confirmation, [
        "confirm_transition",
        "WorkloadSagaConfirmation",
      ]) &&
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
      hasTestsAt(
        sources,
        "crates/nimbus-compute/src/workload_saga/teardown_driver/tests.rs",
        ["teardown_driver_records_exact_five_step_order"],
      ),
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
  const providerImpl =
    extractItem(sources.serverComposition, "impl ServerWorkloadProviders") ||
    extractItem(
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
  const computeFromConfig = extractItem(
    sources.computeState,
    "pub fn from_config",
  );
  const localComposition = extractItem(
    sources.localComposition,
    "fn into_workload_composition",
  );
  const serviceRoute = extractItem(
    sources.httpServices,
    "fn service_lifecycle_route",
  );
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
  const computeSandboxStop = extractItem(
    sources.computeSandboxes,
    "fn stop_sandbox",
  );
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
      hasAll(intoManagedCompute, [
        "teardown_capabilities",
        "ComputeWorkloadComposition::Managed",
      ]) &&
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
      sources.serverComposition.includes(
        "ExactWorkloadTeardownCapabilityRealm::new",
      ) &&
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
      hasAll(definitionRoute, [
        "tenant_context",
        "delete_service_definition",
        "&tenant_context",
      ]) &&
      hasAll(sandboxRoute, [
        "tenant_context",
        "stop_sandbox",
        "&authorization.tenant_context",
      ]) &&
      hasAll(computeServiceLifecycle, [
        "TenantIsolationContext",
        "submit_service_teardown",
      ]) &&
      hasAll(computeDefinitionDelete, [
        "TenantIsolationContext",
        "submit_definition_teardown",
      ]) &&
      hasAll(computeSandboxStop, [
        "TenantIsolationContext",
        "submit_sandbox_teardown",
      ]) &&
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
      hasAll(sourceSignalWait, ["tokio::time::timeout", "entered.acquire"]) &&
      !/yield_now\s*\(/u.test(`${sourceClaimWait}\n${sourceSignalWait}`) &&
      nativeSourceRetirementTestsPass(sources),
  );

  const composeCommandRouter =
    extractItem(
      sources.composeCommand,
      "pub(crate) async fn run_compose_command",
    ) || extractItem(sources.composeCommand, "async fn run_compose_command");
  const composeDown = extractItem(
    sources.composeCommand,
    "async fn run_compose_down",
  );
  const composeRetirement = extractItem(
    sources.composeRetirement,
    "async fn retire_compose_services",
  );
  const compactComposeRetirement = composeRetirement.replace(/\s+/gu, "");
  const composeRuntimeBinding =
    composeRetirement.match(
      /\blet\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*prepared\s*\.\s*activate\s*\([^;{}]*\bsaga_store\b[^;{}]*\)\s*\??\s*;/u,
    )?.[1] ?? "";
  const composeExecutionBinding =
    composeRetirement.match(
      /\blet\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*outcome\s*\.\s*terminal_execution_reference\s*\(\s*\)\s*;/u,
    )?.[1] ?? "";
  const composeRecordedCall = extractCall(
    composeRetirement,
    "ComposeServiceRetirementOutcome::recorded",
  );
  const composeRecordedArm =
    composeRetirement.match(
      /WorkloadTeardownDisposition\s*::\s*Recorded\s*=>\s*(\{[^{}]*\})/u,
    )?.[1] ?? "";
  requireContract(
    /run_compose_down\s*\([^;{}]*persistence_config[^;{}]*\)\s*\.await/u.test(
      composeCommandRouter,
    ) &&
      hasAll(composeDown, [
        "EnginePersistenceConfig",
        "Engine::new_with_persistence_config",
        "retire_compose_services",
        "quiesce",
      ]) &&
      appearsInOrder(composeDown, [
        "Engine::new_with_persistence_config",
        "retire_compose_services",
        "quiesce",
      ]) &&
      hasAll(compactComposeRetirement, [
        "EngineWorkloadSagaStore::new",
        "prepared.activate(",
        "resource_retirer",
        "submit_service_teardown",
        "WorkloadTeardownDisposition::Recorded",
        "terminal_execution_reference",
        "ComposeServiceRetirementOutcome::recorded",
      ]) &&
      appearsInOrder(compactComposeRetirement, [
        "EngineWorkloadSagaStore::new",
        "prepared.activate(",
        "resource_retirer",
        "submit_service_teardown",
        "WorkloadTeardownDisposition::Recorded",
        "terminal_execution_reference",
        "ComposeServiceRetirementOutcome::recorded",
      ]) &&
      composeRuntimeBinding.length > 0 &&
      new RegExp(
        `\\b${escapedRegex(composeRuntimeBinding)}\\s*\\.\\s*resource_retirer\\s*\\(`,
        "u",
      ).test(composeRetirement) &&
      composeExecutionBinding.length > 0 &&
      new RegExp(`\\b${escapedRegex(composeExecutionBinding)}\\b`, "u").test(
        composeRecordedCall,
      ) &&
      returnsCallResult(
        composeRecordedArm,
        "ComposeServiceRetirementOutcome::recorded",
      ) &&
      !/\bCliWorkloadSagaStore\b/u.test(
        `${sources.cli}\n${sources.composeRetirement}`,
      ) &&
      !/\bstop_service_target\b|\bstop_service_sandbox\b|\bservice-sandboxes\.stop\b/u.test(
        `${sources.composeCommand}\n${sources.composeRetirement}`,
      ) &&
      !/\bbackend\s*\.\s*stop\s*\(/u.test(sources.composeRetirement),
    workloadTeardownDiagnostics.compose,
  );

  const exactGuestValidation = extractItem(
    sources.exactGuestTeardown,
    "fn validate(",
  );
  const exactGuestDispatch = extractItem(
    sources.exactGuestTeardown,
    "async fn dispatch(",
  );
  const exactGuestExecute = extractItem(
    sources.exactGuestTeardown,
    "async fn execute(",
  );
  const exactGuestRemoteResult = extractItem(
    sources.exactGuestTeardown,
    "fn remote_result(",
  );
  const exactGuestValidatedBinding =
    exactGuestDispatch.match(
      /\blet\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*self\s*\.\s*validate\s*\([^;]*\)\s*\??\s*;/u,
    )?.[1] ??
    exactGuestDispatch.match(
      /\bOk\s*\(\s*([A-Za-z_][A-Za-z0-9_]*)\s*\)\s*=>/u,
    )?.[1] ??
    "";
  const exactGuestClaimIndex = exactGuestExecute.indexOf(
    "claim_execute_started",
  );
  const exactGuestPreClaim =
    exactGuestClaimIndex >= 0
      ? exactGuestExecute.slice(0, exactGuestClaimIndex)
      : exactGuestExecute;
  const guestRemoteEffectTokens = [
    "remote_result",
    "teardown_workload_phase_prepared",
    "teardown_workload_phase",
    "provision_workload_phase",
  ];
  const coarseGuestTokens = [
    "stop_service_sandbox",
    "service-sandboxes.stop",
    "MACHINE_API_SERVICE_SANDBOX_STOP_PATH",
    "MACHINE_API_STOP_OPERATION",
    "machine_api_service_sandbox_stop_path",
    "MachineApiServiceSandboxStopRequest",
    "MachineApiServiceSandboxStopResponse",
    "MachineApiServiceSandboxStopResponseWire",
  ];
  const initialDesireAdmission =
    extractItem(
      sources.physicalDesireAdmissions,
      "pub async fn submit_intent(",
    ) ||
    extractItem(sources.physicalDesireAdmissions, "async fn submit_intent(");
  const restartDesireAdmission = extractItem(
    sources.physicalDesireAdmissions,
    "async fn compare_and_swap_restart_admission(",
  );
  const physicalStopDecision = extractItem(
    sources.physicalStopDecision,
    "pub async fn authorize_physical_machine_stop(",
  );
  const desireAdmissionAcquire = extractItem(
    sources.physicalStopProvider,
    "fn acquire_blocking(",
  );
  const desireAdmissionPermit = extractItem(
    sources.physicalStopProvider,
    "struct ConfirmedMachineDesireAdmissionPermit",
  );
  const providerAdmissionAbsence = extractItem(
    sources.physicalStopProvider,
    "fn authenticate_workload_admission_absence(",
  );
  const machineStopBarrierDigest = extractItem(
    sources.physicalStopProvider,
    "fn derive_digest(",
  );
  const machineStopBarrierDigestBindings = [
    /\bdomain\s*:\s*STOP_BARRIER_DIGEST_DOMAIN\b/u,
    /\bformat_version\s*:\s*FORMAT_VERSION\b/u,
    /\bmachine_name\s*:\s*&\s*self\s*\.\s*machine_name\b/u,
    /\bforwarder_authority\s*:\s*&\s*self\s*\.\s*forwarder_authority\b/u,
    /\bepoch\s*:\s*self\s*\.\s*epoch\b/u,
    /\bstate\s*:\s*self\s*\.\s*state\b/u,
  ];
  const machineStopBarrierDigestBytes =
    machineStopBarrierDigest.match(
      /\blet\s+([A-Za-z_][A-Za-z0-9_]*)\s*=\s*serde_json\s*::\s*to_vec\s*\(\s*&\s*DigestPayload\s*\{[\s\S]*?\}\s*\)[\s\S]*?\?\s*;/u,
    )?.[1] ?? "";
  const machineStopBarrierReturnsDigest =
    machineStopBarrierDigestBytes.length > 0 &&
    new RegExp(
      `MachineStopBarrierDigest\\s*::\\s*new\\s*\\(\\s*format!\\s*\\(\\s*[^,{}]+,\\s*Sha256\\s*::\\s*digest\\s*\\(\\s*${escapedRegex(machineStopBarrierDigestBytes)}\\s*\\)\\s*\\)\\s*\\)\\s*\\.\\s*map_err\\s*\\(\\s*evidence_error\\s*\\)\\s*$`,
      "u",
    ).test(itemBody(machineStopBarrierDigest));
  const machineAdmissionBarrierTraversal = [
    /\bbody\s*\.\s*stop_barriers\s*\.\s*iter\s*\(\s*\)/u,
    /\.\s*filter\s*\(\s*\|barrier\|\s*barrier\s*\.\s*machine_name\s*==\s*machine_name\s*\)/u,
    /\.\s*max_by_key\s*\(\s*\|barrier\|\s*barrier\s*\.\s*epoch\s*\)/u,
    /\.\s*filter\s*\(\s*\|barrier\|\s*!\s*barrier\s*\.\s*state\s*\.\s*is_terminal\s*\(\s*\)\s*\)/u,
  ];
  const machineAdmissionProviderComparison =
    /barrier\s*\.\s*forwarder_authority\s*\.\s*provider_instance\s*\(\s*\)\s*!=\s*forwarder_authority\s*\.\s*provider_instance\s*\(\s*\)/u;
  const machineAdmissionGenerationComparison =
    /barrier\s*\.\s*forwarder_authority\s*\.\s*generation\s*\(\s*\)\s*!=\s*forwarder_authority\s*\.\s*generation\s*\(\s*\)/u;
  const machineAdmissionProviderRejection =
    /if\s+barrier\s*\.\s*forwarder_authority\s*\.\s*provider_instance\s*\(\s*\)\s*!=\s*forwarder_authority\s*\.\s*provider_instance\s*\(\s*\)\s*\{\s*return\s+Err\s*\([^;{}]*\)\s*;\s*\}/u;
  const machineAdmissionGenerationRejection =
    /if\s+barrier\s*\.\s*forwarder_authority\s*\.\s*generation\s*\(\s*\)\s*!=\s*forwarder_authority\s*\.\s*generation\s*\(\s*\)\s*\{\s*return\s+Err\s*\([^;{}]*\)\s*;\s*\}/u;
  const machineAdmissionFenceRejection = /Err\s*\([^;{}]*\)\s*$/u;
  const machineStopBarrierClaim =
    extractItem(
      sources.physicalStopProvider,
      "pub(crate) fn claim_machine_stop_barrier(",
    ) ||
    extractItem(sources.physicalStopProvider, "fn claim_machine_stop_barrier(");
  const provisionWitnessAdmission =
    extractItem(
      sources.physicalStopProvider,
      "pub(crate) fn authenticate_retirement_witness(",
    ) ||
    extractItem(
      sources.physicalStopProvider,
      "fn authenticate_retirement_witness(",
    );
  const restartWitnessAdmission =
    extractItem(
      sources.physicalStopProvider,
      "pub(crate) fn authenticate_or_stage_restart_witness(",
    ) ||
    extractItem(
      sources.physicalStopProvider,
      "fn authenticate_or_stage_restart_witness(",
    );
  const publicationWitnessAdmission =
    extractItem(
      sources.physicalStopProvider,
      "pub(crate) fn authenticate_or_stage(",
    ) || extractItem(sources.physicalStopProvider, "fn authenticate_or_stage(");
  const provisionValidation = extractItem(
    sources.physicalProviderAdmissions,
    "fn validate_exact_phase(",
  );
  const provisionAdmission = extractItem(
    sources.physicalProviderAdmissions,
    "fn execute_exact_phase(",
  );
  const publicationValidation = extractItem(
    sources.physicalProviderAdmissions,
    "fn validate_publication(",
  );
  const publicationAuthentication = extractItem(
    sources.physicalProviderAdmissions,
    "fn authenticate_parent(",
  );
  const publicationAdmission =
    extractItem(
      sources.physicalProviderAdmissions,
      "impl IngressPublicationCapability",
    ) ||
    extractItem(sources.physicalProviderAdmissions, "fn execute_publication(");
  const restartValidation = extractItem(
    sources.physicalProviderAdmissions,
    "fn validate_restart_phase(",
  );
  const restartProviderAdmission = extractItem(
    sources.physicalProviderAdmissions,
    "fn execute_restart_phase(",
  );
  const standaloneStop = extractItem(
    sources.physicalStopStandalone,
    "async fn run_machine_stop(",
  );
  const serverStop = extractItem(
    sources.physicalStopServer,
    "fn stop_machine<'a>(",
  );
  const restartStop = extractItem(
    sources.physicalStopServer,
    "fn restart_machine<'a>(",
  );
  const bootcRestartStop = extractItem(
    sources.physicalStopOs,
    "fn restart_bootc_machine(",
  );
  const osApplyRestartStop = extractItem(
    sources.physicalStopOs,
    "fn apply_machine_os_change(",
  );
  const osRestartAuthorization = extractItem(
    sources.physicalStopStandalone,
    "async fn authorize_running_machine_os_restart(",
  );
  const authorizedMachineStop = extractItem(
    sources.physicalStopStandalone,
    "impl AuthorizedMachineStop",
  );
  const physicalStopDecisionVariants = enumVariants(
    sources.physicalStopDecision,
    "MachineWorkloadStopDecision",
  );
  const physicalStopEffect =
    extractItem(sources.physicalStopEffects, "pub(super) fn stop_machine(") ||
    extractItem(sources.physicalStopEffects, "fn stop_machine(");
  const machineStopPolicyTokens = [
    "MachineWorkloadStopDecision",
    "classify_machine_stop_authority",
    "list_machine_workload_authority_from_engine",
  ];
  const machineStopPolicyWrongOwners = [
    sources.physicalStopDecisionStoreAdapter,
    sources.physicalStopProvider,
    sources.physicalProviderAdmissions,
  ];
  const machineStopBarrierWrongOwners = [
    sources.physicalStopDecision,
    sources.physicalStopDecisionStoreAdapter,
    sources.physicalProviderAdmissions,
    sources.physicalStopEffects,
    sources.physicalStopStandalone,
    sources.physicalStopServer,
    sources.physicalStopOs,
  ];
  const machineStopBarrierDefinition =
    /\bstruct\s+DurableMachineStopBarrier\b/u;
  requireContract(
    hasAll(sources.exactGuestTeardown, [
      "MachineApiWorkloadTeardownCommandEnvelope",
      "fn build_remote_request",
      "validate_source_and_target",
      "validate_subjects",
      "validate_retirement_order",
      "claim_execute_started",
      "release_parent_batch_after_guest_release",
    ]) &&
      appearsInOrder(exactGuestValidation, [
        "validate_source_and_target",
        "validate_subjects",
        "validate_retirement_order",
      ]) &&
      exactGuestValidatedBinding.length > 0 &&
      new RegExp(
        `self\\s*\\.\\s*execute\\s*\\([^)]*\\b${escapedRegex(exactGuestValidatedBinding)}\\b\\s*\\)`,
        "u",
      ).test(exactGuestDispatch) &&
      /\bremote_request\s*=\s*validated\s*\.\s*remote_request\b/u.test(
        exactGuestExecute,
      ) &&
      exactGuestClaimIndex >= 0 &&
      guestRemoteEffectTokens.every(
        (token) => !exactGuestPreClaim.includes(token),
      ) &&
      appearsInOrder(exactGuestExecute, [
        "claim_execute_started",
        "execute_started_claim_async",
        "remote_result",
      ]) &&
      exactGuestRemoteResult.includes("teardown_workload_phase_prepared") &&
      coarseGuestTokens.every(
        (token) => !sources.coarseGuestApi.includes(token),
      ) &&
      !/machine_api_operation_status\s*\(\s*MACHINE_API_STOP_OPERATION\b/u.test(
        sources.coarseGuestApi,
      ) &&
      [
        "EmptyWithFence",
        "ActiveWorkloadTeardownRequired",
        "AuthorityUnavailable",
        "Ambiguous",
        "Corrupt",
        "Stale",
        "Crossed",
      ].every((variant) => physicalStopDecisionVariants.includes(variant)) &&
      appearsInOrder(initialDesireAdmission, [
        "guard.acquire",
        "self.commit_loaded",
      ]) &&
      (initialDesireAdmission.indexOf("drop(_permit)") < 0 ||
        initialDesireAdmission.indexOf("drop(_permit)") >
          initialDesireAdmission.indexOf("self.commit_loaded")) &&
      appearsInOrder(restartDesireAdmission, [
        "guard.acquire",
        "self.commit_loaded",
        "drop(_permit)",
      ]) &&
      hasAll(sources.physicalStopDecision, machineStopPolicyTokens) &&
      machineStopPolicyWrongOwners.every(
        (owner) =>
          !/\b(?:enum\s+MachineWorkloadStopDecision|fn\s+classify_machine_stop_authority\s*\()/u.test(
            owner,
          ),
      ) &&
      appearsInOrder(physicalStopDecision, [
        "claim_effect_free_barrier",
        "list_machine_workload_authority_from_engine",
        "classify_machine_stop_authority",
        "clear_effect_free_barrier",
      ]) &&
      !/system_projection|ip_address|socket_addr/u.test(
        sources.physicalStopDecision,
      ) &&
      hasAll(sources.physicalStopProvider, [
        "struct DurableMachineStopBarrier",
        "machine_name",
        "forwarder_authority",
        "epoch",
        "state",
        "digest",
        "STOP_BARRIER_DIGEST_DOMAIN",
        "FORMAT_VERSION",
        "authenticate_workload_admission_absence",
      ]) &&
      machineStopBarrierDigestBindings.every((binding) =>
        binding.test(machineStopBarrierDigest),
      ) &&
      machineStopBarrierReturnsDigest &&
      machineAdmissionBarrierTraversal.every((edge) =>
        edge.test(providerAdmissionAbsence),
      ) &&
      appearsInOrder(providerAdmissionAbsence, [
        "stop_barriers",
        "machine_name",
        "max_by_key",
        "is_terminal",
        "provider_instance",
        "generation",
      ]) &&
      machineAdmissionProviderComparison.test(providerAdmissionAbsence) &&
      machineAdmissionGenerationComparison.test(providerAdmissionAbsence) &&
      machineAdmissionProviderRejection.test(providerAdmissionAbsence) &&
      machineAdmissionGenerationRejection.test(providerAdmissionAbsence) &&
      machineAdmissionFenceRejection.test(itemBody(providerAdmissionAbsence)) &&
      appearsInOrder(machineStopBarrierClaim, [
        "self.mutate_with_error",
        "provider_instance",
        "generation",
        "stop_barriers.push",
        "provider_witnesses",
      ]) &&
      appearsInOrder(desireAdmissionAcquire, [
        "acquire_lock",
        "load_envelope",
        "stop_barriers",
        "Ok(lock)",
      ]) &&
      hasAll(desireAdmissionPermit, [
        "ConfirmedMachineDesireAdmissionPermit",
        "_lock",
        "ConfirmedMachinePublicationLock",
      ]) &&
      appearsInOrder(provisionWitnessAdmission, [
        "self.mutate",
        "authenticate_workload_admission_absence",
        "retirement_witnesses",
      ]) &&
      appearsInOrder(restartWitnessAdmission, [
        "self.mutate",
        "authenticate_workload_admission_absence",
        "retirement_witnesses",
      ]) &&
      appearsInOrder(publicationWitnessAdmission, [
        "self.mutate",
        "authenticate_workload_admission_absence",
        "records",
      ]) &&
      provisionValidation.includes("authenticate_retirement_witness") &&
      appearsInOrder(provisionAdmission, [
        "validate_exact_phase",
        "self.phases",
        "execute",
      ]) &&
      publicationValidation.includes("validate_exact_phase") &&
      publicationAuthentication.includes("authenticate_or_stage") &&
      appearsInOrder(publicationAdmission, [
        "validate_publication",
        "authenticate_parent",
        "self.phases.execute",
        "self.publish",
      ]) &&
      restartValidation.includes("authenticate_or_stage_restart_witness") &&
      appearsInOrder(restartProviderAdmission, [
        "validate_restart_phase",
        "self.restart_phases.execute",
      ]) &&
      hasAll(physicalStopEffect, [
        "ConfirmedMachineStopAuthorization",
        "HostMachineStopAuthority",
        "authorization.barrier().machine_name()",
        "authorization.barrier().forwarder_authority()",
        "begin_physical_stop",
        "withdraw_machine_publications",
        "withdraw_machine_ssh_port",
        "stop_provider_machine",
        "stop_pid",
        "stop_exact_process",
        "record_physical_stop_absent",
        "super::write_json_file",
      ]) &&
      appearsInOrder(physicalStopEffect, [
        "authorization.barrier().machine_name()",
        "authorization.barrier().forwarder_authority()",
        "begin_physical_stop",
        "withdraw_machine_publications",
        "withdraw_machine_ssh_port",
        "stop_provider_machine",
        "stop_pid",
        "stop_exact_process",
        "record_physical_stop_absent",
        "super::write_json_file",
      ]) &&
      machineStopBarrierWrongOwners.every(
        (owner) => !machineStopBarrierDefinition.test(owner),
      ) &&
      appearsInOrder(standaloneStop, [
        "HostMachineStopAuthority::new",
        ".authorize",
        "stop_machine_with_layout_authorized",
      ]) &&
      appearsInOrder(serverStop, [
        ".authorize",
        "stop_machine_with_layout_authorized",
      ]) &&
      appearsInOrder(restartStop, [
        ".authorize",
        "restart_machine_with_layout_authorized",
      ]) &&
      appearsInOrder(osRestartAuthorization, [
        "HostMachineStopAuthority::new",
        ".authorize",
        "AuthorizedMachineStop::new",
      ]) &&
      appearsInOrder(authorizedMachineStop, [
        "stop_machine",
        "self.stop_authority",
        "self.authorization",
      ]) &&
      appearsInOrder(bootcRestartStop, [
        "authorized",
        ".take()",
        "missing_machine_stop_authority",
        ".stop",
        "start_machine",
      ]) &&
      appearsInOrder(osApplyRestartStop, [
        "authorized",
        ".take()",
        "missing_machine_stop_authority",
        ".stop",
        "config.guest.image_source",
      ]) &&
      !/\braw_machine_stop\s*\(/u.test(
        `${sources.physicalStopStandalone}\n${sources.physicalStopServer}\n${sources.physicalStopOs}`,
      ) &&
      !/\bstop_machine\s*\(\s*network\s*,\s*paths\s*,\s*config\s*,\s*state\s*\)/u.test(
        sources.physicalStopOs,
      ),
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
      hasAll(sources.forwardedCanonicalComposition, [
        "teardown_capabilities",
        "WorkloadTeardownCapabilityRegistry::new",
        "with_teardown_capabilities",
      ]) &&
      countOccurrences(
        `${sources.forwardedCanonicalComposition}\n${sources.forwardedServerComposition}\n${sources.forwardedComposeComposition}`,
        "WorkloadTeardownCapabilityRegistry::new",
      ) === 1 &&
      returnsCallResult(
        extractItem(
          sources.forwardedServerComposition,
          "fn compose_forwarded_server(",
        ),
        "prepare_forwarded_workload_profile",
      ) &&
      returnsCallResult(
        extractItem(
          sources.forwardedComposeComposition,
          "fn compose_forwarded_foreground(",
        ),
        "prepare_forwarded_workload_profile",
      ) &&
      !hasAll(sources.forwardedServerComposition, [
        "teardown_capabilities",
        "WorkloadTeardownCapabilityRegistry::new",
      ]) &&
      !hasAll(sources.forwardedComposeComposition, [
        "teardown_capabilities",
        "WorkloadTeardownCapabilityRegistry::new",
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
    ]) && forwardedMachineTestsPass(sources, FORWARDED_MACHINE_TESTS.recovery),
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
  if (stage === "physical-machine") {
    const physicalErrors = errors.filter(
      (error) => error === workloadTeardownDiagnostics.machine,
    );
    if (!physicalMachineStopTestsPass(sources)) {
      physicalErrors.push(workloadTeardownDiagnostics.behavior);
    }
    return [...new Set(physicalErrors)];
  }
  if (stage !== "aggregate") {
    return [`unknown workload teardown contract stage: ${stage}`];
  }
  return errors;
}
