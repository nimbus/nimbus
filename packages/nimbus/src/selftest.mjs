import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { build } from "esbuild";

const packageRoot = fileURLToPath(new URL("../", import.meta.url));
const tscPath = fileURLToPath(
  new URL("../../../node_modules/typescript/bin/tsc", import.meta.url),
);
const typecheckOnly = process.argv.includes("--typecheck-only");

async function main() {
  if (typecheckOnly) {
    await typecheckNimbusAuthExtension();
    return;
  }
  await bundleModule("browser.ts", "browser");
  await bundleModule("react.ts", "browser");
  await bundleModule("server.ts", "neutral");
  await bundleModule("values.ts", "neutral");
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
}

async function typecheckNimbusAuthExtension() {
  const fixtureDir = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-ts-"));
  const normalize = (target) => path.relative(fixtureDir, target).replaceAll("\\", "/");
  const serverEntry = normalize(path.join(packageRoot, "src", "server.ts"));
  const browserEntry = normalize(path.join(packageRoot, "src", "browser.ts"));
  const reactEntry = normalize(path.join(packageRoot, "src", "react.ts"));
  const valuesEntry = normalize(path.join(packageRoot, "src", "values.ts"));

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
            "nimbus/server": [serverEntry],
            "nimbus/browser": [browserEntry],
            "nimbus/react": [reactEntry],
            "nimbus/values": [valuesEntry],
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
import { NimbusHttpClient, NimbusReactClient } from "nimbus/browser";
import {
  NimbusProvider,
  NimbusProviderWithAuth,
  NimbusReactClient as ReactClient,
  useNimbus,
  useNimbusAuth,
  useNimbusConnectionState,
  type NimbusAuthState,
} from "nimbus/react";
import {
  action,
  httpAction,
  query,
  type Auth,
  type VerifiedIdentity,
} from "nimbus/server";
import { v } from "nimbus/values";

const _nimbusBrowserClient = NimbusHttpClient;
const _nativeHttpClient = new NimbusHttpClient("http://localhost:8080/nimbus/demo", {
  skipDeploymentUrlCheck: true,
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
