// Source-derived NNCV035 contract for the compute-owned workload teardown.
// The product scan and green mutation fixture share the repository scanner and
// the NNCV034 attributed-test assertion implementation.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

import { maskNonCode, walkRust } from "./source-contract-scanner.mjs";
import {
  BEHAVIOR_TESTS,
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
  compose:
    "teardown-contract/compose: Compose down bypasses the canonical Engine-backed compute saga",
  machine:
    "teardown-contract/machine: guest or physical-machine teardown lacks exact phase and active-workload fences",
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
  "settle_issued_restart_before_teardown",
  "retain_late_restart_result",
  "enter_withdrawal_committed_after_restart_settlement",
  "persist_withdrawal_committed",
  "withdraw_exact_publication",
  "drain_exact_execution",
  "stop_exact_execution",
  "detach_exact_network",
  "release_exact_network",
  "record_terminal_evidence",
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
  return {
    workloads: joinSources(workloadEntries),
    compute: joinSources(computeEntries),
    services: joinSources(serviceEntries),
    server: joinSources(serverEntries),
    cli: joinSources([...cliEntries, ...machineEntries]),
    network: joinSources(networkEntries),
    tests: joinSources(testEntries),
    testEntries,
    plan,
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
  return BEHAVIOR_TESTS.every((name) =>
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
      "fn compare_and_swap_teardown_claim()",
      "fn omitted_teardown_claim()",
    ],
    "missing-revision-fence": [
      "compute",
      "    require_exact_revision();\n",
      "",
    ],
    "missing-commit-loaded": ["compute", "    commit_loaded();\n", ""],
    "missing-command": [
      "compute",
      "struct ConfirmedWorkloadTeardownCommand",
      "struct UnconfirmedWorkloadTeardownCommand",
    ],
    "missing-command-transition": [
      "compute",
      "    transition_id: WorkloadSagaTransitionId,\n",
      "",
    ],
    "missing-command-attempt": [
      "compute",
      "    attempt_id: WorkloadTeardownAttemptId,\n",
      "",
    ],
    "missing-command-epoch": [
      "compute",
      "    dispatch_epoch: WorkloadTeardownDispatchEpoch,\n",
      "",
    ],
    "forgeable-command": [
      "compute",
      "    fn from_confirmed_cas_winner()",
      "    pub fn from_confirmed_cas_winner()",
    ],
    "stop-before-withdraw": [
      "compute",
      "    withdraw_exact_publication();\n    drain_exact_execution();\n    stop_exact_execution();",
      "    stop_exact_execution();\n    withdraw_exact_publication();\n    drain_exact_execution();",
    ],
    "detach-before-stop": [
      "compute",
      "    stop_exact_execution();\n    detach_exact_network();",
      "    detach_exact_network();\n    stop_exact_execution();",
    ],
    "release-before-detach": [
      "compute",
      "    detach_exact_network();\n    release_exact_network();",
      "    release_exact_network();\n    detach_exact_network();",
    ],
    "record-before-release": [
      "compute",
      "    release_exact_network();\n    record_terminal_evidence();",
      "    record_terminal_evidence();\n    release_exact_network();",
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
      "fn project_recorded_service_teardown() {}",
      "",
    ],
    "missing-sandbox-projection": [
      "services",
      "fn project_recorded_sandbox_teardown() {}",
      "",
    ],
    "missing-definition-claim": [
      "services",
      "fn claim_service_definition_retirement() {}",
      "",
    ],
    "missing-provision-join": [
      "services",
      "fn cancel_and_join_inflight_provision() {}",
      "",
    ],
    "missing-late-result-drain": [
      "services",
      "fn retire_late_provision_result() {}",
      "",
    ],
    "missing-definition-finalize": [
      "services",
      "fn finalize_service_definition_after_recorded() {}",
      "",
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
      "compute",
      "    settle_issued_restart_before_teardown();\n    retain_late_restart_result();\n    enter_withdrawal_committed_after_restart_settlement();\n    persist_withdrawal_committed();",
      "    persist_withdrawal_committed();\n    settle_issued_restart_before_teardown();\n    retain_late_restart_result();\n    enter_withdrawal_committed_after_restart_settlement();",
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
    replaceOnceInTest(
      sources,
      `fn ${BEHAVIOR_TESTS[0]}`,
      "fn missing_teardown_behavior_test",
    );
  } else if (mutation === "empty-test-body") {
    replaceOnceInTest(
      sources,
      "    let observed = teardown_trace();\n    let expected = expected_teardown_trace();\n    assert_eq!(observed, expected);",
      "",
    );
  } else if (mutation === "helper-only-test-body") {
    replaceOnceInTest(
      sources,
      "    let observed = teardown_trace();\n    let expected = expected_teardown_trace();\n    assert_eq!(observed, expected);",
      "    run_teardown_fixture();",
    );
  } else if (mutation === "declaration-only-test-body") {
    replaceOnceInTest(
      sources,
      "    let observed = teardown_trace();\n    let expected = expected_teardown_trace();\n    assert_eq!(observed, expected);",
      "    let observed = teardown_trace();",
    );
  } else if (mutation === "tautological-test-assertion") {
    replaceOnceInTest(
      sources,
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
  } else if (mutation in replacements) {
    replaceOnce(sources, ...replacements[mutation]);
  } else if (mutation) {
    throw new Error(`unknown teardown contract mutation: ${mutation}`);
  }
}

export function verifyWorkloadTeardownContract() {
  const fixture = process.env.NIMBUS_NETWORK_VERIFY_TEARDOWN_FIXTURE === "1";
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
  const requireContract = (condition, diagnostic) => {
    if (!condition) errors.push(diagnostic);
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

  const reducer = extractItem(
    sources.compute,
    "fn compare_and_swap_teardown_claim",
  );
  requireContract(
    hasAll(sources.workloads, [
      "WorkloadTeardownAttemptId",
      "WorkloadTeardownClaim",
      "WorkloadTeardownDisposition",
    ]) &&
      hasAll(reducer, [
        "require_exact_revision",
        "require_exact_generation",
        "require_exact_desired_digest",
        "reject_crossed_teardown_subject",
        "commit_loaded",
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
      "saga_id: WorkloadSagaId",
      "transition_id: WorkloadSagaTransitionId",
      "generation: WorkloadGeneration",
      "desired_digest: WorkloadDesiredDigest",
      "attempt_id: WorkloadTeardownAttemptId",
      "dispatch_epoch: WorkloadTeardownDispatchEpoch",
      "issuing_revision: WorkloadSagaRevision",
      "subject: WorkloadTeardownSubjects",
      "provider_target: WorkloadTeardownProviderTarget",
    ]) &&
      hasAll(commandConstructor, [
        "fn from_confirmed_cas_winner",
        "WorkloadSagaConfirmation::AppliedByThisCall",
        "WorkloadTeardownCommandMode::Execute",
      ]) &&
      !/\bpub(?:\([^)]*\))?\s+fn\s+from_confirmed_cas_winner\b/u.test(
        commandConstructor,
      ) &&
      hasAll(result, [
        "authenticate_result_transition",
        "authenticate_result_attempt",
        "authenticate_result_dispatch_epoch",
        "WorkloadTeardownCommandMode::Inspect",
        "inspect_before_retry",
        "same_attempt_next_dispatch_epoch",
      ]),
    workloadTeardownDiagnostics.command,
  );

  const driver = extractItem(sources.compute, "fn drive_confirmed_teardown");
  requireContract(
    appearsInOrder(driver, REQUIRED_ORDER),
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
      !/sandbox_backend\s*:\s*Arc\s*<\s*dyn\s+SandboxBackend/u.test(
        sources.services,
      ) &&
      !/sandbox_backend\s*\.\s*stop\s*\(/u.test(sources.services),
    workloadTeardownDiagnostics.service,
  );

  requireContract(
    hasAll(sources.services, [
      "claim_service_definition_retirement",
      "cancel_and_join_inflight_provision",
      "retire_late_provision_result",
      "finalize_service_definition_after_recorded",
    ]),
    workloadTeardownDiagnostics.definitionDelete,
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

  return errors;
}
