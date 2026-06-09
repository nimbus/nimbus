import assert from "node:assert/strict";
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
  await bundleModule("browser.ts", "browser");
  await bundleModule("react.ts", "browser");
  await bundleModule("server.ts", "neutral");
  await bundleModule("values.ts", "neutral");
  await bundleModule("transports/rest.ts", "neutral");
  await assertExplicitOptionsBypassLocalCredentialFile(indexBundle);
  await typecheckNimbusAuthExtension();
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
import { Nimbus } from "@nimbus/nimbus";
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
const _serviceStart = _sdk.services.start({ name: "db" });
const _serviceStartReady = _sdk.services.start({ name: "db", waitUntil: "ready" });
const _serviceStop = _sdk.services.stop({ name: "db" });
const _serviceRestart = _sdk.services.restart({ name: "db" });
const _serviceGet = _sdk.services.get({ name: "db" });
const _serviceWait = _sdk.services.wait({ name: "db", until: "healthy" });
// @ts-expect-error raw control-plane transport is not exposed on the root SDK.
_sdk.request("/api/tenants/tenant/services/db");
// @ts-expect-error raw control-plane client resolution is not exposed on the root SDK.
_sdk.resolveRestClient();
// @ts-expect-error ensureRunning is intentionally not a public SDK lifecycle verb.
_sdk.services.ensureRunning({ name: "db" });
// @ts-expect-error sandbox routes are not exposed until server-backed resource APIs land.
_sdk.sandboxes.create({ profile: "worker" });
// @ts-expect-error session routes are not exposed until server-backed session APIs land.
_sdk.sessions.open({ target: { service: { name: "browser" } }, channels: ["cdp"] });
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
