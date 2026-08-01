#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const mode = process.argv[2];
const validModes = new Set([
  "forbidden-dependencies-effects",
  "single-definition-owner",
  "address-is-not-identity",
  "sandbox-effect-locality",
  "sealed-effect-capabilities",
  "side-effect-free-sandbox-inspection",
  "compute-network-manager-injection",
  "compute-node-workload-coordinator",
]);
if (!validModes.has(mode)) {
  process.stderr.write(
    "usage: verify-nimbus-network-source-contract.mjs " +
      "[forbidden-dependencies-effects|single-definition-owner|address-is-not-identity|" +
      "sandbox-effect-locality|sealed-effect-capabilities|" +
      "side-effect-free-sandbox-inspection|compute-network-manager-injection|" +
      "compute-node-workload-coordinator]\n",
  );
  process.exit(2);
}

const networkSourceRoot =
  process.env.NIMBUS_NETWORK_VERIFY_NETWORK_SCAN_ROOT ??
  "crates/nimbus-network/src";
const errors = [];

function maskNonCode(rustText) {
  const lexicalView = rustText.split("");
  const blank = (start, end) => {
    for (let cursor = start; cursor < end; cursor += 1) {
      if (lexicalView.at(cursor) !== "\n" && lexicalView.at(cursor) !== "\r") {
        lexicalView.splice(cursor, 1, " ");
      }
    }
  };

  let cursor = 0;
  while (cursor < rustText.length) {
    if (rustText.startsWith("//", cursor)) {
      const end = rustText.indexOf("\n", cursor + 2);
      blank(cursor, end < 0 ? rustText.length : end);
      cursor = end < 0 ? rustText.length : end;
      continue;
    }
    if (rustText.startsWith("/*", cursor)) {
      let depth = 1;
      let end = cursor + 2;
      while (end < rustText.length && depth > 0) {
        if (rustText.startsWith("/*", end)) {
          depth += 1;
          end += 2;
        } else if (rustText.startsWith("*/", end)) {
          depth -= 1;
          end += 2;
        } else {
          end += 1;
        }
      }
      blank(cursor, end);
      cursor = end;
      continue;
    }

    const raw = rustText.slice(cursor).match(/^(?:br|rb|cr|r)(#*)"/);
    if (raw) {
      const terminator = `"${raw[1]}`;
      const contentStart = cursor + raw[0].length;
      const found = rustText.indexOf(terminator, contentStart);
      const end = found < 0 ? rustText.length : found + terminator.length;
      blank(cursor, end);
      cursor = end;
      continue;
    }

    const quoteOffset =
      ["b", "c"].includes(rustText[cursor]) && rustText[cursor + 1] === '"'
        ? 1
        : 0;
    if (rustText[cursor + quoteOffset] === '"') {
      let end = cursor + quoteOffset + 1;
      while (end < rustText.length) {
        if (rustText[end] === "\\") {
          end += 2;
        } else if (rustText[end] === '"') {
          end += 1;
          break;
        } else {
          end += 1;
        }
      }
      blank(cursor, end);
      cursor = end;
      continue;
    }

    if (rustText[cursor] === "'") {
      const character = rustText.slice(cursor).match(/^'(?:\\.|[^\\'\r\n])'/u);
      if (character) {
        blank(cursor, cursor + character[0].length);
        cursor += character[0].length;
        continue;
      }
    }
    cursor += 1;
  }
  return lexicalView.join("");
}

function withoutCfgTestItems(rustText) {
  const lexicalView = maskNonCode(rustText);
  const ranges = [];
  const cfgTest = /#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]/g;
  let attribute;
  while ((attribute = cfgTest.exec(lexicalView)) !== null) {
    if (
      ranges.some(
        ([start, end]) => attribute.index >= start && attribute.index < end,
      )
    ) {
      continue;
    }
    let cursor = cfgTest.lastIndex;
    let parentheses = 0;
    let brackets = 0;
    let itemEnd = -1;
    while (cursor < lexicalView.length) {
      const token = lexicalView.at(cursor);
      if (token === "(") parentheses += 1;
      else if (token === ")") parentheses = Math.max(0, parentheses - 1);
      else if (token === "[") brackets += 1;
      else if (token === "]") brackets = Math.max(0, brackets - 1);
      else if (parentheses === 0 && brackets === 0 && token === ";") {
        itemEnd = cursor + 1;
        break;
      } else if (parentheses === 0 && brackets === 0 && token === "{") {
        let depth = 1;
        cursor += 1;
        while (cursor < lexicalView.length && depth > 0) {
          if (lexicalView.at(cursor) === "{") depth += 1;
          else if (lexicalView.at(cursor) === "}") depth -= 1;
          cursor += 1;
        }
        itemEnd = cursor;
        break;
      }
      cursor += 1;
    }
    ranges.push([attribute.index, itemEnd < 0 ? lexicalView.length : itemEnd]);
  }

  const visible = lexicalView.split("");
  for (const [start, end] of ranges) {
    for (let cursor = start; cursor < end; cursor += 1) {
      if (visible[cursor] !== "\n" && visible[cursor] !== "\r") {
        visible[cursor] = " ";
      }
    }
  }
  return visible.join("");
}

function walkRust(directory) {
  const sources = [];
  if (!fs.existsSync(directory) || !fs.statSync(directory).isDirectory()) {
    return sources;
  }
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "tests" || entry.name === "benches") continue;
      sources.push(...walkRust(full));
    } else if (
      entry.isFile() &&
      entry.name.endsWith(".rs") &&
      entry.name !== "tests.rs"
    ) {
      sources.push({
        file: full.split(path.sep).join("/"),
        source: withoutCfgTestItems(fs.readFileSync(full, "utf8")),
      });
    }
  }
  return sources;
}

function addFixture(sources, environmentName) {
  const fixture = process.env[environmentName];
  if (process.env.NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD === "1" && fixture) {
    const configuredPath = process.env[`${environmentName}_PATH`];
    sources.push({
      file:
        configuredPath ||
        `__nimbus_network_verifier_self_test__/${environmentName}.rs`,
      source: withoutCfgTestItems(fixture),
    });
  }
}

function location(source, offset) {
  return source.slice(0, offset).split("\n").length;
}

function firstMatch(sources, pattern) {
  for (const candidate of sources) {
    const match = candidate.source.match(pattern);
    if (match) {
      return `${candidate.file}:${location(candidate.source, match.index)}:${match[0]
        .replace(/\s+/g, " ")
        .trim()}`;
    }
  }
  return null;
}

function definitions(sources, name) {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const pattern = new RegExp(
    `\\b(?:pub(?:\\s*\\([^)]*\\))?\\s+)?(?:struct|enum|trait|type)\\s+${escaped}\\b`,
    "g",
  );
  const found = [];
  for (const candidate of sources) {
    let match;
    while ((match = pattern.exec(candidate.source)) !== null) {
      found.push(
        `${candidate.file}:${location(candidate.source, match.index)}`,
      );
    }
  }
  return found;
}

