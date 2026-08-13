import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { withoutCfgTestItems, walkRust } from "./source-contract-scanner.mjs";

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const repositoryRoot = path.resolve(scriptDirectory, "../..");

const supportedCommandLabels = [
  "AUTH",
  "HELLO",
  "QUIT",
  "PING",
  "ECHO",
  "COMMAND",
  "CLIENT",
  "SELECT",
  "GET",
  "SET",
  "DEL",
  "FLUSHALL",
  "FUNCTION",
  "EXPIRE",
  "TTL",
  "INCR",
  "NIMBUS.READY",
  "NIMBUS.METRICS",
];

function read(relativePath) {
  return fs.readFileSync(path.join(repositoryRoot, relativePath), "utf8");
}

function productionRust(crateName) {
  return walkRust(path.join(repositoryRoot, "crates", crateName, "src"))
    .map(({ source }) => source)
    .join("\n");
}

function beforeUnitTestModule(source) {
  const marker = source.search(
    /#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]\s*mod\s+tests\b/u,
  );
  return marker < 0 ? source : source.slice(0, marker);
}

function cargoDependencies() {
  const metadata = JSON.parse(
    execFileSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], {
      cwd: repositoryRoot,
      encoding: "utf8",
    }),
  );
  const workspacePackages = new Set(metadata.packages.map(({ name }) => name));
  const packageDependencies = (name) => {
    const packageMetadata = metadata.packages.find(
      (candidate) => candidate.name === name,
    );
    if (!packageMetadata) {
      throw new Error(`cargo metadata has no ${name} package`);
    }
    return new Set(
      packageMetadata.dependencies.map((dependency) => dependency.name),
    );
  };
  const networkDependencies = packageDependencies("nimbus-network");
  return {
    networkDependencies,
    networkWorkspaceDependencies: new Set(
      [...networkDependencies].filter((name) => workspacePackages.has(name)),
    ),
    serverDependencies: packageDependencies("nimbus-server"),
    proxyDependencies: packageDependencies("nimbus-proxy"),
    sandboxDependencies: packageDependencies("nimbus-sandbox"),
  };
}

function rustFunctionRanges(source) {
  const ranges = [];
  const functions = /\bfn\s+([A-Za-z_][A-Za-z0-9_]*)\b/gmu;
  let match;
  while ((match = functions.exec(source)) !== null) {
    const bodyStart = source.indexOf("{", functions.lastIndex);
    const declarationEnd = source.indexOf(";", functions.lastIndex);
    if (bodyStart < 0 || (declarationEnd >= 0 && declarationEnd < bodyStart)) {
      continue;
    }
    let depth = 1;
    let cursor = bodyStart + 1;
    while (cursor < source.length && depth > 0) {
      if (source[cursor] === "{") depth += 1;
      else if (source[cursor] === "}") depth -= 1;
      cursor += 1;
    }
    if (depth === 0) {
      ranges.push({ name: match[1], start: match.index, end: cursor });
      functions.lastIndex = cursor;
    }
  }
  return ranges;
}

function loadLiveFixture() {
  return {
    ...cargoDependencies(),
    network: productionRust("nimbus-network"),
    server: productionRust("nimbus-server"),
    proxy: productionRust("nimbus-proxy"),
    sandbox: productionRust("nimbus-sandbox"),
    kvMetrics: beforeUnitTestModule(read("crates/nimbus-kv/src/metrics.rs")),
    kvServer: beforeUnitTestModule(read("crates/nimbus-kv/src/server.rs")),
    proxyFairness: withoutCfgTestItems(
      read("crates/nimbus-proxy/src/fairness.rs"),
    ),
    serverLatency: withoutCfgTestItems(
      read("crates/nimbus-server/src/latency.rs"),
    ),
  };
}

