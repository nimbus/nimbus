import assert from "node:assert/strict";

import { assertRestClientRouteParity } from "./rest_client_route_parity.mjs";
import { spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { build } from "esbuild";

import { assertCapabilitySurfaceContract } from "./capability_surface_contract.mjs";

const packageRoot = fileURLToPath(new URL("../", import.meta.url));
const tscPath = fileURLToPath(
  new URL("../../../node_modules/typescript/bin/tsc", import.meta.url),
);
const typecheckOnly = process.argv.includes("--typecheck-only");

async function main() {
  await assertCapabilitySurfaceContract();
  if (typecheckOnly) {
    await typecheckNimbusAuthExtension();
    return;
  }
  const indexBundle = await bundleModule("index.ts", "neutral");
  const controlPlaneBundle = await bundleModule("control-plane/client.ts", "neutral");
  const controlPlaneRoutesBundle = await bundleModule("control_plane_routes.ts", "neutral");
  await assertControlPlaneRouteManifest(controlPlaneRoutesBundle);
  await bundleModule("browser.ts", "browser");
  await bundleModule("react.ts", "browser");
  await bundleModule("server.ts", "neutral");
  await bundleModule("values.ts", "neutral");
  const restBundle = await bundleModule("transports/rest.ts", "neutral");
  await assertRestClientRouteParity(restBundle);
  await assertExplicitOptionsBypassLocalCredentialFile(indexBundle);
  await assertLifecycleWaitValidation(indexBundle);
  await assertServiceWaitUsesMonotonicTime(controlPlaneBundle);
  await assertServiceDefinitionRoutes(indexBundle);
  await assertSandboxRoutes(indexBundle);
  await assertSessionRoutes(indexBundle);
  await assertCommitErrorEnvelopeDecoding(indexBundle);
  await typecheckNimbusAuthExtension();
}

async function assertCommitErrorEnvelopeDecoding(indexBundle) {
  const sdk = await import(`${pathToFileURL(indexBundle).href}?errors=${Date.now()}`);
  const cases = [
    ["op.conflict", "conflict", "NimbusConflictError", "retryable", true],
    ["rate.overloaded", "overloaded", "NimbusOverloadedError", "retryable_after_backoff", true],
    ["rate.committer_full", "committer_full", "NimbusCommitterFullError", "retryable_after_backoff", true],
    ["rate.rejected_before_execution", "rejected_before_execution", "NimbusRejectedBeforeExecutionError", "retryable", true],
    ["rate.limited", "rate_limited", "NimbusRateLimitedError", "retryable_after_backoff", true],
    ["op.out_of_retention", "out_of_retention", "NimbusOutOfRetentionError", "restart_transaction", true],
    ["op.cap_exceeded", "cap_exceeded", "NimbusCapExceededError", "terminal", false],
  ];

  for (const [code, kind, name, retryability, retryable] of cases) {
    const error = sdk.decodeNimbusErrorEnvelope({
      error: {
        code,
        message: `${kind} fixture`,
        retryable,
        detail: {
          retryability,
          ...(code === "rate.limited" ? { retryAfterMs: 250 } : {}),
        },
      },
    });
    assert.ok(error instanceof sdk.NimbusCommitError);
    assert.equal(error.name, name);
    assert.equal(error.kind, kind);
    assert.equal(error.retryability, retryability);
    assert.equal(error.retryable, retryable);
    assert.equal(error.retryAfterMs, code === "rate.limited" ? 250 : undefined);
  }
}

async function assertControlPlaneRouteManifest(controlPlaneRoutesBundle) {
  const { NIMBUS_CONTROL_PLANE_ROUTES } = await import(
    `${pathToFileURL(controlPlaneRoutesBundle).href}?routes=${Date.now()}`
  );
  assert.deepEqual(NIMBUS_CONTROL_PLANE_ROUTES, {
    "services.get": {
      verb: "GET",
      path: "/api/tenants/{tenant_id}/services/{service_name}",
    },
    "services.create": {
      verb: "POST",
      path: "/api/tenants/{tenant_id}/services",
    },
    "services.update": {
      verb: "PUT",
      path: "/api/tenants/{tenant_id}/services/{service_name}",
    },
    "services.delete": {
      verb: "DELETE",
      path: "/api/tenants/{tenant_id}/services/{service_name}",
    },
    "services.list": {
      verb: "GET",
      path: "/api/tenants/{tenant_id}/services",
    },
    "services.start": {
      verb: "POST",
      path: "/api/tenants/{tenant_id}/services/{service_name}/start",
    },
    "services.stop": {
      verb: "POST",
      path: "/api/tenants/{tenant_id}/services/{service_name}/stop",
    },
    "services.restart": {
      verb: "POST",
      path: "/api/tenants/{tenant_id}/services/{service_name}/restart",
    },
    "sandboxes.create": {
      verb: "POST",
      path: "/api/tenants/{tenant_id}/sandboxes",
    },
    "sandboxes.get": {
      verb: "GET",
      path: "/api/tenants/{tenant_id}/sandboxes/{sandbox_id}",
    },
    "sandboxes.list": {
      verb: "GET",
      path: "/api/tenants/{tenant_id}/sandboxes",
    },
    "sandboxes.stop": {
      verb: "POST",
      path: "/api/tenants/{tenant_id}/sandboxes/{sandbox_id}/stop",
    },
    "sessions.open": {
      verb: "POST",
      path: "/api/sessions",
    },
    "sessions.get": {
      verb: "GET",
      path: "/api/sessions/{session_id}",
    },
    "sessions.list": {
      verb: "GET",
      path: "/api/sessions",
    },
    "sessions.close": {
      verb: "POST",
      path: "/api/sessions/{session_id}/close",
    },
  });
}

async function bundleModule(relativePath, platform) {
  const outdir = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-package-"));
  const outfile = path.join(outdir, relativePath.replace(".ts", ".mjs"));
  await build({
    entryPoints: [fileURLToPath(new URL(`./${relativePath}`, import.meta.url))],
    bundle: true,
    format: "esm",
    platform,
    outfile,
    logLevel: "silent",
  });
  return outfile;
}

async function assertExplicitOptionsBypassLocalCredentialFile(indexBundle) {
  const fixtureDir = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-sdk-"));
  const badCredentialsPath = path.join(fixtureDir, "application_default_credentials.json");
  await fs.writeFile(badCredentialsPath, "not json", "utf8");

  const previousCredentialsPath = process.env.NIMBUS_APPLICATION_CREDENTIALS;
  process.env.NIMBUS_APPLICATION_CREDENTIALS = badCredentialsPath;
  try {
    const { Nimbus } = await import(`${pathToFileURL(indexBundle).href}?t=${Date.now()}`);
    let observedUrl = "";
    let observedAuthorization = "";
    const client = new Nimbus({
      endpoint: "http://localhost:8080",
      tenantId: "tenant",
      token: "explicit-token",
      fetch: async (input, init = {}) => {
        observedUrl = String(input);
        observedAuthorization = String(
          init.headers && typeof init.headers === "object" && "Authorization" in init.headers
            ? init.headers.Authorization
            : "",
        );
        return new Response(JSON.stringify({ name: "db", state: "ready" }), {
          headers: { "content-type": "application/json" },
        });
      },
    });
    assert.equal("request" in client, false);
    assert.equal("resolveRestClient" in client, false);

    await client.services.get({ name: "db" });
    assert.equal(
      observedUrl,
      "http://localhost:8080/api/tenants/tenant/services/db",
    );
    assert.equal(observedAuthorization, "Bearer explicit-token");
  } finally {
    if (previousCredentialsPath === undefined) {
      delete process.env.NIMBUS_APPLICATION_CREDENTIALS;
    } else {
      process.env.NIMBUS_APPLICATION_CREDENTIALS = previousCredentialsPath;
    }
  }
}

async function assertLifecycleWaitValidation(indexBundle) {
  const { Nimbus } = await import(`${pathToFileURL(indexBundle).href}?validation=${Date.now()}`);
  let fetchCalls = 0;
  const client = new Nimbus({
    endpoint: "http://localhost:8080",
    tenantId: "tenant",
    token: "explicit-token",
    fetch: async () => {
      fetchCalls += 1;
      return new Response(JSON.stringify({
        name: "db",
        lifecycleState: "ready",
        readiness: "ready",
        health: "healthy",
      }), {
        headers: { "content-type": "application/json" },
      });
    },
  });

  await assert.rejects(
    () => client.services.stop({ name: "db", waitUntil: "ready" }),
    /services\.stop\(\{ waitUntil \}\) only supports stopped/,
  );
  assert.equal(fetchCalls, 0);

  await assert.rejects(
    () => client.services.start({ name: "db", waitUntil: "stopped" }),
    /services\.start\(\{ waitUntil \}\) only supports ready, or healthy/,
  );
  assert.equal(fetchCalls, 0);

  await client.services.start({ name: "db", waitUntil: "healthy" });
  assert.equal(fetchCalls, 2, "start with healthy wait should POST then poll GET");
}

async function assertServiceWaitUsesMonotonicTime(controlPlaneBundle) {
  const { Nimbus, NimbusServices } = await import(
    `${pathToFileURL(controlPlaneBundle).href}?monotonic=${Date.now()}`
  );
  const client = new Nimbus({ tenantId: "tenant" });
  const originalDateNow = Date.now;

  async function runCase(wallObservations) {
    let monotonicNow = 0;
    let requests = 0;
    let sleeps = 0;
    Date.now = () => wallObservations.shift() ?? 0;
    const services = new NimbusServices(
      client,
      async () => {
        requests += 1;
        return {
          name: "db",
          lifecycleState: "starting",
          readiness: "starting",
          health: "unknown",
          state: "starting",
        };
      },
      {
        monotonicNow: () => monotonicNow,
        sleep: async (ms) => {
          sleeps += 1;
          monotonicNow += ms;
        },
      },
    );

    await assert.rejects(
      services.wait({ name: "db", until: "healthy", timeoutMs: 10, intervalMs: 6 }),
      new Error(
        "Nimbus service db did not reach healthy within 10ms; last observed status was starting.",
      ),
    );
    assert.equal(requests, 2);
    assert.equal(sleeps, 2);
  }

  try {
    await runCase([0, 100_000, 200_000]);
    await runCase([100_000, 50_000, 0]);
  } finally {
    Date.now = originalDateNow;
  }
}

async function assertServiceDefinitionRoutes(indexBundle) {
  const { Nimbus } = await import(`${pathToFileURL(indexBundle).href}?definitions=${Date.now()}`);
  const observed = [];
  const client = new Nimbus({
    endpoint: "http://localhost:8080",
    tenantId: "tenant",
    token: "explicit-token",
    fetch: async (input, init = {}) => {
      observed.push({
        url: String(input),
        method: init.method ?? "GET",
        body: typeof init.body === "string" ? JSON.parse(init.body) : null,
      });
      return new Response(JSON.stringify({
        metadata: {
          tenantId: "tenant",
          name: "browser",
          generation: 1,
          resourceVersion: "svcdef-v1",
          createdAt: "1970-01-01T00:00:00Z",
          updatedAt: "1970-01-01T00:00:00Z",
          labels: {},
          source: "dynamic",
        },
        spec: {
          backend: { kind: "builtIn", provider: "browser" },
        },
        status: {
          backend: "builtIn",
          lifecycleState: "declared",
          readiness: "unknown",
          health: "unknown",
          conditions: [],
        },
        items: [],
      }), {
        headers: { "content-type": "application/json" },
      });
    },
  });

  await client.services.create({
    name: "browser",
    backend: { kind: "builtIn", provider: "browser" },
  });
  await client.services.update({
    name: "browser",
    ifMatchGeneration: 1,
    backend: { kind: "builtIn", provider: "browser" },
  });
  await client.services.list({ limit: 10, pageToken: "browser" });
  await client.services.delete({ name: "browser", ifMatchGeneration: 2, force: true });

  assert.deepEqual(observed.map((request) => [request.method, request.url]), [
    ["POST", "http://localhost:8080/api/tenants/tenant/services"],
    ["PUT", "http://localhost:8080/api/tenants/tenant/services/browser"],
    ["GET", "http://localhost:8080/api/tenants/tenant/services?limit=10&pageToken=browser"],
    [
      "DELETE",
      "http://localhost:8080/api/tenants/tenant/services/browser?ifMatchGeneration=2&force=true",
    ],
  ]);
  assert.deepEqual(observed[0].body, {
    metadata: { name: "browser", labels: {} },
    spec: { backend: { kind: "builtIn", provider: "browser" } },
  });
  assert.equal(observed[1].body.metadata.generation, 1);
}

async function assertSandboxRoutes(indexBundle) {
  const { Nimbus } = await import(`${pathToFileURL(indexBundle).href}?sandboxes=${Date.now()}`);
  const observed = [];
  const client = new Nimbus({
    endpoint: "http://localhost:8080",
    tenantId: "tenant",
    token: "explicit-token",
    fetch: async (input, init = {}) => {
      observed.push({
        url: String(input),
        method: init.method ?? "GET",
        body: typeof init.body === "string" ? JSON.parse(init.body) : null,
      });
      return new Response(JSON.stringify({
        metadata: {
          tenantId: "tenant",
          id: "sandbox-1",
          generation: 1,
          resourceVersion: "sandbox-v1",
          createdAt: "1970-01-01T00:00:00Z",
          updatedAt: "1970-01-01T00:00:00Z",
          labels: {},
        },
        spec: {
          profile: "worker",
          sandbox: {},
        },
        status: {
          lifecycleState: "ready",
          readiness: "ready",
          health: "healthy",
          backend: "krun",
          endpoints: [],
          conditions: [],
        },
        items: [],
      }), {
        headers: { "content-type": "application/json" },
      });
    },
  });
  const spec = {
    tenantId: "tenant",
    owner: { kind: "standalone" },
    backend: "krun",
    root: { kind: "oci_image", source: { kind: "reference", reference: "registry.example.com/worker:latest" } },
    process: { argv: ["worker"] },
  };

  await client.sandboxes.create({ profile: "worker", spec });
  await client.sandboxes.list({ limit: 5, labelKey: "app", labelValue: "test" });
  await client.sandboxes.get({ id: "sandbox-1" });
  await client.sandboxes.stop({ id: "sandbox-1" });

  assert.deepEqual(observed.map((request) => [request.method, request.url]), [
    ["POST", "http://localhost:8080/api/tenants/tenant/sandboxes"],
    ["GET", "http://localhost:8080/api/tenants/tenant/sandboxes?limit=5&labelKey=app&labelValue=test"],
    ["GET", "http://localhost:8080/api/tenants/tenant/sandboxes/sandbox-1"],
    ["POST", "http://localhost:8080/api/tenants/tenant/sandboxes/sandbox-1/stop"],
  ]);
  assert.deepEqual(observed[0].body, {
    profile: "worker",
    spec,
    labels: {},
  });
}

async function assertSessionRoutes(indexBundle) {
  const { Nimbus } = await import(`${pathToFileURL(indexBundle).href}?sessions=${Date.now()}`);
  const observed = [];
  const client = new Nimbus({
    endpoint: "http://localhost:8080",
    tenantId: "tenant",
    token: "explicit-token",
    fetch: async (input, init = {}) => {
      observed.push({
        url: String(input),
        method: init.method ?? "GET",
        body: typeof init.body === "string" ? JSON.parse(init.body) : null,
      });
      return new Response(JSON.stringify({
        metadata: {
          tenantId: "tenant",
          id: "session-1",
          generation: 1,
          resourceVersion: "session-v1",
          createdAt: "1970-01-01T00:00:00Z",
          updatedAt: "1970-01-01T00:00:00Z",
        },
        spec: {
          target: { service: { name: "browser" } },
          targetSnapshot: {
            service: {
              name: "browser",
              generation: 1,
              backend: "builtIn",
              provider: "browser",
            },
          },
          channels: ["cdp"],
          expiresAt: "1970-01-01T00:15:00Z",
        },
        status: {
          lifecycleState: "open",
          expiresAt: "1970-01-01T00:15:00Z",
          conditions: [],
        },
        items: [],
      }), {
        headers: { "content-type": "application/json" },
      });
    },
  });

  await client.sessions.open({
    target: { service: { name: "browser" } },
    channels: ["cdp", "page"],
    requestedTtlMs: 60_000,
  });
  await client.sessions.list({ limit: 5, state: "open" });
  await client.sessions.get({ id: "session-1" });
  await client.sessions.close({ id: "session-1", reason: "test_complete" });

  assert.deepEqual(observed.map((request) => [request.method, request.url]), [
    ["POST", "http://localhost:8080/api/sessions"],
    ["GET", "http://localhost:8080/api/sessions?tenantId=tenant&limit=5&state=open"],
    ["GET", "http://localhost:8080/api/sessions/session-1"],
    ["POST", "http://localhost:8080/api/sessions/session-1/close"],
  ]);
  assert.deepEqual(observed[0].body, {
    tenantId: "tenant",
    target: { service: { name: "browser" } },
    channels: ["cdp", "page"],
    requestedTtlMs: 60_000,
  });
  assert.deepEqual(observed[3].body, { reason: "test_complete" });
}

async function typecheckNimbusAuthExtension() {
  const fixtureDir = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-ts-"));
  const normalize = (target) => path.relative(fixtureDir, target).replaceAll("\\", "/");
  const serverEntry = normalize(path.join(packageRoot, "src", "server.ts"));
  const browserEntry = normalize(path.join(packageRoot, "src", "browser.ts"));
  const reactEntry = normalize(path.join(packageRoot, "src", "react.ts"));
  const valuesEntry = normalize(path.join(packageRoot, "src", "values.ts"));
  const rootEntry = normalize(path.join(packageRoot, "src", "index.ts"));
  const restEntry = normalize(path.join(packageRoot, "src", "transports", "rest.ts"));

  await fs.writeFile(
    path.join(fixtureDir, "tsconfig.json"),
    JSON.stringify(
      {
        compilerOptions: {
          strict: true,
          noEmit: true,
          target: "ES2022",
          module: "ESNext",
          moduleResolution: "Bundler",
          allowImportingTsExtensions: true,
          jsx: "react-jsx",
          lib: ["ES2022", "DOM"],
          paths: {
            "@nimbus/nimbus": [rootEntry],
            "@nimbus/nimbus/server": [serverEntry],
            "@nimbus/nimbus/browser": [browserEntry],
            "@nimbus/nimbus/react": [reactEntry],
            "@nimbus/nimbus/values": [valuesEntry],
            "@nimbus/nimbus/transports/rest": [restEntry],
          },
        },
        files: ["fixture.ts"],
      },
      null,
      2,
    ),
    "utf8",
  );

  await fs.writeFile(
    path.join(fixtureDir, "fixture.ts"),
    `
\timport {
  Nimbus,
  NimbusCommitError,
  type NimbusCommitErrorKind,
  type NimbusCommitPathError,
  type NimbusRetryability,
  type NimbusSandboxSpec,
  type NimbusSandboxSpecResponse,
} from "@nimbus/nimbus";
import { NimbusHttpClient, NimbusReactClient } from "@nimbus/nimbus/browser";
import {
  NimbusProvider,
  NimbusProviderWithAuth,
  NimbusReactClient as ReactClient,
  useNimbus,
  useNimbusAuth,
  useNimbusConnectionState,
  type NimbusAuthState,
} from "@nimbus/nimbus/react";
import {
  action,
  httpAction,
  query,
  type Auth,
  type VerifiedIdentity,
} from "@nimbus/nimbus/server";
import { NimbusRestClient } from "@nimbus/nimbus/transports/rest";
import { v } from "@nimbus/nimbus/values";

const _sdk = new Nimbus({
  endpoint: "http://localhost:8080",
  tenantId: "tenant",
  token: "test-token",
  fetch: async () => new Response("{}"),
});
declare const commitError: NimbusCommitPathError;
const _commitKind: NimbusCommitErrorKind = commitError.kind;
const _commitRetryability: NimbusRetryability = commitError.retryability;
const _commitRetryable: boolean = commitError.retryable;
if (commitError instanceof NimbusCommitError) {
  const _retryAfterMs: number | undefined = commitError.retryAfterMs;
}
function exhaustCommitErrorKinds(kind: NimbusCommitErrorKind): string {
  switch (kind) {
    case "conflict":
    case "overloaded":
    case "committer_full":
    case "rejected_before_execution":
    case "rate_limited":
    case "out_of_retention":
    case "cap_exceeded":
      return kind;
    default: {
      const neverKind: never = kind;
      return neverKind;
    }
  }
}
void exhaustCommitErrorKinds;
const _serviceStart = _sdk.services.start({ name: "db" });
const _serviceStartReady = _sdk.services.start({ name: "db", waitUntil: "ready" });
const _serviceStartHealthy = _sdk.services.start({ name: "db", waitUntil: "healthy" });
// @ts-expect-error service start waits for activation conditions, not stopped.
_sdk.services.start({ name: "db", waitUntil: "stopped" });
const _serviceStop = _sdk.services.stop({ name: "db" });
const _serviceStopStopped = _sdk.services.stop({ name: "db", waitUntil: "stopped" });
// @ts-expect-error service stop waits for stopped, not readiness.
_sdk.services.stop({ name: "db", waitUntil: "ready" });
const _serviceRestart = _sdk.services.restart({ name: "db" });
const _serviceRestartHealthy = _sdk.services.restart({ name: "db", waitUntil: "healthy" });
// @ts-expect-error service restart waits for activation conditions, not stopped.
_sdk.services.restart({ name: "db", waitUntil: "stopped" });
const _serviceGet = _sdk.services.get({ name: "db" });
const _serviceWait = _sdk.services.wait({ name: "db", until: "healthy" });
const _serviceCreateBuiltIn = _sdk.services.create({
  name: "browser",
  backend: { kind: "builtIn", provider: "browser" },
});
const _serviceCreateExternal = _sdk.services.create({
  name: "api",
  backend: {
    kind: "external",
    endpoint: { url: "https://api.example.com" },
    auth: { kind: "none" },
    health: { kind: "http", path: "/health" },
  },
});
const sandboxSpec = {
  tenantId: "tenant",
  owner: { kind: "service", serviceName: "worker" },
  backend: "krun",
  root: { kind: "oci_image", source: { kind: "reference", reference: "registry.example.com/worker:latest" } },
  process: { argv: ["worker"] },
} satisfies NimbusSandboxSpec;
const standaloneSandboxSpec = {
  tenantId: "tenant",
  owner: { kind: "standalone", displayName: "task" },
  backend: "krun",
  root: { kind: "oci_image", source: { kind: "reference", reference: "registry.example.com/task:latest" } },
  process: { argv: ["task"] },
} satisfies NimbusSandboxSpec;
const _redactedSandboxResponse = {
  tenantId: "tenant",
  owner: { kind: "standalone", displayName: "task" },
  backend: "krun",
  root: { kind: "redacted", redacted: true, reason: "operatorOnlyLaunchInput" },
  process: {
    argv: { redacted: true, valueCount: 1 },
    environment: { redacted: true, valueCount: 2 },
    cwd: "/",
    terminal: false,
  },
} satisfies NimbusSandboxSpecResponse;
// @ts-expect-error sandbox response process summaries do not expose env values.
_redactedSandboxResponse.process.env;
const _serviceCreateSandbox = _sdk.services.create({
  name: "worker",
  backend: { kind: "sandbox", sandbox: sandboxSpec },
});
const _serviceUpdate = _sdk.services.update({
  name: "browser",
  ifMatchGeneration: 1,
  backend: { kind: "builtIn", provider: "browser" },
});
const _serviceDelete = _sdk.services.delete({ name: "browser", ifMatchGeneration: 2, force: true });
const _serviceList = _sdk.services.list({ limit: 25 });
const _sandboxCreate = _sdk.sandboxes.create({
  profile: "worker",
  spec: standaloneSandboxSpec,
  labels: { app: "worker" },
});
const _sandboxList = _sdk.sandboxes.list({ labelKey: "app", labelValue: "worker" });
const _sandboxGet = _sdk.sandboxes.get({ id: "sandbox-1" });
const _sandboxStop = _sdk.sandboxes.stop({ id: "sandbox-1" });
// @ts-expect-error sandbox resources are id-addressed, not name-addressed.
_sdk.sandboxes.get({ name: "worker" });
const _serviceSession = _sdk.sessions.open({
  target: { service: { name: "browser" } },
  channels: ["cdp", "page"],
  requestedTtlMs: 60000,
});
const _sandboxSession = _sdk.sessions.open({
  target: { sandbox: { id: "sandbox-1" } },
  channels: ["stdio", "files"],
});
const _sessionList = _sdk.sessions.list({ state: "open" });
const _sessionGet = _sdk.sessions.get({ id: "session-1" });
const _sessionClose = _sdk.sessions.close({ id: "session-1", reason: "test_complete" });
// @ts-expect-error sessions open against sandbox ids, not sandbox names.
_sdk.sessions.open({ target: { sandbox: { name: "worker" } }, channels: ["stdio"] });
// @ts-expect-error unsupported channels are not part of the public session channel set.
_sdk.sessions.open({ target: { service: { name: "browser" } }, channels: ["ssh"] });
// @ts-expect-error sessions use open, not create.
_sdk.sessions.create({ target: { service: { name: "browser" } }, channels: ["cdp"] });
// @ts-expect-error client-managed renewal is not part of the session lifecycle.
_sdk.sessions.renew({ id: "session-1" });
// @ts-expect-error client-managed extension is not part of the session lifecycle.
_sdk.sessions.extend({ id: "session-1" });
// @ts-expect-error service create uses closed built-in provider ids.
_sdk.services.create({ name: "unknown", backend: { kind: "builtIn", provider: "anything" } });
// @ts-expect-error service update requires a generation precondition.
_sdk.services.update({ name: "browser", backend: { kind: "builtIn", provider: "browser" } });
// @ts-expect-error raw control-plane transport is not exposed on the root SDK.
_sdk.request("/api/tenants/tenant/services/db");
// @ts-expect-error raw control-plane client resolution is not exposed on the root SDK.
_sdk.resolveRestClient();
// @ts-expect-error ensureRunning is intentionally not a public SDK lifecycle verb.
_sdk.services.ensureRunning({ name: "db" });
const _nimbusBrowserClient = NimbusHttpClient;
const _nativeHttpClient = new NimbusHttpClient("http://localhost:8080/nimbus/demo", {
  skipDeploymentUrlCheck: true,
});
const _restClient = new NimbusRestClient("http://localhost:8080", {
  token: "test-token",
});
const _reactClient = NimbusReactClient;
const _reactClientAlias = ReactClient;
const _nimbusReactClient = new NimbusReactClient("http://localhost:8080/nimbus/demo", {
  skipDeploymentUrlCheck: true,
});
const _provider = NimbusProvider;
const _providerWithAuth = NimbusProviderWithAuth;
const _useClient = useNimbus;
const _useAuth = useNimbusAuth;
const _useConnectionState = useNimbusConnectionState;
const _authState = null as NimbusAuthState | null;

declare const auth: Auth;
declare const verified: VerifiedIdentity | null;

const _kind: "oidc" | "custom_jwt" | undefined = verified?.kind;
const _updatedAt: string | undefined = verified?.updatedAt;
const _customClaim = verified?.role;

void auth;

export const whoami = query({
  args: {
    id: v.string(),
  },
  returns: v.string(),
  async handler(ctx, args) {
    const compat = await ctx.auth.getUserIdentity();
    const richer = await ctx.auth.getVerifiedIdentity();
    const _compatUpdatedAt: string | undefined = compat?.updatedAt;
    const _verifiedKind: "oidc" | "custom_jwt" | undefined = richer?.kind;
    const _verifiedUpdatedAt: string | undefined = richer?.updatedAt;
    return args.id;
  },
});

export const runIdentityAction = action({
  async handler(ctx) {
    const richer = await ctx.auth.getVerifiedIdentity();
    return richer?.tokenIdentifier ?? null;
  },
});

export const identityHttp = httpAction(async (ctx) => {
  const richer = await ctx.auth.getVerifiedIdentity();
  return new Response(richer?.tokenIdentifier ?? "anonymous");
});
`,
    "utf8",
  );

  const result = spawnSync(process.execPath, [tscPath, "-p", path.join(fixtureDir, "tsconfig.json")], {
    encoding: "utf8",
    cwd: fixtureDir,
  });
  assert.equal(result.status, 0, result.stderr || result.stdout);
}

await main();