function verifyForbiddenDependenciesAndEffects() {
  if (
    !fs.existsSync(networkSourceRoot) ||
    !fs.statSync(networkSourceRoot).isDirectory()
  ) {
    errors.push(`network source root missing: ${networkSourceRoot}`);
    return;
  }

  let metadata;
  try {
    metadata = JSON.parse(
      execFileSync(
        "cargo",
        ["metadata", "--no-deps", "--format-version", "1"],
        {
          encoding: "utf8",
          maxBuffer: 64 * 1024 * 1024,
        },
      ),
    );
  } catch (error) {
    errors.push(`cargo metadata failed: ${error.message}`);
    return;
  }
  const networkPackage = metadata.packages.find(
    (candidate) => candidate.name === "nimbus-network",
  );
  if (!networkPackage) {
    errors.push("nimbus-network package absent from cargo metadata");
    return;
  }
  const workspaceNames = new Set(metadata.packages.map((pkg) => pkg.name));
  const dependencyNames = networkPackage.dependencies.map(
    (dependency) => dependency.name,
  );
  const injectedDependency =
    process.env.NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD === "1"
      ? process.env.NIMBUS_NETWORK_VERIFY_TEST_FORBIDDEN_DEPENDENCY
      : "";
  if (injectedDependency) dependencyNames.push(injectedDependency);
  const forbiddenDependencies = dependencyNames.filter(
    (name) =>
      (workspaceNames.has(name) && name !== "nimbus-core") ||
      [
        "axum",
        "h2",
        "hickory-client",
        "hickory-resolver",
        "hyper",
        "hyper-util",
        "iroh",
        "mio",
        "netavark",
        "openraft",
        "pingora",
        "quinn",
        "reqwest",
        "rustls",
        "smol",
        "socket2",
        "tokio",
        "tokio-tungstenite",
        "tonic",
        "tower",
        "trust-dns-client",
        "trust-dns-resolver",
        "tungstenite",
      ].includes(name) ||
      /^(?:aws-|azure(?:-|_)|google-cloud|gcloud|kube(?:-|$))/.test(name),
  );
  if (forbiddenDependencies.length) {
    errors.push(
      `forbidden nimbus-network dependencies: ${[
        ...new Set(forbiddenDependencies),
      ]
        .sort()
        .join(", ")}`,
    );
  }

  const sources = walkRust(networkSourceRoot);
  addFixture(sources, "NIMBUS_NETWORK_VERIFY_TEST_FORBIDDEN_EFFECT");
  const forbiddenPatterns = [
    /\b(?:TcpListener|UdpSocket|UnixListener|TcpSocket|Socket)\s*::\s*bind\s*\(/,
    /\b(?:TcpStream|UnixStream)\s*::\s*connect(?:_timeout)?\s*\(/,
    /\b(?:std|tokio)\s*::\s*net\s*::\s*(?:TcpListener|TcpStream|UdpSocket|UnixListener|UnixStream|TcpSocket)\b/,
    /\b(?:std|tokio)\s*::\s*process\s*::\s*Command\b/,
    /\bCommand\s*::\s*new\s*\(/,
    /\b(?:axum|pingora|netavark|iroh|openraft)\s*::/,
    /\bnimbus_(?:adapters|bin|cli|cluster|compute|egress|engine|kv|machine|proxy|runtime|sandbox|server|services|storage|system|tenant|testing|workloads)\s*::/,
    /\bnix\s*::\s*(?:sched|mount|net)\s*::/,
    /\blibc\s*::\s*(?:socket|bind|listen|connect|setns|unshare|mount|umount2)\b/,
    /\btrait\s+(?:NetworkProvider|NetworkAttachmentProvider|ForwardingProvider|IngressProvider|NameProvider|CertificateProvider)\b/,
  ];
  for (const pattern of forbiddenPatterns) {
    const detail = firstMatch(sources, pattern);
    if (detail) errors.push(`forbidden network provider effect: ${detail}`);
  }

  const portableSegment = sources.find((candidate) =>
    candidate.file.endsWith("/segment.rs"),
  );
  if (!portableSegment) {
    errors.push("portable segment source is missing");
  } else {
    const realization = portableSegment.source.match(
      /\b(?:Netavark|bridge_name|interface_name|network_name|netavark_id)\b/i,
    );
    if (realization) {
      errors.push(
        `portable segment contains provider realization: ${
          portableSegment.file
        }:${location(portableSegment.source, realization.index)}:${realization[0]}`,
      );
    }
  }
}

function verifySingleDefinitionOwner() {
  if (
    !fs.existsSync(networkSourceRoot) ||
    !fs.statSync(networkSourceRoot).isDirectory()
  ) {
    errors.push(`network source root missing: ${networkSourceRoot}`);
    return;
  }
  if (!fs.existsSync("crates") || !fs.statSync("crates").isDirectory()) {
    errors.push("workspace source root missing: crates");
    return;
  }

  const networkSources = walkRust(networkSourceRoot);
  const workspaceSources = walkRust("crates");
  addFixture(
    workspaceSources,
    "NIMBUS_NETWORK_VERIFY_TEST_DUPLICATE_DEFINITION",
  );
  const publicDefinitionPattern =
    /\bpub(?:\s*\([^)]*\))?\s+(?:struct|enum|trait|type)\s+([A-Za-z_][A-Za-z0-9_]*)\b/g;
  const networkDefinitions = new Map();
  for (const candidate of networkSources) {
    let match;
    while ((match = publicDefinitionPattern.exec(candidate.source)) !== null) {
      const owners = networkDefinitions.get(match[1]) ?? [];
      owners.push(
        `${candidate.file}:${location(candidate.source, match.index)}`,
      );
      networkDefinitions.set(match[1], owners);
    }
  }
  if (networkDefinitions.size === 0) {
    errors.push("nimbus-network exposes no public portable definitions");
  }
  for (const [name, owners] of networkDefinitions) {
    const allOwners = definitions(workspaceSources, name);
    if (owners.length !== 1 || allOwners.length !== 1) {
      errors.push(
        `${name} definition owners: ${allOwners.join(", ") || "<none>"}`,
      );
    }
  }

  const formerOwnerSources = [
    ...walkRust("crates/nimbus-core/src"),
    ...walkRust("crates/nimbus-sandbox/src"),
  ];
  const compatibilityAlias =
    /\bpub(?:\s*\([^)]*\))?\s+(?:type|use)\b[^;\n]*\b(?:EndpointProtocol|PublishedEndpoint|AllocatedSegment|NetworkAttachmentId|NetworkSegmentId)\b[^;\n]*;/;
  const aliasDetail = firstMatch(formerOwnerSources, compatibilityAlias);
  if (aliasDetail) {
    errors.push(`legacy portable compatibility alias: ${aliasDetail}`);
  }

  const stableIds = [
    "NetworkPlanId",
    "NetworkAttachmentId",
    "NetworkSegmentId",
    "PublishedEndpointId",
    "ListenerId",
    "IngressRouteId",
    "PortLeaseId",
    "NetworkProviderId",
  ];
  const identity = networkSources.find((candidate) =>
    candidate.file.endsWith("/identity.rs"),
  );
  if (!identity) {
    errors.push("nimbus-network identity.rs is missing");
    return;
  }
  const stableIdBackingFields = [
    ...identity.source.matchAll(
      /pub\s+struct\s+\$name\s*\(\s*String\s*\)\s*;/g,
    ),
  ].length;
  if (stableIdBackingFields !== 1) {
    errors.push(
      `stable ID macro must have exactly one opaque String backing field; found ${stableIdBackingFields}`,
    );
  }
  for (const name of stableIds) {
    const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const invocation = new RegExp(
      `define_stable_resource_id!\\s*\\(\\s*${escaped}\\s*,`,
      "g",
    );
    const invocations = [...identity.source.matchAll(invocation)].length;
    const concreteOwners = definitions(workspaceSources, name);
    if (invocations !== 1 || concreteOwners.length !== 0) {
      errors.push(
        `${name} macro owners=${invocations}, concrete owners=${
          concreteOwners.join(", ") || "<none>"
        }`,
      );
    }
  }
}

function verifyAddressIsNotIdentity() {
  if (
    !fs.existsSync(networkSourceRoot) ||
    !fs.statSync(networkSourceRoot).isDirectory()
  ) {
    errors.push(`network source root missing: ${networkSourceRoot}`);
    return;
  }
  const sources = walkRust(networkSourceRoot);
  addFixture(sources, "NIMBUS_NETWORK_VERIFY_TEST_ADDRESS_IDENTITY");
  const addressType =
    "(?:(?:std|core)::net::)?(?:SocketAddr|IpAddr|Ipv4Addr|Ipv6Addr)|Cidr|u(?:8|16|32|64|128|size)|i(?:8|16|32|64|128|size)";
  const patterns = [
    new RegExp(
      `\\b(?:pub(?:\\s*\\([^)]*\\))?\\s+)?struct\\s+\\w*Id\\s*\\(\\s*(?:pub(?:\\s*\\([^)]*\\))?\\s+)?(?:${addressType})\\b`,
    ),
    new RegExp(
      `\\b(?:pub(?:\\s*\\([^)]*\\))?\\s+)?type\\s+\\w*Id\\s*=\\s*(?:${addressType})\\b`,
    ),
    new RegExp(
      `\\b(?:[A-Za-z_][A-Za-z0-9_]*_id|id)\\s*:\\s*(?:${addressType})\\b`,
    ),
    new RegExp(
      `\\bimpl\\s+(?:From|TryFrom)\\s*<\\s*(?:${addressType})\\s*>\\s+for\\s+\\w*Id\\b`,
    ),
    new RegExp(
      `\\bimpl\\s+(?:From|TryFrom)\\s*<\\s*\\w*Id\\s*>\\s+for\\s+(?:${addressType})\\b`,
    ),
    /\bfn\s+\w*id\w*\s*\([^)]*\b(?:addr|address|cidr|port)\b[^)]*\)\s*->\s*\w*Id\b/,
    /\b(?:attachment_id|segment_id|endpoint_id|listener_id|route_id|lease_id|provider_id)\s*:\s*(?:addr|address|cidr|port)\b/,
  ];
  for (const pattern of patterns) {
    const detail = firstMatch(sources, pattern);
    if (detail) errors.push(`address-derived network identity: ${detail}`);
  }

  const segmentSource = sources.find((candidate) =>
    candidate.file.endsWith("/segment.rs"),
  )?.source;
  if (
    !segmentSource ||
    !/\bsegment_id\s*:\s*NetworkSegmentId\b/.test(segmentSource) ||
    !/\bcidr\s*:\s*Cidr\b/.test(segmentSource)
  ) {
    errors.push(
      "AllocatedSegment must keep NetworkSegmentId identity distinct from Cidr location",
    );
  }
}

function allMatches(sources, pattern) {
  const found = [];
  for (const candidate of sources) {
    pattern.lastIndex = 0;
    let match;
    while ((match = pattern.exec(candidate.source)) !== null) {
      found.push({
        file: candidate.file,
        line: location(candidate.source, match.index),
        text: match[0].replace(/\s+/g, " ").trim(),
      });
      if (!pattern.global) break;
    }
  }
  return found;
}

function requireExactOwner(
  sources,
  label,
  pattern,
  allowedFiles,
  expectedCount,
) {
  const found = allMatches(sources, pattern);
  const misplaced = found.filter((match) => !allowedFiles.has(match.file));
  if (found.length !== expectedCount || misplaced.length) {
    errors.push(
      `${label} owners expected=${expectedCount}, found=${found.length}: ` +
        (found
          .map((match) => `${match.file}:${match.line}:${match.text}`)
          .join(", ") || "<none>"),
    );
  }
}

