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
  containerReadiness:
    "crates/nimbus-sandbox/src/backends/container/runtime/attachment_readiness.rs",
  containerManifest:
    "crates/nimbus-sandbox/src/backends/container/runtime/manifest.rs",
  krun: "crates/nimbus-sandbox/src/backends/krun/vm/lifecycle.rs",
  forwarding:
    "crates/nimbus-sandbox/src/backends/oci/network/forwarding.rs",
  forwardingReceipt:
    "crates/nimbus-sandbox/src/backends/oci/network/forwarding/receipt.rs",
  machinePorts:
    "crates/nimbus-sandbox/src/backends/container/runtime/machine_ports.rs",
  machineLifetime:
    "crates/nimbus-sandbox/src/backends/oci/network/process/machine_proxy_lifetime.rs",
  machinePublication:
    "crates/nimbus-sandbox/src/backends/container/runtime/machine_port_publication.rs",
};

function syntheticSources() {
  return {
    composition: [
      "mod attachment_readiness;",
      "mod active_reconciliation;",
      "pub(crate) use attachment_readiness::OciAttachmentReadinessState;",
      "fn inspect_host_managed_readiness() { attachment_readiness::inspect_host_managed_readiness(); }",
      "fn inspect_machine_forwarded_base_readiness() { attachment_readiness::inspect_machine_forwarded_base_readiness(); }",
      "fn complete_machine_forwarded_readiness() { attachment_readiness::complete_machine_forwarded_readiness(); }",
    ].join("\n"),
    readiness: [
      "use nimbus_network::{NetworkCondition, NetworkConditionKind, NetworkConditionState, NetworkObservation};",
      "enum OciAttachmentReadinessState { Ready, NotReady }",
      "enum OciAttachmentBaseReadinessState { Ready, NotReady }",
      "fn inspect_host_managed_readiness() {",
      "  recovery::inspect_provider();",
      "  pin_provider.inspect();",
      "  inspect_active_netavark_bindings_with_lifetimes();",
      "  if let EgressReadinessState::NotReady(reason) = pep {}",
      "  NetworkObservation::new(version, NetworkResourcePhase::Active, Some(provider), vec![NetworkCondition::new(NetworkConditionKind::Ready, NetworkConditionState::True)]);",
      "}",
      "fn inspect_machine_forwarded_base_readiness() { inspect_common_base(); }",
      "fn complete_machine_forwarded_readiness(publication: std::result::Result<MachineForwardedPublicationReadiness, String>) { MachinePublicationRejected; NetworkObservation::new(); }",
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
      "mod attachment_readiness;",
      "fn pre_spawn() { require_complete_attachment_readiness(); }",
      "fn running_status() { complete_attachment_readiness(); }",
    ].join("\n"),
    containerReadiness: [
      "fn host_managed_attachment_readiness() { inspect_host_managed_readiness(); }",
      "fn machine_forwarded_attachment_readiness() { inspect_machine_forwarded_base_readiness(); inspect_machine_forwarded_publication(); complete_machine_forwarded_readiness(); }",
      "fn complete_attachment_readiness() { validated_machine_port_forwarder(); host_managed_attachment_readiness(); machine_forwarded_attachment_readiness(); }",
      "fn require_complete_attachment_readiness() { complete_attachment_readiness(); }",
    ].join("\n"),
    containerManifest: [
      "enum ContainerNetworkPublicationMode { HostManaged, MachineForwarded }",
      "struct ContainerRunnerExecutionConfig {",
      "  network_publication_mode: ContainerNetworkPublicationMode,",
      "  machine_port_forwarder: Option<OciMachinePortForwarderConfig>,",
      "}",
      "fn validated_machine_port_forwarder() {",
      "  match (&self.network_publication_mode, self.machine_port_forwarder.as_ref()) {",
      "    (ContainerNetworkPublicationMode::HostManaged, None) => Ok(None),",
      "    (ContainerNetworkPublicationMode::MachineForwarded, Some(forwarder)) => Ok(Some(forwarder)),",
      '    (ContainerNetworkPublicationMode::HostManaged, Some(_)) => Err("carries machine forwarder authority"),',
      '    (ContainerNetworkPublicationMode::MachineForwarded, None) => Err("has no machine forwarder authority"),',
      "  }",
      "}",
    ].join("\n"),
    krun: [
      "fn host_managed_attachment_readiness() { inspect_host_managed_readiness(); }",
      "fn pre_spawn() { host_managed_attachment_readiness(); }",
      "fn running_status() { host_managed_attachment_readiness(); }",
    ].join("\n"),
    forwarding: [
      "struct GvproxyForwardRoute { local: String, remote: String, protocol: String }",
      "struct GvproxyUnexposeRequest { local: String, protocol: String }",
      "fn inspect_machine_ports() {",
      "  let routes = fetch_machine_forwarder_routes();",
      "  CurrentMachinePortForwardingObservation::authenticated();",
      "}",
      "fn fetch_machine_forwarder_routes(",
      "    config: &OciMachinePortForwarderConfig,",
      ") -> Result<Vec<GvproxyForwardRoute>> {",
      "  let deadline = Instant::now() + MACHINE_FORWARDER_TIMEOUT;",
      '  send_machine_forwarder_request(config, "GET", "/all", &[], deadline);',
      "  serde_json::from_slice::<Vec<GvproxyForwardRoute>>(&response.body);",
      "}",
    ].join("\n"),
    forwardingReceipt: [
      "struct CurrentMachinePortForwardingObservation {",
      "  provider_instance: NetworkProviderHandle,",
      "  provider_generation: NetworkResourceGeneration,",
      "  receipts: Vec<MachinePortForwardReceipt>,",
      "}",
    ].join("\n"),
    machinePorts: [
      "fn inspect_machine_forwarded_publication() {",
      "  let durable_receipts = exposed_machine_port_receipts();",
      "  inspect_current_publication();",
      "}",
    ].join("\n"),
    machineLifetime: [
      "fn inspect_current_publication() {",
      "  machine_port_proxy_routes();",
      "  MachinePortProxyEntry::Running;",
      "  MachinePortProxyLeaseAuthority::Live;",
      "  require_active_machine_bindings_with_lifetimes();",
      "  provider_is_running();",
      "  inspect_machine_ports();",
      "  MachineForwardedPublicationReadiness;",
      "}",
    ].join("\n"),
    machinePublication: [
      "fn exposed_machine_port_receipts() {",
      "  read_machine_port_receipts(MachinePortPublicationPhase::Exposed);",
      "}",
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
      case "missing-machine-current-inspection":
        sources.forwarding = "fn no_current_forwarding_observation() {}";
        break;
      case "machine-inspection-replays-expose":
        sources.forwarding = sources.forwarding.replace(
          '"/all"',
          '"/expose"',
        );
        break;
      case "machine-inspection-uses-invented-endpoint":
        sources.forwarding = sources.forwarding.replace('"/all"', '"/inspect"');
        break;
      case "machine-inspection-multiplies-deadline":
        sources.forwarding = sources.forwarding.replace(
          "  let deadline = Instant::now() + MACHINE_FORWARDER_TIMEOUT;",
          [
            "  for binding in bindings {",
            "    let deadline = Instant::now() + MACHINE_FORWARDER_TIMEOUT;",
            "    inspect(binding, deadline);",
            "  }",
          ].join("\n"),
        );
        break;
      case "machine-native-route-leaks-authority":
        sources.forwarding = sources.forwarding.replace(
          "struct GvproxyForwardRoute { local: String, remote: String, protocol: String }",
          "struct GvproxyForwardRoute { local: String, remote: String, protocol: String, provider_instance: String, provider_generation: u64 }",
        );
        break;
      case "missing-explicit-machine-publication-mode":
        sources.containerManifest = sources.containerManifest
          .replace(
            "enum ContainerNetworkPublicationMode { HostManaged, MachineForwarded }\n",
            "",
          )
          .replace(
            "  network_publication_mode: ContainerNetworkPublicationMode,\n",
            "",
          )
          .replaceAll("&self.network_publication_mode", "&None");
        break;
      case "machine-mode-infers-from-forwarder-option":
        sources.containerReadiness = sources.containerReadiness.replace(
          "validated_machine_port_forwarder();",
          "machine_port_forwarder.is_some();",
        );
        break;
      case "missing-machine-observation-type":
        sources.forwardingReceipt = "struct PersistedReceiptOnly;";
        break;
      case "missing-machine-durable-receipt":
        sources.machinePorts = sources.machinePorts.replace(
          "exposed_machine_port_receipts();",
          "",
        );
        sources.machinePublication = "fn no_exposed_receipt_reader() {}";
        break;
      case "missing-machine-registry-composition":
        for (const token of [
          "machine_port_proxy_routes();",
          "MachinePortProxyEntry::Running;",
          "MachinePortProxyLeaseAuthority::Live;",
          "require_active_machine_bindings_with_lifetimes();",
          "provider_is_running();",
        ]) {
          sources.machineLifetime = sources.machineLifetime.replace(token, "");
        }
        break;
      case "missing-machine-consumer":
        sources.container = [
          "fn host_managed_attachment_readiness() { inspect_host_managed_readiness(); }",
          "fn pre_spawn() { host_managed_attachment_readiness(); }",
          "fn running_status() { host_managed_attachment_readiness(); }",
        ].join("\n");
        sources.containerReadiness = "fn no_machine_readiness_composer() {}";
        break;
      case "missing-machine-mode-completion":
        sources.readiness = sources.readiness.replace(
          "fn complete_machine_forwarded_readiness(publication: std::result::Result<MachineForwardedPublicationReadiness, String>) { MachinePublicationRejected; NetworkObservation::new(); }",
          "",
        );
        sources.composition = sources.composition.replace(
          "fn complete_machine_forwarded_readiness() { attachment_readiness::complete_machine_forwarded_readiness(); }",
          "",
        );
        break;
      case "forgeable-machine-publication-proof":
        sources.readiness = sources.readiness.replaceAll(
          "MachineForwardedPublicationReadiness",
          "()",
        );
        break;
      case "machine-readiness-effect-capability":
        sources.machineLifetime = sources.machineLifetime.replace(
          "machine_port_proxy_routes();",
          "expose_machine_ports(); machine_port_proxy_routes();",
        );
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

function functionBody(source, functionName) {
  const marker = `fn ${functionName}`;
  const start = source.indexOf(marker);
  if (start < 0) return "";
  const open = source.indexOf("{", start + marker.length);
  if (open < 0) return "";
  let depth = 0;
  for (let index = open; index < source.length; index += 1) {
    if (source[index] === "{") depth += 1;
    if (source[index] === "}") {
      depth -= 1;
      if (depth === 0) return source.slice(open + 1, index);
    }
  }
  return "";
}

function verify({ sources, errors }) {
  const composition = sources.composition ?? "";
  const readiness = sources.readiness ?? "";
  const active = sources.active ?? "";
  const pin = sources.pin ?? "";
  const container =
    `${sources.container ?? ""}\n${sources.containerReadiness ?? ""}`;
  const containerReadiness = sources.containerReadiness ?? "";
  const containerManifest = sources.containerManifest ?? "";
  const krun = sources.krun ?? "";
  const forwarding = sources.forwarding ?? "";
  const forwardingReceipt = sources.forwardingReceipt ?? "";
  const machinePorts = sources.machinePorts ?? "";
  const machineLifetime = sources.machineLifetime ?? "";
  const machinePublication = sources.machinePublication ?? "";

  if (!composition.includes("mod attachment_readiness")) {
    errors.push("OCI attachment lifecycle does not own attachment_readiness");
  }
  if (!composition.includes("attachment_readiness::inspect_host_managed_readiness")) {
    errors.push("OCI attachment lifecycle does not expose its readiness seam");
  }
  for (const token of [
    "attachment_readiness::inspect_machine_forwarded_base_readiness",
    "attachment_readiness::complete_machine_forwarded_readiness",
  ]) {
    if (!composition.includes(token)) {
      errors.push(`OCI attachment lifecycle does not expose machine seam: ${token}`);
    }
  }
  for (const token of [
    "OciAttachmentReadinessState",
    "OciAttachmentBaseReadinessState",
    "inspect_host_managed_readiness",
    "inspect_machine_forwarded_base_readiness",
    "complete_machine_forwarded_readiness",
    "publication: std::result::Result<MachineForwardedPublicationReadiness, String>",
    "MachinePublicationRejected",
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
  const completeContainerCalls =
    container.match(/complete_attachment_readiness\s*\(/g) ?? [];
  for (const token of [
    "machine_forwarded_attachment_readiness",
    "inspect_machine_forwarded_base_readiness",
    "inspect_machine_forwarded_publication",
    "complete_machine_forwarded_readiness",
    "require_complete_attachment_readiness",
  ]) {
    if (!container.includes(token)) {
      errors.push(`Container machine readiness lacks consumer seam: ${token}`);
    }
  }
  if (completeContainerCalls.length < 4) {
    errors.push(
      "Container does not consume mode-complete attachment readiness at pre-spawn and status",
    );
  }
  for (const token of [
    "enum ContainerNetworkPublicationMode",
    "ContainerNetworkPublicationMode::HostManaged",
    "ContainerNetworkPublicationMode::MachineForwarded",
    "network_publication_mode: ContainerNetworkPublicationMode",
    "fn validated_machine_port_forwarder",
    "carries machine forwarder authority",
    "has no machine forwarder authority",
  ]) {
    if (!containerManifest.includes(token)) {
      errors.push(`Container manifest lacks explicit publication-mode fence: ${token}`);
    }
  }
  if (!containerReadiness.includes("validated_machine_port_forwarder")) {
    errors.push(
      "Container readiness does not validate explicit publication mode and provider authority",
    );
  }
  if (containerReadiness.includes("machine_port_forwarder.is_some")) {
    errors.push(
      "Container readiness infers publication mode from optional provider authority",
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

  const providerInspect = functionBody(forwarding, "inspect_machine_ports");
  for (const token of [
    "fetch_machine_forwarder_routes",
    "CurrentMachinePortForwardingObservation::authenticated",
  ]) {
    if (!providerInspect.includes(token)) {
      errors.push(`machine provider current inspection lacks read-only seam: ${token}`);
    }
  }
  const providerFetch = functionBody(
    forwarding,
    "fetch_machine_forwarder_routes",
  );
  for (const token of [
    '"GET", "/all"',
    "&[], deadline",
    "serde_json::from_slice",
  ]) {
    if (!providerFetch.includes(token)) {
      errors.push(`machine provider batch fetch lacks read-only seam: ${token}`);
    }
  }
  if (
    !forwarding.includes(
      "fn fetch_machine_forwarder_routes(\n    config: &OciMachinePortForwarderConfig,\n) -> Result<Vec<GvproxyForwardRoute>>",
    )
  ) {
    errors.push("machine provider batch fetch lacks its exact typed route result");
  }
  if (forwarding.includes('"/inspect"')) {
    errors.push("machine provider adapter contains invented /inspect endpoint");
  }
  if (
    (providerFetch.match(/send_machine_forwarder_request\s*\(/g) ?? [])
      .length !== 1
  ) {
    errors.push(
      "machine provider inspection does not use exactly one native batch-list request",
    );
  }
  if (
    (
      providerFetch.match(
        /Instant::now\(\)\s*\+\s*MACHINE_FORWARDER_TIMEOUT/g,
      ) ?? []
    ).length !== 1
  ) {
    errors.push(
      "machine provider inspection does not use exactly one batch deadline",
    );
  }
  if (/\bfor\s+\w+\s+in\b/.test(providerFetch)) {
    errors.push(
      "machine provider inspection loops over bindings instead of using one batch request",
    );
  }
  const nativeRoute = forwarding.match(
    /struct GvproxyForwardRoute\s*\{(?<body>[^}]*)\}/s,
  )?.groups?.body;
  for (const forbidden of ["provider_instance", "provider_generation"]) {
    if (nativeRoute?.includes(forbidden)) {
      errors.push(
        `gvproxy native forwarding shape leaked Nimbus authority field: ${forbidden}`,
      );
    }
  }
  for (const forbidden of [
    '"/expose"',
    "expose_machine_ports",
    "unexpose_machine_ports",
  ]) {
    if (`${providerInspect}\n${providerFetch}`.includes(forbidden)) {
      errors.push(
        `machine provider inspection gained mutating fallback: ${forbidden}`,
      );
    }
  }
  if (
    !forwardingReceipt.includes(
      "struct CurrentMachinePortForwardingObservation",
    )
  ) {
    errors.push(
      "machine forwarding lacks a distinct non-persisted current-observation type",
    );
  }
  for (const token of [
    "exposed_machine_port_receipts",
    "read_machine_port_receipts",
    "MachinePortPublicationPhase::Exposed",
  ]) {
    if (!machinePublication.includes(token)) {
      errors.push(`machine readiness lacks exact durable Exposed evidence: ${token}`);
    }
  }
  const machinePublicationConsumer = functionBody(
    machinePorts,
    "inspect_machine_forwarded_publication",
  );
  for (const token of [
    "exposed_machine_port_receipts",
    "inspect_current_publication",
  ]) {
    if (!machinePublicationConsumer.includes(token)) {
      errors.push(`machine readiness consumer lacks required evidence: ${token}`);
    }
  }
  const machineInspector = functionBody(
    machineLifetime,
    "inspect_current_publication",
  );
  for (const token of [
    "machine_port_proxy_routes",
    "MachinePortProxyEntry::Running",
    "MachinePortProxyLeaseAuthority::Live",
    "require_active_machine_bindings_with_lifetimes",
    "provider_is_running",
    "inspect_machine_ports",
    "MachineForwardedPublicationReadiness",
  ]) {
    if (!machineInspector.includes(token)) {
      errors.push(`machine readiness lacks exact composed evidence: ${token}`);
    }
  }
  for (const forbidden of [
    "expose_machine_ports",
    "unexpose_machine_ports",
    "ensure_machine_port_proxies_running",
    "begin_machine_port_proxy",
    "persist_exposed_machine_port_receipts",
    ".release(",
    ".shutdown(",
  ]) {
    if (machineInspector.includes(forbidden)) {
      errors.push(
        `machine readiness inspection gained forbidden effect capability: ${forbidden}`,
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
