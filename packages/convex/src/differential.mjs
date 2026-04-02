import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { build } from "esbuild";

const cliPath = fileURLToPath(new URL("./cli.mjs", import.meta.url));
const packageRoot = fileURLToPath(new URL("../", import.meta.url));
const repoRoot = fileURLToPath(new URL("../../../", import.meta.url));
const fixtureTemplateDir = fileURLToPath(
  new URL("../fixtures/differential_app", import.meta.url),
);
const officialConvexBrowserEntrySuffix = path.join(
  "convex-backend",
  "npm-packages",
  "convex",
  "src",
  "browser",
  "index.ts",
);

export const SUPPORTED_DIFFERENTIAL_SURFACES = Object.freeze([
  "mutation",
  "query",
  "paginated_query",
  "subscription",
]);

export function assertSupportedDifferentialSurface(surface) {
  if (SUPPORTED_DIFFERENTIAL_SURFACES.includes(surface)) {
    return;
  }
  throw new Error(
    `Surface "${surface}" is outside the documented supported differential subset. Supported surfaces: ${SUPPORTED_DIFFERENTIAL_SURFACES.join(", ")}.`,
  );
}

export function normalizeDifferentialMessage(message) {
  return {
    author: message.author,
    body: message.body,
    rank: message.rank,
  };
}

export function normalizeDifferentialMessages(messages) {
  return messages.map(normalizeDifferentialMessage);
}

export function normalizeDifferentialPage(page) {
  if (Array.isArray(page?.data)) {
    return {
      data: normalizeDifferentialMessages(page.data),
      has_more: Boolean(page.has_more),
      cursor_present: Boolean(page.has_more),
    };
  }
  if (Array.isArray(page?.page)) {
    return {
      data: normalizeDifferentialMessages(page.page),
      has_more: !Boolean(page.isDone),
      cursor_present: !Boolean(page.isDone),
    };
  }
  throw new Error(
    `Unsupported paginated result shape for differential comparison: ${JSON.stringify(page)}`,
  );
}

export async function emitDifferentialFixture(destinationDir) {
  await fs.mkdir(path.dirname(destinationDir), { recursive: true });
  await fs.cp(fixtureTemplateDir, destinationDir, { recursive: true });
}

const DIFFERENTIAL_COMPARISON_PATHS = Object.freeze([
  ["query"],
  ["paginated", "first"],
  ["paginated", "second"],
  ["subscription", "initial"],
  ["subscription", "after_mutation"],
]);

export function collectDifferentialMismatches(actual, expected) {
  const mismatches = [];
  for (const pathSegments of DIFFERENTIAL_COMPARISON_PATHS) {
    const pathLabel = pathSegments.join(".");
    const actualValue = valueAtPath(actual, pathSegments);
    const expectedValue = valueAtPath(expected, pathSegments);
    try {
      assert.deepEqual(actualValue, expectedValue);
    } catch {
      mismatches.push({
        path: pathLabel,
        actual: actualValue,
        expected: expectedValue,
      });
    }
  }
  return mismatches;
}

export function formatDifferentialMismatchReport(mismatches) {
  return mismatches
    .map(
      ({ path, actual, expected }) =>
        `[${path}]\nexpected:\n${formatDifferentialValue(expected)}\nactual:\n${formatDifferentialValue(actual)}`,
    )
    .join("\n\n");
}

function valueAtPath(value, pathSegments) {
  let current = value;
  for (const segment of pathSegments) {
    if (current === null || current === undefined || typeof current !== "object") {
      return undefined;
    }
    current = current[segment];
  }
  return current;
}

function formatDifferentialValue(value) {
  if (value === undefined) {
    return "undefined";
  }
  return JSON.stringify(value, null, 2);
}

