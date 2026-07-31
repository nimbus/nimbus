#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const repoRoot = process.cwd();
const mutation =
  process.env.NIMBUS_NETWORK_VERIFY_TEST_ATTACHMENT_READINESS_MUTATION ?? "";

const sourcePaths = {
  composition:
    "crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle.rs",
  readiness:
    "crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/attachment_readiness.rs",
  active:
    "crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/active_reconciliation.rs",
  pin: "crates/nimbus-sandbox/src/backends/oci/network/egress_pin.rs",
  container: "crates/nimbus-sandbox/src/backends/container/runtime.rs",
  krun: "crates/nimbus-sandbox/src/backends/krun/vm/lifecycle.rs",
};

function syntheticSources() {
  return {
    composition: [
      "mod attachment_readiness;",
      "mod active_reconciliation;",
      "pub(crate) use attachment_readiness::OciAttachmentReadinessState;",
      "fn inspect_host_managed_readiness() { attachment_readiness::inspect_host_managed_readiness(); }",
    ].join("\n"),
    readiness: [
      "use nimbus_network::{NetworkCondition, NetworkConditionKind, NetworkConditionState, NetworkObservation};",
      "enum OciAttachmentReadinessState { Ready, NotReady }",
      "fn inspect_host_managed_readiness() {",
      "  recovery::inspect_provider();",
      "  pin_provider.inspect();",
      "  inspect_active_netavark_bindings_with_lifetimes();",
      "  if let EgressReadinessState::NotReady(reason) = pep {}",
      "  NetworkObservation::new(version, NetworkResourcePhase::Active, Some(provider), vec![NetworkCondition::new(NetworkConditionKind::Ready, NetworkConditionState::True)]);",
      "}",
    ].join("\n"),
    active: [
      "fn reconcile_active_attachment() {",
      "  reconcile_active_netavark_bindings_with_lifetimes();",
      "  after_provider_setup();",
      "}",
    ].join("\n"),
    pin: [
      "trait OciEgressPinProvider {",
      "  fn apply(&self);",
      "  fn inspect(&self);",
      "}",
    ].join("\n"),
    container: [
      "fn host_managed_attachment_readiness() { inspect_host_managed_readiness(); }",
      "fn pre_spawn() { host_managed_attachment_readiness(); }",
      "fn running_status() { host_managed_attachment_readiness(); }",
    ].join("\n"),
    krun: [
      "fn host_managed_attachment_readiness() { inspect_host_managed_readiness(); }",
      "fn pre_spawn() { host_managed_attachment_readiness(); }",
      "fn running_status() { host_managed_attachment_readiness(); }",
    ].join("\n"),
  };
}

function loadSources() {
  if (mutation) {
    const sources = syntheticSources();
    switch (mutation) {
      case "missing-common-module":
        sources.composition = "fn no_readiness_owner() {}";
        break;
      case "missing-container-consumer":
        sources.container = "fn container_uses_pep_only() {}";
        break;
      case "missing-krun-consumer":
        sources.krun = "fn krun_uses_netns_only() {}";
        break;
      case "missing-pin-inspection":
        sources.pin = "fn apply_egress_pin() {}";
        sources.readiness = sources.readiness.replace(
          "pin_provider.inspect();",
          "",
        );
        break;
      case "missing-active-reconciliation":
        sources.active = "fn active_returns_early() {}";
        break;
      case "readiness-effect-capability":
        sources.readiness += "\nteardown_container_network();";
        break;
      default:
        throw new Error(`unknown NNCV019 mutation: ${mutation}`);
    }
    return { sources, errors: [] };
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

function verify({ sources, errors }) {
  const composition = sources.composition ?? "";
  const readiness = sources.readiness ?? "";
  const active = sources.active ?? "";
  const pin = sources.pin ?? "";
  const container = sources.container ?? "";
  const krun = sources.krun ?? "";

  if (!composition.includes("mod attachment_readiness")) {
    errors.push("OCI attachment lifecycle does not own attachment_readiness");
  }
  if (!composition.includes("attachment_readiness::inspect_host_managed_readiness")) {
    errors.push("OCI attachment lifecycle does not expose its readiness seam");
  }
  for (const token of [
    "OciAttachmentReadinessState",
    "inspect_host_managed_readiness",
    "recovery::inspect_provider",
    "pin_provider.inspect",
    "inspect_active_netavark_bindings_with_lifetimes",
    "EgressReadinessState::NotReady",
    "NetworkObservation::new",
    "NetworkConditionKind::Ready",
    "NetworkConditionState::True",
  ]) {
    if (!readiness.includes(token)) {
      errors.push(`common attachment readiness lacks required seam: ${token}`);
    }
  }
  if (!composition.includes("mod active_reconciliation")) {
    errors.push("OCI attachment lifecycle does not own Active reconciliation");
  }
  for (const token of [
    "reconcile_active_attachment",
    "reconcile_active_netavark_bindings_with_lifetimes",
    "after_provider_setup",
  ]) {
    if (`${composition}\n${active}`.includes(token)) continue;
    errors.push(
      `Active attachment recovery lacks required reconciliation seam: ${token}`,
    );
  }
  for (const token of [
    "trait OciEgressPinProvider",
    "fn apply",
    "fn inspect",
  ]) {
    if (!pin.includes(token)) {
      errors.push(`egress pin provider lacks substitutable evidence seam: ${token}`);
    }
  }
  function consumesAtPreSpawnAndStatus(source) {
    return (
      source.includes("inspect_host_managed_readiness") &&
      (source.match(/host_managed_attachment_readiness\s*\(/g) ?? []).length >=
        3
    );
  }
  if (!consumesAtPreSpawnAndStatus(container)) {
    errors.push(
      "Container does not consume common attachment readiness at pre-spawn and status",
    );
  }
  if (!consumesAtPreSpawnAndStatus(krun)) {
    errors.push(
      "Krun does not consume common attachment readiness at pre-spawn and status",
    );
  }
  for (const forbidden of [
    "teardown_container_network",
    "remove_persistent_network_namespace",
    "release_network_segment_hold",
    "finalize_release",
    "expose_machine_ports",
    "ensure_running(",
    ".apply_transition(",
    ".release(",
  ]) {
    if (readiness.includes(forbidden)) {
      errors.push(
        `read-only attachment readiness gained forbidden effect capability: ${forbidden}`,
      );
    }
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