function check(fixture) {
  const errors = [];
  const reject = (condition, id, message) => {
    if (condition) errors.push({ id, message });
  };

  const networkEdges = fixture.networkWorkspaceDependencies;
  reject(
    networkEdges.size !== 1 || !networkEdges.has("nimbus-core"),
    "network-workspace-edge",
    `nimbus-network workspace edges must be exactly nimbus-core; found ${[
      ...networkEdges,
    ].join(", ")}`,
  );
  const forbiddenNetworkTransportDependencies = [
    ...fixture.networkDependencies,
  ].filter((name) =>
    /^(?:rustls|tokio-rustls|rcgen|axum|pingora(?:-|$))/u.test(name),
  );
  const forbiddenNetworkMetricDependencies = [
    ...fixture.networkDependencies,
  ].filter((name) =>
    /^(?:metrics|prometheus|opentelemetry)(?:[-_]|$)/u.test(name),
  );
  reject(
    forbiddenNetworkTransportDependencies.length > 0,
    "network-tls-dependency",
    `nimbus-network must not depend on TLS or transport implementations; found ${forbiddenNetworkTransportDependencies.join(", ")}`,
  );
  reject(
    forbiddenNetworkMetricDependencies.length > 0,
    "network-metric-dependency",
    `nimbus-network must not depend on metric implementations; found ${forbiddenNetworkMetricDependencies.join(", ")}`,
  );
  reject(
    /\b(?:struct|enum|trait|type)\s+(?:CertificateProvider|TlsConfig|WorkloadPepTlsAuthority)\b/u.test(
      fixture.network,
    ) || /\b(?:TlsAcceptor|PrivateKeyDer|KeyPair)\b/u.test(fixture.network),
    "network-certificate-owner",
    "nimbus-network must not own certificates, private keys, or TLS effects",
  );
  reject(
    /\b(?:metrics|prometheus|opentelemetry)(?:_[A-Za-z0-9_]+)?(?:::|!)|\b(?:counter|gauge|histogram|describe_counter|describe_gauge|describe_histogram)!\s*\(/u.test(
      fixture.network,
    ),
    "network-metric-exporter",
    "nimbus-network must not own a metric exporter",
  );

  reject(
    fixture.serverDependencies.has("nimbus-proxy") ||
      /\b(?:WorkloadPepTlsAuthority|trust_anchor_pem)\b/u.test(fixture.server),
    "server-proxy-authority-crossing",
    "nimbus-server must not depend on or reference the PEP interception authority",
  );
  reject(
    fixture.proxyDependencies.has("nimbus-server") ||
      /\bTlsConfig\b/u.test(fixture.proxy),
    "proxy-server-authority-crossing",
    "nimbus-proxy must not depend on or reference the ingress TLS configuration",
  );
  reject(
    fixture.sandboxDependencies.has("nimbus-server"),
    "sandbox-server-authority-crossing",
    "nimbus-sandbox must not depend on the ingress TLS owner",
  );
  reject(
    !/\bpub\s+struct\s+TlsConfig\b/u.test(fixture.server) ||
      !/\bcert_path\s*:\s*PathBuf\b/u.test(fixture.server) ||
      !/\bkey_path\s*:\s*PathBuf\b/u.test(fixture.server),
    "server-ingress-owner",
    "nimbus-server must retain the operator certificate and key configuration",
  );
  reject(
    !/\bpub\s+struct\s+WorkloadPepTlsAuthority\b/u.test(fixture.proxy) ||
      !/\bca_key\s*:\s*KeyPair\b/u.test(fixture.proxy) ||
      !/\bpub\s+fn\s+trust_anchor_(?:der|pem)\b/u.test(fixture.proxy) ||
      !/\bLEAF_CACHE_CAP\s*:\s*usize\s*=\s*64\b/u.test(fixture.proxy),
    "proxy-interception-owner",
    "nimbus-proxy must retain a private ephemeral CA and capped public trust-anchor surface",
  );
  reject(
    !/\bWorkloadPepTlsAuthority::generate_ephemeral\b/u.test(fixture.sandbox) ||
      !/\btrust_anchor_pem\s*\(\s*\)/u.test(fixture.sandbox),
    "sandbox-public-anchor",
    "nimbus-sandbox must publish only the PEP public trust anchor",
  );

  reject(
    !/\benum\s+CommandMetricLabel\b/u.test(fixture.kvMetrics) ||
      !/commands\s*:\s*Mutex\s*<\s*BTreeMap\s*<\s*CommandMetricLabel\s*,\s*CommandMetrics\s*>\s*>/u.test(
        fixture.kvMetrics,
      ) ||
      !/\bfn\s+classify\s*\(\s*name\s*:\s*&str\s*\)\s*->\s*Self\b/u.test(
        fixture.kvMetrics,
      ) ||
      !/\bfn\s+label\s*\(\s*self\s*\)\s*->\s*&'static\s+str\b/u.test(
        fixture.kvMetrics,
      ),
    "closed-command-label",
    "KV command metrics must use a closed key type with static output labels",
  );
  reject(
    /BTreeMap\s*<\s*String\s*,\s*CommandMetrics\s*>/u.test(fixture.kvMetrics) ||
      /commands\.entry\s*\(\s*name\.to_ascii_uppercase/u.test(
        fixture.kvMetrics,
      ),
    "dynamic-command-label",
    "KV command metrics must not retain client-supplied strings as map keys",
  );
  for (const label of [...supportedCommandLabels, "UNKNOWN"]) {
    reject(
      !fixture.kvMetrics.includes(`=> "${label}"`),
      "missing-command-label",
      `KV command metrics are missing the closed ${label} label`,
    );
  }
  const dispatchStart = fixture.kvServer.indexOf("fn execute_command");
  const dispatchEnd = fixture.kvServer.indexOf("fn authenticated_tenant");
  const dispatchBoundariesValid =
    dispatchStart >= 0 && dispatchEnd > dispatchStart;
  reject(
    !dispatchBoundariesValid,
    "kv-dispatch-boundary",
    "the KV execute_command dispatch boundary must exist before authenticated_tenant",
  );
  const executeCommand = dispatchBoundariesValid
    ? fixture.kvServer.slice(dispatchStart, dispatchEnd)
    : "";
  const dispatchedCommands = new Set(
    [
      ...executeCommand.matchAll(
        /^\s*"((?:\\.|[^"\\])*)"(?:\s+if\b[^=]*)?\s*=>/gmu,
      ),
    ].map((match) => match[1]),
  );
  const missingDispatch = supportedCommandLabels.filter(
    (label) => !dispatchedCommands.has(label),
  );
  const extraDispatch = [...dispatchedCommands].filter(
    (label) => !supportedCommandLabels.includes(label),
  );
  reject(
    missingDispatch.length > 0 || extraDispatch.length > 0,
    "kv-dispatch-census",
    `KV dispatch must match the 18 closed command labels; missing=${missingDispatch.join(",") || "none"} extra=${extraDispatch.join(",") || "none"}`,
  );
  for (const label of dispatchedCommands) {
    reject(
      !fixture.kvMetrics.includes(`=> "${label}"`),
      "unclassified-dispatched-command",
      `the dispatched ${label} command has no closed metric label`,
    );
  }
  reject(
    !/metrics\.record_command\s*\(\s*&name\b/u.test(fixture.kvServer),
    "kv-command-recording-seam",
    "the KV protocol path must record the classified command name",
  );

  const fairnessFunctions = rustFunctionRanges(fixture.proxyFairness);
  const tenantInsertionOwners = [];
  const tenantInsertionIndexes = new Set();
  for (const pattern of [
    /\.(?:entry|insert)\s*\(/gmu,
    /\b(?:[A-Za-z_][A-Za-z0-9_]*::)+(?:entry|insert)\s*\(/gmu,
  ]) {
    for (const insertion of fixture.proxyFairness.matchAll(pattern)) {
      tenantInsertionIndexes.add(insertion.index);
    }
  }
  for (const insertionIndex of tenantInsertionIndexes) {
    const owner = fairnessFunctions.find(
      ({ start, end }) => insertionIndex >= start && insertionIndex < end,
    );
    tenantInsertionOwners.push(owner?.name ?? "<outside-function>");
  }
  reject(
    tenantInsertionOwners.length !== 1 ||
      tenantInsertionOwners[0] !== "checkout",
    "unpinned-tenant-lookup",
    `production proxy tenant-map insertion must occur exactly once in checkout; found ${tenantInsertionOwners.join(", ") || "none"}`,
  );
  reject(
    /\bpub(?:\s*\(\s*crate\s*\))?\s+fn\s+tenant\s*\(/u.test(
      fixture.proxyFairness,
    ),
    "raw-tenant-accessor",
    "production proxy telemetry must not expose a raw tenant accessor",
  );
  reject(
    !/\bpub\s+fn\s+checkout\s*\(\s*self\s*:\s*&Arc<Self>/u.test(
      fixture.proxyFairness,
    ) ||
      !/\bimpl\s+Drop\s+for\s+TenantLease\b/u.test(fixture.proxyFairness) ||
      !/release_lease\s*\(\s*&self\.handle\s*\)/u.test(fixture.proxyFairness),
    "tenant-lease-lifecycle",
    "proxy tenant telemetry must be pinned by TenantLease and released on drop",
  );

  const metricKeySources = `${fixture.network}\n${fixture.kvMetrics}\n${fixture.proxy}\n${fixture.proxyFairness}`;
  const resourceIdentity =
    "(?:tenant|attachment|endpoint|listener|route|port|segment|provider_handle)(?:_id)?";
  const resourceMetricPatterns = [
    new RegExp(
      `\\b(?:labels?|metrics?|counters?)\\.entry\\s*\\([^\\n;]*${resourceIdentity}`,
      "u",
    ),
    new RegExp(
      `\\b(?:metric_label|label_key)\\s*\\([^\\n;]*${resourceIdentity}`,
      "u",
    ),
    new RegExp(
      `\\b(?:metrics::)?(?:counter|gauge|histogram)!\\s*\\([^;]*${resourceIdentity}`,
      "su",
    ),
  ];
  reject(
    resourceMetricPatterns.some((pattern) => pattern.test(metricKeySources)),
    "resource-identity-label",
    "stable resource identities and provider handles must not become metric keys",
  );
  reject(
    !/\benum\s+LatencySegment\b/u.test(fixture.serverLatency) ||
      !/\bfn\s+label\s*\(\s*self\s*\)\s*->\s*&'static\s+str\b/u.test(
        fixture.serverLatency,
      ),
    "static-server-latency-label",
    "server latency labels must remain a closed static set",
  );

  return errors;
}

function greenFixture() {
  const labels = [...supportedCommandLabels, "UNKNOWN"]
    .map((label) => `Self::Label => "${label}",`)
    .join("\n");
  return {
    networkDependencies: new Set(["nimbus-core", "serde"]),
    networkWorkspaceDependencies: new Set(["nimbus-core"]),
    serverDependencies: new Set(["nimbus-core"]),
    proxyDependencies: new Set(["nimbus-core"]),
    sandboxDependencies: new Set(["nimbus-proxy"]),
    network: "pub enum NetworkTlsBehavior { Disabled }",
    server:
      "pub struct TlsConfig { pub cert_path: PathBuf, pub key_path: PathBuf }",
    proxy:
      "pub struct WorkloadPepTlsAuthority { ca_key: KeyPair } const LEAF_CACHE_CAP: usize = 64; pub fn trust_anchor_pem(&self) {}",
    sandbox:
      "WorkloadPepTlsAuthority::generate_ephemeral(); authority.trust_anchor_pem();",
    kvMetrics: `enum CommandMetricLabel { Label } commands: Mutex<BTreeMap<CommandMetricLabel, CommandMetrics>>; fn classify(name: &str) -> Self { Self::Label } fn label(self) -> &'static str { match self { ${labels} } }`,
    kvServer: `fn execute_command() { match name {\n${supportedCommandLabels
      .map((label) => `"${label}" => command(),`)
      .join(
        "\n",
      )} } } metrics.record_command(&name); fn authenticated_tenant() {}`,
    proxyFairness:
      "pub fn checkout(self: &Arc<Self>, tenant: &TenantId) -> TenantLease { let mut map = self.tenants.lock(); map.entry(tenant.clone()); TenantLease {} } impl Drop for TenantLease { fn drop(&mut self) { self.registry.release_lease(&self.handle); } }",
    serverLatency:
      'enum LatencySegment { Auth } fn label(self) -> &\'static str { "server.auth" }',
  };
}

function runSelfTest() {
  const baseline = greenFixture();
  const baselineErrors = check(baseline);
  if (baselineErrors.length > 0) {
    for (const error of baselineErrors) {
      console.error(`SELFTEST BASELINE FAIL ${error.id}: ${error.message}`);
    }
    process.exitCode = 1;
    return;
  }

  const mutations = [
    [
      "network-certificate-owner",
      (fixture) => ({
        ...fixture,
        network: `${fixture.network}\npub struct CertificateProvider;`,
      }),
    ],
    [
      "server-proxy-authority-crossing",
      (fixture) => ({
        ...fixture,
        server: `${fixture.server}\nuse nimbus_proxy::WorkloadPepTlsAuthority;`,
      }),
    ],
    [
      "server-proxy-authority-crossing",
      (fixture) => ({
        ...fixture,
        serverDependencies: new Set([
          ...fixture.serverDependencies,
          "nimbus-proxy",
        ]),
      }),
    ],
    [
      "network-metric-dependency",
      (fixture) => ({
        ...fixture,
        networkDependencies: new Set([
          ...fixture.networkDependencies,
          "metrics-exporter-prometheus",
        ]),
      }),
    ],
    [
      "network-metric-dependency",
      (fixture) => ({
        ...fixture,
        networkDependencies: new Set([
          ...fixture.networkDependencies,
          "opentelemetry_sdk",
        ]),
      }),
    ],
    [
      "network-metric-exporter",
      (fixture) => ({
        ...fixture,
        network: `${fixture.network}\nuse opentelemetry_sdk::metrics::SdkMeterProvider;`,
      }),
    ],
    [
      "dynamic-command-label",
      (fixture) => ({
        ...fixture,
        kvMetrics: `${fixture.kvMetrics}\ncommands: Mutex<BTreeMap<String, CommandMetrics>>; commands.entry(name.to_ascii_uppercase());`,
      }),
    ],
    [
      "kv-dispatch-boundary",
      (fixture) => ({
        ...fixture,
        kvServer: fixture.kvServer.replace("fn execute_command", "fn dispatch"),
      }),
    ],
    [
      "kv-dispatch-census",
      (fixture) => ({
        ...fixture,
        kvServer: fixture.kvServer.replace(
          '"NIMBUS.METRICS" => command(),',
          '"NIMBUS.METRICS" => command(),\n"NIMBUS.V2" => command(),',
        ),
      }),
    ],
    [
      "unpinned-tenant-lookup",
      (fixture) => ({
        ...fixture,
        proxyFairness: `${fixture.proxyFairness}\npub fn lookup(&self, tenant: &TenantId) { let mut map = self.tenants.lock(); map.entry(tenant.clone()); }`,
      }),
    ],
    [
      "unpinned-tenant-lookup",
      (fixture) => ({
        ...fixture,
        proxyFairness: `${fixture.proxyFairness}\npub fn ufcs_lookup(&self, tenant: &TenantId) { let mut map = self.tenants.lock(); HashMap::entry(&mut *map, tenant.clone()).or_default(); }`,
      }),
    ],
    [
      "raw-tenant-accessor",
      (fixture) => ({
        ...fixture,
        proxyFairness: `${fixture.proxyFairness}\npub fn tenant(&self) -> &str { self.tenant.as_str() }`,
      }),
    ],
    [
      "resource-identity-label",
      (fixture) => ({
        ...fixture,
        proxyFairness: `${fixture.proxyFairness}\nstruct MetricSink { labels: HashMap<String, u64> } impl MetricSink { fn record(&mut self, tenant_id: &TenantId) { self.labels.entry(tenant_id.as_str().to_owned()).or_default(); } }`,
      }),
    ],
    [
      "resource-identity-label",
      (fixture) => ({
        ...fixture,
        proxy: `${fixture.proxy}\nuse metrics::counter; fn record_metric(tenant_id: &TenantId) { counter!("proxy.requests", "tenant_id" => tenant_id.as_str()); }`,
      }),
    ],
    [
      "network-metric-exporter",
      (fixture) => ({
        ...fixture,
        network: `${fixture.network}\nmetrics::counter!("network_attachment", "attachment_id" => attachment_id.to_string());`,
      }),
    ],
  ];
  let passed = 0;
  for (const [expected, mutate] of mutations) {
    const fixture = mutate(baseline);
    const errors = check(fixture);
    if (errors.some(({ id }) => id === expected)) {
      passed += 1;
    } else {
      console.error(`SELFTEST FAIL ${expected}: named mutation passed`);
    }
  }
  const failed = mutations.length - passed;
  console.log(
    `NNC7.6 static contract self-test: ${passed} passed, ${failed} failed`,
  );
  if (failed > 0) process.exitCode = 1;
}

if (process.argv.includes("--self-test")) {
  runSelfTest();
} else {
  const errors = check(loadLiveFixture());
  if (errors.length === 0) {
    console.log("NNC7.6 static contract: PASS");
  } else {
    for (const error of errors) {
      console.error(`FAIL NNC7.6 ${error.id}: ${error.message}`);
    }
    process.exitCode = 1;
  }
}
