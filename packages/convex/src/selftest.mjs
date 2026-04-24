import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import vm from "node:vm";

import { build } from "esbuild";
import { buildBrowserBundle } from "../build.mjs";
import {
  assertSupportedDifferentialSurface,
  collectDifferentialMismatches,
  emitDifferentialFixture,
  formatDifferentialMismatchReport,
  normalizeDifferentialPage,
} from "./differential.mjs";

const cliPath = fileURLToPath(new URL("./cli.mjs", import.meta.url));
const packageRoot = fileURLToPath(new URL("../", import.meta.url));
const tscPath = fileURLToPath(
  new URL("../../../node_modules/typescript/bin/tsc", import.meta.url),
);
const typecheckOnly = process.argv.includes("--typecheck-only");

async function main() {
  if (typecheckOnly) {
    await typecheckConvexSurface();
    return;
  }
  await testConvexCliCodegenSmoke();
  await typecheckConvexSurface();
  await testDifferentialSurfaceGuardrails();
  await testDifferentialPageNormalization();
  await testDifferentialMismatchAggregation();
  await testDifferentialFixtureEmission();
  await testIifeBundleBuild();
  const browserModule = await loadBundledBrowserModule();
  await testStringRefsAndAnyApiUseNamedRequests(browserModule);
  await testInjectedNodeSocketSupportsAnyApiSubscriptions(browserModule);
  await testNamedLiveQueriesRejectPaginationOptions(browserModule);
  await testHttpClientAuthFetcherRetriesUnauthorized(browserModule);
  await testSocketAuthenticatesBeforeSubscriptions(browserModule);
  await testSocketAuthErrorForcesTokenRefresh(browserModule);
  await testSocketSchedulesPreemptiveTokenRefresh(browserModule);
  await testReconnectResubscribesActiveQueries(browserModule);
  await testPaginatedSubscriptionsUseNamedConvexFlow(browserModule);
  await testPaginatedSubscriptionsCarryWindowSize(browserModule);
  await testPaginatedReconnectPreservesWindowSizeAndSuppressesUnchangedReplay(
    browserModule,
  );
  await testUnchangedSubscriptionResultsDoNotNotifyAgain(browserModule);
  await testReconnectDoesNotNotifyWhenResubscribedResultIsUnchanged(browserModule);
}

async function testConvexCliCodegenSmoke() {
  const appDir = await fs.mkdtemp(path.join(os.tmpdir(), "convex_cli_codegen_"));
  await fs.mkdir(path.join(appDir, "convex"), { recursive: true });
  await fs.writeFile(
    path.join(appDir, "convex", "messages.ts"),
    `
import { defineQuery } from "convex/browser";

export const list = defineQuery("messages:list", () => ({
  table: "messages",
  filters: [],
  order: null,
  limit: 5,
}));
`,
    "utf8",
  );

  const result = spawnSync(process.execPath, [cliPath, "codegen", "--app", appDir], {
    encoding: "utf8",
  });
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const generatedApi = await fs.readFile(
    path.join(appDir, "convex", "_generated", "api.ts"),
    "utf8",
  );
  const generatedServer = await fs.readFile(
    path.join(appDir, "convex", "_generated", "server.ts"),
    "utf8",
  );
  assert.match(
    generatedApi,
    /makeQueryReference<\{\}, unknown\[]>\("messages:list", "public"\)/,
  );
  assert.match(generatedApi, /from "convex\/browser"/);
  assert.match(generatedServer, /ActionCtx/);
  assert.match(generatedServer, /MutationCtx/);
  assert.match(generatedServer, /QueryCtx/);
  assert.match(generatedServer, /from "convex\/server"/);

  const neovexAppDir = await fs.mkdtemp(path.join(os.tmpdir(), "convex_cli_codegen_"));
  await fs.mkdir(path.join(neovexAppDir, "neovex"), { recursive: true });
  await fs.writeFile(
    path.join(neovexAppDir, "neovex", "messages.ts"),
    `
import { defineQuery } from "convex/browser";

export const list = defineQuery("messages:list", () => ({
  table: "messages",
  filters: [],
  order: null,
  limit: 5,
}));
`,
    "utf8",
  );

  const neovexResult = spawnSync(process.execPath, [cliPath, "codegen", "--app", neovexAppDir], {
    encoding: "utf8",
  });
  assert.equal(neovexResult.status, 0, neovexResult.stderr || neovexResult.stdout);

  const neovexGeneratedApi = await fs.readFile(
    path.join(neovexAppDir, "neovex", "_generated", "api.ts"),
    "utf8",
  );
  assert.match(
    neovexGeneratedApi,
    /makeQueryReference<\{\}, unknown\[]>\("messages:list", "public"\)/,
  );
  assert.match(neovexGeneratedApi, /from "neovex\/browser"/);
}

