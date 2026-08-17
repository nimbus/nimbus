#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import process from "node:process";

const scanner = "scripts/verify-nimbus-network-bind-census.mjs";
const startBootPath = "crates/nimbus-cli/src/start/boot.rs";
const kvCliPath = "crates/nimbus-cli/src/kv.rs";
const kvListenerPath = "crates/nimbus-kv/src/listener.rs";
const kvServerPath = "crates/nimbus-kv/src/server.rs";
const selfTestFixturePath =
  "__nimbus_network_verifier_self_test__/cfg-test-followed-by-production.rs";
const errors = [];

function option(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

const inventoryPath = option("--inventory");
const censusPath = option("--census");
if (!inventoryPath || !censusPath) {
  process.stderr.write(
    "usage: verify-nimbus-network-composition-census.mjs " +
      "--inventory <bind-inventory-path> --census <composition-census-path>\n",
  );
  process.exit(2);
}

const allowedClassifications = new Set([
  "owning-manager",
  "manager-derived-handle",
  "admitted-cross-process-reconstruction",
  "test-fixture",
]);
const allowedRealms = new Set([
  "portable",
  "current-os-node",
  "local-node",
  "parent-host",
  "guest-node",
  "current-os-node-child",
  "future-cluster-node",
]);

// These compiled direct seams exist for explicit embedder/test construction.
// Their authority classification is owned by the exact source occurrence,
// never by a path/function pair or broad `direct` name match that a sibling
// impl method could inherit.
const approvedDirectFixtureOccurrences = new Set([
  "crates/nimbus-kv/src/listener.rs|kv-direct-listener-reconstruction-declaration|reconstruct_direct|1",
  "crates/nimbus-kv/src/listener.rs|kv-direct-listener-incarnation-reconstruction-declaration|reconstruct_direct_for_incarnation|1",
  "crates/nimbus-kv/src/listener.rs|primitive-port-authority-open|reconstruct_direct_for_incarnation|1",
  "crates/nimbus-sandbox/src/backends/container/runtime/network_composition.rs|direct-container-backend-constructor-declaration|new|1",
  "crates/nimbus-sandbox/src/backends/container/runtime/network_composition.rs|segment-direct-reconstruction|new|1",
  "crates/nimbus-sandbox/src/backends/container/runtime/network_composition.rs|ipam-direct-reconstruction|with_segment_allocator_and_process|1",
  "crates/nimbus-sandbox/src/backends/container/runtime/network_composition.rs|port-coordinator-direct-reconstruction|with_segment_allocator_and_process|1",
  "crates/nimbus-sandbox/src/backends/container/runtime/test_hooks.rs|direct-container-backend-construction|reopen_network_teardown_fixture|1",
  "crates/nimbus-sandbox/src/backends/krun/vm.rs|direct-krun-backend-constructor-declaration|new|1",
  "crates/nimbus-sandbox/src/backends/krun/vm.rs|segment-direct-reconstruction|new|1",
  "crates/nimbus-sandbox/src/backends/krun/vm.rs|ipam-direct-reconstruction|with_segment_allocator_and_process|1",
  "crates/nimbus-sandbox/src/backends/krun/vm.rs|port-coordinator-direct-reconstruction|with_segment_allocator_and_process|1",
  "crates/nimbus-sandbox/src/backends/krun/vm/test_hooks.rs|direct-krun-backend-construction|reopen_network_teardown_fixture|1",
  "crates/nimbus-sandbox/src/backends/oci/network/ipam/authority.rs|ipam-direct-reconstruction-declaration|reconstruct_direct|1",
  "crates/nimbus-sandbox/src/backends/oci/network/segment.rs|segment-direct-reconstruction-declaration|reconstruct_direct|1",
  "crates/nimbus-sandbox/src/backends/oci/port_lifecycle/authority.rs|port-coordinator-direct-reconstruction-declaration|reconstruct_direct|1",
  "crates/nimbus-server/src/construction.rs|direct-serve-options-reconstruction-declaration|reconstruct_direct|1",
  "crates/nimbus-server/src/construction.rs|direct-serve-options-reconstruction-declaration|reconstruct_direct_at|1",
  "crates/nimbus-server/src/construction.rs|server-internal-direct-reconstruction|reconstruct_direct_at|1",
  "crates/nimbus-server/src/listener_lease.rs|direct-prebound-listener-reconstruction-declaration|reconstruct_direct|1",
  "crates/nimbus-server/src/listener_lease.rs|server-internal-direct-reconstruction|reconstruct_direct|1",
  "crates/nimbus-server/src/listener_lease.rs|server-internal-direct-reconstruction-declaration|reconstruct_direct|1",
  "crates/nimbus-server/src/listener_lease.rs|server-primitive-direct-reconstruction|reconstruct_direct|1",
  "crates/nimbus-server/src/network_composition.rs|server-primitive-direct-reconstruction-declaration|reconstruct_direct|1",
  "crates/nimbus-server/src/network_composition.rs|primitive-port-authority-open|reconstruct_direct|1",
]);
const approvedPrimitiveManagerOccurrences = new Set([
  "crates/nimbus-network/src/manager.rs|primitive-state-store-open|bootstrap|1",
  "crates/nimbus-network/src/manager.rs|primitive-state-store-open|bootstrap|2",
]);
const approvedPrimitiveReconstructionOccurrences = new Set([
  "crates/nimbus-network/src/attachment_state.rs|primitive-state-store-open|open|1",
  "crates/nimbus-network/src/port_lease.rs|primitive-port-authority-open-declaration|open|1",
  "crates/nimbus-network/src/port_lease.rs|primitive-state-store-open|open|1",
  "crates/nimbus-network/src/state_store.rs|primitive-state-store-open-declaration|open|1",
  "crates/nimbus-network/src/state_store.rs|primitive-state-store-open-declaration|open_with_options|1",
  "crates/nimbus-sandbox/src/backends/oci/network/ipam/authority.rs|primitive-state-store-open|reconstruct|1",
  "crates/nimbus-sandbox/src/backends/oci/network/segment.rs|primitive-state-store-open|reconstruct_from_state_root|1",
  "crates/nimbus-sandbox/src/backends/oci/network/segment.rs|primitive-state-store-open|reconstruct_for_cluster_lease|1",
  "crates/nimbus-sandbox/src/backends/oci/network/segment/cleanup.rs|primitive-state-store-open|reconstruct_for_cluster_cleanup|1",
  "crates/nimbus-sandbox/src/backends/oci/port_lifecycle/authority.rs|primitive-port-authority-open|from_reconstructed_authority|1",
]);
const approvedSegmentPrimitiveReconstructionOccurrences = new Set([
  "crates/nimbus-sandbox/src/backends/oci/network/segment.rs|segment-primitive-reconstruction-declaration|reconstruct_from_state_root|1",
]);
const approvedParentForwarderMint =
  "crates/nimbus-cli/src/machine/manager/launch.rs|" +
  "machine-forwarder-authority-mint|next_machine_forwarder_authority|1";

function readJson(sourcePath, label) {
  if (!fs.existsSync(sourcePath)) {
    errors.push(`${label} missing: ${sourcePath}`);
    return undefined;
  }
  try {
    return JSON.parse(fs.readFileSync(sourcePath, "utf8"));
  } catch (error) {
    errors.push(`${label} is invalid JSON: ${sourcePath}: ${error.message}`);
    return undefined;
  }
}

function occurrenceKey(occurrence) {
  return [
    occurrence.path,
    occurrence.kind,
    occurrence.symbol,
    occurrence.ordinal,
  ].join("|");
}

function validOccurrenceIdentity(occurrence) {
  return (
    occurrence &&
    typeof occurrence.path === "string" &&
    (occurrence.path.startsWith("crates/") ||
      occurrence.path === selfTestFixturePath) &&
    typeof occurrence.kind === "string" &&
    occurrence.kind.length > 0 &&
    typeof occurrence.symbol === "string" &&
    occurrence.symbol.length > 0 &&
    Number.isSafeInteger(occurrence.ordinal) &&
    occurrence.ordinal > 0 &&
    Number.isSafeInteger(occurrence.line) &&
    occurrence.line > 0
  );
}

function isDirectFixtureKind(kind) {
  return (
    kind.includes("direct") ||
    kind.startsWith("kv-direct") ||
    kind.startsWith("server-internal-direct") ||
    kind.startsWith("server-primitive-direct") ||
    kind.startsWith("primitive-")
  );
}

function isSourceOnlyFutureOccurrence(occurrence) {
  return (
    occurrence.os_node_realm === "future-cluster-node" ||
    occurrence.kind.includes("cluster") ||
    occurrence.kind.includes("cleanup-handle") ||
    occurrence.symbol.includes("cluster_lease") ||
    occurrence.symbol.includes("cluster_cleanup")
  );
}

function expectedClassification(occurrence) {
  const { kind } = occurrence;
  if (
    kind.includes("manager-derived") ||
    kind.startsWith("capability-") ||
    kind === "attachment-registration-construction" ||
    kind === "ingress-registration-construction" ||
    kind === "oci-process-construction" ||
    kind.startsWith("machine-boot-") ||
    kind.startsWith("machine-forwarder-")
  ) {
    return "manager-derived-handle";
  }
  if (
    kind.includes("manager") ||
    kind === "local-node-root-resolver" ||
    kind === "local-node-root-resolver-declaration" ||
    kind.startsWith("cli-")
  ) {
    return "owning-manager";
  }
  if (
    kind.includes("runner") ||
    kind.includes("cluster") ||
    kind.includes("cleanup-handle")
  ) {
    return "admitted-cross-process-reconstruction";
  }
  if (isDirectFixtureKind(kind)) {
    const key = occurrenceKey(occurrence);
    if (approvedDirectFixtureOccurrences.has(key)) {
      return "test-fixture";
    }
    if (
      kind.startsWith("primitive-") &&
      approvedPrimitiveManagerOccurrences.has(key)
    ) {
      return "owning-manager";
    }
    if (
      kind.startsWith("primitive-") &&
      approvedPrimitiveReconstructionOccurrences.has(key)
    ) {
      return "admitted-cross-process-reconstruction";
    }
    return undefined;
  }
  if (
    kind === "segment-primitive-reconstruction" ||
    kind === "segment-primitive-reconstruction-declaration"
  ) {
    return approvedSegmentPrimitiveReconstructionOccurrences.has(
      occurrenceKey(occurrence),
    )
      ? "admitted-cross-process-reconstruction"
      : undefined;
  }
  return undefined;
}

function expectedRealm(occurrence) {
  const { kind, path: sourcePath } = occurrence;
  if (
    sourcePath.includes("/machine/api/network_composition.rs") ||
    sourcePath.includes("/machine/api/service_workloads/")
  ) {
    return "guest-node";
  }
  if (
    sourcePath.includes("/machine/") ||
    kind.includes("parent-machine") ||
    kind.startsWith("machine-")
  ) {
    return sourcePath.includes("nimbus-machine/src/state.rs")
      ? "portable"
      : "parent-host";
  }
  if (kind.includes("runner")) return "current-os-node-child";
  if (kind.includes("cluster") || kind.includes("cleanup-handle")) {
    return "future-cluster-node";
  }
  if (sourcePath.includes("nimbus-operator/src/")) return "portable";
  if (
    sourcePath.includes("nimbus-network/src/") ||
    sourcePath.includes("nimbus-sandbox/src/") ||
    sourcePath.includes("nimbus-server/src/") ||
    sourcePath.includes("nimbus-kv/src/")
  ) {
    return "current-os-node";
  }
  return "local-node";
}

function runStructuralScan() {
  if (!fs.existsSync(scanner)) {
    errors.push(`composition census scanner missing: ${scanner}`);
    return [];
  }
  if (!fs.existsSync(inventoryPath)) {
    errors.push(`composition bind inventory missing: ${inventoryPath}`);
    return [];
  }
  const result = spawnSync(
    process.execPath,
    [scanner, "--inventory", inventoryPath, "--print-composition"],
    {
      encoding: "utf8",
      env: process.env,
      maxBuffer: 64 * 1024 * 1024,
    },
  );
  if (result.error) {
    errors.push(`composition census failed to start: ${result.error.message}`);
    return [];
  }
  if (result.status !== 0) {
    errors.push(
      `composition census source scan exited ${result.status}: ${
        [result.stderr, result.stdout].filter(Boolean).join("\n").trim() ||
        "<no output>"
      }`,
    );
    return [];
  }
  try {
    const output = JSON.parse(result.stdout);
    if (!Array.isArray(output)) {
      errors.push("composition census output must be an array");
      return [];
    }
    return output;
  } catch (error) {
    errors.push(`composition census returned invalid JSON: ${error.message}`);
    return [];
  }
}

function validateEvidence(census, rows) {
  const evidence = census.evidence;
  if (!evidence || typeof evidence !== "object" || Array.isArray(evidence)) {
    errors.push("composition census evidence must be an object");
    return;
  }
  if (
    evidence["source-contract"]?.kind !== "source-contract" ||
    typeof evidence["source-contract"]?.truth !== "string" ||
    !evidence["source-contract"].truth.trim()
  ) {
    errors.push(
      "composition census lacks the explicit source-contract evidence",
    );
  }
  for (const [evidenceId, claim] of Object.entries(evidence)) {
    if (!claim || typeof claim !== "object" || Array.isArray(claim)) {
      errors.push(`composition evidence ${evidenceId} must be an object`);
      continue;
    }
    if (claim.kind === "source-contract") {
      if (evidenceId !== "source-contract") {
        errors.push(
          `only source-contract may use source-contract evidence: ${evidenceId}`,
        );
      }
      continue;
    }
    if (claim.kind !== "behavioral") {
      errors.push(
        `composition evidence ${evidenceId} has invalid kind ${claim.kind ?? "<missing>"}`,
      );
      continue;
    }
    if (
      typeof claim.artifact !== "string" ||
      !claim.artifact.startsWith(
        "docs/private/plans/proof/nimbus-network-control-plane/",
      ) ||
      typeof claim.marker !== "string" ||
      !claim.marker.trim()
    ) {
      errors.push(`behavioral evidence ${evidenceId} lacks artifact/marker`);
      continue;
    }
    if (!fs.existsSync(claim.artifact)) {
      errors.push(
        `behavioral evidence artifact missing: ${evidenceId}:${claim.artifact}`,
      );
      continue;
    }
    if (!fs.readFileSync(claim.artifact, "utf8").includes(claim.marker)) {
      errors.push(
        `behavioral evidence marker is absent: ${evidenceId}:${claim.marker}`,
      );
    }
  }

  for (const row of rows) {
    const claim = evidence[row.evidence];
    if (!claim) {
      errors.push(
        `composition occurrence references unknown evidence: ${occurrenceKey(row)}:${row.evidence ?? "<missing>"}`,
      );
      continue;
    }
    if (
      row.classification === "test-fixture" &&
      claim.kind !== "source-contract"
    ) {
      errors.push(
        `test fixture cannot claim runtime proof: ${occurrenceKey(row)}:${row.evidence}`,
      );
    }
    if (isSourceOnlyFutureOccurrence(row) && claim.kind !== "source-contract") {
      errors.push(
        `source-only future network seam cannot claim runtime proof: ${occurrenceKey(row)}:${row.evidence}`,
      );
    }
  }
}

function validateCensus(census, observedOccurrences) {
  if (!census) return;
  if (census.schema_version !== 1) {
    errors.push(
      `composition census schema must be 1; observed ${census.schema_version ?? "<missing>"}`,
    );
  }
  if (
    !census.scope ||
    typeof census.scope.source !== "string" ||
    typeof census.scope.exclusions !== "string" ||
    typeof census.scope.proof_limit !== "string"
  ) {
    errors.push("composition census scope is incomplete");
  }
  const declaredClasses = new Set(census.allowed_classifications ?? []);
  const declaredRealms = new Set(census.allowed_os_node_realms ?? []);
  for (const classification of allowedClassifications) {
    if (!declaredClasses.has(classification)) {
      errors.push(`composition census lacks classification ${classification}`);
    }
  }
  for (const classification of declaredClasses) {
    if (!allowedClassifications.has(classification)) {
      errors.push(
        `composition census declares unknown classification ${classification}`,
      );
    }
  }
  for (const realm of allowedRealms) {
    if (!declaredRealms.has(realm)) {
      errors.push(`composition census lacks OS-node realm ${realm}`);
    }
  }
  for (const realm of declaredRealms) {
    if (!allowedRealms.has(realm)) {
      errors.push(`composition census declares unknown OS-node realm ${realm}`);
    }
  }

  const rows = Array.isArray(census.occurrences) ? census.occurrences : [];
  if (!Array.isArray(census.occurrences)) {
    errors.push("composition census occurrences must be an array");
  }
  const mutation =
    process.env.NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD === "1"
      ? process.env.NIMBUS_NETWORK_VERIFY_TEST_COMPOSITION_MUTATION
      : "";
  if (mutation === "wrong-os-node-realm") {
    const row = rows.find(
      (candidate) =>
        candidate.path ===
          "crates/nimbus-cli/src/machine/api/network_composition.rs" &&
        candidate.kind === "manager-bootstrap",
    );
    if (row) row.os_node_realm = "parent-host";
  } else if (mutation === "false-runtime-proof") {
    const fixture = rows.find(
      (candidate) => candidate.classification === "test-fixture",
    );
    if (fixture) fixture.evidence = "nnc4.6e";
    const future = rows.find(isSourceOnlyFutureOccurrence);
    if (future) future.evidence = "nnc4.6e";
  } else if (mutation === "bless-unapproved-direct") {
    const directCollision = observedOccurrences.find(
      (candidate) =>
        candidate.path === selfTestFixturePath &&
        candidate.kind === "direct-krun-backend-construction",
    );
    if (directCollision) {
      directCollision.path = "crates/nimbus-server/src/listener_lease.rs";
      directCollision.kind =
        "server-internal-direct-reconstruction-declaration";
      directCollision.symbol = "reconstruct_direct";
      directCollision.ordinal = 2;
    }
    const unapproved = observedOccurrences.filter(
      (candidate) =>
        candidate === directCollision ||
        (candidate.path === selfTestFixturePath &&
          candidate.kind === "segment-primitive-reconstruction"),
    );
    for (const occurrence of unapproved) {
      const classification =
        occurrence === directCollision
          ? "test-fixture"
          : "admitted-cross-process-reconstruction";
      rows.push({
        ...occurrence,
        classification,
        os_node_realm: expectedRealm(occurrence),
        evidence: "source-contract",
      });
      census.summary.occurrences += 1;
      if (classification === "test-fixture") {
        census.summary.test_fixtures += 1;
      } else {
        census.summary.admitted_cross_process_reconstructions += 1;
      }
    }
  }

  validateEvidence(census, rows);

  const observed = new Map();
  for (const occurrence of observedOccurrences) {
    if (!validOccurrenceIdentity(occurrence)) {
      errors.push(
        "structural scanner returned a malformed composition occurrence",
      );
      continue;
    }
    const key = occurrenceKey(occurrence);
    if (observed.has(key)) {
      errors.push(`duplicate observed composition occurrence: ${key}`);
    } else {
      observed.set(key, occurrence);
    }
    if (
      occurrence.kind === "machine-forwarder-authority-mint" &&
      key !== approvedParentForwarderMint
    ) {
      errors.push(`guest-minted parent identity is forbidden: ${key}`);
    }
  }
  const droppedKey =
    process.env.NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD === "1"
      ? process.env.NIMBUS_NETWORK_VERIFY_TEST_DROP_COMPOSITION_KEY
      : "";
  if (droppedKey) observed.delete(droppedKey);

  const classified = new Map();
  for (const row of rows) {
    if (!validOccurrenceIdentity(row)) {
      errors.push("composition census contains a malformed occurrence row");
      continue;
    }
    const key = occurrenceKey(row);
    if (classified.has(key)) {
      errors.push(`duplicate composition occurrence classification: ${key}`);
      continue;
    }
    classified.set(key, row);
    if (!allowedClassifications.has(row.classification)) {
      errors.push(
        `composition occurrence has invalid classification: ${key}:${row.classification ?? "<missing>"}`,
      );
    }
    const expectedClass = expectedClassification(row);
    if (!expectedClass) {
      errors.push(
        `composition occurrence kind has no authority policy: ${key}`,
      );
    } else if (row.classification !== expectedClass) {
      errors.push(
        `composition classification mismatch: ${key}:expected=${expectedClass}:observed=${row.classification}`,
      );
    }
    if (!allowedRealms.has(row.os_node_realm)) {
      errors.push(
        `composition occurrence has invalid OS-node realm: ${key}:${row.os_node_realm ?? "<missing>"}`,
      );
    }
    const expectedOsNodeRealm = expectedRealm(row);
    if (row.os_node_realm !== expectedOsNodeRealm) {
      errors.push(
        `composition OS-node realm mismatch: ${key}:expected=${expectedOsNodeRealm}:observed=${row.os_node_realm}`,
      );
    }
  }

  for (const [key, occurrence] of observed) {
    const row = classified.get(key);
    if (!row) {
      errors.push(
        `unclassified production network authority: ${key}:line=${occurrence.line}`,
      );
    } else if (row.line !== occurrence.line) {
      errors.push(
        `stale production network authority line: ${key}:census=${row.line}:source=${occurrence.line}`,
      );
    }
  }
  for (const key of classified.keys()) {
    if (!observed.has(key)) {
      errors.push(`stale production network authority classification: ${key}`);
    }
  }
  for (const [policy, classification, label] of [
    [approvedDirectFixtureOccurrences, "test-fixture", "direct fixture"],
    [
      approvedPrimitiveManagerOccurrences,
      "owning-manager",
      "manager primitive",
    ],
    [
      approvedPrimitiveReconstructionOccurrences,
      "admitted-cross-process-reconstruction",
      "primitive reconstruction",
    ],
    [
      approvedSegmentPrimitiveReconstructionOccurrences,
      "admitted-cross-process-reconstruction",
      "segment primitive reconstruction",
    ],
  ]) {
    for (const key of policy) {
      if (
        !rows.some(
          (row) =>
            row.classification === classification && occurrenceKey(row) === key,
        )
      ) {
        errors.push(`approved ${label} occurrence is stale: ${key}`);
      }
    }
  }

  const counts = {
    occurrences: rows.length,
    owning_managers: rows.filter(
      (row) => row.classification === "owning-manager",
    ).length,
    manager_derived_handles: rows.filter(
      (row) => row.classification === "manager-derived-handle",
    ).length,
    admitted_cross_process_reconstructions: rows.filter(
      (row) => row.classification === "admitted-cross-process-reconstruction",
    ).length,
    test_fixtures: rows.filter((row) => row.classification === "test-fixture")
      .length,
  };
  for (const [name, count] of Object.entries(counts)) {
    if (census.summary?.[name] !== count) {
      errors.push(
        `composition census summary is stale: ${name}:census=${census.summary?.[name] ?? "<missing>"}:rows=${count}`,
      );
    }
  }
}

function validateStartAndKvOrdering() {
  if (!fs.existsSync(startBootPath)) {
    errors.push(`start composition root missing: ${startBootPath}`);
  } else {
    const startBoot = fs.readFileSync(startBootPath, "utf8");
    const activatedPolicy = startBoot.indexOf(
      "let activated_listener = if command.systemd_socket_activation",
    );
    const preparedProfile = startBoot.indexOf(
      "let prepared_server_profile = prepared_network.prepare_server_workload_profile()?",
    );
    const retainedAuthority = startBoot.indexOf(
      "let prepared_network_authority = prepared_network.authority();",
    );
    const completedProfile = startBoot.indexOf(".complete(engine.clone())?");
    const machineLifecycle = startBoot.indexOf(
      "crate::machine::host_machine_lifecycle_manager(",
    );
    if (
      activatedPolicy < 0 ||
      preparedProfile < 0 ||
      retainedAuthority < 0 ||
      completedProfile < 0 ||
      machineLifecycle < 0 ||
      retainedAuthority > preparedProfile ||
      preparedProfile > completedProfile ||
      completedProfile > activatedPolicy ||
      activatedPolicy > machineLifecycle
    ) {
      errors.push(
        "effect-free workload-profile completion must precede activated bind policy, which must precede machine-lifecycle effects",
      );
    }
    const schedulerShutdown = startBoot.indexOf(
      "let _ = scheduler_handle.await;",
    );
    const networkRelease = startBoot.indexOf("drop(prepared_network);");
    if (
      schedulerShutdown < 0 ||
      networkRelease < 0 ||
      networkRelease < schedulerShutdown
    ) {
      errors.push(
        "prepared local-network composition must remain retained through scheduler shutdown",
      );
    }
  }

  if (!fs.existsSync(kvCliPath)) {
    errors.push(`standalone KV composition root missing: ${kvCliPath}`);
  } else {
    const kvCli = fs.readFileSync(kvCliPath, "utf8");
    const kvTestModule = kvCli.indexOf("#[cfg(test)]\nmod tests");
    const kvProduction =
      kvTestModule < 0 ? kvCli : kvCli.slice(0, kvTestModule);
    for (const required of [
      "LocalNodeNetworkRoot::resolve_for_current_platform(explicit_root)",
      "LocalNetworkManager::bootstrap(root.as_path())",
      "NetworkCapabilityRegistry::new(Vec::new())",
      "NimbusKvListenerConfig::from_network_authority(network.authority())",
      "prepare_and_announce_kv_command(command, &mut output).await?",
    ]) {
      if (!kvProduction.includes(required)) {
        errors.push(
          `standalone KV composition lacks required seam: ${required}`,
        );
      }
    }
    for (const forbidden of [
      "control_data_dir",
      "NIMBUS_CONTROL_DATA_DIR",
      "kv_network_state_root",
      "NimbusKvListenerConfig::reconstruct_direct",
      "LocalPortLeaseAuthority::open",
    ]) {
      if (kvProduction.includes(forbidden)) {
        errors.push(
          `standalone KV production composition contains forbidden seam: ${forbidden}`,
        );
      }
    }
    const prepareStart = kvProduction.indexOf("async fn prepare_kv_command");
    const validate = kvProduction.indexOf(
      "validate_kv_command(&command)?",
      prepareStart,
    );
    const manager = kvProduction.indexOf(
      "prepare_standalone_kv_network(",
      prepareStart,
    );
    const store = kvProduction.indexOf(
      "kv_store_for_command(&command, &tenant)?",
      prepareStart,
    );
    const bind = kvProduction.indexOf(
      "bind_listener(&config).await?",
      prepareStart,
    );
    const observation = kvProduction.indexOf(
      "let startup = KvStartupObservation",
      prepareStart,
    );
    if (
      prepareStart < 0 ||
      validate < prepareStart ||
      manager < validate ||
      store < manager ||
      bind < store ||
      observation < bind
    ) {
      errors.push(
        "standalone KV must validate, claim/freeze, create its store, bind, then construct startup observation",
      );
    }
    const announceStart = kvProduction.indexOf(
      "async fn prepare_and_announce_kv_command",
    );
    const prepare = kvProduction.indexOf(
      "prepare_kv_command(command).await?",
      announceStart,
    );
    const write = kvProduction.indexOf(
      "prepared.startup.write_to(output)",
      announceStart,
    );
    const cleanup = kvProduction.indexOf(
      "prepared.close_after_output_error(error)",
      announceStart,
    );
    if (
      announceStart < 0 ||
      prepare < announceStart ||
      write < prepare ||
      cleanup < write
    ) {
      errors.push(
        "standalone KV startup output must consume bound observation and settle on output failure",
      );
    }
  }

  if (!fs.existsSync(kvListenerPath)) {
    errors.push(`standalone KV listener authority missing: ${kvListenerPath}`);
  } else {
    const kvListener = fs.readFileSync(kvListenerPath, "utf8");
    for (const required of [
      "ManagerDerived(LocalNetworkAuthority)",
      "Direct(LocalPortLeaseAuthority)",
      "pub fn from_network_authority(",
      "authority: NimbusKvListenerAuthority",
      "let authority = config.authority();",
      "let port_leases = authority.port_leases();",
    ]) {
      if (!kvListener.includes(required)) {
        errors.push(
          `standalone KV listener lacks retained authority seam: ${required}`,
        );
      }
    }
    const primitiveOpens =
      kvListener.match(/LocalPortLeaseAuthority::open\s*\(/g)?.length ?? 0;
    if (primitiveOpens !== 1) {
      errors.push(
        `standalone KV listener must have exactly one explicitly reconstructed primitive open; observed ${primitiveOpens}`,
      );
    }
  }

  if (!fs.existsSync(kvServerPath)) {
    errors.push(`standalone KV leased serve seam missing: ${kvServerPath}`);
  } else {
    const kvServer = fs.readFileSync(kvServerPath, "utf8");
    if (!kvServer.includes("pub async fn serve_listener(")) {
      errors.push(
        "standalone KV must expose one active-listener serve transition",
      );
    }
    if (kvServer.includes("async fn serve_leased(")) {
      errors.push(
        "standalone KV retains a duplicate private leased serve transition",
      );
    }
  }
}

const census = readJson(censusPath, "composition authority census");
const observed = runStructuralScan();
validateCensus(census, observed);
validateStartAndKvOrdering();

process.stdout.write(errors.join("\n"));
process.exit(errors.length === 0 ? 0 : 1);