function parseCliArgs(argv) {
  const parsed = {
    neovexOnly: false,
    requireExternal: false,
    emitFixtureDir: null,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--neovex-only") {
      parsed.neovexOnly = true;
      continue;
    }
    if (arg === "--require-external") {
      parsed.requireExternal = true;
      continue;
    }
    if (arg === "--emit-fixture-dir") {
      parsed.emitFixtureDir = argv[index + 1] ?? null;
      index += 1;
      continue;
    }
    throw new Error(
      `Unsupported differential argument "${arg}". Supported flags: --neovex-only, --require-external, --emit-fixture-dir <dir>.`,
    );
  }

  if (parsed.emitFixtureDir === null && argv.includes("--emit-fixture-dir")) {
    throw new Error("--emit-fixture-dir requires a destination path.");
  }

  return parsed;
}

async function loadBundledModule(entryPath, outdirPrefix) {
  const outdir = await fs.mkdtemp(path.join(os.tmpdir(), outdirPrefix));
  const outfile = path.join(outdir, "bundle.mjs");
  await build({
    entryPoints: [entryPath],
    bundle: true,
    format: "esm",
    platform: "browser",
    outfile,
    logLevel: "silent",
  });
  return import(pathToFileURL(outfile).href);
}

async function reservePort() {
  return await new Promise((resolve, reject) => {
    const server = net.createServer();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        reject(new Error("failed to reserve a TCP port"));
        return;
      }
      const { port } = address;
      server.close((error) => {
        if (error) {
          reject(error);
          return;
        }
        resolve(port);
      });
    });
  });
}

async function waitForHealth(url, child, stderrBuffer) {
  const startedAt = Date.now();
  while (Date.now() - startedAt < 30_000) {
    if (child.exitCode !== null) {
      throw new Error(
        `Neovex server exited before becoming healthy.\n${stderrBuffer.join("")}`,
      );
    }

    try {
      const response = await fetch(url);
      if (response.ok) {
        return;
      }
    } catch {
      // Ignore startup races.
    }

    await delay(100);
  }

  throw new Error(
    `Timed out waiting for ${url} to become healthy.\n${stderrBuffer.join("")}`,
  );
}

async function startNeovexFixtureServer(appDir) {
  const port = await reservePort();
  const dataDir = await fs.mkdtemp(path.join(os.tmpdir(), "neovex-convex-diff-data-"));
  const stderrBuffer = [];
  const child = spawn(
    "cargo",
    [
      "run",
      "-p",
      "neovex-bin",
      "--",
      "--port",
      String(port),
      "--data-dir",
      dataDir,
      "--convex-app-dir",
      appDir,
    ],
    {
      cwd: repoRoot,
      stdio: ["ignore", "ignore", "pipe"],
    },
  );
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk) => {
    stderrBuffer.push(chunk);
    while (stderrBuffer.length > 40) {
      stderrBuffer.shift();
    }
  });

  const baseUrl = `http://127.0.0.1:${port}`;
  await waitForHealth(`${baseUrl}/health`, child, stderrBuffer);

  const tenantResponse = await fetch(`${baseUrl}/api/tenants`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ id: "demo" }),
  });
  if (!tenantResponse.ok) {
    throw new Error(
      `failed to create differential tenant on Neovex: ${tenantResponse.status} ${await tenantResponse.text()}`,
    );
  }

  return {
    deploymentUrl: `${baseUrl}/convex/demo`,
    async shutdown() {
      child.kill("SIGTERM");
      await new Promise((resolve) => {
        const timeout = setTimeout(() => {
          if (child.exitCode === null) {
            child.kill("SIGKILL");
          }
        }, 5_000);
        child.once("exit", () => {
          clearTimeout(timeout);
          resolve();
        });
      });
      await fs.rm(dataDir, { recursive: true, force: true });
    },
  };
}

async function prepareNeovexAppFixture() {
  const appDir = await fs.mkdtemp(path.join(os.tmpdir(), "neovex-convex-diff-app-"));
  await emitDifferentialFixture(appDir);
  const result = spawnSync(process.execPath, [cliPath, "codegen", "--app", appDir], {
    encoding: "utf8",
    cwd: repoRoot,
  });
  if (result.status !== 0) {
    throw new Error(result.stderr || result.stdout || "neovex codegen failed");
  }
  return appDir;
}

