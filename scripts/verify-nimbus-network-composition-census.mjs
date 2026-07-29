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
const errors = [];

const expected = new Set([
  "crates/nimbus-cli/src/compose/execution.rs|manager-derived-krun-backend-construction|prepare_local_service_manager_for_selection_with_isolation_mode|1",
  "crates/nimbus-cli/src/compose/mod.rs|cli-staged-manager-claim|prepare_standalone_compose_network|1",
  "crates/nimbus-cli/src/compose/mod.rs|cli-attachment-only-composition|prepare_standalone_compose_network|1",
  "crates/nimbus-cli/src/dev.rs|cli-staged-manager-claim|run_dev_command|1",
  "crates/nimbus-cli/src/dev/plan.rs|cli-complete-composition|resolve_dev_plan_inner|1",
  "crates/nimbus-cli/src/dev/wire.rs|manager-derived-prebound-listeners|resolve_wire_plan|1",
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
]);

function inNnc46dScope(occurrence) {
  const sourcePath = occurrence.path;
  return (
    sourcePath ===
      "__nimbus_network_verifier_self_test__/cfg-test-followed-by-production.rs" ||
    sourcePath === "crates/nimbus-cli/src/network_composition.rs" ||
    sourcePath === "crates/nimbus-cli/src/dev.rs" ||
    sourcePath.startsWith("crates/nimbus-cli/src/dev/") ||
    sourcePath.startsWith("crates/nimbus-cli/src/start/") ||
    sourcePath.startsWith("crates/nimbus-cli/src/compose/") ||
    sourcePath.startsWith("crates/nimbus-server/src/") ||
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
for (const occurrence of composition.filter(inNnc46dScope)) {
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

process.stdout.write(errors.join("\n"));
process.exit(errors.length === 0 ? 0 : 1);
