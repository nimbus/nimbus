#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import process from "node:process";

const inventoryFlag = process.argv.indexOf("--inventory");
if (inventoryFlag < 0 || !process.argv[inventoryFlag + 1]) {
  process.stderr.write(
    "usage: verify-nimbus-network-composition-census.mjs --inventory <path>\n",
  );
  process.exit(2);
}

const inventoryPath = process.argv[inventoryFlag + 1];
const scanner = "scripts/verify-nimbus-network-bind-census.mjs";
const startBootPath = "crates/nimbus-cli/src/start/boot.rs";
const kvCliPath = "crates/nimbus-cli/src/kv.rs";
const kvListenerPath = "crates/nimbus-kv/src/listener.rs";
const kvServerPath = "crates/nimbus-kv/src/server.rs";
const errors = [];

const expected = new Set([
  "crates/nimbus-cli/src/compose/execution.rs|manager-derived-krun-backend-construction|prepare_local_service_manager_for_selection_with_isolation_mode|1",
  "crates/nimbus-cli/src/compose/mod.rs|cli-staged-manager-claim|prepare_standalone_compose_network|1",
  "crates/nimbus-cli/src/compose/mod.rs|cli-attachment-only-composition|prepare_standalone_compose_network|1",
  "crates/nimbus-cli/src/dev.rs|cli-staged-manager-claim|run_dev_command|1",
  "crates/nimbus-cli/src/dev/plan.rs|cli-complete-composition|resolve_dev_plan_inner|1",
  "crates/nimbus-cli/src/dev/wire.rs|manager-derived-prebound-listeners|resolve_wire_plan|1",
  "crates/nimbus-cli/src/kv.rs|capability-registry-construction|prepare_standalone_kv_network|1",
  "crates/nimbus-cli/src/kv.rs|manager-bootstrap|prepare_standalone_kv_network|1",
  "crates/nimbus-cli/src/network_composition.rs|capability-bundle-construction|into_registry|1",
  "crates/nimbus-cli/src/network_composition.rs|capability-registry-construction|into_registry|1",
  "crates/nimbus-cli/src/network_composition.rs|manager-bootstrap|claim|1",
  "crates/nimbus-cli/src/network_composition.rs|oci-process-construction|prepare_krun_process|1",
  "crates/nimbus-cli/src/start/boot.rs|cli-staged-manager-claim|run_start_command|1",
  "crates/nimbus-cli/src/start/boot.rs|cli-complete-composition|run_start_command_inner|1",
  "crates/nimbus-cli/src/start/boot.rs|manager-derived-serve-options|run_start_command_inner|1",
  "crates/nimbus-sandbox/src/backends/capabilities.rs|attachment-registration-construction|host_managed_attachment_registration_for_target|1",
  "crates/nimbus-server/src/construction.rs|direct-reconstruction-declaration|reconstruct_direct|1",
  "crates/nimbus-server/src/construction.rs|direct-reconstruction-declaration|reconstruct_direct_at|1",
  "crates/nimbus-server/src/construction.rs|server-internal-direct-reconstruction|reconstruct_direct_at|1",
  "crates/nimbus-server/src/listener_lease.rs|direct-reconstruction-declaration|reconstruct_direct|1",
  "crates/nimbus-server/src/listener_lease.rs|server-internal-direct-reconstruction|reconstruct_direct|1",
  "crates/nimbus-server/src/listener_lease.rs|direct-reconstruction-declaration|reconstruct_direct|2",
  "crates/nimbus-server/src/listener_lease.rs|server-primitive-direct-reconstruction|reconstruct_direct|1",
  "crates/nimbus-server/src/network_capabilities.rs|ingress-registration-construction|nimbus_owned_local_ingress_registration|1",
  "crates/nimbus-server/src/network_composition.rs|direct-reconstruction-declaration|reconstruct_direct|1",
  "crates/nimbus-server/src/network_composition.rs|primitive-port-authority-open|reconstruct_direct|1",
  "crates/nimbus-kv/src/listener.rs|direct-reconstruction-declaration|reconstruct_direct|1",
  "crates/nimbus-kv/src/listener.rs|primitive-port-authority-open|reconstruct_direct_for_incarnation|1",
]);

function inLocalCompositionScope(occurrence) {
  const sourcePath = occurrence.path;
  return (
    sourcePath ===
      "__nimbus_network_verifier_self_test__/cfg-test-followed-by-production.rs" ||
    sourcePath === "crates/nimbus-cli/src/network_composition.rs" ||
    sourcePath === kvCliPath ||
    sourcePath === "crates/nimbus-cli/src/dev.rs" ||
    sourcePath.startsWith("crates/nimbus-cli/src/dev/") ||
    sourcePath.startsWith("crates/nimbus-cli/src/start/") ||
    sourcePath.startsWith("crates/nimbus-cli/src/compose/") ||
    sourcePath.startsWith("crates/nimbus-server/src/") ||
    sourcePath === kvListenerPath ||
    sourcePath === "crates/nimbus-sandbox/src/backends/capabilities.rs"
  );
}

