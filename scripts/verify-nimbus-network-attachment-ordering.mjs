#!/usr/bin/env node

import fs from "node:fs";

const paths = {
  attachmentState: "crates/nimbus-network/src/attachment_state.rs",
  stateStore: "crates/nimbus-network/src/state_store.rs",
  segment: "crates/nimbus-network/src/segment.rs",
  lifecycle: "crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle.rs",
  host: "crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/host.rs",
  machineLifecycle:
    "crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/machine_forwarded.rs",
  netavark: "crates/nimbus-sandbox/src/backends/oci/network/netavark.rs",
  providerOperation:
    "crates/nimbus-sandbox/src/backends/oci/network/ipam/provider_operation.rs",
  restart: "crates/nimbus-sandbox/src/backends/container/runtime/restart.rs",
  executionCleanup:
    "crates/nimbus-sandbox/src/backends/container/runtime/execution_cleanup.rs",
  reaper: "crates/nimbus-sandbox/src/backends/oci/network/reaper.rs",
};
const errors = [];
const source = {};
for (const [name, path] of Object.entries(paths)) {
  try {
    source[name] = fs.readFileSync(path, "utf8");
  } catch (error) {
    errors.push(
      `missing or unreadable attachment-ordering input ${path}: ${error.message}`,
    );
    source[name] = "";
  }
}

switch (process.env.NIMBUS_NETWORK_VERIFY_TEST_ATTACHMENT_ORDERING_MUTATION || "") {
  case "":
    break;
  case "missing-association":
    source.attachmentState = source.attachmentState.replaceAll(
      "association: NetworkAttachmentSegmentAssociation",
      "association_removed: ()",
    );
    break;
  case "setup-fence":
    source.netavark = source.netavark.replaceAll(
      "begin_netavark_setup_execution",
      "removed_netavark_setup_execution_fence",
    );
    break;
  case "teardown-fence":
    source.netavark = source.netavark.replaceAll(
      "begin_netavark_teardown_execution",
      "removed_netavark_teardown_execution_fence",
    );
    break;
  case "machine-bypass":
    source.restart = source.restart.replaceAll(
      ".detach_machine_forwarded(",
      ".bypass_shared_attachment_lifecycle(",
    );
    source.executionCleanup = source.executionCleanup.replaceAll(
      ".detach_machine_forwarded(",
      ".bypass_shared_attachment_lifecycle(",
    );
    break;
  case "legacy-purge":
    source.reaper += "\nfn purge_legacy_nimbus0_bridge_marker() {}\n";
    break;
  default:
    errors.push(
      "unknown attachment-ordering verifier mutation: " +
        process.env.NIMBUS_NETWORK_VERIFY_TEST_ATTACHMENT_ORDERING_MUTATION,
    );
}

function requireContains(name, text, token) {
  if (!text.includes(token)) errors.push(`${name} lacks ${token}`);
}

function requireOrdered(name, text, tokens) {
  let cursor = -1;
  for (const token of tokens) {
    const next = text.indexOf(token, cursor + 1);
    if (next < 0) {
      errors.push(`${name} lacks ordered token ${token}`);
      return;
    }
    if (next <= cursor) {
      errors.push(`${name} does not preserve required ordering at ${token}`);
      return;
    }
    cursor = next;
  }
}

requireContains(
  "portable attachment record",
  source.attachmentState,
  "association: NetworkAttachmentSegmentAssociation",
);
requireContains(
  "portable attachment validation",
  source.attachmentState,
  "self.association.lease_epoch() != self.resource.version().lease_epoch()",
);
requireContains(
  "portable exact replay",
  source.attachmentState,
  "existing.association != candidate.association",
);
requireContains("network state format", source.stateStore, "const FORMAT_VERSION: u32 = 2;");
requireContains(
  "allocator inspection contract",
  source.segment,
  "Result<NetworkAttachmentReservationObservation, Self::Error>",
);

requireContains(
  "host setup capability",
  source.host,
  "prepared: PreparedNetavarkSetup",
);
requireContains(
  "host teardown capability",
  source.host,
  "prepared: PreparedNetavarkTeardown",
);
requireOrdered("shared attachment setup", source.lifecycle, [
  "let association = self.authenticate_attach_authority",
  "let durable_record = durable.reserve()?",
  "host.inspect_provider",
  "host.prepare_provider_setup",
  "host.create_namespace",
  "claim_netavark_bindings_with_lifetimes",
  "host.setup_provider",
]);
requireOrdered("shared host-managed teardown", source.lifecycle, [
  "recovery::prepare_detach(&durable, durable_record, provider_observation)",
  "host.prepare_provider_teardown",
  "before_provider_detach(auxiliary_disposition)",
  "host.teardown_provider",
]);
requireOrdered("shared machine-forwarded teardown", source.machineLifecycle, [
  "recovery::prepare_detach(&durable, durable_record, provider_observation)",
  "host.prepare_provider_teardown",
  "before_provider_detach()",
  "host.teardown_provider",
  "after_provider_detach(publication)",
]);

requireOrdered("Netavark setup execution", source.netavark, [
  "fn execute_prepared_container_network_setup_with_runner(",
  "begin_netavark_setup_execution",
  'runner("setup"',
  "complete_netavark_setup",
]);
requireOrdered("Netavark teardown execution", source.netavark, [
  "fn execute_teardown_plan(",
  "begin_netavark_teardown_execution",
  'runner("teardown"',
  "confirm_netavark_provider_detached",
]);
for (const token of [
  "NetavarkProviderOperation::SetupPrepared",
  "NetavarkProviderOperation::Provisioning",
  "NetavarkProviderOperation::TeardownPrepared",
  "NetavarkProviderOperation::Deleting",
  "exact_live_allocation_for_setup_claim",
  "exact_live_allocation_for_teardown_claim",
]) {
  requireContains("sandbox provider-attempt journal", source.providerOperation, token);
}

requireContains(
  "machine restart route",
  source.restart,
  ".detach_machine_forwarded(",
);
requireContains(
  "machine final-cleanup route",
  source.executionCleanup,
  ".detach_machine_forwarded(",
);

const legacyPurge =
  /(?:purge[^a-z0-9]*(?:legacy|nimbus0)|(?:legacy|nimbus0)[^a-z0-9]*purge|legacy_bridge_marker)/i;
for (const [name, text] of Object.entries({
  lifecycle: source.lifecycle,
  host: source.host,
  netavark: source.netavark,
  reaper: source.reaper,
})) {
  if (legacyPurge.test(text)) {
    errors.push(`${name} retains obsolete per-attachment legacy bridge purge authority`);
  }
}

const portableEffects =
  /\b(?:TcpListener|TcpStream|UdpSocket)::|\bstd::process::Command\b|\bnetavark\b|\bcreate_persistent_network_namespace\b/;
for (const path of fs.existsSync("crates/nimbus-network/src")
  ? fs.readdirSync("crates/nimbus-network/src", { recursive: true })
  : []) {
  if (typeof path !== "string" || !path.endsWith(".rs")) continue;
  const absolute = `crates/nimbus-network/src/${path}`;
  const text = fs.readFileSync(absolute, "utf8");
  const code = text
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/\/\/.*$/gm, "")
    .replace(/"(?:\\.|[^"\\])*"/g, '""');
  if (portableEffects.test(code)) {
    errors.push(
      `portable network crate contains provider/transport effect vocabulary: ${absolute}`,
    );
  }
}

process.stdout.write(errors.join("\n"));
process.exit(errors.length === 0 ? 0 : 1);
