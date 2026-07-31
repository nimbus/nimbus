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
]);
if (!validModes.has(mode)) {
  process.stderr.write(
    "usage: verify-nimbus-network-source-contract.mjs " +
      "[forbidden-dependencies-effects|single-definition-owner|address-is-not-identity|" +
      "sandbox-effect-locality|sealed-effect-capabilities]\n",
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

function requireExactOwner(sources, label, pattern, allowedFiles, expectedCount) {
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
    const source = sandboxSources.find((candidate) => candidate.file === file)?.source;
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
    [...netavark.matchAll(/\bpub\s*\(\s*super\s*\)\s+struct\s+PreparedNetavark(?:Setup|Teardown)\b/g)]
      .length !== 2
  ) {
    errors.push("prepared Netavark capabilities must remain network-private");
  }
  if (
    [...netns.matchAll(/\bpub\s*\(\s*super\s*\)\s+fn\s+(?:create|remove)_persistent_network_namespace\b/g)]
      .length !== 2
  ) {
    errors.push("namespace effects must remain network-private");
  }
  if (/\bpub[^{;\n]*\buse\s+netns\b/.test(networkRoot)) {
    errors.push("namespace effects must not be reexported from the network root");
  }
  const widenedSandboxCapability = firstMatch(
    sandboxSources,
    /\bpub\s*\(\s*crate\s*\)\s+(?:trait\s+AttachmentHostEffects|struct\s+PreparedNetavark(?:Setup|Teardown)|fn\s+(?:create|remove)_persistent_network_namespace)\b|\bpub[^{;\n]*\buse\s+netns\b/,
  );
  if (widenedSandboxCapability) {
    errors.push(`privileged sandbox capability widened: ${widenedSandboxCapability}`);
  }
  if (
    !/\btrait\s+OciEgressPinObserver\b/.test(egressPin) ||
    !/\btrait\s+OciEgressPinProvider\s*:\s*OciEgressPinObserver\b/.test(
      egressPin,
    )
  ) {
    errors.push("egress-pin observation and mutation capabilities are not separated");
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
      ? process.env.NIMBUS_NETWORK_VERIFY_TEST_SEALED_EFFECT_CAPABILITY ?? ""
      : "";
  if (/\bOciEgressPinProvider\b/.test(injectedSeal)) {
    errors.push("attachment readiness mutation fixture acquired apply authority");
  }

  const portableCapability = firstMatch(
    networkSources,
    /\b(?:NetworkProvider|ForwardingProvider|IngressProvider|NameProvider|CertificateProvider|PreparedNetavark|OciNetwork|netns_path|provider_effect|effect_callback)\b/,
  );
  if (portableCapability) {
    errors.push(`portable crate acquired provider-effect capability: ${portableCapability}`);
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
} else {
  verifySealedEffectCapabilities();
}

if (errors.length) {
  process.stdout.write(errors.join("\n"));
  process.exit(1);
}