async function resolveOfficialConvexBrowserEntry() {
  if (process.env.NEOVEX_CONVEX_DIFF_OFFICIAL_BROWSER_ENTRY) {
    return process.env.NEOVEX_CONVEX_DIFF_OFFICIAL_BROWSER_ENTRY;
  }

  const directSibling = path.resolve(repoRoot, "..", officialConvexBrowserEntrySuffix);
  if (await pathExists(directSibling)) {
    return directSibling;
  }

  const organizationRoot = path.resolve(repoRoot, "..", "..");
  try {
    const organizations = await fs.readdir(organizationRoot, { withFileTypes: true });
    for (const organization of organizations) {
      if (!organization.isDirectory()) {
        continue;
      }
      const candidate = path.join(
        organizationRoot,
        organization.name,
        officialConvexBrowserEntrySuffix,
      );
      if (await pathExists(candidate)) {
        return candidate;
      }
    }
  } catch (error) {
    if (error?.code !== "ENOENT") {
      throw error;
    }
  }

  return null;
}

async function pathExists(targetPath) {
  try {
    await fs.access(targetPath);
    return true;
  } catch (error) {
    if (error?.code === "ENOENT") {
      return false;
    }
    throw error;
  }
}

async function resolveExternalTargetConfig() {
  const deploymentUrl =
    process.env.NEOVEX_CONVEX_DIFF_EXTERNAL_URL ?? process.env.CONVEX_SELF_HOSTED_URL ?? null;
  const officialBrowserEntry = await resolveOfficialConvexBrowserEntry();
  return {
    deploymentUrl,
    officialBrowserEntry,
    officialPackageRoot:
      officialBrowserEntry === null
        ? null
        : path.resolve(path.dirname(officialBrowserEntry), "../.."),
  };
}

function neovexFunctionRefs(browserModule) {
  return {
    byAuthor: browserModule.makeQueryReference("messages:byAuthor"),
    listPage: browserModule.makeQueryReference("messages:listPage"),
    send: browserModule.makeMutationReference("messages:send"),
  };
}

async function waitForSubscriptionStates(subscribe, mutate, timeoutMs = 10_000) {
  const updates = [];
  let resolveInitial;
  let resolveAfterMutation;
  let rejectSubscription;
  const initial = new Promise((resolve, reject) => {
    resolveInitial = resolve;
    rejectSubscription = reject;
  });
  const afterMutation = new Promise((resolve, reject) => {
    resolveAfterMutation = resolve;
    rejectSubscription = reject;
  });

  const unsubscribe = subscribe(
    (value) => {
      const normalized = normalizeDifferentialMessages(value);
      const previous = updates.at(-1);
      if (JSON.stringify(previous) === JSON.stringify(normalized)) {
        return;
      }
      updates.push(normalized);
      if (updates.length === 1) {
        resolveInitial(normalized);
      } else if (normalized.length >= 3) {
        resolveAfterMutation(normalized);
      }
    },
    (error) => {
      rejectSubscription(error);
    },
  );

  try {
    const initialValue = await withTimeout(initial, timeoutMs, "initial subscription result");
    await mutate();
    const afterMutationValue = await withTimeout(
      afterMutation,
      timeoutMs,
      "post-mutation subscription result",
    );
    return {
      initial: initialValue,
      after_mutation: afterMutationValue,
    };
  } finally {
    unsubscribe();
  }
}

async function runNeovexDifferentialSubset(browserModule, deploymentUrl, runId) {
  SUPPORTED_DIFFERENTIAL_SURFACES.forEach(assertSupportedDifferentialSurface);
  const refs = neovexFunctionRefs(browserModule);
  const http = new browserModule.ConvexHttpClient(deploymentUrl, {
    skipConvexDeploymentUrlCheck: true,
  });
  const socket = new browserModule.ConvexClient(deploymentUrl, {
    skipConvexDeploymentUrlCheck: true,
  });
  const author = `${runId}:Ada`;

  await http.mutation(refs.send, { author, body: "alpha", rank: 1 });
  await http.mutation(refs.send, { author, body: "beta", rank: 2 });

  const queryResult = normalizeDifferentialMessages(
    await http.query(refs.byAuthor, { author }),
  );
  const firstPageRaw = await http.query(refs.listPage, {
    author,
    paginationOpts: { numItems: 1, cursor: null },
  });
  const secondPageRaw = await http.query(refs.listPage, {
    author,
    paginationOpts: { numItems: 1, cursor: firstPageRaw.continueCursor },
  });

  const subscription = await waitForSubscriptionStates(
    (onValue, onError) => socket.onUpdate(refs.byAuthor, { author }, onValue, onError),
    async () => {
      await http.mutation(refs.send, { author, body: "gamma", rank: 3 });
    },
  );

  return {
    query: queryResult,
    paginated: {
      first: normalizeDifferentialPage(firstPageRaw),
      second: normalizeDifferentialPage(secondPageRaw),
    },
    subscription,
  };
}