async function typecheckConvexSurface() {
  const fixtureDir = await fs.mkdtemp(path.join(os.tmpdir(), "convex-ts-"));
  const normalize = (target) => path.relative(fixtureDir, target).replaceAll("\\", "/");
  const browserEntry = normalize(path.join(packageRoot, "src", "browser.ts"));
  const reactEntry = normalize(path.join(packageRoot, "src", "react.ts"));
  const serverEntry = normalize(path.join(packageRoot, "src", "server.ts"));
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
            "convex/browser": [browserEntry],
            "convex/react": [reactEntry],
            "convex/server": [serverEntry],
            "convex/values": [valuesEntry],
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
	import { ConvexHttpClient, ConvexReactClient, anyApi } from "convex/browser";
import {
  ConvexProvider,
  ConvexProviderWithAuth,
  useConvex,
  useConvexAuth,
  useConvexConnectionState,
  type ConvexAuthState,
} from "convex/react";
	import {
	  action,
	  type ActionCtx,
	  httpAction,
	  paginationOptsValidator,
	  query,
	  type MutationCtx,
	  type QueryCtx,
	  type Auth,
	  type PaginationResult,
	  type UserIdentity,
} from "convex/server";
import { v } from "convex/values";

const _convexHttpClient = new ConvexHttpClient("http://localhost:8080/convex/demo", {
  skipConvexDeploymentUrlCheck: true,
});
const _convexReactClient = new ConvexReactClient("http://localhost:8080/convex/demo", {
  skipConvexDeploymentUrlCheck: true,
});
const _provider = ConvexProvider;
const _providerWithAuth = ConvexProviderWithAuth;
const _useConvex = useConvex;
	const _useConvexAuth = useConvexAuth;
	const _useConvexConnectionState = useConvexConnectionState;
	const _authState = null as ConvexAuthState | null;
	const _anyApi = anyApi;

	declare const auth: Auth;
	declare const queryCtx: QueryCtx;
	declare const mutationCtx: MutationCtx;
	declare const actionCtx: ActionCtx;
	declare const identity: UserIdentity | null;

	const _updatedAt: string | undefined = identity?.updatedAt;
	void auth;
	void queryCtx;
	void mutationCtx;
	void actionCtx;
	void _anyApi;

export const whoami = query({
  args: {
    id: v.string(),
  },
  returns: v.string(),
  async handler(ctx, args) {
    const user = await ctx.auth.getUserIdentity();
    const _userUpdatedAt: string | undefined = user?.updatedAt;
    return args.id;
  },
});

export const runIdentityAction = action({
  async handler(ctx) {
    const user = await ctx.auth.getUserIdentity();
    return user?.tokenIdentifier ?? null;
  },
});

export const identityHttp = httpAction(async (ctx) => {
  const user = await ctx.auth.getUserIdentity();
  return new Response(user?.tokenIdentifier ?? "anonymous");
});

	export const paginatedWhoami = query({
  args: {
    id: v.string(),
    paginationOpts: paginationOptsValidator,
  },
	  async handler(ctx, args) {
	    const page = await ctx.db.query("messages").paginate(args.paginationOpts);
	    const _page: PaginationResult<unknown> = page;
	    return page;
	  },
	});

	async function exerciseNamedRefs() {
	  await _convexHttpClient.query("messages:list", {});
	  await _convexHttpClient.query(anyApi.messages.list, {});
	  await _convexHttpClient.scheduleAfter("messages:send", {}, 10);
	}

	void exerciseNamedRefs;
	`,
    "utf8",
  );

  const result = spawnSync(process.execPath, [tscPath, "-p", path.join(fixtureDir, "tsconfig.json")], {
    encoding: "utf8",
    cwd: fixtureDir,
  });
  assert.equal(result.status, 0, result.stderr || result.stdout);
}

async function loadBundledBrowserModule() {
  const outdir = await fs.mkdtemp(path.join(os.tmpdir(), "neovex-convex-browser-"));
  const outfile = path.join(outdir, "browser.mjs");
  await build({
    entryPoints: [fileURLToPath(new URL("./browser.ts", import.meta.url))],
    bundle: true,
    format: "esm",
    platform: "browser",
    outfile,
    logLevel: "silent",
  });
  return import(pathToFileURL(outfile).href);
}

async function testDifferentialSurfaceGuardrails() {
  assert.doesNotThrow(() => assertSupportedDifferentialSurface("query"));
  assert.throws(
    () => assertSupportedDifferentialSurface("action"),
    /outside the documented supported differential subset/,
  );
}

async function testDifferentialPageNormalization() {
  const expected = {
    data: [
      { author: "Ada", body: "alpha", rank: 1 },
      { author: "Ada", body: "beta", rank: 2 },
    ],
    has_more: true,
    cursor_present: true,
  };

  assert.deepEqual(
    normalizeDifferentialPage({
      data: [
        { _id: "m1", author: "Ada", body: "alpha", rank: 1 },
        { _id: "m2", author: "Ada", body: "beta", rank: 2 },
      ],
      has_more: true,
      next_cursor: "cursor-1",
    }),
    expected,
  );

  assert.deepEqual(
    normalizeDifferentialPage({
      page: [
        { _id: "m1", author: "Ada", body: "alpha", rank: 1 },
        { _id: "m2", author: "Ada", body: "beta", rank: 2 },
      ],
      isDone: false,
      continueCursor: "cursor-1",
    }),
    expected,
  );
}

async function testDifferentialMismatchAggregation() {
  const actual = {
    query: [{ author: "Ada", body: "alpha", rank: 1 }],
    paginated: {
      first: {
        data: [{ author: "Ada", body: "alpha", rank: 1 }],
        has_more: true,
        cursor_present: true,
      },
      second: {
        data: [{ author: "Ada", body: "beta", rank: 2 }],
        has_more: false,
        cursor_present: false,
      },
    },
    subscription: {
      initial: [{ author: "Ada", body: "alpha", rank: 1 }],
      after_mutation: [
        { author: "Ada", body: "alpha", rank: 1 },
        { author: "Ada", body: "beta", rank: 2 },
      ],
    },
  };
  const expected = {
    query: [{ author: "Ada", body: "omega", rank: 1 }],
    paginated: {
      first: {
        data: [{ author: "Ada", body: "alpha", rank: 1 }],
        has_more: true,
        cursor_present: true,
      },
      second: {
        data: [{ author: "Ada", body: "beta", rank: 2 }],
        has_more: true,
        cursor_present: true,
      },
    },
    subscription: {
      initial: [{ author: "Ada", body: "alpha", rank: 1 }],
      after_mutation: [
        { author: "Ada", body: "alpha", rank: 1 },
        { author: "Ada", body: "beta", rank: 2 },
        { author: "Ada", body: "gamma", rank: 3 },
      ],
    },
  };

  const mismatches = collectDifferentialMismatches(actual, expected);
  assert.deepEqual(
    mismatches.map((mismatch) => mismatch.path),
    ["query", "paginated.second", "subscription.after_mutation"],
  );

  const report = formatDifferentialMismatchReport(mismatches);
  assert.match(report, /\[query\]/);
  assert.match(report, /\[paginated\.second\]/);
  assert.match(report, /\[subscription\.after_mutation\]/);
}

async function testDifferentialFixtureEmission() {
  const fixtureDir = await fs.mkdtemp(path.join(os.tmpdir(), "convex-differential-fixture-"));
  await emitDifferentialFixture(fixtureDir);

  const messagesSource = await fs.readFile(
    path.join(fixtureDir, "convex", "messages.ts"),
    "utf8",
  );
  const schemaSource = await fs.readFile(
    path.join(fixtureDir, "convex", "schema.ts"),
    "utf8",
  );

  assert.match(messagesSource, /export const byAuthor = query/);
  assert.match(messagesSource, /paginationOptsValidator/);
  assert.match(messagesSource, /export const listPage = query/);
  assert.match(messagesSource, /\.paginate\(paginationOpts\)/);
  assert.match(messagesSource, /export const send = mutation/);
  const packageJson = await fs.readFile(path.join(fixtureDir, "package.json"), "utf8");
  assert.match(packageJson, /"convex": "\*"/);
  assert.match(schemaSource, /\.index\("by_rank", \["rank"\]\)/);
  assert.match(schemaSource, /from "convex\/server"/);
}

async function testIifeBundleBuild() {
  const { distFile, servedFile } = await buildBrowserBundle();
  const bundleSource = await fs.readFile(servedFile, "utf8");
  const distSource = await fs.readFile(distFile, "utf8");
  const context = {};

  vm.runInNewContext(bundleSource, context);

  assert.equal(typeof context.convex.ConvexClient, "function");
  assert.equal(typeof context.convex.anyApi, "object");
  assert.match(distSource, /var convex/);
}

async function testStringRefsAndAnyApiUseNamedRequests(browserModule) {
  const { ConvexHttpClient, anyApi } = browserModule;
  const requests = [];
  const fetchImpl = async (url, init) => {
    requests.push({
      url: String(url),
      method: init?.method ?? "POST",
      body: init?.body ? JSON.parse(init.body) : null,
    });
    if (String(url).endsWith("/schedule/run_after") || String(url).endsWith("/schedule/run_at")) {
      return jsonResponse(200, { job_id: "job-1" });
    }
    return jsonResponse(200, { ok: true });
  };

  const client = new ConvexHttpClient("http://localhost:8080/convex/demo", {
    skipConvexDeploymentUrlCheck: true,
    fetch: fetchImpl,
  });

  await client.query("messages:list", { author: "Ada" });
  await client.mutation(anyApi.messages.send, { author: "Ada", body: "hello" });
  await client.action(anyApi.dashboard.messages.run, { author: "Ada" });
  await client.scheduleAfter("messages:send", { author: "Ada", body: "later" }, 50);
  await client.scheduleAt(anyApi.dashboard.messages.send, { author: "Ada", body: "at" }, 100);

  assert.deepEqual(requests, [
    {
      url: "http://localhost:8080/convex/demo/query",
      method: "POST",
      body: { name: "messages:list", args: { author: "Ada" } },
    },
    {
      url: "http://localhost:8080/convex/demo/mutation",
      method: "POST",
      body: { name: "messages:send", args: { author: "Ada", body: "hello" } },
    },
    {
      url: "http://localhost:8080/convex/demo/action",
      method: "POST",
      body: { name: "dashboard/messages:run", args: { author: "Ada" } },
    },
    {
      url: "http://localhost:8080/convex/demo/schedule/run_after",
      method: "POST",
      body: {
        name: "messages:send",
        args: { author: "Ada", body: "later" },
        run_after_ms: 50,
      },
    },
    {
      url: "http://localhost:8080/convex/demo/schedule/run_at",
      method: "POST",
      body: {
        name: "dashboard/messages:send",
        args: { author: "Ada", body: "at" },
        run_at_ms: 100,
      },
    },
  ]);
}

async function testInjectedNodeSocketSupportsAnyApiSubscriptions(browserModule) {
  const { ConvexClient, anyApi } = browserModule;
  FakeNodeWebSocket.reset();
  const values = [];

  const client = new ConvexClient("http://localhost:8080/convex/demo", {
    skipConvexDeploymentUrlCheck: true,
    webSocket: FakeNodeWebSocket,
  });

  client.onUpdate(anyApi.messages.list, {}, (value) => {
    values.push(value);
  });

  await delay(75);
  assert.equal(FakeNodeWebSocket.instances.length, 1);
  const socket = FakeNodeWebSocket.instances[0];
  socket.open();
  await delay(0);

  assert.deepEqual(socket.sent[0], {
    type: "subscribe_named",
    request_id: "convex-1",
    name: "messages:list",
    args: {},
  });

  socket.message({
    type: "subscription_result",
    request_id: "convex-1",
    subscription_id: 7,
    data: [{ body: "hello" }],
  });
  await delay(0);

  assert.deepEqual(values, [[{ body: "hello" }]]);
  client.close();
}

async function testNamedLiveQueriesRejectPaginationOptions(browserModule) {
  const { ConvexClient } = browserModule;
  const client = new ConvexClient("http://localhost:8080/convex/demo", {
    skipConvexDeploymentUrlCheck: true,
    disabled: true,
  });

  assert.throws(
    () =>
      client.onUpdate(
        "messages:listPage",
        {},
        () => {},
        undefined,
        { pageSize: 10, cursor: null },
      ),
    /do not support paginated live queries yet/,
  );
}

async function testReconnectResubscribesActiveQueries(browserModule) {
  const { ConvexClient, makeQueryReference } = browserModule;
  const originalWebSocket = globalThis.WebSocket;
  FakeWebSocket.reset();
  globalThis.WebSocket = FakeWebSocket;

  try {
    const query = makeQueryReference("messages:list");
    const client = new ConvexClient("http://localhost:8080/convex/demo", {
      skipConvexDeploymentUrlCheck: true,
    });
    const updates = [];
    const errors = [];

    const unsubscribe = client.onUpdate(
      query,
      { author: "Ada" },
      (value) => {
        updates.push(value);
      },
      (error) => {
        errors.push(error.message);
      },
    );

    await delay(75);
    assert.equal(FakeWebSocket.instances.length, 1);
    const firstSocket = FakeWebSocket.instances[0];
    assert.equal(
      firstSocket.url,
      "ws://localhost:8080/convex/demo/ws",
    );

    firstSocket.open();
    await delay(0);
    assert.equal(firstSocket.sent.length, 1);
    assert.equal(firstSocket.sent[0].type, "subscribe_named");
    assert.equal(firstSocket.sent[0].name, "messages:list");
    assert.deepEqual(firstSocket.sent[0].args, { author: "Ada" });

    const firstRequestId = firstSocket.sent[0].request_id;
    firstSocket.message({
      type: "subscription_result",
      request_id: firstRequestId,
      subscription_id: 7,
      data: [{ body: "first" }],
    });
    await delay(0);
    assert.deepEqual(updates, [[{ body: "first" }]]);
    assert.deepEqual(unsubscribe.getCurrentValue(), [{ body: "first" }]);

    firstSocket.close();
    await delay(75);
    assert.equal(FakeWebSocket.instances.length, 2);
    const secondSocket = FakeWebSocket.instances[1];

    secondSocket.open();
    await delay(0);
    assert.equal(secondSocket.sent.length, 1);
    assert.equal(secondSocket.sent[0].type, "subscribe_named");
    assert.equal(secondSocket.sent[0].name, "messages:list");
    assert.deepEqual(secondSocket.sent[0].args, { author: "Ada" });
    assert.notEqual(secondSocket.sent[0].request_id, firstRequestId);

    secondSocket.message({
      type: "subscription_result",
      request_id: secondSocket.sent[0].request_id,
      subscription_id: 11,
      data: [{ body: "second" }],
    });
    await delay(0);
    assert.deepEqual(updates, [[{ body: "first" }], [{ body: "second" }]]);
    assert.equal(client.connectionState().connectionCount, 2);
    assert.equal(client.connectionState().isWebSocketConnected, true);

    unsubscribe();
    assert.deepEqual(secondSocket.sent.at(-1), {
      type: "unsubscribe",
      subscription_id: 11,
    });

    client.close();
    assert.deepEqual(errors, []);
  } finally {
    globalThis.WebSocket = originalWebSocket;
  }
}

async function testHttpClientAuthFetcherRetriesUnauthorized(browserModule) {
  const { ConvexHttpClient, makeQueryReference } = browserModule;
  const observedHeaders = [];
  const authStates = [];
  const fetchImpl = async (_url, init) => {
    observedHeaders.push(init?.headers?.Authorization ?? null);
    if (init?.headers?.Authorization === "Bearer stale-token") {
      return jsonResponse(401, { error: "expired" });
    }
    return jsonResponse(200, [{ body: "ok" }]);
  };

  const client = new ConvexHttpClient("http://localhost:8080/convex/demo", {
    skipConvexDeploymentUrlCheck: true,
    fetch: fetchImpl,
  });
  client.setAuth(
    async ({ forceRefreshToken }) => (
      forceRefreshToken ? "fresh-token" : "stale-token"
    ),
    (isAuthenticated) => {
      authStates.push(isAuthenticated);
    },
  );

  const result = await client.query(makeQueryReference("messages:list"), {});
  assert.deepEqual(result, [{ body: "ok" }]);
  assert.deepEqual(observedHeaders, ["Bearer stale-token", "Bearer fresh-token"]);
  assert.equal(authStates.at(-1), true);
}

async function testSocketAuthenticatesBeforeSubscriptions(browserModule) {
  const { ConvexClient, makeQueryReference } = browserModule;
  const originalWebSocket = globalThis.WebSocket;
  FakeWebSocket.reset();
  globalThis.WebSocket = FakeWebSocket;

  try {
    const authTransitions = [];
    const client = new ConvexClient("http://localhost:8080/convex/demo", {
      skipConvexDeploymentUrlCheck: true,
    });
    client.setAuth(
      async () => "socket-token",
      (isAuthenticated) => {
        authTransitions.push(isAuthenticated);
      },
    );
    client.onUpdate(makeQueryReference("messages:list"), {}, () => {});

    await delay(75);
    const socket = FakeWebSocket.instances[0];
    socket.open();
    await delay(0);

    assert.deepEqual(socket.sent[0], {
      type: "authenticate",
      token: "socket-token",
    });
    assert.equal(socket.sent.length, 1);

    socket.message({ type: "authenticated", is_authenticated: true });
    await delay(0);

    assert.equal(socket.sent[1].type, "subscribe_named");
    assert.equal(socket.sent[1].name, "messages:list");
    assert.equal(authTransitions.at(-1), true);

    client.close();
  } finally {
    globalThis.WebSocket = originalWebSocket;
  }
}

async function testSocketAuthErrorForcesTokenRefresh(browserModule) {
  const { ConvexClient, makeQueryReference } = browserModule;
  const originalWebSocket = globalThis.WebSocket;
  FakeWebSocket.reset();
  globalThis.WebSocket = FakeWebSocket;

  try {
    const authRequests = [];
    const authTransitions = [];
    const client = new ConvexClient("http://localhost:8080/convex/demo", {
      skipConvexDeploymentUrlCheck: true,
    });
    client.setAuth(
      async ({ forceRefreshToken }) => {
        authRequests.push(forceRefreshToken);
        return forceRefreshToken ? "fresh-token" : "stale-token";
      },
      (isAuthenticated) => {
        authTransitions.push(isAuthenticated);
      },
    );
    client.onUpdate(makeQueryReference("messages:list"), {}, () => {});

    await delay(75);
    const socket = FakeWebSocket.instances[0];
    socket.open();
    await delay(0);
    assert.deepEqual(socket.sent[0], {
      type: "authenticate",
      token: "stale-token",
    });

    socket.message({ type: "auth_error", message: "expired" });
    await delay(0);
    assert.deepEqual(socket.sent[1], {
      type: "authenticate",
      token: "fresh-token",
    });

    socket.message({ type: "authenticated", is_authenticated: true });
    await delay(0);

    assert.deepEqual(authRequests, [false, true]);
    assert.equal(socket.sent[2].type, "subscribe_named");
    assert.equal(authTransitions.at(-1), true);

    client.close();
  } finally {
    globalThis.WebSocket = originalWebSocket;
  }
}

async function testSocketSchedulesPreemptiveTokenRefresh(browserModule) {
  const { ConvexClient, makeQueryReference } = browserModule;
  const originalWebSocket = globalThis.WebSocket;
  FakeWebSocket.reset();
  globalThis.WebSocket = FakeWebSocket;

  try {
    const issuedAt = Math.floor(Date.now() / 1000);
    const initialToken = makeJwt({
      sub: "user-123",
      iat: issuedAt,
      exp: issuedAt + 3,
    });
    const refreshedToken = makeJwt({
      sub: "user-123",
      iat: issuedAt + 1,
      exp: issuedAt + 301,
    });
    const authRequests = [];
    const client = new ConvexClient("http://localhost:8080/convex/demo", {
      skipConvexDeploymentUrlCheck: true,
      authRefreshTokenLeewaySeconds: 3,
    });
    client.setAuth(async ({ forceRefreshToken }) => {
      authRequests.push(forceRefreshToken);
      return forceRefreshToken ? refreshedToken : initialToken;
    });
    client.onUpdate(makeQueryReference("messages:list"), {}, () => {});

    await delay(75);
    const socket = FakeWebSocket.instances[0];
    socket.open();
    await delay(0);

    assert.deepEqual(socket.sent[0], {
      type: "authenticate",
      token: initialToken,
    });

    socket.message({ type: "authenticated", is_authenticated: true });
    await delay(10);

    assert.deepEqual(socket.sent.at(-1), {
      type: "authenticate",
      token: refreshedToken,
    });

    socket.message({ type: "authenticated", is_authenticated: true });
    await delay(0);

    assert.deepEqual(authRequests, [false, true]);

    client.close();
  } finally {
    globalThis.WebSocket = originalWebSocket;
  }
}

async function testPaginatedSubscriptionsUseNamedConvexFlow(browserModule) {
  const { ConvexClient, makePaginatedQueryReference } = browserModule;
  const originalWebSocket = globalThis.WebSocket;
  FakeWebSocket.reset();
  globalThis.WebSocket = FakeWebSocket;

  try {
    const query = makePaginatedQueryReference("messages:listPage");
    const client = new ConvexClient("http://localhost:8080/convex/demo", {
      skipConvexDeploymentUrlCheck: true,
    });
    const updates = [];

    const unsubscribe = client.onUpdate(query, {}, (value) => {
      updates.push(value);
    });

    await delay(75);
    assert.equal(FakeWebSocket.instances.length, 1);
    const socket = FakeWebSocket.instances[0];
    socket.open();
    await delay(0);
    assert.equal(socket.sent.length, 1);
    assert.equal(socket.sent[0].type, "subscribe_named");
    assert.equal(socket.sent[0].name, "messages:listPage");

    socket.message({
      type: "subscription_result",
      request_id: socket.sent[0].request_id,
      subscription_id: 5,
      data: [{ body: "page item" }],
    });
    await delay(0);
    assert.deepEqual(updates, [[{ body: "page item" }]]);

    unsubscribe();
    client.close();
  } finally {
    globalThis.WebSocket = originalWebSocket;
  }
}

async function testPaginatedSubscriptionsCarryWindowSize(browserModule) {
  const { ConvexClient, makePaginatedQueryReference } = browserModule;
  const originalWebSocket = globalThis.WebSocket;
  FakeWebSocket.reset();
  globalThis.WebSocket = FakeWebSocket;

  try {
    const query = makePaginatedQueryReference("messages:listPage");
    const client = new ConvexClient("http://localhost:8080/convex/demo", {
      skipConvexDeploymentUrlCheck: true,
    });

    const unsubscribe = client.onUpdate(
      query,
      {},
      () => {},
      undefined,
      { pageSize: 7, cursor: null },
    );

    await delay(75);
    assert.equal(FakeWebSocket.instances.length, 1);
    const socket = FakeWebSocket.instances[0];
    socket.open();
    await delay(0);
    assert.equal(socket.sent.length, 1);
    assert.equal(socket.sent[0].type, "subscribe_named");
    assert.equal(socket.sent[0].name, "messages:listPage");
    assert.equal(socket.sent[0].page_size, 7);
    assert.equal(socket.sent[0].cursor, null);

    unsubscribe();
    client.close();
  } finally {
    globalThis.WebSocket = originalWebSocket;
  }
}

async function testPaginatedReconnectPreservesWindowSizeAndSuppressesUnchangedReplay(
  browserModule,
) {
  const { ConvexClient, makePaginatedQueryReference } = browserModule;
  const originalWebSocket = globalThis.WebSocket;
  FakeWebSocket.reset();
  globalThis.WebSocket = FakeWebSocket;

  try {
    const query = makePaginatedQueryReference("messages:listPage");
    const client = new ConvexClient("http://localhost:8080/convex/demo", {
      skipConvexDeploymentUrlCheck: true,
    });
    const updates = [];

    const unsubscribe = client.onUpdate(
      query,
      { author: "Ada" },
      (value) => {
        updates.push(value);
      },
      undefined,
      { pageSize: 3, cursor: null },
    );

    await delay(75);
    const firstSocket = FakeWebSocket.instances[0];
    firstSocket.open();
    await delay(0);

    assert.equal(firstSocket.sent[0].type, "subscribe_named");
    assert.equal(firstSocket.sent[0].name, "messages:listPage");
    assert.equal(firstSocket.sent[0].page_size, 3);

    firstSocket.message({
      type: "subscription_result",
      request_id: firstSocket.sent[0].request_id,
      subscription_id: 13,
      data: [{ body: "stable page item" }],
    });
    await delay(0);

    firstSocket.close();
    await delay(75);
    const secondSocket = FakeWebSocket.instances[1];
    secondSocket.open();
    await delay(0);

    assert.equal(secondSocket.sent[0].type, "subscribe_named");
    assert.equal(secondSocket.sent[0].name, "messages:listPage");
    assert.equal(secondSocket.sent[0].page_size, 3);
    assert.equal(secondSocket.sent[0].cursor, null);

    secondSocket.message({
      type: "subscription_result",
      request_id: secondSocket.sent[0].request_id,
      subscription_id: 14,
      data: [{ body: "stable page item" }],
    });
    await delay(0);

    assert.deepEqual(updates, [[{ body: "stable page item" }]]);

    unsubscribe();
    client.close();
  } finally {
    globalThis.WebSocket = originalWebSocket;
  }
}

async function testUnchangedSubscriptionResultsDoNotNotifyAgain(browserModule) {
  const { ConvexClient, makeQueryReference } = browserModule;
  const originalWebSocket = globalThis.WebSocket;
  FakeWebSocket.reset();
  globalThis.WebSocket = FakeWebSocket;

  try {
    const query = makeQueryReference("messages:list");
    const client = new ConvexClient("http://localhost:8080/convex/demo", {
      skipConvexDeploymentUrlCheck: true,
    });
    const updates = [];

    const unsubscribe = client.onUpdate(query, {}, (value) => {
      updates.push(value);
    });

    await delay(75);
    const socket = FakeWebSocket.instances[0];
    socket.open();
    await delay(0);

    socket.message({
      type: "subscription_result",
      request_id: socket.sent[0].request_id,
      subscription_id: 9,
      data: [{ body: "same" }],
    });
    await delay(0);

    socket.message({
      type: "subscription_result",
      subscription_id: 9,
      data: [{ body: "same" }],
    });
    await delay(0);

    socket.message({
      type: "subscription_result",
      subscription_id: 9,
      data: [{ body: "changed" }],
    });
    await delay(0);

    assert.deepEqual(updates, [[{ body: "same" }], [{ body: "changed" }]]);

    unsubscribe();
    client.close();
  } finally {
    globalThis.WebSocket = originalWebSocket;
  }
}

async function testReconnectDoesNotNotifyWhenResubscribedResultIsUnchanged(browserModule) {
  const { ConvexClient, makeQueryReference } = browserModule;
  const originalWebSocket = globalThis.WebSocket;
  FakeWebSocket.reset();
  globalThis.WebSocket = FakeWebSocket;

  try {
    const query = makeQueryReference("messages:list");
    const client = new ConvexClient("http://localhost:8080/convex/demo", {
      skipConvexDeploymentUrlCheck: true,
    });
    const updates = [];

    const unsubscribe = client.onUpdate(query, {}, (value) => {
      updates.push(value);
    });

    await delay(75);
    const firstSocket = FakeWebSocket.instances[0];
    firstSocket.open();
    await delay(0);

    firstSocket.message({
      type: "subscription_result",
      request_id: firstSocket.sent[0].request_id,
      subscription_id: 3,
      data: [{ body: "stable" }],
    });
    await delay(0);

    firstSocket.close();
    await delay(75);
    const secondSocket = FakeWebSocket.instances[1];
    secondSocket.open();
    await delay(0);

    secondSocket.message({
      type: "subscription_result",
      request_id: secondSocket.sent[0].request_id,
      subscription_id: 4,
      data: [{ body: "stable" }],
    });
    await delay(0);

    assert.deepEqual(updates, [[{ body: "stable" }]]);

    unsubscribe();
    client.close();
  } finally {
    globalThis.WebSocket = originalWebSocket;
  }
}

function delay(ms) {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

function jsonResponse(status, payload) {
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: {
      get(name) {
        return name.toLowerCase() === "content-type" ? "application/json" : null;
      },
    },
    async json() {
      return payload;
    },
    async text() {
      return JSON.stringify(payload);
    },
  };
}

function makeJwt(payload) {
  const encode = (value) =>
    Buffer.from(JSON.stringify(value)).toString("base64url");
  return `${encode({ alg: "RS256", typ: "JWT" })}.${encode(payload)}.signature`;
}

class FakeWebSocket {
  static instances = [];

  static reset() {
    FakeWebSocket.instances = [];
  }

  constructor(url) {
    this.url = url;
    this.sent = [];
    this.listeners = new Map();
    FakeWebSocket.instances.push(this);
  }

  addEventListener(type, listener) {
    const listeners = this.listeners.get(type) ?? [];
    listeners.push(listener);
    this.listeners.set(type, listeners);
  }

  send(payload) {
    this.sent.push(JSON.parse(payload));
  }

  close() {
    this.dispatch("close", {});
  }

  open() {
    this.dispatch("open", {});
  }

  message(payload) {
    this.dispatch("message", { data: JSON.stringify(payload) });
  }

  dispatch(type, event) {
    for (const listener of this.listeners.get(type) ?? []) {
      listener(event);
    }
  }
}

class FakeNodeWebSocket {
  static instances = [];

  static reset() {
    FakeNodeWebSocket.instances = [];
  }

  constructor(url) {
    this.url = url;
    this.sent = [];
    this.listeners = new Map();
    FakeNodeWebSocket.instances.push(this);
  }

  on(type, listener) {
    const listeners = this.listeners.get(type) ?? [];
    listeners.push(listener);
    this.listeners.set(type, listeners);
  }

  send(payload) {
    this.sent.push(JSON.parse(payload));
  }

  close() {
    this.dispatch("close", undefined);
  }

  open() {
    this.dispatch("open", undefined);
  }

  message(payload) {
    this.dispatch("message", JSON.stringify(payload));
  }

  dispatch(type, event) {
    for (const listener of this.listeners.get(type) ?? []) {
      listener(event);
    }
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