function verifySandboxEffectLocality() {
  const sandboxRoot = "crates/nimbus-sandbox/src";
  if (!fs.existsSync(sandboxRoot)) {
    errors.push(`sandbox source root missing: ${sandboxRoot}`);
    return;
  }
  const sources = walkRust(sandboxRoot).filter(
    (candidate) =>
      !candidate.file.endsWith("_tests.rs") &&
      !candidate.file.endsWith("/test_api.rs") &&
      !candidate.file.endsWith("/test_support.rs") &&
      candidate.file !==
        "crates/nimbus-sandbox/src/backends/container/runtime/lifecycle.rs",
  );
  addFixture(sources, "NIMBUS_NETWORK_VERIFY_TEST_SANDBOX_EFFECT_LOCALITY");
  const only = (...files) => new Set(files);
  requireExactOwner(
    sources,
    "namespace syscalls",
    /\blibc\s*::\s*(?:unshare|mount|umount2)\b/g,
    only("crates/nimbus-sandbox/src/backends/oci/network/netns.rs"),
    3,
  );
  requireExactOwner(
    sources,
    "namespace capability call path",
    /\b(?:create|remove)_persistent_network_namespace\s*\(/g,
    only(
      "crates/nimbus-sandbox/src/backends/oci/network/netns.rs",
      "crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/host.rs",
    ),
    5,
  );
  requireExactOwner(
    sources,
    "Netavark process launch",
    /\bstd\s*::\s*process\s*::\s*Command\s*::\s*new\s*\(\s*&operation\s*\.\s*config\s*\.\s*netavark_path\s*\)/g,
    only("crates/nimbus-sandbox/src/backends/oci/network/netavark.rs"),
    1,
  );
  requireExactOwner(
    sources,
    "prepared Netavark capability call path",
    /\b(?:prepare_container_network_(?:setup|teardown)|execute_prepared_container_network_(?:setup|teardown))\s*\(/g,
    only(
      "crates/nimbus-sandbox/src/backends/oci/network/netavark.rs",
      "crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/host.rs",
    ),
    8,
  );
  requireExactOwner(
    sources,
    "sandbox production process launch",
    /\bCommand\s*::\s*new\s*\(/g,
    only(
      "crates/nimbus-sandbox/src/bin/nimbus-guest-user-switch.rs",
      "crates/nimbus-sandbox/src/backends/conmon/lifecycle.rs",
      "crates/nimbus-sandbox/src/backends/oci/command.rs",
      "crates/nimbus-sandbox/src/backends/oci/network/egress_pin.rs",
      "crates/nimbus-sandbox/src/backends/oci/network/netavark.rs",
      "crates/nimbus-sandbox/src/backends/oci/network/reaper.rs",
    ),
    7,
  );
  requireExactOwner(
    sources,
    "sandbox network connect effects",
    /\bTcpStream\s*::\s*connect_timeout\s*\(/g,
    only(
      "crates/nimbus-sandbox/src/backends/readiness_probe.rs",
      "crates/nimbus-sandbox/src/backends/oci/network/forwarding.rs",
      "crates/nimbus-sandbox/src/backends/oci/network/proxy.rs",
    ),
    5,
  );
  requireExactOwner(
    sources,
    "machine proxy listener bind",
    /\bTcpListener\s*::\s*bind\s*\(/g,
    only("crates/nimbus-sandbox/src/backends/oci/network/proxy.rs"),
    1,
  );
  requireExactOwner(
    sources,
    "machine proxy byte forwarding",
    /\bcopy_machine_port_stream\s*\(/g,
    only("crates/nimbus-sandbox/src/backends/oci/network/proxy.rs"),
    3,
  );
  requireExactOwner(
    sources,
    "real machine-forwarding provider",
    /\bimpl\s+MachinePortForwardingProvider\s+for\s+OciMachinePortForwarderConfig\b/g,
    only("crates/nimbus-sandbox/src/backends/oci/network/forwarding.rs"),
    1,
  );
  requireExactOwner(
    sources,
    "native gvproxy request owner",
    /\bsend_machine_forwarder_request\s*\(/g,
    only("crates/nimbus-sandbox/src/backends/oci/network/forwarding.rs"),
    4,
  );
  requireExactOwner(
    sources,
    "machine-forwarding mutation caller",
    /\bprovider\s*\.\s*(?:expose_one|withdraw_one)\s*\(/g,
    only(
      "crates/nimbus-sandbox/src/backends/container/runtime/machine_port_publication.rs",
    ),
    2,
  );
  requireExactOwner(
    sources,
    "readiness provider definition",
    /\btrait\s+ReadinessProbeProvider\b/g,
    only("crates/nimbus-sandbox/src/backends/readiness_probe.rs"),
    1,
  );
  requireExactOwner(
    sources,
    "readiness provider implementation",
    /\bimpl\s+ReadinessProbeProvider\s+for\s+SocketReadinessProbeProvider\b/g,
    only("crates/nimbus-sandbox/src/backends/readiness_probe.rs"),
    1,
  );

  for (const file of [
    "crates/nimbus-sandbox/src/backends/container/runtime/status.rs",
    "crates/nimbus-sandbox/src/backends/container/runtime/network_composition.rs",
    "crates/nimbus-sandbox/src/backends/container/runtime.rs",
    "crates/nimbus-sandbox/src/backends/krun/vm/readiness.rs",
    "crates/nimbus-sandbox/src/backends/krun/vm.rs",
  ]) {
    const source = sources.find((candidate) => candidate.file === file)?.source;
    if (!source) {
      errors.push(`required readiness consumer missing: ${file}`);
    } else if (
      /\b(?:TcpStream|TcpListener|Command)\b/.test(source) ||
      /\bfn\s+(?:probe_target_ready|probe_http_ready|readiness_probe_target)\b/.test(
        source,
      )
    ) {
      errors.push(`backend bypasses shared readiness capability: ${file}`);
    }
  }
}

function verifySealedEffectCapabilities() {
  const sandboxSources = walkRust("crates/nimbus-sandbox/src");
  addFixture(
    sandboxSources,
    "NIMBUS_NETWORK_VERIFY_TEST_SEALED_EFFECT_CAPABILITY",
  );
  const networkSources = walkRust(networkSourceRoot);
  addFixture(
    networkSources,
    "NIMBUS_NETWORK_VERIFY_TEST_PORTABLE_EFFECT_CAPABILITY",
  );

  const requiredSource = (file) => {
    const source = sandboxSources.find(
      (candidate) => candidate.file === file,
    )?.source;
    if (!source) errors.push(`required capability owner missing: ${file}`);
    return source ?? "";
  };
  const host = requiredSource(
    "crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/host.rs",
  );
  const netavark = requiredSource(
    "crates/nimbus-sandbox/src/backends/oci/network/netavark.rs",
  );
  const netns = requiredSource(
    "crates/nimbus-sandbox/src/backends/oci/network/netns.rs",
  );
  const networkRoot = requiredSource(
    "crates/nimbus-sandbox/src/backends/oci/network.rs",
  );
  const egressPin = requiredSource(
    "crates/nimbus-sandbox/src/backends/oci/network/egress_pin.rs",
  );
  const readinessFiles = new Set([
    "crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle.rs",
    "crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/attachment_readiness.rs",
  ]);
  const readinessSources = sandboxSources.filter((candidate) =>
    readinessFiles.has(candidate.file),
  );
  for (const file of readinessFiles) requiredSource(file);
  const readiness = readinessSources
    .map((candidate) => candidate.source)
    .join("\n");
  const forwarding = requiredSource(
    "crates/nimbus-sandbox/src/backends/oci/network/forwarding.rs",
  );

  if (!/\bpub\s*\(\s*super\s*\)\s+trait\s+AttachmentHostEffects\b/.test(host)) {
    errors.push("AttachmentHostEffects must remain lifecycle-private");
  }
  if (
    [
      ...netavark.matchAll(
        /\bpub\s*\(\s*super\s*\)\s+struct\s+PreparedNetavark(?:Setup|Teardown)\b/g,
      ),
    ].length !== 2
  ) {
    errors.push("prepared Netavark capabilities must remain network-private");
  }
  if (
    [
      ...netns.matchAll(
        /\bpub\s*\(\s*super\s*\)\s+fn\s+(?:create|remove)_persistent_network_namespace\b/g,
      ),
    ].length !== 2
  ) {
    errors.push("namespace effects must remain network-private");
  }
  if (/\bpub[^{;\n]*\buse\s+netns\b/.test(networkRoot)) {
    errors.push(
      "namespace effects must not be reexported from the network root",
    );
  }
  const widenedSandboxCapability = firstMatch(
    sandboxSources,
    /\bpub\s*\(\s*crate\s*\)\s+(?:trait\s+AttachmentHostEffects|struct\s+PreparedNetavark(?:Setup|Teardown)|fn\s+(?:create|remove)_persistent_network_namespace)\b|\bpub[^{;\n]*\buse\s+netns\b/,
  );
  if (widenedSandboxCapability) {
    errors.push(
      `privileged sandbox capability widened: ${widenedSandboxCapability}`,
    );
  }
  if (
    !/\btrait\s+OciEgressPinObserver\b/.test(egressPin) ||
    !/\btrait\s+OciEgressPinProvider\s*:\s*OciEgressPinObserver\b/.test(
      egressPin,
    )
  ) {
    errors.push(
      "egress-pin observation and mutation capabilities are not separated",
    );
  }
  if (
    /\bOciEgressPinProvider\b/.test(readiness) ||
    readinessSources.length < readinessFiles.size ||
    readinessSources.some(
      (candidate) => !/\bOciEgressPinObserver\b/.test(candidate.source),
    ) ||
    /\.\s*apply\s*\(/.test(readiness)
  ) {
    errors.push(
      "attachment readiness and its adapter wrappers must receive observation-only egress-pin authority",
    );
  }
  if (
    !/\bpub\s*\(\s*crate\s*\)\s+trait\s+MachinePortForwardingProvider\b/.test(
      forwarding,
    )
  ) {
    errors.push("machine-forwarding provider must remain sandbox-private");
  }
  const widenedForwardingProvider = firstMatch(
    sandboxSources,
    /\bpub\s+trait\s+MachinePortForwardingProvider\b/,
  );
  if (widenedForwardingProvider) {
    errors.push(
      `machine-forwarding provider widened: ${widenedForwardingProvider}`,
    );
  }
  const injectedSeal =
    process.env.NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD === "1" &&
    !process.env.NIMBUS_NETWORK_VERIFY_TEST_SEALED_EFFECT_CAPABILITY_PATH
      ? (process.env.NIMBUS_NETWORK_VERIFY_TEST_SEALED_EFFECT_CAPABILITY ?? "")
      : "";
  if (/\bOciEgressPinProvider\b/.test(injectedSeal)) {
    errors.push(
      "attachment readiness mutation fixture acquired apply authority",
    );
  }

  const portableCapability = firstMatch(
    networkSources,
    /\b(?:NetworkProvider|ForwardingProvider|IngressProvider|NameProvider|CertificateProvider|PreparedNetavark|OciNetwork|netns_path|provider_effect|effect_callback)\b/,
  );
  if (portableCapability) {
    errors.push(
      `portable crate acquired provider-effect capability: ${portableCapability}`,
    );
  }
}

function verifySideEffectFreeSandboxInspection() {
  const sandboxSources = walkRust("crates/nimbus-sandbox/src");
  const servicesSources = walkRust("crates/nimbus-services/src");
  const computeSources = walkRust("crates/nimbus-compute/src");
  const machineSources = walkRust("crates/nimbus-machine/src");
  const cliSources = walkRust("crates/nimbus-cli/src");
  const networkSources = walkRust(networkSourceRoot);
  const allSources = [
    ...sandboxSources,
    ...servicesSources,
    ...computeSources,
    ...machineSources,
    ...cliSources,
  ];
  const requiredSource = (sources, file) => {
    const candidate = sources.find((entry) => entry.file === file);
    if (!candidate) {
      errors.push(`required inspection contract source missing: ${file}`);
      return { file, source: "" };
    }
    return candidate;
  };
  const appendTo = (sources, file, text) => {
    const candidate = requiredSource(sources, file);
    candidate.source += `\n${text}\n`;
  };
  const replaceIn = (sources, file, before, after) => {
    const candidate = requiredSource(sources, file);
    if (!candidate.source.includes(before)) {
      errors.push(
        `inspection self-test mutation target missing: ${file}:${before}`,
      );
      return;
    }
    candidate.source = candidate.source.replace(before, after);
  };

  if (process.env.NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD === "1") {
    const mutation =
      process.env.NIMBUS_NETWORK_VERIFY_TEST_INSPECTION_MUTATION ?? "";
    const inspectionFile =
      "crates/nimbus-sandbox/src/backends/container/runtime/inspection.rs";
    const injectedEffect = {
      "inspection-restart":
        "fn injected() { mark_restart_decision_after_exit(); }",
      "inspection-launch": "fn injected() { launch_manifest(); }",
      "inspection-reset": "fn injected() { reset_runtime_for_restart(); }",
      "inspection-release": "fn injected() { release_network_authority(); }",
      "inspection-cleanup": "fn injected() { cleanup_provider_artifacts(); }",
      "inspection-finalize": "fn injected() { finalize_network_release(); }",
      "inspection-pep-start":
        "fn injected() { ensure_egress_proxy_running_with_release_authority(); }",
      "inspection-write":
        "fn injected() { write_existing_workload_manifest(); }",
      "inspection-effect-barrier":
        "fn injected() { persist_effect_barrier(); }",
    }[mutation];
    if (injectedEffect) {
      appendTo(sandboxSources, inspectionFile, injectedEffect);
    } else if (mutation === "creating-lock") {
      replaceIn(
        sandboxSources,
        "crates/nimbus-sandbox/src/backends/container/runtime/runner.rs",
        ".write(true)\n        .open(&lock_path)",
        ".write(true)\n        .create(true)\n        .open(&lock_path)",
      );
    } else if (mutation === "third-inspect-owner") {
      appendTo(
        sandboxSources,
        "crates/nimbus-sandbox/src/backends/mod.rs",
        "fn inspect_sync() {}",
      );
    } else if (mutation === "handle-only-trait") {
      appendTo(
        sandboxSources,
        "crates/nimbus-sandbox/src/backend.rs",
        "fn inspect_handle(&self, id: &SandboxId) -> SandboxFuture<Option<SandboxHandle>>;",
      );
    } else if (mutation === "handle-only-machine-dto") {
      replaceIn(
        machineSources,
        "crates/nimbus-machine/src/api.rs",
        "pub inspection: Option<SandboxInspection>",
        "pub inspection: Option<SandboxHandle>",
      );
    } else if (mutation === "missing-krun-classifier") {
      replaceIn(
        sandboxSources,
        "crates/nimbus-sandbox/src/backends/krun/vm/inspection.rs",
        "let restart = assess_restart(RestartAssessmentInput",
        "let restart = bypassed_restart_classifier(RestartAssessmentInput",
      );
    } else if (mutation === "discarded-service-assessment") {
      appendTo(
        servicesSources,
        "crates/nimbus-services/src/manager/handles.rs",
        "fn discard_assessment(inspection: SandboxInspection) -> SandboxInspection { SandboxInspection::provider_reported(inspection.handle) }",
      );
    } else if (mutation === "cleanup-retained-eviction") {
      replaceIn(
        servicesSources,
        "crates/nimbus-services/src/manager/handles.rs",
        "&& inspection.cleanup == SandboxCleanupObservation::Finalized",
        "",
      );
    } else if (mutation === "fabricated-forwarded-candidate") {
      replaceIn(
        cliSources,
        "crates/nimbus-cli/src/machine/backend.rs",
        "client.inspect_service_sandbox(&sandbox_id)",
        "{ let _ = nimbus_sandbox::SandboxInspection::provider_reported(handle); client.inspect_service_sandbox(&sandbox_id) }",
      );
    } else if (mutation === "implicit-launch-caller") {
      appendTo(
        sandboxSources,
        "crates/nimbus-sandbox/src/backends/container/runtime/status.rs",
        "fn inspect_and_launch(backend: &ContainerSandboxBackend, manifest: &mut ContainerSandboxManifest) { let _ = backend.launch_manifest(manifest, true); }",
      );
    } else if (mutation === "nimbus-network-effect") {
      appendTo(
        networkSources,
        "crates/nimbus-network/src/lib.rs",
        'fn provider_effect() { let _ = std::process::Command::new("netavark"); }',
      );
    } else if (mutation) {
      errors.push(`unknown inspection self-test mutation: ${mutation}`);
    }
  }

  const only = (...files) => new Set(files);
  requireExactOwner(
    sandboxSources,
    "sandbox inspect_sync",
    /\bfn\s+inspect_sync\s*\(/g,
    only(
      "crates/nimbus-sandbox/src/backends/container/runtime/inspection.rs",
      "crates/nimbus-sandbox/src/backends/krun/vm/inspection.rs",
    ),
    2,
  );

  const backendContract = requiredSource(
    sandboxSources,
    "crates/nimbus-sandbox/src/backend.rs",
  ).source;
  if (
    !/\bfn\s+inspect\s*\([^)]*&SandboxId[^)]*\)\s*->\s*SandboxFuture\s*<\s*Option\s*<\s*SandboxInspection\s*>\s*>/.test(
      backendContract,
    ) ||
    /\bfn\s+inspect[A-Za-z0-9_]*\s*\([^)]*&SandboxId[^)]*\)\s*->\s*SandboxFuture\s*<\s*Option\s*<\s*(?:crate::)?SandboxHandle\s*>\s*>/.test(
      backendContract,
    )
  ) {
    errors.push(
      "SandboxBackend::inspect must expose the typed SandboxInspection contract only",
    );
  }

  const inspectionFiles = new Set([
    "crates/nimbus-sandbox/src/backends/container/runtime/inspection.rs",
    "crates/nimbus-sandbox/src/backends/krun/vm/inspection.rs",
  ]);
  const inspectionSources = sandboxSources.filter((candidate) =>
    inspectionFiles.has(candidate.file),
  );
  for (const file of inspectionFiles) {
    requiredSource(sandboxSources, file);
  }
  const forbiddenInspectionEffect =
    /\b(?:maybe_restart_after_exit|mark_restart_decision_after_exit|launch_manifest|reset_runtime(?:_for_restart)?|release_[A-Za-z0-9_]*|cleanup_[A-Za-z0-9_]*|finalize_[A-Za-z0-9_]*|ensure_egress_proxy_running[A-Za-z0-9_]*|start_egress_proxy[A-Za-z0-9_]*|write_manifest|write_existing_workload_manifest|persist_effect_barrier|execute_prepared_container_network_[A-Za-z0-9_]*)\s*\(|\.\s*(?:stop_with_assignment|apply|expose_one|withdraw_one)\s*\(/;
  const inspectionEffect = firstMatch(
    inspectionSources,
    forbiddenInspectionEffect,
  );
  if (inspectionEffect) {
    errors.push(
      `inspection acquired provider-effect authority: ${inspectionEffect}`,
    );
  }

  requireExactOwner(
    sandboxSources,
    "pure restart classifier",
    /\bfn\s+assess_restart\s*\(/g,
    only("crates/nimbus-sandbox/src/backends/inspection.rs"),
    1,
  );
  requireExactOwner(
    sandboxSources,
    "restart classifier consumers",
    /\bassess_restart\s*\(\s*RestartAssessmentInput\b/g,
    inspectionFiles,
    2,
  );
  const classifier = requiredSource(
    sandboxSources,
    "crates/nimbus-sandbox/src/backends/inspection.rs",
  ).source;
  if (
    /\b(?:Instant|SystemTime|OffsetDateTime)\s*::|\b(?:std::)?fs::|\bCommand\s*::\s*new\s*\(|\b(?:maybe_restart_after_exit|mark_restart_decision_after_exit|reset_runtime(?:_for_restart)?|write_manifest|write_existing_workload_manifest|persist_effect_barrier|launch_manifest|cleanup_[A-Za-z0-9_]*|release_[A-Za-z0-9_]*)\s*\(/.test(
      classifier,
    )
  ) {
    errors.push(
      "restart classifier performs clock, filesystem, process, or mutation work",
    );
  }

  const functionBody = (source, functionName) => {
    const name = functionName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const match = source.match(new RegExp(`\\bfn\\s+${name}\\s*\\(`));
    if (!match) return "";
    const start = source.indexOf("{", match.index);
    if (start < 0) return "";
    let depth = 1;
    let cursor = start + 1;
    while (cursor < source.length && depth > 0) {
      if (source.at(cursor) === "{") depth += 1;
      else if (source.at(cursor) === "}") depth -= 1;
      cursor += 1;
    }
    return source.slice(start, cursor);
  };
  for (const [file, functionName] of [
    [
      "crates/nimbus-sandbox/src/backends/container/runtime/runner.rs",
      "lock_current_inspection_with_timeout",
    ],
    [
      "crates/nimbus-sandbox/src/backends/krun/vm/lifecycle.rs",
      "lock_current_inspection_with_timeout",
    ],
  ]) {
    const source = requiredSource(sandboxSources, file).source;
    const body = functionBody(source, functionName);
    if (
      !body ||
      !/\.open\s*\(\s*&lock_path\s*\)/.test(body) ||
      !/\btry_lock_shared\s*\(/.test(body) ||
      !/\b(?:read_manifest|read_runner_manifest)\s*\(/.test(body) ||
      /\.create(?:_new)?\s*\(|\bFile\s*::\s*create(?:_new)?\s*\(|\bcreate_dir(?:_all)?\s*\(/.test(
        body,
      )
    ) {
      errors.push(
        `inspection lock must open existing shared state and reread the manifest: ${file}`,
      );
    }
  }

  const machineApi = requiredSource(
    machineSources,
    "crates/nimbus-machine/src/api.rs",
  ).source;
  if (
    !/\bstruct\s+MachineApiServiceSandboxInspectResponse\s*\{[^}]*\binspection\s*:\s*Option\s*<\s*SandboxInspection\s*>/s.test(
      machineApi,
    )
  ) {
    errors.push(
      "Machine API inspection DTO must carry the complete SandboxInspection",
    );
  }

  const forwardedBackend = requiredSource(
    cliSources,
    "crates/nimbus-cli/src/machine/backend.rs",
  ).source;
  const forwardedClient = requiredSource(
    cliSources,
    "crates/nimbus-cli/src/machine/client.rs",
  ).source;
  const forwardedStub = requiredSource(
    cliSources,
    "crates/nimbus-cli/src/machine/stub/client.rs",
  ).source;
  const forwardedInspect = functionBody(forwardedBackend, "inspect");
  if (
    /\b(?:provider_reported|with_provider_projection(?:_evidence)?)\s*\(/.test(
      forwardedInspect,
    ) ||
    !/\bclient\s*\.\s*inspect_service_sandbox\s*\(/.test(forwardedInspect) ||
    !/\bOk\s*\(\s*response\s*\.\s*inspection\s*\)/.test(forwardedClient) ||
    !/inspection\s*\.\s*handle\s*\.\s*id\s*!=\s*\*sandbox_id/.test(
      forwardedClient,
    ) ||
    !/Result\s*<\s*Option\s*<\s*SandboxInspection\s*>/.test(forwardedStub)
  ) {
    errors.push(
      "forwarded Machine API must preserve typed evidence and authenticate its identity",
    );
  }

  const guestFacade = requiredSource(
    cliSources,
    "crates/nimbus-cli/src/machine/api/service_workloads.rs",
  ).source;
  if (
    !/\bbase\s*\.\s*with_provider_projection_evidence\s*\(/.test(guestFacade) ||
    !/\bMachineApiServiceFuture\s*<\s*'a\s*,\s*Option\s*<\s*SandboxInspection\s*>\s*>/.test(
      guestFacade,
    )
  ) {
    errors.push(
      "guest-node inspection must preserve and extend typed evidence",
    );
  }

  const serviceHandles = requiredSource(
    servicesSources,
    "crates/nimbus-services/src/manager/handles.rs",
  ).source;
  const serviceActivation = requiredSource(
    servicesSources,
    "crates/nimbus-services/src/manager/activation.rs",
  ).source;
  const serviceDefinitions = requiredSource(
    servicesSources,
    "crates/nimbus-services/src/manager/definitions.rs",
  ).source;
  const serviceSandboxes = requiredSource(
    servicesSources,
    "crates/nimbus-services/src/manager/sandboxes.rs",
  ).source;
  const composeLifecycle = requiredSource(
    cliSources,
    "crates/nimbus-cli/src/compose/lifecycle.rs",
  ).source;
  const computeServices = requiredSource(
    computeSources,
    "crates/nimbus-compute/src/services.rs",
  ).source;
  const computeSandboxes = requiredSource(
    computeSources,
    "crates/nimbus-compute/src/sandboxes.rs",
  ).source;
  if (
    !/\brefresh_inspection_async\b/.test(serviceHandles) ||
    !/Result\s*<\s*Option\s*<\s*SandboxInspection\s*>/.test(serviceHandles) ||
    !/\bvalidate_service_inspection_identity\s*\(/.test(serviceHandles) ||
    !/\bpub\s+async\s+fn\s+inspect_service_lifecycle_for_context_async\s*\([^)]*\)\s*->\s*Result\s*<\s*Option\s*<\s*SandboxInspection\s*>\s*,\s*Error\s*>/s.test(
      serviceHandles,
    ) ||
    !/\bpub\s+async\s+fn\s+inspect_sandbox_resource_async\s*\([^)]*\)\s*->\s*Result\s*<\s*Option\s*<\s*\(\s*SandboxResource\s*,\s*(?:nimbus_sandbox::)?SandboxInspection\s*\)\s*>\s*,\s*Error\s*>/s.test(
      serviceSandboxes,
    ) ||
    /\bSandboxInspection\s*::\s*provider_reported\s*\(/.test(serviceHandles) ||
    !/inspection\s*\.\s*cleanup\s*==\s*SandboxCleanupObservation\s*::\s*Finalized/.test(
      serviceHandles,
    ) ||
    !/cleanup_is_final/.test(serviceActivation) ||
    !/inspection\s*\.\s*cleanup\s*!=\s*SandboxCleanupObservation\s*::\s*Finalized/.test(
      serviceDefinitions,
    ) ||
    !/inspection\s*\.\s*cleanup\s*!=\s*SandboxCleanupObservation\s*::\s*Finalized/.test(
      serviceSandboxes,
    ) ||
    !/inspection\s*\.\s*cleanup\s*==\s*nimbus_sandbox\s*::\s*SandboxCleanupObservation\s*::\s*Finalized/.test(
      composeLifecycle,
    ) ||
    !/\binspect_service_lifecycle_for_context_async\s*\(/.test(
      computeServices,
    ) ||
    !/\(resource\s*,\s*_inspection\)/.test(computeSandboxes) ||
    !/\bvalidate_sandbox_inspection_identity\s*\(/.test(serviceSandboxes) ||
    !/\bvalidate_compose_inspection_identity\s*\(/.test(composeLifecycle)
  ) {
    errors.push(
      "services, compute, and Compose must retain typed cleanup-pending inspection evidence",
    );
  }

  const portableEffect = firstMatch(
    networkSources,
    /\b(?:std|tokio)\s*::\s*process\s*::|\bCommand\s*::\s*new\s*\(|\b(?:TcpListener|UdpSocket|UnixListener|TcpSocket|Socket)\s*::\s*bind\s*\(|\b(?:TcpStream|UnixStream)\s*::\s*connect(?:_timeout)?\s*\(/,
  );
  if (portableEffect) {
    errors.push(
      `nimbus-network acquired an inspection/provider effect: ${portableEffect}`,
    );
  }
  const networkWorktreeChanges = execFileSync(
    "git",
    [
      "status",
      "--porcelain",
      "--untracked-files=all",
      "--",
      "crates/nimbus-network",
    ],
    { encoding: "utf8" },
  ).trim();
  if (networkWorktreeChanges) {
    errors.push("NNC5.6 changed the transport-free nimbus-network crate");
  }

  requireExactOwner(
    sandboxSources,
    "launch_manifest callers",
    /\.\s*launch_manifest\s*\(/g,
    only(
      "crates/nimbus-sandbox/src/backends/container/runtime/direct_execution.rs",
      "crates/nimbus-sandbox/src/backends/krun/vm/lifecycle.rs",
    ),
    2,
  );
  const containerLaunchBody = functionBody(
    requiredSource(
      sandboxSources,
      "crates/nimbus-sandbox/src/backends/container/runtime/direct_execution.rs",
    ).source,
    "execute_start_after_preflight_with_cleanup",
  );
  const krunLaunchBody = functionBody(
    requiredSource(
      sandboxSources,
      "crates/nimbus-sandbox/src/backends/krun/vm/lifecycle.rs",
    ).source,
    "execute_start_after_preflight",
  );
  if (
    [...containerLaunchBody.matchAll(/\.\s*launch_manifest\s*\(/g)].length !==
      1 ||
    [...krunLaunchBody.matchAll(/\.\s*launch_manifest\s*\(/g)].length !== 1
  ) {
    errors.push(
      "launch_manifest calls must remain inside the two explicit start command bodies",
    );
  }

  const staleHandleOnly = firstMatch(
    allSources,
    /\bfn\s+inspect[A-Za-z0-9_]*(?:\s*<'[^>]+>)?\s*\([^)]*&SandboxId[^)]*\)\s*->\s*(?:SandboxFuture|MachineApiServiceFuture\s*<\s*'[^,>]+\s*,)\s*<?\s*Option\s*<\s*(?:crate::)?SandboxHandle\s*>/,
  );
  if (staleHandleOnly) {
    errors.push(
      `handle-only sandbox inspection seam remains: ${staleHandleOnly}`,
    );
  }
}

function verifyComputeNetworkManagerInjection() {
  const computeSources = walkRust("crates/nimbus-compute/src");
  const serverSources = walkRust("crates/nimbus-server/src");
  const cliSources = walkRust("crates/nimbus-cli/src");
  let computeManifest = fs.existsSync("crates/nimbus-compute/Cargo.toml")
    ? fs.readFileSync("crates/nimbus-compute/Cargo.toml", "utf8")
    : "";
  const requiredSource = (sources, file) => {
    const candidate = sources.find((entry) => entry.file === file);
    if (!candidate) {
      errors.push(`required compute-manager source missing: ${file}`);
      return { file, source: "" };
    }
    return candidate;
  };
  const replaceIn = (sources, file, before, after) => {
    const candidate = requiredSource(sources, file);
    if (!candidate.source.includes(before)) {
      errors.push(
        `compute-manager self-test mutation target missing: ${file}:${before}`,
      );
      return;
    }
    candidate.source = candidate.source.replace(before, after);
  };
  if (process.env.NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD === "1") {
    const mutation =
      process.env.NIMBUS_NETWORK_VERIFY_TEST_COMPUTE_MANAGER_MUTATION ?? "";
    if (mutation === "missing-compute-dependency") {
      computeManifest = computeManifest.replace(
        'nimbus-network = { path = "../nimbus-network" }',
        "",
      );
    } else if (mutation === "missing-config-manager") {
      replaceIn(
        computeSources,
        "crates/nimbus-compute/src/state.rs",
        "pub network_manager: Option<Arc<LocalNetworkManager>>",
        "pub omitted_network_manager: ()",
      );
    } else if (mutation === "missing-state-manager") {
      replaceIn(
        computeSources,
        "crates/nimbus-compute/src/state.rs",
        "pub active_deployment: Arc<ActiveDeployment>,\n    network_manager: Option<Arc<LocalNetworkManager>>",
        "pub active_deployment: Arc<ActiveDeployment>,\n    omitted_network_manager: ()",
      );
    } else if (mutation === "missing-compute-accessor") {
      replaceIn(
        computeSources,
        "crates/nimbus-compute/src/state.rs",
        "pub fn network_manager(&self)",
        "pub fn omitted_network_manager(&self)",
      );
    } else if (mutation === "missing-compute-profile-fence") {
      replaceIn(
        computeSources,
        "crates/nimbus-compute/src/state.rs",
        "Self::require_network_manager_for_workloads(&network_manager, &node_services);",
        "",
      );
    } else if (mutation === "copied-capability-registry") {
      const state = requiredSource(
        computeSources,
        "crates/nimbus-compute/src/state.rs",
      );
      state.source +=
        "\nfn copied_registry(manager: &LocalNetworkManager) { let _ = manager.capability_registry().clone(); }\n";
    } else if (mutation === "hidden-prepared-manager") {
      replaceIn(
        cliSources,
        "crates/nimbus-cli/src/network_composition.rs",
        "pub(crate) fn manager(&self) -> Arc<LocalNetworkManager>",
        "pub(crate) fn hidden_manager(&self) -> Arc<LocalNetworkManager>",
      );
    } else if (mutation === "authority-only-start") {
      replaceIn(
        cliSources,
        "crates/nimbus-cli/src/start/boot.rs",
        "ServeOptions::new(engine.clone(), prepared_network.manager())",
        "ServeOptions::new(engine.clone(), prepared_network.authority())",
      );
    } else if (mutation === "authority-only-serve") {
      replaceIn(
        serverSources,
        "crates/nimbus-server/src/construction.rs",
        "network_manager: Arc<LocalNetworkManager>",
        "network_authority: LocalNetworkAuthority",
      );
    } else if (mutation === "manager-less-router") {
      replaceIn(
        serverSources,
        "crates/nimbus-server/src/router.rs",
        "pub fn new(engine: Arc<Engine>, network_manager: Arc<LocalNetworkManager>)",
        "pub fn new(engine: Arc<Engine>)",
      );
    } else if (mutation === "missing-router-build-handoff") {
      replaceIn(
        serverSources,
        "crates/nimbus-server/src/router.rs",
        "network_manager,\n            deployment: DeploymentConfig {",
        "network_manager: None,\n            deployment: DeploymentConfig {",
      );
    } else if (mutation === "protocol-service-bypass") {
      replaceIn(
        serverSources,
        "crates/nimbus-server/src/router.rs",
        "self.require_network_manager(",
        "bypassed_network_manager_guard(",
      );
    } else if (mutation === "protocol-machine-bypass") {
      const router = requiredSource(
        serverSources,
        "crates/nimbus-server/src/router.rs",
      );
      const target = "self.require_network_manager(";
      const index = router.source.lastIndexOf(target);
      if (index < 0) {
        errors.push(
          "compute-manager self-test mutation target missing: machine network-manager guard",
        );
      } else {
        router.source =
          router.source.slice(0, index) +
          "bypassed_network_manager_guard(" +
          router.source.slice(index + target.length);
      }
    } else if (mutation === "parallel-compute-manager") {
      const state = requiredSource(
        computeSources,
        "crates/nimbus-compute/src/state.rs",
      );
      state.source +=
        '\nfn parallel_manager() { let _ = LocalNetworkManager::bootstrap("parallel"); }\n';
    } else if (mutation === "parallel-server-manager") {
      const construction = requiredSource(
        serverSources,
        "crates/nimbus-server/src/construction.rs",
      );
      construction.source +=
        '\nfn parallel_manager() { let _ = LocalNetworkManager::open("parallel", todo!()); }\n';
    } else if (mutation) {
      errors.push(`unknown compute-manager self-test mutation: ${mutation}`);
    }
  }

  if (
    !/^nimbus-network\s*=\s*\{\s*path\s*=\s*"\.\.\/nimbus-network"\s*\}\s*$/m.test(
      computeManifest,
    )
  ) {
    errors.push("nimbus-compute must depend directly on nimbus-network");
  }

  const computeState = requiredSource(
    computeSources,
    "crates/nimbus-compute/src/state.rs",
  ).source;
  const managerFields = [
    ...computeState.matchAll(
      /\bnetwork_manager\s*:\s*Option\s*<\s*Arc\s*<\s*LocalNetworkManager\s*>\s*>/g,
    ),
  ];
  if (managerFields.length < 2) {
    errors.push(
      "ComputeStateConfig and ComputeState must both retain the injected manager",
    );
  }
  if (
    !/\bpub\s+fn\s+network_manager\s*\(\s*&self\s*\)\s*->\s*Option\s*<\s*Arc\s*<\s*LocalNetworkManager\s*>\s*>\s*\{[^}]*self\.network_manager\.clone\(\)/s.test(
      computeState,
    )
  ) {
    errors.push(
      "ComputeState must return the retained manager Arc without reconstruction",
    );
  }
  if (/capability_registry\s*\(\s*\)\s*\.\s*clone\s*\(/.test(computeState)) {
    errors.push("ComputeState must not copy the manager capability registry");
  }
  if (
    !/Self\s*::\s*require_network_manager_for_workloads\s*\(\s*&network_manager\s*,\s*&node_services\s*\)/s.test(
      computeState,
    ) ||
    !/fn\s+require_network_manager_for_workloads[\s\S]*?network_manager\.is_some\(\)[\s\S]*?service_manager\(\)\.is_none\(\)[\s\S]*?machine_lifecycle_manager\(\)\.is_none\(\)/.test(
      computeState,
    )
  ) {
    errors.push(
      "ComputeState must reject workload lifecycle without the shared manager",
    );
  }

  const cliComposition = requiredSource(
    cliSources,
    "crates/nimbus-cli/src/network_composition.rs",
  ).source;
  const managerAccessors = [
    ...cliComposition.matchAll(
      /\bpub\s*\(\s*crate\s*\)\s+fn\s+manager\s*\(\s*&self\s*\)\s*->\s*Arc\s*<\s*LocalNetworkManager\s*>/g,
    ),
  ];
  if (managerAccessors.length < 2) {
    errors.push(
      "frozen and prepared CLI composition must expose the retained manager Arc",
    );
  }
  const startBoot = requiredSource(
    cliSources,
    "crates/nimbus-cli/src/start/boot.rs",
  ).source;
  if (
    !/ServeOptions\s*::\s*new\s*\(\s*engine\.clone\(\)\s*,\s*prepared_network\.manager\(\)\s*\)/s.test(
      startBoot,
    ) ||
    /ServeOptions\s*::\s*new\s*\([^;]*prepared_network\.authority\(\)/s.test(
      startBoot,
    )
  ) {
    errors.push(
      "CLI start must inject the prepared manager rather than an authority-only view",
    );
  }

  const serverConstruction = requiredSource(
    serverSources,
    "crates/nimbus-server/src/construction.rs",
  ).source;
  if (
    !/\bpub\s+fn\s+new\s*\(\s*engine\s*:\s*Arc\s*<\s*Engine\s*>\s*,\s*network_manager\s*:\s*Arc\s*<\s*LocalNetworkManager\s*>\s*\)/s.test(
      serverConstruction,
    ) ||
    !/ServerListenerLeaseAuthority\s*::\s*new\s*\(\s*network_manager\.authority\(\)\s*\)/s.test(
      serverConstruction,
    ) ||
    !/RouterOptions\s*::\s*new\s*\(\s*engine\s*,\s*network_manager\s*\)/s.test(
      serverConstruction,
    )
  ) {
    errors.push(
      "ServeOptions must derive listeners and compute from one manager Arc",
    );
  }

  const serverRouter = requiredSource(
    serverSources,
    "crates/nimbus-server/src/router.rs",
  ).source;
  if (
    !/\bnetwork_manager\s*:\s*Option\s*<\s*Arc\s*<\s*LocalNetworkManager\s*>\s*>/.test(
      serverRouter,
    ) ||
    !/\bpub\s+fn\s+new\s*\(\s*engine\s*:\s*Arc\s*<\s*Engine\s*>\s*,\s*network_manager\s*:\s*Arc\s*<\s*LocalNetworkManager\s*>\s*\)/s.test(
      serverRouter,
    ) ||
    !/\bpub\s+fn\s+protocol_only\s*\(\s*engine\s*:\s*Arc\s*<\s*Engine\s*>\s*\)/s.test(
      serverRouter,
    ) ||
    [...serverRouter.matchAll(/self\.require_network_manager\s*\(/g)].length !==
      2 ||
    !/fn\s+require_network_manager\s*\([^)]*\)\s*\{[^}]*network_manager\.is_some\(\)/s.test(
      serverRouter,
    ) ||
    !/fn\s+into_state[\s\S]*?AppState\s*::\s*from_config\s*\(\s*AppStateConfig\s*\{[\s\S]*?\bnetwork_manager\s*,/.test(
      serverRouter,
    ) ||
    !/pub\s*\(\s*crate\s*\)\s+fn\s+build\s*\(\s*self\s*\)[\s\S]*?self\.into_state\(\)/.test(
      serverRouter,
    )
  ) {
    errors.push(
      "managed and protocol-only RouterOptions must be explicit and workload-fenced",
    );
  }

  const serverState = requiredSource(
    serverSources,
    "crates/nimbus-server/src/state.rs",
  ).source;
  if (
    !/\bnetwork_manager\s*:\s*Option\s*<\s*Arc\s*<\s*LocalNetworkManager\s*>\s*>/.test(
      serverState,
    ) ||
    !/ComputeStateConfig\s*\{[^}]*\bnetwork_manager\s*,/s.test(serverState)
  ) {
    errors.push(
      "AppState must preserve the exact manager into ComputeStateConfig",
    );
  }

  const forbiddenConstruction = firstMatch(
    [...computeSources, ...serverSources],
    /\bLocalNetworkManager\s*::\s*(?:open|bootstrap)\s*\(/,
  );
  if (forbiddenConstruction) {
    errors.push(
      `compute/server constructed a parallel manager: ${forbiddenConstruction}`,
    );
  }
}

function verifyComputeNodeWorkloadCoordinator() {
  const productionSources = walkRust("crates").filter(
    (entry) =>
      entry.file.includes("/src/") &&
      !entry.file.endsWith("/tests.rs") &&
      !entry.file.includes("/tests/"),
  );
  const sourcesUnder = (prefix) =>
    productionSources.filter((entry) => entry.file.startsWith(prefix));
  const nodeSources = sourcesUnder("crates/nimbus-node/src/");
  const computeSources = sourcesUnder("crates/nimbus-compute/src/");
  const cliSources = sourcesUnder("crates/nimbus-cli/src/");
  const systemSources = sourcesUnder("crates/nimbus-system/src/");
  const requiredSource = (sources, file) => {
    const candidate = sources.find((entry) => entry.file === file);
    if (!candidate) {
      errors.push(`required compute-node-coordinator source missing: ${file}`);
      return { file, source: "" };
    }
    return candidate;
  };
  const replaceIn = (sources, file, before, after) => {
    const candidate = requiredSource(sources, file);
    if (!candidate.source.includes(before)) {
      errors.push(
        `compute-node-coordinator self-test mutation target missing: ${file}:${before}`,
      );
      return;
    }
    candidate.source = candidate.source.replace(before, after);
  };
  const functionBody = (source, functionName) => {
    const escaped = functionName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const match = source.match(new RegExp(`\\bfn\\s+${escaped}\\s*\\(`));
    if (!match) return "";
    const start = source.indexOf("{", match.index);
    if (start < 0) return "";
    let depth = 1;
    let cursor = start + 1;
    while (cursor < source.length && depth > 0) {
      if (source.at(cursor) === "{") depth += 1;
      else if (source.at(cursor) === "}") depth -= 1;
      cursor += 1;
    }
    return source.slice(start, cursor);
  };
  if (process.env.NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD === "1") {
    const mutation =
      process.env.NIMBUS_NETWORK_VERIFY_TEST_COMPUTE_COORDINATOR_MUTATION ?? "";
    if (mutation === "missing-node-capability") {
      replaceIn(
        nodeSources,
        "crates/nimbus-node/src/reconciler.rs",
        "pub trait NodeWorkloadReconcileCapability",
        "pub trait OmittedNodeWorkloadReconcileCapability",
      );
    } else if (mutation === "missing-compute-coordinator") {
      replaceIn(
        computeSources,
        "crates/nimbus-compute/src/node_workloads.rs",
        "pub struct NodeWorkloadCoordinator",
        "pub struct OmittedNodeWorkloadCoordinator",
      );
    } else if (mutation === "missing-state-coordinator") {
      replaceIn(
        computeSources,
        "crates/nimbus-compute/src/config/node_services.rs",
        "node_workload_coordinator: Option<Arc<NodeWorkloadCoordinator>>",
        "omitted_node_workload_coordinator: ()",
      );
    } else if (mutation === "missing-profile-fence") {
      replaceIn(
        computeSources,
        "crates/nimbus-compute/src/state.rs",
        "node_workload_coordinator().is_none()",
        "bypassed_node_workload_coordinator_fence()",
      );
    } else if (mutation === "direct-cli-reconcile") {
      requiredSource(
        cliSources,
        "crates/nimbus-cli/src/node_workload_executor.rs",
      ).source += "\nfn bypass(node_agent: NodeAgent<(), ()>) { node_agent.reconcile_assignment(todo!()); }\n";
    } else if (mutation === "direct-guest-reconcile") {
      requiredSource(
        cliSources,
        "crates/nimbus-cli/src/machine/api/service_workloads.rs",
      ).source += "\nfn bypass(node_agent: NodeAgent<(), ()>) { node_agent.reconcile_assignments([]); }\n";
    } else if (mutation === "direct-guest-inspect") {
      requiredSource(
        cliSources,
        "crates/nimbus-cli/src/machine/api/service_workloads.rs",
      ).source += "\nfn bypass(node_agent: NodeAgent<(), ()>) { node_agent.reconciler().backend().inspect(todo!()); }\n";
    } else if (mutation === "runner-provider-restart") {
      replaceIn(
        nodeSources,
        "crates/nimbus-node/src/host_lifecycle.rs",
        "HostLifecycleProperty::Restart(HostRestartPolicy::No)",
        "HostLifecycleProperty::Restart(HostRestartPolicy::OnFailure)",
      );
    } else if (mutation === "missing-restart-fence") {
      replaceIn(
        nodeSources,
        "crates/nimbus-node/src/reconciler.rs",
        "request.ensure_external_restart_disabled()?;",
        "",
      );
    } else if (mutation === "duplicate-restart-accepted") {
      replaceIn(
        nodeSources,
        "crates/nimbus-node/src/host_lifecycle.rs",
        "restart_properties.len() <= 1",
        "true",
      );
    } else if (mutation === "coordinator-desired-store") {
      requiredSource(
        computeSources,
        "crates/nimbus-compute/src/node_workloads.rs",
      ).source += "\nuse nimbus_workloads::DesiredWorkloadStore;\n";
    } else if (mutation === "coordinator-network-authority") {
      requiredSource(
        computeSources,
        "crates/nimbus-compute/src/node_workloads.rs",
      ).source += "\nuse nimbus_network::LocalNetworkManager;\n";
    } else if (mutation === "second-coordinator") {
      requiredSource(
        computeSources,
        "crates/nimbus-compute/src/node_workloads.rs",
      ).source += "\nstruct AnotherNodeWorkloadCoordinator;\n";
    } else if (mutation === "duplicate-saga-coordinator") {
      requiredSource(
        productionSources,
        "crates/nimbus-server/src/lib.rs",
      ).source += "\npub struct WorkloadSagaCoordinator;\n";
    } else if (mutation === "duplicate-saga-coordinator-enum") {
      requiredSource(
        productionSources,
        "crates/nimbus-server/src/lib.rs",
      ).source += "\npub enum WorkloadSagaCoordinator { Duplicate }\n";
    } else if (mutation) {
      errors.push(`unknown compute-node-coordinator self-test mutation: ${mutation}`);
    }
  }

  const nodeReconciler = requiredSource(
    nodeSources,
    "crates/nimbus-node/src/reconciler.rs",
  ).source;
  if (
    !/pub\s+trait\s+NodeWorkloadReconcileCapability\s*:\s*Send\s*\+\s*Sync/.test(
      nodeReconciler,
    ) ||
    !/impl\s*<[^>]*>\s+NodeWorkloadReconcileCapability\s+for\s+NodeAgent/.test(
      nodeReconciler,
    ) ||
    !/fn\s+reconcile_assignment\s*<'a>/.test(nodeReconciler) ||
    !/fn\s+reconcile_assignments\s*<'a>/.test(nodeReconciler) ||
    !/fn\s+inspect_assignment\s*<'a>/.test(nodeReconciler)
  ) {
    errors.push(
      "nimbus-node must expose one object-safe reconcile/inspect capability implemented by NodeAgent",
    );
  }

  const computeCoordinator = requiredSource(
    computeSources,
    "crates/nimbus-compute/src/node_workloads.rs",
  ).source;
  if (
    !/pub\s+struct\s+NodeWorkloadCoordinator\s*\{[^}]*Arc\s*<\s*dyn\s+NodeWorkloadReconcileCapability\s*>/s.test(
      computeCoordinator,
    ) ||
    !/pub\s+async\s+fn\s+reconcile_assignment[\s\S]*?\.reconcile_assignment\s*\(/.test(
      computeCoordinator,
    ) ||
    !/pub\s+async\s+fn\s+reconcile_assignments[\s\S]*?\.reconcile_assignments\s*\(/.test(
      computeCoordinator,
    ) ||
    !/pub\s+async\s+fn\s+inspect_assignment[\s\S]*?\.inspect_assignment\s*\(/.test(
      computeCoordinator,
    )
  ) {
    errors.push(
      "nimbus-compute must own one concrete coordinator over the node capability",
    );
  }
  if (
    /\bnimbus_workloads\b|\bDesiredWorkload(?:Store|State)?\b|\bnimbus_network\b|\bLocalNetworkManager\b|\bNetworkPlan\b|\bnimbus_system\b|\bSystemTenantStatusEvidenceWriter\b/.test(
      computeCoordinator,
    )
  ) {
    errors.push(
      "compute node coordinator acquired desired-state, network, or projection authority",
    );
  }
  const coordinatorDefinitions = firstMatch(
    [...computeSources, ...cliSources],
    /\bstruct\s+(?!NodeWorkloadCoordinator\b)(?!WorkloadSagaCoordinator\b)[A-Za-z0-9_]*(?:NodeWorkload|Saga|Reconcile)[A-Za-z0-9_]*Coordinator\b/,
  );
  if (coordinatorDefinitions) {
    errors.push(`second production workload coordinator exists: ${coordinatorDefinitions}`);
  }
  const sagaCoordinatorOwners = productionSources.flatMap((entry) =>
    [
      ...entry.source.matchAll(
        /\b(?:pub(?:\s*\([^)]*\))?\s+)?(?:struct|enum|union|trait|type)\s+WorkloadSagaCoordinator\b/g,
      ),
    ].map(() => entry.file),
  );
  if (
    sagaCoordinatorOwners.length !== 1 ||
    sagaCoordinatorOwners[0] !== "crates/nimbus-compute/src/workload_saga.rs"
  ) {
    errors.push(
      `exactly one WorkloadSagaCoordinator must exist in its compute owner: ${sagaCoordinatorOwners.join(",") || "none"}`,
    );
  }

  const nodeServices = requiredSource(
    computeSources,
    "crates/nimbus-compute/src/config/node_services.rs",
  ).source;
  const computeState = requiredSource(
    computeSources,
    "crates/nimbus-compute/src/state.rs",
  ).source;
  if (
    !/node_workload_coordinator\s*:\s*Option\s*<\s*Arc\s*<\s*NodeWorkloadCoordinator\s*>\s*>/.test(
      nodeServices,
    ) ||
    !/pub\s+fn\s+node_workload_coordinator\s*\(\s*&self\s*\)\s*->\s*Option\s*<\s*Arc\s*<\s*NodeWorkloadCoordinator\s*>\s*>/.test(
      computeState,
    ) ||
    !/fn\s+require_network_manager_for_workloads[\s\S]*?node_workload_coordinator\(\)\.is_none\(\)/.test(
      computeState,
    )
  ) {
    errors.push(
      "ComputeState must retain the optional coordinator and fence it from protocol-only profiles",
    );
  }

  const nodeExecutor = requiredSource(
    cliSources,
    "crates/nimbus-cli/src/node_workload_executor.rs",
  ).source;
  const guestService = requiredSource(
    cliSources,
    "crates/nimbus-cli/src/machine/api/service_workloads.rs",
  ).source;
  if (
    !/NodeWorkloadCoordinator\s*::\s*new\s*\(/.test(nodeExecutor) ||
    !/\.reconcile_assignment\s*\(/.test(nodeExecutor) ||
    /\bNodeWorkloadReconciler\b|node_agent\s*\.\s*reconcile_assignments?\s*\(/.test(
      nodeExecutor,
    )
  ) {
    errors.push("standalone node executor must route reconciliation through compute");
  }
  if (
    !/NodeWorkloadCoordinator/.test(guestService) ||
    !/\.reconcile_assignments\s*\(/.test(guestService) ||
    !/\.inspect_assignment\s*\(/.test(guestService) ||
    /node_agent\s*\.\s*reconcile_assignments?\s*\(|\.reconciler\s*\(\s*\)\s*\.\s*backend\s*\(\s*\)/.test(
      guestService,
    )
  ) {
    errors.push("guest Machine API must route reconcile and inspect through compute");
  }

  const hostLifecycle = requiredSource(
    nodeSources,
    "crates/nimbus-node/src/host_lifecycle.rs",
  ).source;
  const runnerBody = functionBody(hostLifecycle, "into_host_lifecycle_request");
  const restartFenceBody = functionBody(
    hostLifecycle,
    "ensure_external_restart_disabled",
  );
  const reconcileBody = functionBody(nodeReconciler, "reconcile_binding");
  if (
    !/HostLifecycleProperty\s*::\s*Restart\s*\(\s*HostRestartPolicy\s*::\s*No\s*\)/.test(
      runnerBody,
    ) ||
    /HostRestartPolicy\s*::\s*(?:OnFailure|Always)/.test(runnerBody)
  ) {
    errors.push("RunnerSpec must disable provider-owned tenant-workload restart");
  }
  if (
    !/restart_properties\.len\(\)\s*<=\s*1/.test(restartFenceBody) ||
    !/HostRestartPolicy\s*::\s*No/.test(restartFenceBody) ||
    !/request\.ensure_external_restart_disabled\(\)\?;[\s\S]*?backend\.validate/.test(
      reconcileBody,
    )
  ) {
    errors.push(
      "node reconciliation must reject provider restart and duplicates before backend validation",
    );
  }

  const engineStatusWriters = [
    ...systemSources
      .map((entry) => entry.source)
      .join("\n")
      .matchAll(
        /impl\s+StatusEvidenceWriter\s+for\s+SystemTenantStatusEvidenceWriter/g,
      ),
  ];
  if (engineStatusWriters.length !== 1) {
    errors.push(
      `expected one system observed-status writer, found ${engineStatusWriters.length}`,
    );
  }
}

if (mode === "forbidden-dependencies-effects") {
  verifyForbiddenDependenciesAndEffects();
} else if (mode === "single-definition-owner") {
  verifySingleDefinitionOwner();
} else if (mode === "address-is-not-identity") {
  verifyAddressIsNotIdentity();
} else if (mode === "sandbox-effect-locality") {
  verifySandboxEffectLocality();
} else if (mode === "side-effect-free-sandbox-inspection") {
  verifySideEffectFreeSandboxInspection();
} else if (mode === "compute-network-manager-injection") {
  verifyComputeNetworkManagerInjection();
} else if (mode === "compute-node-workload-coordinator") {
  verifyComputeNodeWorkloadCoordinator();
} else {
  verifySealedEffectCapabilities();
}

if (errors.length) {
  process.stdout.write(errors.join("\n"));
  process.exit(1);
}