async function runOfficialConvexDifferentialSubset(browserModule, deploymentUrl, runId) {
  SUPPORTED_DIFFERENTIAL_SURFACES.forEach(assertSupportedDifferentialSurface);
  const http = new browserModule.ConvexHttpClient(deploymentUrl, {
    skipConvexDeploymentUrlCheck: true,
  });
  const socket = new browserModule.ConvexClient(deploymentUrl, {
    skipConvexDeploymentUrlCheck: true,
  });
  const author = `${runId}:Ada`;

  await http.mutation("messages:send", { author, body: "alpha", rank: 1 });
  await http.mutation("messages:send", { author, body: "beta", rank: 2 });

  const queryResult = normalizeDifferentialMessages(
    await http.query("messages:byAuthor", { author }),
  );
  const firstPageRaw = await http.query("messages:listPage", {
    author,
    paginationOpts: { numItems: 1, cursor: null },
  });
  const secondPageRaw = await http.query("messages:listPage", {
    author,
    paginationOpts: { numItems: 1, cursor: firstPageRaw.continueCursor },
  });

  const subscription = await waitForSubscriptionStates(
    (onValue, onError) => socket.onUpdate("messages:byAuthor", { author }, onValue, onError),
    async () => {
      await http.mutation("messages:send", { author, body: "gamma", rank: 3 });
    },
  );

  return {
    query: queryResult,
    paginated: {
      first: normalizeDifferentialPage(firstPageRaw),
      second: normalizeDifferentialPage(secondPageRaw),
    },
    subscription,
  };
}

async function prepareOfficialAppFixture(officialPackageRoot) {
  const appDir = await fs.mkdtemp(path.join(os.tmpdir(), "official-convex-diff-app-"));
  await emitDifferentialFixture(appDir);
  const nodeModulesDir = path.join(appDir, "node_modules");
  await fs.mkdir(nodeModulesDir, { recursive: true });
  await fs.symlink(officialPackageRoot, path.join(nodeModulesDir, "convex"), "dir");
  return appDir;
}

async function waitForOfficialFixtureReady(
  browserModule,
  deploymentUrl,
  child,
  outputBuffer,
) {
  const http = new browserModule.ConvexHttpClient(deploymentUrl, {
    skipConvexDeploymentUrlCheck: true,
  });
  const startedAt = Date.now();
  let lastErrorMessage = "deployment did not become ready";
  while (Date.now() - startedAt < 60_000) {
    if (child.exitCode !== null) {
      throw new Error(
        `Official Convex local deployment exited before becoming ready.\n${outputBuffer.join("")}`,
      );
    }
    try {
      await http.query("messages:byAuthor", { author: "readiness-check" });
      return;
    } catch (error) {
      lastErrorMessage = error instanceof Error ? error.message : String(error);
    }
    await delay(250);
  }
  throw new Error(
    `Timed out waiting for official Convex local deployment readiness: ${lastErrorMessage}\n${outputBuffer.join("")}`,
  );
}