function occurrenceKey(occurrence) {
  return [
    occurrence.path,
    occurrence.kind,
    occurrence.symbol,
    occurrence.ordinal,
  ].join("|");
}

if (!fs.existsSync(scanner)) {
  errors.push(`composition census scanner missing: ${scanner}`);
}
if (!fs.existsSync(inventoryPath)) {
  errors.push(`composition census inventory missing: ${inventoryPath}`);
}

let composition = [];
if (errors.length === 0) {
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
  } else if (result.status !== 0) {
    errors.push(
      `composition census source scan exited ${result.status}: ${
        [result.stderr, result.stdout].filter(Boolean).join("\n").trim() ||
        "<no output>"
      }`,
    );
  } else {
    try {
      composition = JSON.parse(result.stdout);
    } catch (error) {
      errors.push(`composition census returned invalid JSON: ${error.message}`);
    }
  }
}

if (!Array.isArray(composition)) {
  errors.push("composition census output must be an array");
  composition = [];
}

const observed = new Map();
for (const occurrence of composition.filter(inLocalCompositionScope)) {
  const key = occurrenceKey(occurrence);
  if (observed.has(key)) {
    errors.push(`duplicate local composition occurrence: ${key}`);
    continue;
  }
  observed.set(key, occurrence);
}

const droppedKey =
  process.env.NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD === "1"
    ? process.env.NIMBUS_NETWORK_VERIFY_TEST_DROP_COMPOSITION_KEY
    : "";
if (droppedKey) {
  observed.delete(droppedKey);
}

for (const [key, occurrence] of observed) {
  if (!expected.has(key)) {
    errors.push(
      `unclassified local composition authority: ${key}:line=${occurrence.line}`,
    );
  }
}
for (const key of expected) {
  if (!observed.has(key)) {
    errors.push(`stale local composition authority classification: ${key}`);
  }
}

if (!fs.existsSync(startBootPath)) {
  errors.push(`start composition root missing: ${startBootPath}`);
} else {
  const startBoot = fs.readFileSync(startBootPath, "utf8");
  const activatedPolicy = startBoot.indexOf(
    "let activated_listener = if command.systemd_socket_activation",
  );
  const forwardedManager = startBoot.indexOf(
    "if prepared_network.requires_forwarded_service_manager()",
  );
  const machineLifecycle = startBoot.indexOf(
    "let machine_lifecycle_manager =",
  );
  if (
    activatedPolicy < 0 ||
    forwardedManager < 0 ||
    machineLifecycle < 0 ||
    activatedPolicy > forwardedManager ||
    activatedPolicy > machineLifecycle
  ) {
    errors.push(
      "activated systemd bind policy must precede forwarded-service and machine-lifecycle effects",
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
  const kvCliProduction =
    kvTestModule < 0 ? kvCli : kvCli.slice(0, kvTestModule);
  for (const required of [
    "LocalNodeNetworkRoot::resolve_for_current_platform(explicit_root)",
    "LocalNetworkManager::bootstrap(root.as_path())",
    "NetworkCapabilityRegistry::new(Vec::new())",
    "NimbusKvListenerConfig::from_network_authority(network.authority())",
    "prepare_and_announce_kv_command(command, &mut output).await?",
  ]) {
    if (!kvCliProduction.includes(required)) {
      errors.push(`standalone KV composition lacks required seam: ${required}`);
    }
  }
  for (const forbidden of [
    "control_data_dir",
    "NIMBUS_CONTROL_DATA_DIR",
    "kv_network_state_root",
    "NimbusKvListenerConfig::reconstruct_direct",
    "LocalPortLeaseAuthority::open",
  ]) {
    if (kvCliProduction.includes(forbidden)) {
      errors.push(`standalone KV production composition contains forbidden seam: ${forbidden}`);
    }
  }

  const prepareStart = kvCliProduction.indexOf("async fn prepare_kv_command");
  const validate = kvCliProduction.indexOf("validate_kv_command(&command)?", prepareStart);
  const manager = kvCliProduction.indexOf("prepare_standalone_kv_network(", prepareStart);
  const store = kvCliProduction.indexOf("kv_store_for_command(&command, &tenant)?", prepareStart);
  const bind = kvCliProduction.indexOf("bind_listener(&config).await?", prepareStart);
  const observation = kvCliProduction.indexOf("let startup = KvStartupObservation", prepareStart);
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

  const announceStart = kvCliProduction.indexOf("async fn prepare_and_announce_kv_command");
  const prepare = kvCliProduction.indexOf("prepare_kv_command(command).await?", announceStart);
  const write = kvCliProduction.indexOf("prepared.startup.write_to(output)", announceStart);
  const cleanup = kvCliProduction.indexOf("prepared.close_after_output_error(error)", announceStart);
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
      errors.push(`standalone KV listener lacks retained authority seam: ${required}`);
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
    errors.push("standalone KV must expose one active-listener serve transition");
  }
  if (kvServer.includes("async fn serve_leased(")) {
    errors.push("standalone KV retains a duplicate private leased serve transition");
  }
}

process.stdout.write(errors.join("\n"));
process.exit(errors.length === 0 ? 0 : 1);
