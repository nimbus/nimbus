#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const repoRoot = process.cwd();
const mutation = process.env.NIMBUS_NETWORK_VERIFY_TEST_STARTUP_ORPHAN_MUTATION ?? "";

const sourcePaths = {
  network: "crates/nimbus-sandbox/src/backends/oci/network.rs",
  startup:
    "crates/nimbus-sandbox/src/backends/oci/network/startup_reconciliation.rs",
  container:
    "crates/nimbus-sandbox/src/backends/container/runtime/network_composition.rs",
  krun: "crates/nimbus-sandbox/src/backends/krun/vm.rs",
  portableSegment: "crates/nimbus-network/src/segment.rs",
  sandboxSegment: "crates/nimbus-sandbox/src/backends/oci/network/segment.rs",
  cluster: "crates/nimbus-sandbox/src/backends/oci/network/cluster.rs",
  cleanup:
    "crates/nimbus-sandbox/src/backends/oci/network/segment/cleanup.rs",
  testSupport:
    "crates/nimbus-sandbox/src/backends/oci/network/test_support.rs",
  reaper: "crates/nimbus-sandbox/src/backends/oci/network/reaper.rs",
};

function syntheticSources() {
  return {
    network: [
      "mod startup_reconciliation;",
      "pub(crate) use startup_reconciliation::reconcile_startup_network_state;",
    ].join("\n"),
    startup: [
      "use nimbus_network::{LocalNetworkAttachmentAuthority, NetworkResourcePhase, NetworkTransitionEvidence};",
      "fn reconcile_startup_network_state(attachments: &LocalNetworkAttachmentAuthority, allocator: &OciSegmentAllocator) {",
      "  let report = collect_oci_orphan_evidence();",
      "  for classification in classify_oci_orphan_evidence(&report).candidate_classifications() {",
      "    match classification.disposition() {",
      "      OciOrphanDisposition::Adopt => {}",
      "      OciOrphanDisposition::Quarantine(_) => {",
      "        attachments.apply_transition(NetworkResourcePhase::CleanupPending, NetworkTransitionEvidence::AmbiguousEffect);",
      "        allocator.quarantine();",
      "      }",
      "    }",
      "  }",
      "}",
    ].join("\n"),
    container:
      "reconcile_startup_network_state(attachment_authority, &ipam_authority, segment_allocator.as_ref());",
    krun:
      "reconcile_startup_network_state(attachment_authority, &ipam_authority, segment_allocator.as_ref());",
    portableSegment: "pub trait NetworkSegmentAllocator {}",
    sandboxSegment: "impl NetworkSegmentAllocator for Allocator {}",
    cluster: "impl NetworkSegmentAllocator for ClusterAllocator {}",
    cleanup: "impl CleanupAllocator {}",
    testSupport: "impl NetworkSegmentAllocator for RecordingAllocator {}",
    reaper: "fn reap_bridge_interface() {}",
  };
}

function loadSources() {
  if (mutation) {
    const sources = syntheticSources();
    switch (mutation) {
      case "legacy-live-set":
        sources.portableSegment +=
          "\nfn reconcile_orphans(live: &BTreeSet<(TenantId, NetworkAttachmentId)>) {}";
        break;
      case "missing-container-injection":
        sources.container = "fn construct_container_without_reconciliation() {}";
        break;
      case "missing-krun-injection":
        sources.krun = "fn construct_krun_without_reconciliation() {}";
        break;
      case "cleanup-capability":
        sources.startup += "\nrelease_network_segment_hold();";
        break;
      case "missing-exact-quarantine":
        sources.startup = sources.startup
          .replace("attachments.apply_transition", "inspect_only")
          .replace("allocator.quarantine", "inspect_allocator");
        break;
      default:
        throw new Error(`unknown NNCV018 mutation: ${mutation}`);
    }
    return sources;
  }

  const sources = {};
  const errors = [];
  for (const [key, relative] of Object.entries(sourcePaths)) {
    const absolute = path.join(repoRoot, relative);
    if (!fs.existsSync(absolute)) {
      errors.push(`missing required source: ${relative}`);
      continue;
    }
    sources[key] = fs.readFileSync(absolute, "utf8");
  }
  return { sources, errors };
}

function verify(input) {
  const errors = Array.isArray(input.errors) ? [...input.errors] : [];
  const sources = input.sources ?? input;

  const legacyScan = [
    "portableSegment",
    "sandboxSegment",
    "cluster",
    "cleanup",
    "testSupport",
    "reaper",
    "network",
  ]
    .map((key) => sources[key] ?? "")
    .join("\n");
  for (const legacy of [
    "fn reconcile_orphans",
    "reconcile_network_segment_orphans",
    "live_netns_holds",
  ]) {
    if (legacyScan.includes(legacy)) {
      errors.push(`legacy filename/live-set authority remains: ${legacy}`);
    }
  }

  const network = sources.network ?? "";
  const startup = sources.startup ?? "";
  if (!network.includes("mod startup_reconciliation")) {
    errors.push("OCI network composition does not own startup_reconciliation");
  }
  if (!network.includes("startup_reconciliation::reconcile_startup_network_state")) {
    errors.push("OCI network composition does not export the sole startup reconciler");
  }

  for (const token of [
    "LocalNetworkAttachmentAuthority",
    "collect_oci_orphan_evidence",
    "classify_oci_orphan_evidence",
    "OciOrphanDisposition::Adopt",
    "OciOrphanDisposition::Quarantine",
    "NetworkResourcePhase::CleanupPending",
    "NetworkTransitionEvidence::AmbiguousEffect",
    ".apply_transition",
    ".quarantine",
  ]) {
    if (!startup.includes(token)) {
      errors.push(`startup reconciler lacks required exact seam: ${token}`);
    }
  }

  for (const forbidden of [
    "reconcile_terminal_container_ipam_releases",
    "release_network_segment_hold",
    "finalize_release",
    "remove_persistent_network_namespace",
    "teardown_container_network",
    "reap_bridge_interface",
    ".release(",
  ]) {
    if (startup.includes(forbidden)) {
      errors.push(`startup reconciler gained forbidden cleanup capability: ${forbidden}`);
    }
  }

  const callPattern =
    /reconcile_startup_network_state\s*\([\s\S]{0,500}attachment_authority/;
  if (!callPattern.test(sources.container ?? "")) {
    errors.push("container startup does not inject durable attachment evidence");
  }
  if (!callPattern.test(sources.krun ?? "")) {
    errors.push("krun startup does not inject durable attachment evidence");
  }

  return errors;
}

let loaded;
try {
  loaded = loadSources();
} catch (error) {
  process.stderr.write(`${error.message}\n`);
  process.exit(1);
}
const errors = verify(loaded);
if (errors.length > 0) {
  process.stderr.write(`${errors.join("\n")}\n`);
  process.exit(1);
}