async function startOfficialLocalDeployment(browserModule, officialPackageRoot) {
  const appDir = await prepareOfficialAppFixture(officialPackageRoot);
  const port = await reservePort();
  const sitePort = await reservePort();
  const outputBuffer = [];
  const child = spawn(
    process.execPath,
    [
      path.join(officialPackageRoot, "bin", "main.js"),
      "dev",
      "--typecheck",
      "disable",
      "--tail-logs",
      "disable",
      "--local",
      "--local-cloud-port",
      String(port),
      "--local-site-port",
      String(sitePort),
    ],
    {
      cwd: appDir,
      stdio: ["ignore", "pipe", "pipe"],
      env: {
        ...process.env,
        CONVEX_AGENT_MODE: "anonymous",
      },
    },
  );
  for (const stream of [child.stdout, child.stderr]) {
    stream.setEncoding("utf8");
    stream.on("data", (chunk) => {
      outputBuffer.push(chunk);
      while (outputBuffer.length > 80) {
        outputBuffer.shift();
      }
    });
  }

  const deploymentUrl = `http://127.0.0.1:${port}`;
  await waitForOfficialFixtureReady(browserModule, deploymentUrl, child, outputBuffer);

  return {
    deploymentUrl,
    async shutdown() {
      child.kill("SIGTERM");
      await new Promise((resolve) => {
        const timeout = setTimeout(() => {
          if (child.exitCode === null) {
            child.kill("SIGKILL");
          }
        }, 5_000);
        child.once("exit", () => {
          clearTimeout(timeout);
          resolve();
        });
      });
      await fs.rm(appDir, { recursive: true, force: true });
    },
  };
}

function withTimeout(promise, timeoutMs, description) {
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      setTimeout(() => reject(new Error(`timed out waiting for ${description}`)), timeoutMs);
    }),
  ]);
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function main() {
  const args = parseCliArgs(process.argv.slice(2));
  if (args.emitFixtureDir) {
    await emitDifferentialFixture(path.resolve(args.emitFixtureDir));
    console.log(`wrote supported Convex differential fixture to ${path.resolve(args.emitFixtureDir)}`);
    return;
  }

  const neovexBrowser = await loadBundledModule(
    fileURLToPath(new URL("./browser.ts", import.meta.url)),
    "neovex-convex-browser-",
  );

  const appDir = await prepareNeovexAppFixture();
  const server = await startNeovexFixtureServer(appDir);
  const runId = `diff-${Date.now()}`;

  try {
    const neovexResult = await runNeovexDifferentialSubset(
      neovexBrowser,
      server.deploymentUrl,
      runId,
    );
    console.log("Neovex supported Convex differential subset passed.");

    if (args.neovexOnly) {
      return;
    }

    const external = await resolveExternalTargetConfig();
    if (!external.officialBrowserEntry) {
      throw new Error(
        "External Convex target is configured, but the official Convex browser source was not found. Set NEOVEX_CONVEX_DIFF_OFFICIAL_BROWSER_ENTRY to a local convex-backend checkout.",
      );
    }

    const officialBrowser = await loadBundledModule(
      external.officialBrowserEntry,
      "official-convex-browser-",
    );
    const externalTarget =
      external.deploymentUrl !== null
        ? { deploymentUrl: external.deploymentUrl, shutdown: async () => {} }
        : external.officialPackageRoot !== null
          ? await startOfficialLocalDeployment(officialBrowser, external.officialPackageRoot)
          : null;

    if (externalTarget === null) {
      if (args.requireExternal) {
        throw new Error(
          "External Convex target is not configured and no nearby official convex-backend checkout was usable for an automatic local deployment.",
        );
      }
      console.log(
        "External Convex target not configured; skipping external comparison. Set CONVEX_SELF_HOSTED_URL or place a usable convex-backend checkout nearby to run the full differential suite.",
      );
      return;
    }

    try {
      const externalResult = await runOfficialConvexDifferentialSubset(
        officialBrowser,
        externalTarget.deploymentUrl,
        runId,
      );

      const mismatches = collectDifferentialMismatches(neovexResult, externalResult);
      if (mismatches.length > 0) {
        throw new Error(
          `Neovex diverged from the supported Convex differential subset across ${mismatches.length} semantic slice(s).\n\n${formatDifferentialMismatchReport(mismatches)}`,
        );
      }
      console.log("External Convex differential comparison passed.");
    } finally {
      await externalTarget.shutdown();
    }
  } finally {
    await server.shutdown();
    await fs.rm(appDir, { recursive: true, force: true });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error.message);
    process.exit(1);
  });
}
