import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";

import { encodeEnvelope } from "@connectrpc/connect/protocol";
import { trailerFlag, trailerSerialize } from "@connectrpc/connect/protocol-grpc-web";
import { build } from "esbuild";

const require = createRequire(import.meta.url);
const packageRoot = fileURLToPath(new URL("../", import.meta.url));
const packageJsonPath = fileURLToPath(new URL("../package.json", import.meta.url));
const tscPath = fileURLToPath(
  new URL("../../../node_modules/typescript/bin/tsc", import.meta.url),
);
const buildOnly = process.argv.includes("--build-only");
const typecheckOnly = process.argv.includes("--typecheck-only");
const smokeBaseUrl = optionalFlagValue("--smoke-base-url");

function optionalFlagValue(flag) {
  const index = process.argv.indexOf(flag);
  if (index === -1) {
    return null;
  }
  const value = process.argv[index + 1];
  assert.ok(value, `${flag} requires a value.`);
  return value;
}

async function main() {
  await assertPackageExports();
  await assertGeneratedProtoSurface();
  if (buildOnly) {
    await buildPackageSurface();
    return;
  }
  if (typecheckOnly) {
    await typecheckFirebaseSurface();
    return;
  }

  const bundleDir = await buildPackageSurface();
  if (smokeBaseUrl) {
    await testSmokeSurface(bundleDir, smokeBaseUrl);
    return;
  }

  await testRuntimeSurface(bundleDir);
  await typecheckFirebaseSurface();
}

async function assertPackageExports() {
  const packageJson = JSON.parse(await fs.readFile(packageJsonPath, "utf8"));
  assert.equal(packageJson.name, "@nimbus/firebase");
  assert.deepEqual(packageJson.exports, {
    ".": "./src/index.ts",
    "./app": "./src/app.ts",
    "./firestore": "./src/firestore.ts",
  });
}

async function buildPackageSurface() {
  const bundleDir = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-firebase-package-"));
  for (const entry of [
    { name: "app", source: "./app.ts" },
    { name: "firestore", source: "./firestore.ts" },
    { name: "index", source: "./index.ts" },
    { name: "internal-protobuf", source: "./internal/protobuf.ts" },
  ]) {
    const source = fileURLToPath(new URL(entry.source, import.meta.url));
    await buildEntry(source, path.join(bundleDir, `${entry.name}.mjs`), "esm");
    await buildEntry(source, path.join(bundleDir, `${entry.name}.cjs`), "cjs");
  }
  return bundleDir;
}

async function buildEntry(entryPoint, outfile, format) {
  await build({
    entryPoints: [entryPoint],
    bundle: true,
    format,
    outfile,
    logLevel: "silent",
    platform: "neutral",
    target: "es2022",
  });
}

class FakeWebSocket {
  constructor(url, protocols = []) {
    this.url = url;
    this.protocols = Array.isArray(protocols)
      ? [...protocols]
      : protocols
        ? [protocols]
        : [];
    this.binaryType = "blob";
    this.closed = false;
    this.closeCalls = [];
    this.listeners = new Map();
    this.sentFrames = [];
  }

  addEventListener(type, listener) {
    const listeners = this.listeners.get(type) ?? [];
    listeners.push(listener);
    this.listeners.set(type, listeners);
  }

  send(data) {
    this.sentFrames.push(normalizeWebSocketBinaryFrame(data));
  }

  close(code, reason) {
    this.closed = true;
    this.closeCalls.push({ code: code ?? null, reason: reason ?? null });
  }

  emitOpen() {
    this.#emit("open", { type: "open" });
  }

  emitBinary(data) {
    this.#emit("message", { data });
  }

  emitClose(code = 1000, reason = "") {
    this.closed = true;
    this.#emit("close", { code, reason });
  }

  emitError(error = new Error("socket error")) {
    this.#emit("error", error);
  }

  #emit(type, event) {
    for (const listener of this.listeners.get(type) ?? []) {
      listener(event);
    }
  }
}

function normalizeWebSocketBinaryFrame(data) {
  if (data instanceof Uint8Array) {
    return data;
  }
  if (ArrayBuffer.isView(data)) {
    return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  }
  if (data instanceof ArrayBuffer) {
    return new Uint8Array(data);
  }
  throw new Error(`Expected binary WebSocket frame, received ${typeof data}.`);
}

function decodeListenAuthSubprotocol(protocols) {
  const offered = protocols.find((protocol) =>
    protocol.startsWith("nimbus.firebase.auth."),
  );
  if (!offered) {
    return null;
  }
  const encoded = offered.slice("nimbus.firebase.auth.".length);
  return new TextDecoder().decode(Buffer.from(encoded, "base64url"));
}

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, reject, resolve };
}

async function flushMicrotasks() {
  await Promise.resolve();
  await Promise.resolve();
}

async function withImmediateTimeouts(run) {
  const originalSetTimeout = globalThis.setTimeout;
  const originalClearTimeout = globalThis.clearTimeout;
  const cancelled = new Set();
  const scheduledDelays = [];
  let nextId = 1;

  globalThis.setTimeout = ((handler, delay = 0, ...args) => {
    const id = nextId;
    nextId += 1;
    scheduledDelays.push(Number(delay));
    queueMicrotask(() => {
      if (cancelled.has(id)) {
        return;
      }
      if (typeof handler === "function") {
        handler(...args);
        return;
      }
      throw new Error("String-based timeouts are not supported in the Firebase selftest.");
    });
    return id;
  });
  globalThis.clearTimeout = ((id) => {
    cancelled.add(id);
  });

  try {
    return await run(scheduledDelays);
  } finally {
    globalThis.setTimeout = originalSetTimeout;
    globalThis.clearTimeout = originalClearTimeout;
  }
}

async function testRuntimeSurface(bundleDir) {
  const appModule = await import(pathToFileURL(path.join(bundleDir, "app.mjs")).href);
  const firestoreModule = await import(pathToFileURL(path.join(bundleDir, "firestore.mjs")).href);
  const indexModule = await import(pathToFileURL(path.join(bundleDir, "index.mjs")).href);
  const protobufModule = await import(
    pathToFileURL(path.join(bundleDir, "internal-protobuf.mjs")).href,
  );

  await testAppLifecycle(appModule);
  await testFirestoreLifecycle(firestoreModule, appModule);
  await testCrudTransportSurface(firestoreModule, appModule);
  await testTransactionSurface(firestoreModule, appModule);
  await testFieldValueSentinelWriteSurface(firestoreModule, appModule);
  await testAuthRefreshAndErrorMapping(firestoreModule, appModule);
  await testQueryConstraintSurface(firestoreModule, appModule);
  await testQueryExecutionSurface(firestoreModule, appModule);
  await testEqualityHelpers(firestoreModule, appModule);
  await testConverterSurface(firestoreModule, appModule);
  await testProtobufFoundation(protobufModule);
  await testGrpcWebUnaryTransportSurface(firestoreModule, appModule, protobufModule);
  await testGrpcWebTransactionSurface(firestoreModule, appModule, protobufModule);
  await testGrpcWebFieldValueSentinelSurface(firestoreModule, appModule, protobufModule);
  await testListenWatchSurface(firestoreModule, appModule, protobufModule);
  await testRootReexports(indexModule);
  testCommonJsSurface(bundleDir);
}

async function assertGeneratedProtoSurface() {
  for (const relativePath of [
    "src/gen/google/firestore/v1/document_pb.ts",
    "src/gen/google/firestore/v1/firestore_pb.ts",
    "src/gen/google/firestore/v1/query_pb.ts",
    "src/gen/google/firestore/v1/write_pb.ts",
    "src/gen/google/protobuf/timestamp_pb.ts",
  ]) {
    try {
      await fs.access(path.join(packageRoot, relativePath));
    } catch {
      throw new Error(
        `Missing generated Firestore protobuf output at ${relativePath}. Run "npm run codegen:proto --workspace @nimbus/firebase" first.`,
      );
    }
  }
}

async function testAppLifecycle(appModule) {
  const app = appModule.initializeApp({ projectId: "demo-project", apiKey: "demo-key" });
  assert.equal(app.name, "[DEFAULT]");
  assert.equal(app.options.projectId, "demo-project");
  assert.equal(appModule.getApps().length, 1);
  assert.equal(appModule.getApp().name, "[DEFAULT]");

  const named = appModule.initializeApp({ projectId: "named-project" }, "staging");
  assert.equal(named.name, "staging");
  assert.equal(appModule.getApp("staging").options.projectId, "named-project");

  await appModule.deleteApp(named);
  assert.throws(() => appModule.getApp("staging"), /has not been initialized/);
}

async function testFirestoreLifecycle(firestoreModule, appModule) {
  const app = appModule.getApp();
  const firestore = firestoreModule.getFirestore(app);
  assert.equal(firestore.databaseId, "(default)");
  assert.equal(firestore.settings.host, "firestore.googleapis.com");
  assert.equal(firestore.settings.ssl, true);

  const cities = firestoreModule.collection(firestore, "cities");
  assert.equal(cities.path, "cities");
  assert.equal(cities.parent, null);

  const sanFrancisco = firestoreModule.doc(cities, "SF");
  assert.equal(sanFrancisco.path, "cities/SF");
  assert.equal(sanFrancisco.parent.path, "cities");

  const landmarks = firestoreModule.collection(sanFrancisco, "landmarks");
  assert.equal(landmarks.path, "cities/SF/landmarks");
  assert.equal(landmarks.parent?.path, "cities/SF");

  const parks = firestoreModule.collection(firestore, "cities/SF/parks");
  assert.equal(parks.path, "cities/SF/parks");

  const collectionGroup = firestoreModule.collectionGroup(firestore, "landmarks");
  assert.equal(collectionGroup.id, "landmarks");

  const analytics = firestoreModule.initializeFirestore(
    app,
    { ignoreUndefinedProperties: true },
    "analytics",
  );
  assert.equal(analytics.databaseId, "analytics");
  assert.equal(analytics.settings.ignoreUndefinedProperties, true);

  firestoreModule.connectFirestoreEmulator(firestore, "127.0.0.1", 8080, {
    mockUserToken: { sub: "user-1" },
  });
  assert.equal(firestore.settings.host, "127.0.0.1:8080");
  assert.equal(firestore.settings.ssl, false);
  assert.equal(firestore.settings.useFetchStreams, false);

  assert.throws(
    () => firestoreModule.collection(firestore, "cities/SF"),
    /odd number of path segments/,
  );
  assert.throws(
    () => firestoreModule.doc(firestore, "cities"),
    /even number of path segments/,
  );
  assert.throws(
    () => firestoreModule.collectionGroup(firestore, "cities/landmarks"),
    /single collection segment/,
  );

  await firestoreModule.terminate(analytics);
  assert.equal(firestoreModule.getFirestore(app, "analytics").databaseId, "analytics");
}

async function testRootReexports(indexModule) {
  const clientApp = indexModule.initializeApp({ projectId: "root-project" }, "root");
  const firestore = indexModule.getFirestore(clientApp);
  assert.equal(firestore.app.name, "root");
  assert.equal(typeof indexModule.connectFirestoreEmulator, "function");
  assert.equal(indexModule.collection(firestore, "cities").path, "cities");
  assert.equal(typeof indexModule.documentId, "function");
  assert.equal(typeof indexModule.getDoc, "function");
  assert.equal(typeof indexModule.getDocs, "function");
  assert.equal(typeof indexModule.onSnapshot, "function");
  assert.equal(typeof indexModule.refEqual, "function");
  assert.equal(typeof indexModule.queryEqual, "function");
  assert.equal(typeof indexModule.snapshotEqual, "function");
  assert.equal(typeof indexModule.setDoc, "function");
  assert.equal(typeof indexModule.updateDoc, "function");
  assert.equal(typeof indexModule.deleteDoc, "function");
  assert.equal(typeof indexModule.addDoc, "function");
  assert.equal(typeof indexModule.arrayRemove, "function");
  assert.equal(typeof indexModule.arrayUnion, "function");
  assert.equal(typeof indexModule.deleteField, "function");
  assert.equal(typeof indexModule.runTransaction, "function");
  assert.equal(typeof indexModule.serverTimestamp, "function");
  assert.equal(typeof indexModule.writeBatch, "function");
  assert.equal(typeof indexModule.increment, "function");
  assert.equal(typeof indexModule.query, "function");
  assert.equal(typeof indexModule.where, "function");
  assert.equal(typeof indexModule.orderBy, "function");
  assert.equal(typeof indexModule.limit, "function");
  assert.equal(typeof indexModule.startAt, "function");
  assert.equal(typeof indexModule.startAfter, "function");
  assert.equal(typeof indexModule.endAt, "function");
  assert.equal(typeof indexModule.endBefore, "function");
  await indexModule.deleteApp(clientApp);
}

function testCommonJsSurface(bundleDir) {
  const appModule = require(path.join(bundleDir, "app.cjs"));
  const firestoreModule = require(path.join(bundleDir, "firestore.cjs"));
  const indexModule = require(path.join(bundleDir, "index.cjs"));
  assert.equal(typeof appModule.initializeApp, "function");
  assert.equal(typeof firestoreModule.getFirestore, "function");
  assert.equal(typeof firestoreModule.getDoc, "function");
  assert.equal(typeof firestoreModule.getDocs, "function");
  assert.equal(typeof firestoreModule.refEqual, "function");
  assert.equal(typeof firestoreModule.queryEqual, "function");
  assert.equal(typeof firestoreModule.snapshotEqual, "function");
  assert.equal(typeof firestoreModule.setDoc, "function");
  assert.equal(typeof firestoreModule.updateDoc, "function");
  assert.equal(typeof firestoreModule.deleteDoc, "function");
  assert.equal(typeof firestoreModule.addDoc, "function");
  assert.equal(typeof firestoreModule.arrayRemove, "function");
  assert.equal(typeof firestoreModule.arrayUnion, "function");
  assert.equal(typeof firestoreModule.deleteField, "function");
  assert.equal(typeof firestoreModule.runTransaction, "function");
  assert.equal(typeof firestoreModule.increment, "function");
  assert.equal(typeof firestoreModule.serverTimestamp, "function");
  assert.equal(typeof firestoreModule.writeBatch, "function");
  assert.equal(typeof firestoreModule.query, "function");
  assert.equal(typeof firestoreModule.where, "function");
  assert.equal(typeof indexModule.initializeApp, "function");
}

function createJsonResponse(status, body) {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "content-type": "application/json",
    },
  });
}

function createJsonLinesResponse(lines) {
  return new Response(lines.map((line) => JSON.stringify(line)).join("\n"), {
    status: 200,
    headers: {
      "content-type": "application/json",
    },
  });
}

function concatBinaryChunks(chunks) {
  const totalLength = chunks.reduce((sum, chunk) => sum + chunk.byteLength, 0);
  const combined = new Uint8Array(totalLength);
  let offset = 0;
  for (const chunk of chunks) {
    combined.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return combined;
}

function binaryBodyToUint8Array(body) {
  if (body instanceof Uint8Array) {
    return body;
  }
  if (body instanceof ArrayBuffer) {
    return new Uint8Array(body);
  }
  if (ArrayBuffer.isView(body)) {
    return new Uint8Array(body.buffer, body.byteOffset, body.byteLength);
  }
  throw new Error(`Unsupported binary body type: ${Object.prototype.toString.call(body)}.`);
}

function decodeGrpcWebUnaryRequest(body) {
  const bytes = binaryBodyToUint8Array(body);
  assert.ok(bytes.byteLength >= 5, "gRPC-Web unary requests must include one envelope.");
  assert.equal(bytes[0], 0);
  const messageLength =
    (bytes[1] << 24) | (bytes[2] << 16) | (bytes[3] << 8) | bytes[4];
  return bytes.slice(5, 5 + messageLength);
}

function createGrpcWebResponse(messageBytes, trailerHeaders = new Headers({ "grpc-status": "0" })) {
  const body = concatBinaryChunks([
    ...messageBytes.map((message) => encodeEnvelope(0, message)),
    encodeEnvelope(trailerFlag, trailerSerialize(trailerHeaders)),
  ]);
  return new Response(body, {
    status: 200,
    headers: {
      "content-type": "application/grpc-web+proto",
    },
  });
}

async function recordRequest(url, options) {
  return {
    body: options?.body ? JSON.parse(String(options.body)) : undefined,
    headers: new Headers(options?.headers ?? {}),
    method: options?.method ?? "GET",
    url: String(url),
  };
}

async function testCrudTransportSurface(firestoreModule, appModule) {
  const app = appModule.initializeApp(
    {
      apiKey: "sdk-api-key",
      appId: "sdk-app-id",
      projectId: "sdk-project",
    },
    "crud-runtime",
  );
  const requests = [];
  const queuedResponses = [];
  const fetch = async (url, options) => {
    requests.push(await recordRequest(url, options));
    const nextResponse = queuedResponses.shift();
    assert.ok(nextResponse, `Unexpected Firestore request to ${url}`);
    return nextResponse();
  };

  const firestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalAuthToken: "unit-token",
      experimentalFetch: fetch,
      experimentalHeaders: {
        "x-sdk-test": "enabled",
      },
      host: "sdk.test",
      ssl: false,
    },
    "sdk",
  );

  const cities = firestoreModule.collection(firestore, "cities.v2");
  const city = firestoreModule.doc(cities, "日本語 __.SF");
  const cityName =
    "projects/sdk-project/databases/sdk/documents/cities.v2/日本語 __.SF";

  queuedResponses.push(() => createJsonResponse(200, { commitTime: "2026-04-25T00:00:00Z" }));
  await firestoreModule.setDoc(city, {
    name: "Tokyo",
    nested: { active: true },
    visits: 3,
  });

  assert.equal(
    requests[0].url,
    "http://sdk.test/v1/projects/sdk-project/databases/sdk/documents:commit",
  );
  assert.equal(requests[0].method, "POST");
  assert.equal(requests[0].headers.get("authorization"), "Bearer unit-token");
  assert.equal(requests[0].headers.get("x-goog-api-key"), "sdk-api-key");
  assert.equal(requests[0].headers.get("x-firebase-gmpid"), "sdk-app-id");
  assert.equal(requests[0].headers.get("x-sdk-test"), "enabled");
  assert.deepEqual(requests[0].body, {
    database: "projects/sdk-project/databases/sdk",
    writes: [
      {
        update: {
          fields: {
            name: { stringValue: "Tokyo" },
            nested: {
              mapValue: {
                fields: {
                  active: { booleanValue: true },
                },
              },
            },
            visits: { integerValue: "3" },
          },
          name: cityName,
        },
      },
    ],
  });

  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        found: {
          fields: {
            count: { integerValue: "3" },
            nested: {
              mapValue: {
                fields: {
                  active: { booleanValue: true },
                },
              },
            },
            score: { doubleValue: "-0" },
            title: { stringValue: "Tokyo" },
          },
          name: cityName,
        },
      },
    ]),
  );
  const snapshot = await firestoreModule.getDoc(city);
  assert.equal(snapshot.exists(), true);
  assert.deepEqual(snapshot.data(), {
    count: 3,
    nested: { active: true },
    score: -0,
    title: "Tokyo",
  });
  assert.equal(Object.is(snapshot.get("score"), -0), true);
  assert.equal(snapshot.get("nested.active"), true);
  assert.deepEqual(requests[1].body, {
    database: "projects/sdk-project/databases/sdk",
    documents: [cityName],
  });

  queuedResponses.push(() => createJsonResponse(200, { commitTime: "2026-04-25T00:00:01Z" }));
  await firestoreModule.updateDoc(city, {
    "nested.active": false,
    visits: 4,
  });
  assert.deepEqual(requests[2].body, {
    database: "projects/sdk-project/databases/sdk",
    writes: [
      {
        currentDocument: {
          exists: true,
        },
        update: {
          fields: {
            nested: {
              mapValue: {
                fields: {
                  active: { booleanValue: false },
                },
              },
            },
            visits: { integerValue: "4" },
          },
          name: cityName,
        },
        updateMask: {
          fieldPaths: ["nested.active", "visits"],
        },
      },
    ],
  });

  queuedResponses.push(() => createJsonResponse(200, { commitTime: "2026-04-25T00:00:02Z" }));
  await firestoreModule.deleteDoc(city);
  assert.deepEqual(requests[3].body, {
    database: "projects/sdk-project/databases/sdk",
    writes: [
      {
        delete: cityName,
      },
    ],
  });

  const landmarks = firestoreModule.collection(city, "landmarks.__");
  queuedResponses.push(() => createJsonResponse(200, { commitTime: "2026-04-25T00:00:03Z" }));
  const landmark = await firestoreModule.addDoc(landmarks, {
    category: "tower",
    name: "Skytree",
  });
  assert.match(landmark.id, /^[A-Za-z0-9]{20}$/u);
  assert.equal(landmark.parent.path, "cities.v2/日本語 __.SF/landmarks.__");
  assert.deepEqual(requests[4].body, {
    database: "projects/sdk-project/databases/sdk",
    writes: [
      {
        currentDocument: {
          exists: false,
        },
        update: {
          fields: {
            category: { stringValue: "tower" },
            name: { stringValue: "Skytree" },
          },
          name: `projects/sdk-project/databases/sdk/documents/cities.v2/日本語 __.SF/landmarks.__/${landmark.id}`,
        },
      },
    ],
  });

  queuedResponses.push(() => createJsonLinesResponse([{ missing: cityName }]));
  const missingSnapshot = await firestoreModule.getDoc(city);
  assert.equal(missingSnapshot.exists(), false);
  assert.equal(missingSnapshot.data(), undefined);

  queuedResponses.push(() => createJsonResponse(200, { commitTime: "2026-04-25T00:00:04Z" }));
  const oakland = firestoreModule.doc(cities, "OAK");
  const sanJose = firestoreModule.doc(cities, "SJC");
  const batch = firestoreModule.writeBatch(firestore);
  assert.equal(
    batch
      .set(oakland, { name: "Oakland" })
      .set(sanJose, { name: "San Jose" })
      .delete(city),
    batch,
  );
  await batch.commit();
  assert.deepEqual(requests[6].body, {
    database: "projects/sdk-project/databases/sdk",
    writes: [
      {
        update: {
          fields: {
            name: { stringValue: "Oakland" },
          },
          name: "projects/sdk-project/databases/sdk/documents/cities.v2/OAK",
        },
      },
      {
        update: {
          fields: {
            name: { stringValue: "San Jose" },
          },
          name: "projects/sdk-project/databases/sdk/documents/cities.v2/SJC",
        },
      },
      {
        delete: cityName,
      },
    ],
  });
  await assert.rejects(() => batch.commit(), /cannot be used after commit/i);

  await appModule.deleteApp(app);
}

async function testTransactionSurface(firestoreModule, appModule) {
  const app = appModule.initializeApp({ projectId: "txn-project" }, "transaction-runtime");
  const requests = [];
  let beginCalls = 0;
  let batchGetCalls = 0;
  let runQueryCalls = 0;
  let commitCalls = 0;
  let rollbackCalls = 0;
  const transactionTokens = [
    Buffer.from("txn-rest-1").toString("base64"),
    Buffer.from("txn-rest-2").toString("base64"),
    Buffer.from("txn-rest-3").toString("base64"),
    Buffer.from("txn-rest-4").toString("base64"),
  ];
  const fetch = async (url, options) => {
    requests.push(await recordRequest(url, options));
    const request = requests.at(-1);
    assert.ok(request, "transaction request should be recorded");

    if (String(url).endsWith(":beginTransaction")) {
      const transaction = transactionTokens[beginCalls];
      beginCalls += 1;
      return createJsonResponse(200, { transaction });
    }

    if (String(url).endsWith(":batchGet")) {
      const expectedTransaction = transactionTokens[batchGetCalls + 2];
      batchGetCalls += 1;
      assert.equal(request.body.transaction, expectedTransaction);
      return createJsonLinesResponse([
        {
          found: {
            fields: {
              name: { stringValue: "San Francisco" },
              visits: { integerValue: "1" },
            },
            name: "projects/txn-project/databases/txn/documents/cities/SF",
          },
        },
      ]);
    }

    if (String(url).endsWith(":runQuery")) {
      const expectedTransaction = [
        transactionTokens[0],
        transactionTokens[1],
        transactionTokens[3],
      ][runQueryCalls];
      runQueryCalls += 1;
      assert.equal(request.body.transaction, expectedTransaction);
      return createJsonLinesResponse([
        {
          document: {
            fields: {
              name: { stringValue: "San Francisco" },
              visits: { integerValue: "1" },
            },
            name: "projects/txn-project/databases/txn/documents/cities/SF",
          },
        },
      ]);
    }

    if (String(url).endsWith(":commit")) {
      const expectedTransaction = commitCalls === 0 ? transactionTokens[0] : transactionTokens[1];
      commitCalls += 1;
      assert.equal(request.body.transaction, expectedTransaction);
      if (commitCalls === 1) {
        return createJsonResponse(409, {
          error: {
            message: "transaction conflict",
            status: "ABORTED",
          },
        });
      }
      return createJsonResponse(200, { commitTime: "2026-04-25T00:00:05Z" });
    }

    if (String(url).endsWith(":rollback")) {
      rollbackCalls += 1;
      const expectedTransaction = rollbackCalls === 1 ? transactionTokens[2] : transactionTokens[3];
      assert.equal(request.body.transaction, expectedTransaction);
      return createJsonResponse(200, {});
    }

    throw new Error(`Unexpected Firestore transaction request to ${url}`);
  };

  const firestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalFetch: fetch,
      host: "txn.test",
      ssl: false,
    },
    "txn",
  );
  const city = firestoreModule.doc(firestoreModule.collection(firestore, "cities"), "SF");
  const citiesQuery = firestoreModule.query(
    firestoreModule.collection(firestore, "cities"),
    firestoreModule.where("name", "==", "San Francisco"),
  );

  let attempts = 0;
  const result = await firestoreModule.runTransaction(
    firestore,
    async (transaction) => {
      attempts += 1;
      const snapshot = await transaction.get(citiesQuery);
      transaction.update(city, {
        visits: Number(snapshot.docs[0]?.data()?.visits ?? 0) + 1,
      });
      return attempts;
    },
    { maxAttempts: 2 },
  );

  assert.equal(result, 2);
  assert.equal(attempts, 2);
  assert.equal(beginCalls, 2);
  assert.equal(commitCalls, 2);
  assert.equal(rollbackCalls, 0);
  assert.equal(
    requests[0].url,
    "http://txn.test/v1/projects/txn-project/databases/txn/documents:beginTransaction",
  );
  assert.equal(
    requests[1].url,
    "http://txn.test/v1/projects/txn-project/databases/txn/documents:runQuery",
  );
  assert.equal(
    requests[2].url,
    "http://txn.test/v1/projects/txn-project/databases/txn/documents:commit",
  );

  await assert.rejects(
    () =>
      firestoreModule.runTransaction(firestore, async (transaction) => {
        const snapshot = await transaction.get(city);
        transaction.set(city, {
          name: snapshot.data()?.name,
          visits: 99,
        });
        throw new Error("abort transaction");
      }),
    /abort transaction/u,
  );
  assert.equal(rollbackCalls, 1);
  assert.equal(
    requests.at(-1)?.url,
    "http://txn.test/v1/projects/txn-project/databases/txn/documents:rollback",
  );

  const readOnlyCount = await firestoreModule.runTransaction(firestore, async (transaction) => {
    const snapshot = await transaction.get(citiesQuery);
    return snapshot.size;
  });
  assert.equal(readOnlyCount, 1);
  assert.equal(rollbackCalls, 2);
  assert.equal(
    requests.at(-1)?.url,
    "http://txn.test/v1/projects/txn-project/databases/txn/documents:rollback",
  );

  await appModule.deleteApp(app);
}

async function testFieldValueSentinelWriteSurface(firestoreModule, appModule) {
  const app = appModule.initializeApp({ projectId: "field-value-project" }, "field-value-runtime");
  const requests = [];
  const transactionToken = Buffer.from("field-value-txn").toString("base64");
  const fetch = async (url, options) => {
    const request = await recordRequest(url, options);
    requests.push(request);

    if (String(url).endsWith(":beginTransaction")) {
      return createJsonResponse(200, { transaction: transactionToken });
    }

    if (String(url).endsWith(":commit")) {
      const writes = Array.isArray(request.body?.writes) ? request.body.writes : [];
      return createJsonResponse(200, {
        commitTime: "2026-04-25T00:00:06Z",
        writeResults: writes.map((write) => ({
          transformResults: Array.isArray(write.updateTransforms)
            ? write.updateTransforms.map((transform) => {
                if (transform.setToServerValue === "REQUEST_TIME") {
                  return { timestampValue: "2026-04-25T00:00:06Z" };
                }
                if (transform.increment) {
                  return transform.increment;
                }
                if (transform.appendMissingElements) {
                  return {
                    arrayValue: transform.appendMissingElements,
                  };
                }
                if (transform.removeAllFromArray) {
                  return {
                    arrayValue: transform.removeAllFromArray,
                  };
                }
                return { nullValue: null };
              })
            : [],
        })),
      });
    }

    throw new Error(`Unexpected FieldValue request to ${url}`);
  };

  const firestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalFetch: fetch,
      host: "field-value.test",
      ssl: false,
    },
    "field-values",
  );
  const city = firestoreModule.doc(firestoreModule.collection(firestore, "cities"), "SF");
  const cityName =
    "projects/field-value-project/databases/field-values/documents/cities/SF";

  await firestoreModule.setDoc(
    city,
    {
      name: "San Francisco",
      obsolete: firestoreModule.deleteField(),
      tags: firestoreModule.arrayUnion("west", "coast"),
      updatedAt: firestoreModule.serverTimestamp(),
      visits: firestoreModule.increment(1),
    },
    {
      mergeFields: ["name", "obsolete", "tags", "updatedAt", "visits"],
    },
  );
  assert.deepEqual(requests[0].body, {
    database: "projects/field-value-project/databases/field-values",
    writes: [
      {
        update: {
          fields: {
            name: { stringValue: "San Francisco" },
          },
          name: cityName,
        },
        updateMask: {
          fieldPaths: ["name", "obsolete"],
        },
        updateTransforms: [
          {
            appendMissingElements: {
              values: [{ stringValue: "west" }, { stringValue: "coast" }],
            },
            fieldPath: "tags",
          },
          {
            fieldPath: "updatedAt",
            setToServerValue: "REQUEST_TIME",
          },
          {
            fieldPath: "visits",
            increment: { integerValue: "1" },
          },
        ],
      },
    ],
  });

  await firestoreModule.updateDoc(city, {
    title: "Updated",
    "stats.legacy": firestoreModule.deleteField(),
    "stats.visits": firestoreModule.increment(2),
    tags: firestoreModule.arrayRemove("stale"),
    updatedAt: firestoreModule.serverTimestamp(),
  });
  assert.deepEqual(requests[1].body, {
    database: "projects/field-value-project/databases/field-values",
    writes: [
      {
        currentDocument: {
          exists: true,
        },
        update: {
          fields: {
            title: { stringValue: "Updated" },
          },
          name: cityName,
        },
        updateMask: {
          fieldPaths: ["stats.legacy", "title"],
        },
        updateTransforms: [
          {
            fieldPath: "stats.visits",
            increment: { integerValue: "2" },
          },
          {
            fieldPath: "tags",
            removeAllFromArray: {
              values: [{ stringValue: "stale" }],
            },
          },
          {
            fieldPath: "updatedAt",
            setToServerValue: "REQUEST_TIME",
          },
        ],
      },
    ],
  });

  const batch = firestoreModule.writeBatch(firestore);
  batch.set(
    city,
    {
      archivedAt: firestoreModule.serverTimestamp(),
      obsolete: firestoreModule.deleteField(),
    },
    { merge: true },
  );
  batch.update(city, {
    "stats.tags": firestoreModule.arrayUnion("north"),
  });
  await batch.commit();
  assert.deepEqual(requests[2].body, {
    database: "projects/field-value-project/databases/field-values",
    writes: [
      {
        update: {
          fields: {},
          name: cityName,
        },
        updateMask: {
          fieldPaths: ["obsolete"],
        },
        updateTransforms: [
          {
            fieldPath: "archivedAt",
            setToServerValue: "REQUEST_TIME",
          },
        ],
      },
      {
        currentDocument: {
          exists: true,
        },
        update: {
          fields: {},
          name: cityName,
        },
        updateMask: {
          fieldPaths: [],
        },
        updateTransforms: [
          {
            appendMissingElements: {
              values: [{ stringValue: "north" }],
            },
            fieldPath: "stats.tags",
          },
        ],
      },
    ],
  });

  const transactionResult = await firestoreModule.runTransaction(
    firestore,
    async (transaction) => {
      transaction.set(
        city,
        {
          clearedAt: firestoreModule.deleteField(),
          status: "active",
          updatedAt: firestoreModule.serverTimestamp(),
        },
        { merge: true },
      );
      transaction.update(city, {
        "stats.visits": firestoreModule.increment(3),
      });
      return "ok";
    },
  );
  assert.equal(transactionResult, "ok");
  assert.deepEqual(requests[3].body, {
    database: "projects/field-value-project/databases/field-values",
  });
  assert.deepEqual(requests[4].body, {
    database: "projects/field-value-project/databases/field-values",
    transaction: transactionToken,
    writes: [
      {
        update: {
          fields: {
            status: { stringValue: "active" },
          },
          name: cityName,
        },
        updateMask: {
          fieldPaths: ["status", "clearedAt"],
        },
        updateTransforms: [
          {
            fieldPath: "updatedAt",
            setToServerValue: "REQUEST_TIME",
          },
        ],
      },
      {
        currentDocument: {
          exists: true,
        },
        update: {
          fields: {},
          name: cityName,
        },
        updateMask: {
          fieldPaths: [],
        },
        updateTransforms: [
          {
            fieldPath: "stats.visits",
            increment: { integerValue: "3" },
          },
        ],
      },
    ],
  });

  await assert.rejects(
    () =>
      firestoreModule.setDoc(city, {
        obsolete: firestoreModule.deleteField(),
      }),
    /deleteField\(\) can only be used/u,
  );
  await assert.rejects(
    () =>
      firestoreModule.setDoc(
        city,
        {
          profile: {
            updatedAt: firestoreModule.serverTimestamp(),
          },
        },
        {
          mergeFields: ["profile"],
        },
      ),
    /cannot target a subtree containing FieldValue sentinels/u,
  );
  await assert.rejects(
    () =>
      firestoreModule.updateDoc(city, {
        stats: {
          visits: 1,
        },
        "stats.visits": firestoreModule.increment(1),
      }),
    /cannot apply both a regular value and a transform to "stats\.visits"/u,
  );
  await assert.rejects(
    () =>
      firestoreModule.updateDoc(city, {
        tags: [firestoreModule.serverTimestamp()],
      }),
    /FieldValue sentinels must be used as direct document field values/u,
  );

  await appModule.deleteApp(app);
}

async function testAuthRefreshAndErrorMapping(firestoreModule, appModule) {
  const app = appModule.initializeApp({ projectId: "auth-project" }, "crud-auth");
  const authCalls = [];
  const requests = [];
  const queuedResponses = [
    createJsonResponse(401, {
      error: {
        message: "expired",
        status: "UNAUTHENTICATED",
      },
    }),
    createJsonResponse(200, { commitTime: "2026-04-25T00:00:04Z" }),
    createJsonResponse(409, {
      error: {
        message: "duplicate write",
        status: "ALREADY_EXISTS",
      },
    }),
  ];
  const fetch = async (url, options) => {
    requests.push(await recordRequest(url, options));
    const nextResponse = queuedResponses.shift();
    assert.ok(nextResponse, `Unexpected Firestore request to ${url}`);
    return nextResponse;
  };

  const firestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalAuthToken: async ({ forceRefresh }) => {
        authCalls.push(forceRefresh);
        return forceRefresh ? "fresh-token" : "stale-token";
      },
      experimentalFetch: fetch,
      host: "auth.test",
      ssl: false,
    },
    "auth",
  );

  const city = firestoreModule.doc(firestoreModule.collection(firestore, "cities"), "SF");
  await firestoreModule.setDoc(city, { name: "San Francisco" });
  assert.deepEqual(authCalls, [false, true]);
  assert.equal(requests[0].headers.get("authorization"), "Bearer stale-token");
  assert.equal(requests[1].headers.get("authorization"), "Bearer fresh-token");

  await assert.rejects(
    () => firestoreModule.setDoc(city, { name: "Duplicate" }),
    (error) => {
      assert.ok(error instanceof firestoreModule.FirestoreError);
      assert.equal(error.code, "ALREADY_EXISTS");
      assert.equal(error.message, "duplicate write");
      assert.equal(error.status, 409);
      return true;
    },
  );

  await appModule.deleteApp(app);
}

async function testQueryConstraintSurface(firestoreModule, appModule) {
  const app = appModule.initializeApp({ projectId: "query-project" }, "query-runtime");
  const firestore = firestoreModule.getFirestore(app, "queries");
  const cities = firestoreModule.collection(firestore, "cities");

  const constrainedQuery = firestoreModule.query(
    cities,
    firestoreModule.where("state", "==", "CA"),
    firestoreModule.orderBy("name", "desc"),
    firestoreModule.limit(5),
    firestoreModule.startAt("Los Angeles"),
  );
  assert.deepEqual(constrainedQuery.structuredQuery, {
    from: [{ collectionId: "cities" }],
    limit: 5,
    orderBy: [
      {
        direction: "DESCENDING",
        field: { fieldPath: "name" },
      },
    ],
    startAt: {
      before: true,
      values: ["Los Angeles"],
    },
    where: {
      fieldFilter: {
        field: { fieldPath: "state" },
        op: "EQUAL",
        value: "CA",
      },
    },
  });

  const collectionGroupQuery = firestoreModule.query(
    firestoreModule.collectionGroup(firestore, "landmarks"),
    firestoreModule.where(
      firestoreModule.documentId(),
      "==",
      "projects/demo/databases/(default)/documents/cities/SF/landmarks/coit",
    ),
    firestoreModule.orderBy(firestoreModule.documentId()),
    firestoreModule.endBefore(
      "projects/demo/databases/(default)/documents/cities/SEA/landmarks/needle",
    ),
  );
  assert.deepEqual(collectionGroupQuery.structuredQuery, {
    endAt: {
      before: true,
      values: [
        "projects/demo/databases/(default)/documents/cities/SEA/landmarks/needle",
      ],
    },
    from: [{ allDescendants: true, collectionId: "landmarks" }],
    orderBy: [
      {
        direction: "ASCENDING",
        field: { fieldPath: "__name__" },
      },
    ],
    where: {
      fieldFilter: {
        field: { fieldPath: "__name__" },
        op: "EQUAL",
        value: "projects/demo/databases/(default)/documents/cities/SF/landmarks/coit",
      },
    },
  });

  const chainedQuery = firestoreModule.query(
    constrainedQuery,
    firestoreModule.where("capital", "==", true),
  );
  assert.deepEqual(chainedQuery.structuredQuery.where, {
    compositeFilter: {
      filters: [
        {
          fieldFilter: {
            field: { fieldPath: "state" },
            op: "EQUAL",
            value: "CA",
          },
        },
        {
          fieldFilter: {
            field: { fieldPath: "capital" },
            op: "EQUAL",
            value: true,
          },
        },
      ],
      op: "AND",
    },
  });

  assert.throws(
    () => firestoreModule.where("nested.active", "==", true),
    /nested field paths are not supported yet/,
  );
  assert.throws(
    () =>
      firestoreModule.query(
        cities,
        firestoreModule.limit(1),
        firestoreModule.limit(2),
      ),
    /at most one limit/,
  );
  assert.throws(
    () => firestoreModule.startAfter(),
    /requires at least one cursor value/,
  );

  await appModule.deleteApp(app);
}

async function testQueryExecutionSurface(firestoreModule, appModule) {
  const app = appModule.initializeApp({ projectId: "query-results-project" }, "query-results");
  const requests = [];
  const queuedResponses = [];
  const fetch = async (url, options) => {
    requests.push(await recordRequest(url, options));
    const nextResponse = queuedResponses.shift();
    assert.ok(nextResponse, `Unexpected Firestore query request to ${url}`);
    return nextResponse();
  };
  const firestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalFetch: fetch,
      host: "query.test",
      ssl: false,
    },
    "queries",
  );

  const cities = firestoreModule.collection(firestore, "cities");
  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        document: {
          fields: {
            name: { stringValue: "Alpha" },
            rank: { integerValue: "1" },
          },
          name: "projects/query-results-project/databases/queries/documents/cities/alpha",
        },
        readTime: "2026-04-25T00:00:00Z",
        skippedResults: 1,
      },
      {
        document: {
          fields: {
            name: { stringValue: "Bravo" },
            rank: { integerValue: "2" },
          },
          name: "projects/query-results-project/databases/queries/documents/cities/bravo",
        },
        readTime: "2026-04-25T00:00:00Z",
      },
    ]),
  );

  const cityResults = await firestoreModule.getDocs(
    firestoreModule.query(
      cities,
      firestoreModule.where("name", ">=", "Alpha"),
      firestoreModule.orderBy("name"),
      firestoreModule.limit(2),
      firestoreModule.startAt("Alpha"),
    ),
  );
  assert.equal(cityResults.empty, false);
  assert.equal(cityResults.size, 2);
  assert.equal(cityResults.metadata.fromCache, false);
  assert.equal(cityResults.metadata.hasPendingWrites, false);
  assert.equal(cityResults.docs[0].ref.path, "cities/alpha");
  assert.deepEqual(cityResults.docs[0].data(), { name: "Alpha", rank: 1 });
  assert.equal(cityResults.docs[1].get("rank"), 2);
  assert.deepEqual(requests[0].body, {
    parent: "projects/query-results-project/databases/queries/documents",
    structuredQuery: {
      from: [{ collectionId: "cities" }],
      limit: 2,
      orderBy: [{ direction: "ASCENDING", field: { fieldPath: "name" } }],
      startAt: {
        before: true,
        values: [{ stringValue: "Alpha" }],
      },
      where: {
        fieldFilter: {
          field: { fieldPath: "name" },
          op: "GREATER_THAN_OR_EQUAL",
          value: { stringValue: "Alpha" },
        },
      },
    },
  });
  assert.equal(
    requests[0].url,
    "http://query.test/v1/projects/query-results-project/databases/queries/documents:runQuery",
  );

  const landmarks = firestoreModule.collectionGroup(firestore, "landmarks");
  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        document: {
          fields: {
            name: { stringValue: "Coit Tower" },
          },
          name: "projects/query-results-project/databases/queries/documents/cities/SF/landmarks/coit",
        },
        readTime: "2026-04-25T00:00:01Z",
      },
    ]),
  );

  const landmarkResults = await firestoreModule.getDocs(
    firestoreModule.query(
      landmarks,
      firestoreModule.where(
        firestoreModule.documentId(),
        "==",
        "projects/query-results-project/databases/queries/documents/cities/SF/landmarks/coit",
      ),
    ),
  );
  assert.equal(landmarkResults.size, 1);
  assert.equal(landmarkResults.docs[0].ref.path, "cities/SF/landmarks/coit");
  assert.deepEqual(landmarkResults.docs[0].data(), { name: "Coit Tower" });
  assert.deepEqual(requests[1].body, {
    parent: "projects/query-results-project/databases/queries/documents",
    structuredQuery: {
      from: [{ allDescendants: true, collectionId: "landmarks" }],
      where: {
        fieldFilter: {
          field: { fieldPath: "__name__" },
          op: "EQUAL",
          value: {
            referenceValue:
              "projects/query-results-project/databases/queries/documents/cities/SF/landmarks/coit",
          },
        },
      },
    },
  });

  const sfLandmarks = firestoreModule.collection(firestoreModule.doc(cities, "SF"), "landmarks");
  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        readTime: "2026-04-25T00:00:02Z",
      },
    ]),
  );
  const emptyResults = await firestoreModule.getDocs(sfLandmarks);
  assert.equal(emptyResults.empty, true);
  assert.equal(emptyResults.size, 0);
  assert.deepEqual(emptyResults.docs, []);
  assert.equal(
    requests[2].url,
    "http://query.test/v1/projects/query-results-project/databases/queries/documents/cities/SF:runQuery",
  );
  assert.deepEqual(requests[2].body, {
    parent: "projects/query-results-project/databases/queries/documents/cities/SF",
    structuredQuery: {
      from: [{ collectionId: "landmarks" }],
    },
  });

  const iterated = [];
  cityResults.forEach((snapshot) => {
    iterated.push(snapshot.id);
  });
  assert.deepEqual(iterated, ["alpha", "bravo"]);

  await appModule.deleteApp(app);
}

async function testEqualityHelpers(firestoreModule, appModule) {
  const app = appModule.initializeApp({ projectId: "eq-project" }, "eq-runtime");
  const firestore = firestoreModule.getFirestore(app, "eq");
  const cities = firestoreModule.collection(firestore, "cities");
  const cityA = firestoreModule.doc(cities, "alpha");
  const cityACopy = firestoreModule.doc(cities, "alpha");
  const cityB = firestoreModule.doc(cities, "bravo");
  const landmarksA = firestoreModule.collectionGroup(firestore, "landmarks");
  const landmarksACopy = firestoreModule.collectionGroup(firestore, "landmarks");
  const landmarksB = firestoreModule.collectionGroup(firestore, "districts");

  assert.equal(firestoreModule.refEqual(cityA, cityACopy), true);
  assert.equal(firestoreModule.refEqual(cityA, cityB), false);
  assert.equal(firestoreModule.refEqual(cities, firestoreModule.collection(firestore, "cities")), true);
  assert.equal(firestoreModule.refEqual(landmarksA, landmarksACopy), true);
  assert.equal(firestoreModule.refEqual(landmarksA, landmarksB), false);

  const queryA = firestoreModule.query(
    cities,
    firestoreModule.where("state", "==", "CA"),
    firestoreModule.orderBy("name"),
  );
  const queryACopy = firestoreModule.query(
    firestoreModule.collection(firestore, "cities"),
    firestoreModule.where("state", "==", "CA"),
    firestoreModule.orderBy("name"),
  );
  const queryB = firestoreModule.query(
    cities,
    firestoreModule.where("state", "==", "NV"),
    firestoreModule.orderBy("name"),
  );
  assert.equal(firestoreModule.queryEqual(queryA, queryACopy), true);
  assert.equal(firestoreModule.queryEqual(queryA, queryB), false);

  const requests = [];
  const queuedResponses = [];
  const fetch = async (url, options) => {
    requests.push(await recordRequest(url, options));
    const nextResponse = queuedResponses.shift();
    assert.ok(nextResponse, `Unexpected Firestore equality request to ${url}`);
    return nextResponse();
  };
  const eqFirestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalFetch: fetch,
      host: "eq.test",
      ssl: false,
    },
    "eq-data",
  );
  const eqCities = firestoreModule.collection(eqFirestore, "cities");
  const eqCity = firestoreModule.doc(eqCities, "alpha");

  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        found: {
          fields: {
            name: { stringValue: "Alpha" },
          },
          name: "projects/eq-project/databases/eq-data/documents/cities/alpha",
        },
      },
    ]),
  );
  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        found: {
          fields: {
            name: { stringValue: "Alpha" },
          },
          name: "projects/eq-project/databases/eq-data/documents/cities/alpha",
        },
      },
    ]),
  );
  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        found: {
          fields: {
            name: { stringValue: "Bravo" },
          },
          name: "projects/eq-project/databases/eq-data/documents/cities/alpha",
        },
      },
    ]),
  );

  const snapshotA = await firestoreModule.getDoc(eqCity);
  const snapshotACopy = await firestoreModule.getDoc(eqCity);
  const snapshotB = await firestoreModule.getDoc(eqCity);
  assert.equal(firestoreModule.snapshotEqual(snapshotA, snapshotACopy), true);
  assert.equal(firestoreModule.snapshotEqual(snapshotA, snapshotB), false);

  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        document: {
          fields: {
            name: { stringValue: "Alpha" },
          },
          name: "projects/eq-project/databases/eq-data/documents/cities/alpha",
        },
        readTime: "2026-04-25T00:00:00Z",
      },
    ]),
  );
  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        document: {
          fields: {
            name: { stringValue: "Alpha" },
          },
          name: "projects/eq-project/databases/eq-data/documents/cities/alpha",
        },
        readTime: "2026-04-25T00:00:00Z",
      },
    ]),
  );
  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        readTime: "2026-04-25T00:00:00Z",
      },
    ]),
  );

  const resultA = await firestoreModule.getDocs(eqCities);
  const resultACopy = await firestoreModule.getDocs(eqCities);
  const resultB = await firestoreModule.getDocs(eqCities);
  assert.equal(firestoreModule.snapshotEqual(resultA, resultACopy), true);
  assert.equal(firestoreModule.snapshotEqual(resultA, resultB), false);
  assert.ok(requests.length >= 6);

  await appModule.deleteApp(app);
}

async function testConverterSurface(firestoreModule, appModule) {
  class CityView {
    constructor(name, population, slug) {
      this.name = name;
      this.population = population;
      this.slug = slug;
    }
  }

  const app = appModule.initializeApp({ projectId: "converter-project" }, "converter-runtime");
  const requests = [];
  const queuedResponses = [];
  const fetch = async (url, options) => {
    requests.push(await recordRequest(url, options));
    const nextResponse = queuedResponses.shift();
    assert.ok(nextResponse, `Unexpected Firestore converter request to ${url}`);
    return nextResponse();
  };
  const firestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalFetch: fetch,
      host: "converter.test",
      ssl: false,
    },
    "typed",
  );

  const cityConverter = {
    toFirestore(city) {
      return {
        name: city.name,
        population: city.population,
        slug: city.slug,
      };
    },
    fromFirestore(snapshot) {
      const data = snapshot.data();
      return new CityView(data.name, data.population, data.slug);
    },
  };

  const cities = firestoreModule.collection(firestore, "cities").withConverter(cityConverter);
  const city = firestoreModule.doc(cities, "alpha");
  const rawCity = city.withConverter(null);
  assert.equal(city.converter, cityConverter);
  assert.equal(rawCity.converter, null);

  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        found: {
          fields: {
            name: { stringValue: "Alpha" },
            population: { integerValue: "7" },
            slug: { stringValue: "alpha" },
          },
          name: "projects/converter-project/databases/typed/documents/cities/alpha",
        },
      },
    ]),
  );
  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        found: {
          fields: {
            name: { stringValue: "Alpha" },
            population: { integerValue: "7" },
            slug: { stringValue: "alpha" },
          },
          name: "projects/converter-project/databases/typed/documents/cities/alpha",
        },
      },
    ]),
  );
  queuedResponses.push(() =>
    createJsonLinesResponse([
      {
        document: {
          fields: {
            name: { stringValue: "Alpha" },
            population: { integerValue: "7" },
            slug: { stringValue: "alpha" },
          },
          name: "projects/converter-project/databases/typed/documents/cities/alpha",
        },
        readTime: "2026-04-25T00:00:00Z",
      },
    ]),
  );
  queuedResponses.push(() =>
    createJsonResponse(200, {
      commitTime: "2026-04-25T00:00:01Z",
      writeResults: [],
    }),
  );
  queuedResponses.push(() =>
    createJsonResponse(200, {
      commitTime: "2026-04-25T00:00:02Z",
      writeResults: [],
    }),
  );

  const convertedSnapshot = await firestoreModule.getDoc(city);
  assert.ok(convertedSnapshot.data() instanceof CityView);
  assert.equal(convertedSnapshot.data()?.slug, "alpha");

  const rawSnapshot = await firestoreModule.getDoc(rawCity);
  assert.deepEqual(rawSnapshot.data(), {
    name: "Alpha",
    population: 7,
    slug: "alpha",
  });

  const convertedQuery = firestoreModule.query(
    firestoreModule.collection(firestore, "cities"),
    firestoreModule.where("population", ">=", 1),
  ).withConverter(cityConverter);
  const queryResults = await firestoreModule.getDocs(convertedQuery);
  assert.equal(queryResults.size, 1);
  assert.ok(queryResults.docs[0].data() instanceof CityView);

  await firestoreModule.setDoc(city, new CityView("Bravo", 9, "bravo"));
  assert.deepEqual(requests[3].body.writes[0].update.fields, {
    name: { stringValue: "Bravo" },
    population: { integerValue: "9" },
    slug: { stringValue: "bravo" },
  });

  await firestoreModule.addDoc(cities, new CityView("Charlie", 11, "charlie"));
  assert.deepEqual(requests[4].body.writes[0].update.fields, {
    name: { stringValue: "Charlie" },
    population: { integerValue: "11" },
    slug: { stringValue: "charlie" },
  });

  await appModule.deleteApp(app);
}

async function testProtobufFoundation(protobufModule) {
  const {
    create,
    fromBinary,
    toBinary,
    firestoreDocumentV1,
    firestoreV1,
  } = protobufModule;

  const commitRequest = create(firestoreV1.CommitRequestSchema, {
    database: "projects/demo-project/databases/(default)",
    writes: [
      {
        operation: {
          case: "update",
          value: {
            name: "projects/demo-project/databases/(default)/documents/cities/SF",
            fields: {
              name: {
                valueType: {
                  case: "stringValue",
                  value: "San Francisco",
                },
              },
              population: {
                valueType: {
                  case: "integerValue",
                  value: 883305n,
                },
              },
            },
          },
        },
      },
    ],
  });
  const commitBytes = toBinary(firestoreV1.CommitRequestSchema, commitRequest);
  const commitRoundTrip = fromBinary(firestoreV1.CommitRequestSchema, commitBytes);
  assert.equal(commitRoundTrip.database, "projects/demo-project/databases/(default)");
  assert.equal(commitRoundTrip.writes.length, 1);
  assert.equal(commitRoundTrip.writes[0]?.operation.case, "update");
  assert.equal(
    commitRoundTrip.writes[0]?.operation.value.fields.name.valueType.case,
    "stringValue",
  );
  assert.equal(
    commitRoundTrip.writes[0]?.operation.value.fields.name.valueType.value,
    "San Francisco",
  );
  assert.equal(
    commitRoundTrip.writes[0]?.operation.value.fields.population.valueType.case,
    "integerValue",
  );
  assert.equal(
    commitRoundTrip.writes[0]?.operation.value.fields.population.valueType.value,
    883305n,
  );

  const listenRequest = create(firestoreV1.ListenRequestSchema, {
    database: "projects/demo-project/databases/(default)",
    labels: {
      "goog-listen-tags": "browser-selftest",
    },
    targetChange: {
      case: "addTarget",
      value: {
        targetId: 7,
        once: true,
        targetType: {
          case: "documents",
          value: {
            documents: [
              "projects/demo-project/databases/(default)/documents/cities/SF",
              "projects/demo-project/databases/(default)/documents/cities/NYC",
            ],
          },
        },
      },
    },
  });
  const listenBytes = toBinary(firestoreV1.ListenRequestSchema, listenRequest);
  const listenRoundTrip = fromBinary(firestoreV1.ListenRequestSchema, listenBytes);
  assert.equal(listenRoundTrip.database, "projects/demo-project/databases/(default)");
  assert.equal(listenRoundTrip.targetChange.case, "addTarget");
  assert.equal(listenRoundTrip.targetChange.value.targetId, 7);
  assert.equal(listenRoundTrip.targetChange.value.once, true);
  assert.equal(listenRoundTrip.targetChange.value.targetType.case, "documents");
  assert.deepEqual(listenRoundTrip.targetChange.value.targetType.value.documents, [
    "projects/demo-project/databases/(default)/documents/cities/SF",
    "projects/demo-project/databases/(default)/documents/cities/NYC",
  ]);
  assert.equal(listenRoundTrip.labels["goog-listen-tags"], "browser-selftest");

  const document = create(firestoreDocumentV1.DocumentSchema, {
    name: "projects/demo-project/databases/(default)/documents/cities/SF",
    fields: {
      nickname: {
        valueType: {
          case: "stringValue",
          value: "Golden Gate",
        },
      },
    },
  });
  const documentBytes = toBinary(firestoreDocumentV1.DocumentSchema, document);
  const documentRoundTrip = fromBinary(firestoreDocumentV1.DocumentSchema, documentBytes);
  assert.equal(documentRoundTrip.fields.nickname.valueType.case, "stringValue");
  assert.equal(documentRoundTrip.fields.nickname.valueType.value, "Golden Gate");
}

async function testGrpcWebUnaryTransportSurface(
  firestoreModule,
  appModule,
  protobufModule,
) {
  const {
    create,
    fromBinary,
    fromJson,
    toBinary,
    toJson,
    firestoreV1,
  } = protobufModule;

  const app = appModule.initializeApp(
    {
      apiKey: "grpc-api-key",
      appId: "grpc-app-id",
      projectId: "grpc-project",
    },
    "grpc-web-runtime",
  );

  const requests = [];
  const tokenCalls = [];
  let commitAttempts = 0;
  const fetch = async (url, options) => {
    const headers = new Headers(options?.headers ?? {});
    requests.push({
      body: options?.body ? decodeGrpcWebUnaryRequest(options.body) : null,
      headers,
      method: options?.method ?? "GET",
      url: String(url),
    });

    if (String(url).endsWith("/Commit")) {
      const request = fromBinary(
        firestoreV1.CommitRequestSchema,
        requests.at(-1).body,
      );
      const requestJson = toJson(firestoreV1.CommitRequestSchema, request);
      assert.equal(requestJson.database, "projects/grpc-project/databases/grpc");
      assert.equal(requestJson.writes.length, 1);
      if (commitAttempts === 0) {
        commitAttempts += 1;
        return new Response(null, {
          status: 401,
          headers: {
            "content-type": "application/grpc-web+proto",
          },
        });
      }
      return createGrpcWebResponse([
        toBinary(
          firestoreV1.CommitResponseSchema,
          create(firestoreV1.CommitResponseSchema, {
            commitTime: {
              nanos: 123456000,
              seconds: 1_777_068_800n,
            },
            writeResults: [],
          }),
        ),
      ]);
    }

    if (String(url).endsWith("/BatchGetDocuments")) {
      const request = fromBinary(
        firestoreV1.BatchGetDocumentsRequestSchema,
        requests.at(-1).body,
      );
      const requestJson = toJson(firestoreV1.BatchGetDocumentsRequestSchema, request);
      assert.equal(requestJson.database, "projects/grpc-project/databases/grpc");
      assert.deepEqual(requestJson.documents, [
        "projects/grpc-project/databases/grpc/documents/cities/SF",
      ]);
      return createGrpcWebResponse([
        toBinary(
          firestoreV1.BatchGetDocumentsResponseSchema,
          fromJson(firestoreV1.BatchGetDocumentsResponseSchema, {
            found: {
              fields: {
                name: { stringValue: "San Francisco" },
                population: { integerValue: "883305" },
              },
              name: "projects/grpc-project/databases/grpc/documents/cities/SF",
            },
          }),
        ),
      ]);
    }

    if (String(url).endsWith("/RunQuery")) {
      const request = fromBinary(firestoreV1.RunQueryRequestSchema, requests.at(-1).body);
      const requestJson = toJson(firestoreV1.RunQueryRequestSchema, request);
      assert.equal(requestJson.parent, "projects/grpc-project/databases/grpc/documents");
      assert.equal(requestJson.structuredQuery.from[0]?.collectionId, "cities");
      return createGrpcWebResponse([
        toBinary(
          firestoreV1.RunQueryResponseSchema,
          fromJson(firestoreV1.RunQueryResponseSchema, {
            document: {
              fields: {
                name: { stringValue: "San Francisco" },
              },
              name: "projects/grpc-project/databases/grpc/documents/cities/SF",
            },
          }),
        ),
        toBinary(
          firestoreV1.RunQueryResponseSchema,
          fromJson(firestoreV1.RunQueryResponseSchema, {
            done: true,
          }),
        ),
      ]);
    }

    throw new Error(`Unexpected gRPC-Web request to ${url}`);
  };

  const firestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalAuthToken: async ({ forceRefresh }) => {
        tokenCalls.push(forceRefresh);
        return forceRefresh ? "fresh-token" : "stale-token";
      },
      experimentalFetch: fetch,
      experimentalUnaryTransport: "grpc-web",
      host: "grpc-web.test",
      ssl: false,
    },
    "grpc",
  );
  const city = firestoreModule.doc(firestoreModule.collection(firestore, "cities"), "SF");

  await firestoreModule.setDoc(city, {
    name: "San Francisco",
    population: 883305,
  });
  const snapshot = await firestoreModule.getDoc(city);
  assert.deepEqual(snapshot.data(), {
    name: "San Francisco",
    population: 883305,
  });
  const querySnapshot = await firestoreModule.getDocs(
    firestoreModule.query(
      firestoreModule.collection(firestore, "cities"),
      firestoreModule.orderBy("name"),
    ),
  );
  assert.equal(querySnapshot.size, 1);
  assert.equal(querySnapshot.docs[0].data().name, "San Francisco");

  assert.deepEqual(tokenCalls, [false, true, false, false]);
  assert.equal(requests[0].url, "http://grpc-web.test/google.firestore.v1.Firestore/Commit");
  assert.equal(requests[0].headers.get("authorization"), "Bearer stale-token");
  assert.equal(requests[1].headers.get("authorization"), "Bearer fresh-token");
  assert.equal(requests[1].headers.get("x-goog-api-key"), "grpc-api-key");
  assert.equal(requests[1].headers.get("x-firebase-gmpid"), "grpc-app-id");
  assert.equal(requests[1].headers.get("x-grpc-web"), "1");
  assert.match(
    requests[1].headers.get("content-type") ?? "",
    /^application\/grpc-web\+proto/i,
  );
  assert.equal(
    requests[2].url,
    "http://grpc-web.test/google.firestore.v1.Firestore/BatchGetDocuments",
  );
  assert.equal(
    requests[3].url,
    "http://grpc-web.test/google.firestore.v1.Firestore/RunQuery",
  );

  const errorFirestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalFetch: async () =>
        createGrpcWebResponse([], new Headers({
          "grpc-message": "permission denied",
          "grpc-status": "7",
        })),
      experimentalUnaryTransport: "grpc-web",
      host: "grpc-web.test",
      ssl: false,
    },
    "grpc-error",
  );

  await assert.rejects(
    () =>
      firestoreModule.setDoc(
        firestoreModule.doc(
          firestoreModule.collection(errorFirestore, "cities"),
          "DENIED",
        ),
        { name: "Denied" },
      ),
    (error) =>
      error instanceof firestoreModule.FirestoreError &&
      error.code === "PERMISSION_DENIED" &&
      error.status === 403,
  );

  await appModule.deleteApp(app);
}

async function testGrpcWebTransactionSurface(
  firestoreModule,
  appModule,
  protobufModule,
) {
  const {
    create,
    fromBinary,
    fromJson,
    toBinary,
    toJson,
    firestoreV1,
  } = protobufModule;

  const app = appModule.initializeApp({ projectId: "grpc-txn-project" }, "grpc-web-transaction");
  const requests = [];
  const firstTransaction = Uint8Array.from([1, 2, 3]);
  const secondTransaction = Uint8Array.from([4, 5, 6]);
  let beginCalls = 0;
  let commitCalls = 0;
  let batchGetCalls = 0;
  let runQueryCalls = 0;
  let rollbackCalls = 0;
  const fetch = async (url, options) => {
    const headers = new Headers(options?.headers ?? {});
    requests.push({
      body: options?.body ? decodeGrpcWebUnaryRequest(options.body) : null,
      headers,
      method: options?.method ?? "GET",
      url: String(url),
    });
    const request = requests.at(-1);
    assert.ok(request, "gRPC-Web transaction request should be recorded");

    if (String(url).endsWith("/BeginTransaction")) {
      const transaction = beginCalls === 0 ? firstTransaction : secondTransaction;
      beginCalls += 1;
      return createGrpcWebResponse([
        toBinary(
          firestoreV1.BeginTransactionResponseSchema,
          create(firestoreV1.BeginTransactionResponseSchema, {
            transaction,
          }),
        ),
      ]);
    }

    if (String(url).endsWith("/BatchGetDocuments")) {
      const requestMessage = fromBinary(
        firestoreV1.BatchGetDocumentsRequestSchema,
        request.body,
      );
      const requestJson = toJson(firestoreV1.BatchGetDocumentsRequestSchema, requestMessage);
      const expectedTransaction = batchGetCalls === 0
        ? Buffer.from(firstTransaction).toString("base64")
        : Buffer.from(secondTransaction).toString("base64");
      batchGetCalls += 1;
      assert.equal(requestJson.transaction, expectedTransaction);
      return createGrpcWebResponse([
        toBinary(
          firestoreV1.BatchGetDocumentsResponseSchema,
          fromJson(firestoreV1.BatchGetDocumentsResponseSchema, {
            found: {
              fields: {
                visits: { integerValue: "7" },
              },
              name: "projects/grpc-txn-project/databases/grpc-txn/documents/cities/SF",
            },
          }),
        ),
      ]);
    }

    if (String(url).endsWith("/RunQuery")) {
      const requestMessage = fromBinary(firestoreV1.RunQueryRequestSchema, request.body);
      const requestJson = toJson(firestoreV1.RunQueryRequestSchema, requestMessage);
      const expectedTransaction = runQueryCalls === 0
        ? Buffer.from(firstTransaction).toString("base64")
        : Buffer.from(secondTransaction).toString("base64");
      runQueryCalls += 1;
      assert.equal(requestJson.transaction, expectedTransaction);
      return createGrpcWebResponse([
        toBinary(
          firestoreV1.RunQueryResponseSchema,
          fromJson(firestoreV1.RunQueryResponseSchema, {
            document: {
              fields: {
                visits: { integerValue: "7" },
              },
              name: "projects/grpc-txn-project/databases/grpc-txn/documents/cities/SF",
            },
          }),
        ),
      ]);
    }

    if (String(url).endsWith("/Commit")) {
      const requestMessage = fromBinary(firestoreV1.CommitRequestSchema, request.body);
      const requestJson = toJson(firestoreV1.CommitRequestSchema, requestMessage);
      const expectedTransaction = commitCalls === 0
        ? Buffer.from(firstTransaction).toString("base64")
        : Buffer.from(secondTransaction).toString("base64");
      commitCalls += 1;
      assert.equal(requestJson.transaction, expectedTransaction);
      if (commitCalls === 1) {
        return createGrpcWebResponse([], new Headers({
          "grpc-message": "transaction conflict",
          "grpc-status": "10",
        }));
      }
      return createGrpcWebResponse([
        toBinary(
          firestoreV1.CommitResponseSchema,
          create(firestoreV1.CommitResponseSchema, {
            commitTime: {
              nanos: 0,
              seconds: 1_777_068_801n,
            },
            writeResults: [],
          }),
        ),
      ]);
    }

    if (String(url).endsWith("/Rollback")) {
      const requestMessage = fromBinary(firestoreV1.RollbackRequestSchema, request.body);
      const requestJson = toJson(firestoreV1.RollbackRequestSchema, requestMessage);
      rollbackCalls += 1;
      assert.equal(
        requestJson.transaction,
        Buffer.from(secondTransaction).toString("base64"),
      );
      return createGrpcWebResponse([new Uint8Array()]);
    }

    throw new Error(`Unexpected gRPC-Web transaction request to ${url}`);
  };

  const firestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalFetch: fetch,
      experimentalUnaryTransport: "grpc-web",
      host: "grpc-web.test",
      ssl: false,
    },
    "grpc-txn",
  );
  const city = firestoreModule.doc(firestoreModule.collection(firestore, "cities"), "SF");
  const citiesQuery = firestoreModule.query(
    firestoreModule.collection(firestore, "cities"),
    firestoreModule.where("visits", ">=", 1),
  );

  let attempts = 0;
  const result = await firestoreModule.runTransaction(
    firestore,
    async (transaction) => {
      attempts += 1;
      const snapshot = await transaction.get(citiesQuery);
      transaction.update(city, {
        visits: Number(snapshot.docs[0]?.data()?.visits ?? 0) + 1,
      });
      return attempts;
    },
    { maxAttempts: 2 },
  );
  assert.equal(result, 2);
  assert.equal(attempts, 2);
  assert.equal(beginCalls, 2);
  assert.equal(commitCalls, 2);

  const readOnlyResult = await firestoreModule.runTransaction(firestore, async (transaction) => {
    const snapshot = await transaction.get(citiesQuery);
    return snapshot.docs[0]?.data()?.visits;
  });
  assert.equal(readOnlyResult, 7);
  assert.equal(rollbackCalls, 1);
  assert.equal(
    requests.at(-1)?.url,
    "http://grpc-web.test/google.firestore.v1.Firestore/Rollback",
  );

  await appModule.deleteApp(app);
}

async function testGrpcWebFieldValueSentinelSurface(
  firestoreModule,
  appModule,
  protobufModule,
) {
  const {
    create,
    fromBinary,
    fromJson,
    toBinary,
    toJson,
    firestoreV1,
  } = protobufModule;

  const app = appModule.initializeApp({ projectId: "grpc-field-value-project" }, "grpc-field-values");
  const requests = [];
  const transactionBytes = Uint8Array.from([9, 8, 7]);
  let beginCalls = 0;
  let commitCalls = 0;
  const fetch = async (url, options) => {
    const headers = new Headers(options?.headers ?? {});
    const body = options?.body ? decodeGrpcWebUnaryRequest(options.body) : null;
    requests.push({
      body,
      headers,
      method: options?.method ?? "GET",
      url: String(url),
    });

    if (String(url).endsWith("/BeginTransaction")) {
      beginCalls += 1;
      return createGrpcWebResponse([
        toBinary(
          firestoreV1.BeginTransactionResponseSchema,
          create(firestoreV1.BeginTransactionResponseSchema, {
            transaction: transactionBytes,
          }),
        ),
      ]);
    }

    if (String(url).endsWith("/Commit")) {
      const requestMessage = fromBinary(firestoreV1.CommitRequestSchema, body);
      const requestJson = toJson(firestoreV1.CommitRequestSchema, requestMessage);
      let responseJson;

      if (commitCalls === 0) {
        assert.deepEqual(requestJson.writes, [
          {
            update: {
              fields: {
                name: { stringValue: "San Francisco" },
              },
              name: "projects/grpc-field-value-project/databases/(default)/documents/cities/SF",
            },
            updateMask: {
              fieldPaths: ["name", "clearedAt"],
            },
            updateTransforms: [
              {
                fieldPath: "updatedAt",
                setToServerValue: "REQUEST_TIME",
              },
            ],
          },
        ]);
        responseJson = {
          commitTime: "2026-04-25T00:00:06Z",
          writeResults: [
            {
              transformResults: [
                {
                  timestampValue: "2026-04-25T00:00:06Z",
                },
              ],
            },
          ],
        };
      } else if (commitCalls === 1) {
        assert.deepEqual(requestJson.writes, [
          {
            update: {
              name: "projects/grpc-field-value-project/databases/(default)/documents/cities/SF",
            },
            updateMask: {
              fieldPaths: ["batchDeleted"],
            },
            updateTransforms: [
              {
                fieldPath: "batchStamp",
                setToServerValue: "REQUEST_TIME",
              },
            ],
          },
          {
            currentDocument: {
              exists: true,
            },
            update: {
              name: "projects/grpc-field-value-project/databases/(default)/documents/cities/SF",
            },
            updateMask: {},
            updateTransforms: [
              {
                appendMissingElements: {
                  values: [{ stringValue: "west" }],
                },
                fieldPath: "tags",
              },
            ],
          },
        ]);
        responseJson = {
          commitTime: "2026-04-25T00:00:06Z",
          writeResults: [
            {
              transformResults: [
                {
                  timestampValue: "2026-04-25T00:00:06Z",
                },
              ],
            },
            {
              transformResults: [
                {
                  arrayValue: {
                    values: [{ stringValue: "west" }],
                  },
                },
              ],
            },
          ],
        };
      } else {
        assert.equal(
          requestJson.transaction,
          Buffer.from(transactionBytes).toString("base64"),
        );
        assert.deepEqual(requestJson.writes, [
          {
            update: {
              name: "projects/grpc-field-value-project/databases/(default)/documents/cities/SF",
            },
            updateMask: {
              fieldPaths: ["txnDeleted"],
            },
            updateTransforms: [
              {
                fieldPath: "txnStamp",
                setToServerValue: "REQUEST_TIME",
              },
            ],
          },
          {
            currentDocument: {
              exists: true,
            },
            update: {
              name: "projects/grpc-field-value-project/databases/(default)/documents/cities/SF",
            },
            updateMask: {},
            updateTransforms: [
              {
                fieldPath: "visits",
                increment: { integerValue: "1" },
              },
            ],
          },
        ]);
        responseJson = {
          commitTime: "2026-04-25T00:00:06Z",
          writeResults: [
            {
              transformResults: [
                {
                  timestampValue: "2026-04-25T00:00:06Z",
                },
              ],
            },
            {
              transformResults: [
                {
                  integerValue: "1",
                },
              ],
            },
          ],
        };
      }

      commitCalls += 1;
      return createGrpcWebResponse([
        toBinary(
          firestoreV1.CommitResponseSchema,
          fromJson(firestoreV1.CommitResponseSchema, responseJson),
        ),
      ]);
    }

    throw new Error(`Unexpected gRPC-Web FieldValue request to ${url}`);
  };

  const firestore = firestoreModule.initializeFirestore(
    app,
    {
      experimentalFetch: fetch,
      experimentalUnaryTransport: "grpc-web",
      host: "grpc-web.test",
      ssl: false,
    },
  );
  const city = firestoreModule.doc(firestoreModule.collection(firestore, "cities"), "SF");

  await firestoreModule.setDoc(
    city,
    {
      clearedAt: firestoreModule.deleteField(),
      name: "San Francisco",
      updatedAt: firestoreModule.serverTimestamp(),
    },
    { merge: true },
  );

  const batch = firestoreModule.writeBatch(firestore);
  batch.set(
    city,
    {
      batchDeleted: firestoreModule.deleteField(),
      batchStamp: firestoreModule.serverTimestamp(),
    },
    { merge: true },
  );
  batch.update(city, {
    tags: firestoreModule.arrayUnion("west"),
  });
  await batch.commit();

  const transactionResult = await firestoreModule.runTransaction(
    firestore,
    async (transaction) => {
      transaction.set(
        city,
        {
          txnDeleted: firestoreModule.deleteField(),
          txnStamp: firestoreModule.serverTimestamp(),
        },
        { merge: true },
      );
      transaction.update(city, {
        visits: firestoreModule.increment(1),
      });
      return "grpc-field-values";
    },
  );

  assert.equal(transactionResult, "grpc-field-values");
  assert.equal(beginCalls, 1);
  assert.equal(commitCalls, 3);
  await appModule.deleteApp(app);
}

async function testListenWatchSurface(firestoreModule, appModule, protobufModule) {
  await withImmediateTimeouts(async (scheduledRetryDelays) => {
    const { create, fromBinary, toBinary, firestoreV1 } = protobufModule;

    const app = appModule.initializeApp(
      {
        apiKey: "listen-api-key",
        appId: "listen-app-id",
        projectId: "listen-project",
      },
      "listen-runtime",
    );

    const sockets = [];
    const firestore = firestoreModule.initializeFirestore(
      app,
      {
        experimentalWebSocketFactory: (url, protocols) => {
          const socket = new FakeWebSocket(url, protocols);
          sockets.push(socket);
          return socket;
        },
        host: "listen.test",
        ssl: false,
      },
      "listen",
    );

    const cities = firestoreModule.collection(firestore, "cities");
    const city = firestoreModule.doc(cities, "SF");

  const documentSnapshotResult = deferred();
  const documentErrors = [];
  const unsubscribeDocument = firestoreModule.onSnapshot(
    city,
    (snapshot) => documentSnapshotResult.resolve(snapshot),
    (error) => documentErrors.push(error),
  );
  await flushMicrotasks();
  assert.equal(sockets.length, 1);
  const documentSocket = sockets[0];
  assert.equal(
    documentSocket.url,
    "ws://listen.test/google.firestore.v1.Firestore/Listen",
  );
  assert.deepEqual(documentSocket.protocols, ["nimbus.firebase.listen.v1"]);

  documentSocket.emitOpen();
  await flushMicrotasks();
  assert.equal(documentSocket.sentFrames.length, 1);
  const addDocumentRequest = fromBinary(
    firestoreV1.ListenRequestSchema,
    documentSocket.sentFrames[0],
  );
  assert.equal(
    addDocumentRequest.database,
    "projects/listen-project/databases/listen",
  );
  assert.equal(addDocumentRequest.targetChange.case, "addTarget");
  assert.equal(addDocumentRequest.targetChange.value.targetId, 1);
  assert.equal(addDocumentRequest.targetChange.value.targetType.case, "documents");
  assert.deepEqual(addDocumentRequest.targetChange.value.targetType.value.documents, [
    "projects/listen-project/databases/listen/documents/cities/SF",
  ]);

  documentSocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "targetChange",
          value: {
            targetChangeType: firestoreV1.TargetChange_TargetChangeType.ADD,
            targetIds: [1],
          },
        },
      }),
    ),
  );
  documentSocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "documentChange",
          value: {
            document: {
              name: "projects/listen-project/databases/listen/documents/cities/SF",
              fields: {
                name: {
                  valueType: {
                    case: "stringValue",
                    value: "San Francisco",
                  },
                },
                population: {
                  valueType: {
                    case: "integerValue",
                    value: 883305n,
                  },
                },
              },
            },
            targetIds: [1],
          },
        },
      }),
    ),
  );
  documentSocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "targetChange",
          value: {
            targetChangeType: firestoreV1.TargetChange_TargetChangeType.CURRENT,
            targetIds: [1],
          },
        },
      }),
    ),
  );

  const documentSnapshot = await documentSnapshotResult.promise;
  assert.equal(documentErrors.length, 0);
  assert.equal(documentSnapshot.exists(), true);
  assert.equal(documentSnapshot.ref.path, "cities/SF");
  assert.deepEqual(documentSnapshot.data(), {
    name: "San Francisco",
    population: 883305,
  });
  assert.equal(documentSnapshot.metadata.fromCache, false);
  assert.equal(documentSnapshot.metadata.hasPendingWrites, false);

  unsubscribeDocument();
  await flushMicrotasks();
  assert.equal(documentSocket.closed, true);
  assert.equal(documentSocket.sentFrames.length, 2);
  const removeDocumentRequest = fromBinary(
    firestoreV1.ListenRequestSchema,
    documentSocket.sentFrames[1],
  );
  assert.equal(removeDocumentRequest.targetChange.case, "removeTarget");
  assert.equal(removeDocumentRequest.targetChange.value, 1);

  const querySnapshotResult = deferred();
  const queryErrors = [];
  const unsubscribeQuery = firestoreModule.onSnapshot(
    firestoreModule.query(cities, firestoreModule.orderBy("name")),
    (snapshot) => querySnapshotResult.resolve(snapshot),
    (error) => queryErrors.push(error),
  );
  await flushMicrotasks();
  assert.equal(sockets.length, 2);
  const querySocket = sockets[1];
  assert.deepEqual(querySocket.protocols, ["nimbus.firebase.listen.v1"]);

  querySocket.emitOpen();
  await flushMicrotasks();
  assert.equal(querySocket.sentFrames.length, 1);
  const addQueryRequest = fromBinary(
    firestoreV1.ListenRequestSchema,
    querySocket.sentFrames[0],
  );
  assert.equal(addQueryRequest.targetChange.case, "addTarget");
  assert.equal(addQueryRequest.targetChange.value.targetType.case, "query");
  assert.equal(
    addQueryRequest.targetChange.value.targetType.value.parent,
    "projects/listen-project/databases/listen/documents",
  );
  assert.equal(
    addQueryRequest.targetChange.value.targetType.value.queryType.case,
    "structuredQuery",
  );
  assert.equal(
    addQueryRequest.targetChange.value.targetType.value.queryType.value.orderBy[0]?.field
      ?.fieldPath,
    "name",
  );

  querySocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "targetChange",
          value: {
            targetChangeType: firestoreV1.TargetChange_TargetChangeType.ADD,
            targetIds: [1],
          },
        },
      }),
    ),
  );
  querySocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "documentChange",
          value: {
            document: {
              name: "projects/listen-project/databases/listen/documents/cities/SF",
              fields: {
                name: {
                  valueType: {
                    case: "stringValue",
                    value: "San Francisco",
                  },
                },
              },
            },
            targetIds: [1],
          },
        },
      }),
    ),
  );
  querySocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "documentChange",
          value: {
            document: {
              name: "projects/listen-project/databases/listen/documents/cities/LA",
              fields: {
                name: {
                  valueType: {
                    case: "stringValue",
                    value: "Los Angeles",
                  },
                },
              },
            },
            targetIds: [1],
          },
        },
      }),
    ),
  );
  querySocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "targetChange",
          value: {
            targetChangeType: firestoreV1.TargetChange_TargetChangeType.CURRENT,
            targetIds: [1],
          },
        },
      }),
    ),
  );

  const querySnapshot = await querySnapshotResult.promise;
  assert.equal(queryErrors.length, 0);
  assert.equal(querySnapshot.size, 2);
  assert.deepEqual(
    querySnapshot.docs.map((snapshot) => snapshot.data().name),
    ["Los Angeles", "San Francisco"],
  );
  assert.equal(querySnapshot.metadata.fromCache, false);
  assert.equal(querySnapshot.metadata.hasPendingWrites, false);

  unsubscribeQuery();
  await flushMicrotasks();
  assert.equal(querySocket.closed, true);
  assert.equal(querySocket.sentFrames.length, 2);
  const removeQueryRequest = fromBinary(
    firestoreV1.ListenRequestSchema,
    querySocket.sentFrames[1],
  );
  assert.equal(removeQueryRequest.targetChange.case, "removeTarget");
  assert.equal(removeQueryRequest.targetChange.value, 1);

  const reconnectSnapshots = [];
  const reconnectErrors = [];
  const unsubscribeReconnectQuery = firestoreModule.onSnapshot(
    firestoreModule.query(cities, firestoreModule.orderBy("name")),
    (snapshot) => reconnectSnapshots.push(snapshot),
    (error) => reconnectErrors.push(error),
  );
  await flushMicrotasks();
  assert.equal(sockets.length, 3);
  const reconnectQuerySocket = sockets[2];

  reconnectQuerySocket.emitOpen();
  await flushMicrotasks();
  const initialReconnectRequest = fromBinary(
    firestoreV1.ListenRequestSchema,
    reconnectQuerySocket.sentFrames[0],
  );
  assert.equal(
    initialReconnectRequest.targetChange.value.resumeType.case,
    undefined,
  );

  reconnectQuerySocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "targetChange",
          value: {
            targetChangeType: firestoreV1.TargetChange_TargetChangeType.ADD,
            targetIds: [1],
          },
        },
      }),
    ),
  );
  reconnectQuerySocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "documentChange",
          value: {
            document: {
              name: "projects/listen-project/databases/listen/documents/cities/SF",
              fields: {
                name: {
                  valueType: {
                    case: "stringValue",
                    value: "San Francisco",
                  },
                },
              },
            },
            targetIds: [1],
          },
        },
      }),
    ),
  );
  reconnectQuerySocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "targetChange",
          value: {
            resumeToken: new Uint8Array([1, 2, 3]),
            readTime: {
              seconds: 1_745_452_800n,
              nanos: 123,
            },
            targetChangeType: firestoreV1.TargetChange_TargetChangeType.CURRENT,
            targetIds: [1],
          },
        },
      }),
    ),
  );
  await flushMicrotasks();
  assert.equal(reconnectErrors.length, 0);
  assert.equal(reconnectSnapshots.length, 1);
  assert.deepEqual(
    reconnectSnapshots[0].docs.map((snapshot) => snapshot.data().name),
    ["San Francisco"],
  );

  reconnectQuerySocket.emitClose(1006, "dropped");
  await flushMicrotasks();
  assert.equal(sockets.length, 4);
  const resumedQuerySocket = sockets[3];

  resumedQuerySocket.emitOpen();
  await flushMicrotasks();
  const resumedQueryRequest = fromBinary(
    firestoreV1.ListenRequestSchema,
    resumedQuerySocket.sentFrames[0],
  );
  assert.equal(
    resumedQueryRequest.targetChange.value.resumeType.case,
    "resumeToken",
  );
  assert.deepEqual(
    Array.from(resumedQueryRequest.targetChange.value.resumeType.value),
    [1, 2, 3],
  );

  resumedQuerySocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "documentChange",
          value: {
            document: {
              name: "projects/listen-project/databases/listen/documents/cities/LA",
              fields: {
                name: {
                  valueType: {
                    case: "stringValue",
                    value: "Los Angeles",
                  },
                },
              },
            },
            targetIds: [1],
          },
        },
      }),
    ),
  );
  await flushMicrotasks();
  assert.equal(reconnectSnapshots.length, 1);

  resumedQuerySocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "targetChange",
          value: {
            resumeToken: new Uint8Array([4, 5, 6]),
            readTime: {
              seconds: 1_745_452_801n,
              nanos: 456,
            },
            targetChangeType: firestoreV1.TargetChange_TargetChangeType.CURRENT,
            targetIds: [1],
          },
        },
      }),
    ),
  );
  await flushMicrotasks();
  assert.equal(reconnectErrors.length, 0);
  assert.equal(reconnectSnapshots.length, 2);
  assert.deepEqual(
    reconnectSnapshots[1].docs.map((snapshot) => snapshot.data().name),
    ["Los Angeles", "San Francisco"],
  );

  unsubscribeReconnectQuery();
  await flushMicrotasks();
  assert.equal(resumedQuerySocket.closed, true);
  resumedQuerySocket.emitClose(1006, "after unsubscribe");
  await flushMicrotasks();
  assert.equal(sockets.length, 4);

  const readTimeSnapshots = [];
  const readTimeErrors = [];
  const unsubscribeReadTimeDocument = firestoreModule.onSnapshot(
    city,
    (snapshot) => readTimeSnapshots.push(snapshot),
    (error) => readTimeErrors.push(error),
  );
  await flushMicrotasks();
  assert.equal(sockets.length, 5);
  const readTimeDocumentSocket = sockets[4];

  readTimeDocumentSocket.emitOpen();
  await flushMicrotasks();
  const initialReadTimeRequest = fromBinary(
    firestoreV1.ListenRequestSchema,
    readTimeDocumentSocket.sentFrames[0],
  );
  assert.equal(
    initialReadTimeRequest.targetChange.value.resumeType.case,
    undefined,
  );

  readTimeDocumentSocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "targetChange",
          value: {
            targetChangeType: firestoreV1.TargetChange_TargetChangeType.ADD,
            targetIds: [1],
          },
        },
      }),
    ),
  );
  readTimeDocumentSocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "documentChange",
          value: {
            document: {
              name: "projects/listen-project/databases/listen/documents/cities/SF",
              fields: {
                name: {
                  valueType: {
                    case: "stringValue",
                    value: "San Francisco",
                  },
                },
              },
            },
            targetIds: [1],
          },
        },
      }),
    ),
  );
  readTimeDocumentSocket.emitBinary(
    toBinary(
      firestoreV1.ListenResponseSchema,
      create(firestoreV1.ListenResponseSchema, {
        responseType: {
          case: "targetChange",
          value: {
            readTime: {
              seconds: 1_745_452_802n,
              nanos: 789,
            },
            targetChangeType: firestoreV1.TargetChange_TargetChangeType.CURRENT,
            targetIds: [1],
          },
        },
      }),
    ),
  );
  await flushMicrotasks();
  assert.equal(readTimeErrors.length, 0);
  assert.equal(readTimeSnapshots.length, 1);
  assert.equal(readTimeSnapshots[0].exists(), true);

  readTimeDocumentSocket.emitClose(1006, "dropped");
  await flushMicrotasks();
  assert.equal(sockets.length, 6);
  const resumedReadTimeSocket = sockets[5];

  resumedReadTimeSocket.emitOpen();
  await flushMicrotasks();
  const resumedReadTimeRequest = fromBinary(
    firestoreV1.ListenRequestSchema,
    resumedReadTimeSocket.sentFrames[0],
  );
  assert.equal(
    resumedReadTimeRequest.targetChange.value.resumeType.case,
    "readTime",
  );
  assert.equal(
    resumedReadTimeRequest.targetChange.value.resumeType.value.seconds,
    1_745_452_802n,
  );
  assert.equal(
    resumedReadTimeRequest.targetChange.value.resumeType.value.nanos,
    789,
  );

  unsubscribeReadTimeDocument();
  await flushMicrotasks();
  assert.equal(resumedReadTimeSocket.closed, true);

    const fatalPolicyErrors = [];
    const unsubscribeFatalPolicy = firestoreModule.onSnapshot(
      city,
      () => {
        throw new Error("Policy-close watch should not deliver a snapshot.");
      },
      (error) => fatalPolicyErrors.push(error),
    );
    await flushMicrotasks();
    assert.equal(sockets.length, 7);
    const fatalPolicySocket = sockets[6];
    fatalPolicySocket.emitOpen();
    await flushMicrotasks();
    fatalPolicySocket.emitClose(1008, "request-level policy failure");
    await flushMicrotasks();
    assert.equal(fatalPolicyErrors.length, 1);
    assert.equal(fatalPolicyErrors[0].code, "FAILED_PRECONDITION");
    assert.equal(fatalPolicyErrors[0].status, 400);
    assert.equal(sockets.length, 7);
    unsubscribeFatalPolicy();
    await flushMicrotasks();

    const unsupportedFrameErrors = [];
    const unsubscribeUnsupportedFrame = firestoreModule.onSnapshot(
      city,
      () => {
        throw new Error("Unsupported-close watch should not deliver a snapshot.");
      },
      (error) => unsupportedFrameErrors.push(error),
    );
    await flushMicrotasks();
    assert.equal(sockets.length, 8);
    const unsupportedFrameSocket = sockets[7];
    unsupportedFrameSocket.emitOpen();
    await flushMicrotasks();
    unsupportedFrameSocket.emitClose(1003, "binary protobuf required");
    await flushMicrotasks();
    assert.equal(unsupportedFrameErrors.length, 1);
    assert.equal(unsupportedFrameErrors[0].code, "INVALID_ARGUMENT");
    assert.equal(unsupportedFrameErrors[0].status, 400);
    assert.equal(sockets.length, 8);
    unsubscribeUnsupportedFrame();
    await flushMicrotasks();

    const retryErrors = [];
    const unsubscribeRetryBudget = firestoreModule.onSnapshot(
      city,
      () => {
        throw new Error("Retry-budget watch should not deliver a snapshot.");
      },
      (error) => retryErrors.push(error),
    );
    await flushMicrotasks();
    assert.equal(sockets.length, 9);
    const retrySocketOne = sockets[8];
    retrySocketOne.emitOpen();
    await flushMicrotasks();
    retrySocketOne.emitClose(1011, "backpressure 1");
    await flushMicrotasks();
    assert.equal(sockets.length, 10);

    const retrySocketTwo = sockets[9];
    retrySocketTwo.emitOpen();
    await flushMicrotasks();
    retrySocketTwo.emitClose(1011, "backpressure 2");
    await flushMicrotasks();
    assert.equal(sockets.length, 11);

    const retrySocketThree = sockets[10];
    retrySocketThree.emitOpen();
    await flushMicrotasks();
    retrySocketThree.emitClose(1011, "backpressure 3");
    await flushMicrotasks();
    assert.equal(sockets.length, 12);
    assert.deepEqual(scheduledRetryDelays.slice(-3), [0, 50, 250]);

    const retrySocketFour = sockets[11];
    retrySocketFour.emitOpen();
    await flushMicrotasks();
    retrySocketFour.emitClose(1011, "backpressure exhausted");
    await flushMicrotasks();
    assert.equal(retryErrors.length, 1);
    assert.equal(retryErrors[0].code, "UNAVAILABLE");
    assert.equal(retryErrors[0].status, 503);
    assert.equal(retryErrors[0].message, "backpressure exhausted");
    assert.equal(sockets.length, 12);
    unsubscribeRetryBudget();
    await flushMicrotasks();

    const listenAuthCalls = [];
    const authSockets = [];
    const authFirestore = firestoreModule.initializeFirestore(
      app,
      {
        experimentalAuthToken: async ({ forceRefresh }) => {
          listenAuthCalls.push(forceRefresh);
          return forceRefresh ? "fresh-listen-token" : "stale-listen-token";
        },
        experimentalWebSocketFactory: (url, protocols) => {
          const socket = new FakeWebSocket(url, protocols);
          authSockets.push(socket);
          return socket;
        },
        host: "listen-auth.test",
        ssl: false,
      },
      "listen-auth",
    );
    const authCity = firestoreModule.doc(
      firestoreModule.collection(authFirestore, "cities"),
      "SFO",
    );
    const authSnapshots = [];
    const authErrors = [];
    const unsubscribeAuthWatch = firestoreModule.onSnapshot(
      authCity,
      (snapshot) => authSnapshots.push(snapshot),
      (error) => authErrors.push(error),
    );
    await flushMicrotasks();
    assert.equal(authSockets.length, 1);
    assert.deepEqual(authSockets[0].protocols[0], "nimbus.firebase.listen.v1");
    assert.equal(
      decodeListenAuthSubprotocol(authSockets[0].protocols),
      "stale-listen-token",
    );

    authSockets[0].emitOpen();
    await flushMicrotasks();
    authSockets[0].emitClose(1008, "Firestore Listen unauthenticated.");
    await flushMicrotasks();
    await flushMicrotasks();
    assert.equal(authSockets.length, 2);
    assert.deepEqual(listenAuthCalls, [false, true]);
    assert.equal(
      decodeListenAuthSubprotocol(authSockets[1].protocols),
      "fresh-listen-token",
    );

    authSockets[1].emitOpen();
    await flushMicrotasks();
    authSockets[1].emitBinary(
      toBinary(
        firestoreV1.ListenResponseSchema,
        create(firestoreV1.ListenResponseSchema, {
          responseType: {
            case: "targetChange",
            value: {
              targetChangeType: firestoreV1.TargetChange_TargetChangeType.ADD,
              targetIds: [1],
            },
          },
        }),
      ),
    );
    authSockets[1].emitBinary(
      toBinary(
        firestoreV1.ListenResponseSchema,
        create(firestoreV1.ListenResponseSchema, {
          responseType: {
            case: "documentChange",
            value: {
              document: {
                name: "projects/listen-project/databases/listen-auth/documents/cities/SFO",
                fields: {
                  name: {
                    valueType: {
                      case: "stringValue",
                      value: "San Francisco Authenticated",
                    },
                  },
                },
              },
              targetIds: [1],
            },
          },
        }),
      ),
    );
    authSockets[1].emitBinary(
      toBinary(
        firestoreV1.ListenResponseSchema,
        create(firestoreV1.ListenResponseSchema, {
          responseType: {
            case: "targetChange",
            value: {
              targetChangeType: firestoreV1.TargetChange_TargetChangeType.CURRENT,
              targetIds: [1],
            },
          },
        }),
      ),
    );
    await flushMicrotasks();
    assert.equal(authErrors.length, 0);
    assert.equal(authSnapshots.length, 1);
    assert.equal(authSnapshots[0].data().name, "San Francisco Authenticated");

    authSockets[1].emitClose(1008, "Firestore Listen unauthenticated.");
    await flushMicrotasks();
    await flushMicrotasks();
    assert.equal(authSockets.length, 3);
    assert.deepEqual(listenAuthCalls, [false, true, true]);
    assert.equal(
      decodeListenAuthSubprotocol(authSockets[2].protocols),
      "fresh-listen-token",
    );
    authSockets[2].emitOpen();
    await flushMicrotasks();
    authSockets[2].emitClose(1008, "Firestore Listen unauthenticated.");
    await flushMicrotasks();
    await flushMicrotasks();
    assert.equal(authErrors.length, 1);
    assert.equal(authErrors[0].code, "UNAUTHENTICATED");
    assert.equal(authErrors[0].status, 401);
    unsubscribeAuthWatch();
    await flushMicrotasks();

    await appModule.deleteApp(app);
  });
}

async function runFieldValueSmokeFlow(
  firestoreModule,
  firestore,
  documentReference,
) {
  await firestoreModule.setDoc(documentReference, {
    batchDelete: "remove-batch",
    count: 1,
    legacy: "old",
    name: "Transform City",
    tags: ["seed"],
    txnDelete: "remove-txn",
  });

  await firestoreModule.setDoc(
    documentReference,
    {
      mergeDelete: firestoreModule.deleteField(),
      mergeStamp: firestoreModule.serverTimestamp(),
    },
    { merge: true },
  );
  let snapshot = await firestoreModule.getDoc(documentReference);
  let data = snapshot.data();
  assert.equal(data.count, 1);
  assert.equal(data.legacy, "old");
  assert.deepEqual(data.tags, ["seed"]);
  assert.equal(data.mergeDelete, undefined);
  assert.equal(typeof data.mergeStamp, "string");

  await firestoreModule.updateDoc(documentReference, {
    count: firestoreModule.increment(2),
    legacy: firestoreModule.deleteField(),
    tags: firestoreModule.arrayUnion("north"),
    updatedAt: firestoreModule.serverTimestamp(),
  });
  snapshot = await firestoreModule.getDoc(documentReference);
  data = snapshot.data();
  assert.equal(data.count, 3);
  assert.equal(data.legacy, undefined);
  assert.deepEqual(data.tags, ["seed", "north"]);
  assert.equal(typeof data.updatedAt, "string");

  const batch = firestoreModule.writeBatch(firestore);
  batch.set(
    documentReference,
    {
      batchDelete: firestoreModule.deleteField(),
      batchStamp: firestoreModule.serverTimestamp(),
    },
    { merge: true },
  );
  batch.update(documentReference, {
    count: firestoreModule.increment(1),
    tags: firestoreModule.arrayRemove("seed"),
  });
  await batch.commit();
  snapshot = await firestoreModule.getDoc(documentReference);
  data = snapshot.data();
  assert.equal(data.count, 4);
  assert.equal(data.batchDelete, undefined);
  assert.deepEqual(data.tags, ["north"]);
  assert.equal(typeof data.batchStamp, "string");

  const priorCount = await firestoreModule.runTransaction(
    firestore,
    async (transaction) => {
      const current = await transaction.get(documentReference);
      transaction.set(
        documentReference,
        {
          txnDelete: firestoreModule.deleteField(),
          txnStamp: firestoreModule.serverTimestamp(),
        },
        { merge: true },
      );
      transaction.update(documentReference, {
        count: firestoreModule.increment(1),
        tags: firestoreModule.arrayUnion("txn"),
      });
      return Number(current.get("count") ?? 0);
    },
  );
  assert.equal(priorCount, 4);
  snapshot = await firestoreModule.getDoc(documentReference);
  data = snapshot.data();
  assert.equal(data.count, 5);
  assert.equal(data.txnDelete, undefined);
  assert.deepEqual(data.tags, ["north", "txn"]);
  assert.equal(typeof data.txnStamp, "string");
}

async function testSmokeSurface(bundleDir, smokeBaseUrl) {
  const appModule = await import(pathToFileURL(path.join(bundleDir, "app.mjs")).href);
  const firestoreModule = await import(pathToFileURL(path.join(bundleDir, "firestore.mjs")).href);
  const baseUrl = new URL(smokeBaseUrl);
  const smokeAuthToken = process.env.NIMBUS_FIREBASE_SMOKE_MOCK_USER_TOKEN;
  assert.ok(baseUrl.hostname, "Smoke base URL must include a hostname.");
  assert.ok(baseUrl.port, "Smoke base URL must include an explicit port.");

  const app = appModule.initializeApp({ projectId: "demo" }, "smoke");
  const firestore = firestoreModule.getFirestore(app);
  firestoreModule.connectFirestoreEmulator(
    firestore,
    baseUrl.hostname,
    Number.parseInt(baseUrl.port, 10),
  );

  const cities = firestoreModule.collection(firestore, "cities.v2");
  const city = firestoreModule.doc(cities, "日本語 __.SF");

  await firestoreModule.setDoc(city, {
    count: 1,
    displayName: "Tokyo",
    nested: {
      active: true,
    },
  });

  const initial = await firestoreModule.getDoc(city);
  assert.equal(initial.exists(), true);
  assert.deepEqual(initial.data(), {
    count: 1,
    displayName: "Tokyo",
    nested: {
      active: true,
    },
  });

  await firestoreModule.updateDoc(city, {
    "nested.active": false,
    count: 2,
  });
  const updated = await firestoreModule.getDoc(city);
  assert.deepEqual(updated.data(), {
    count: 2,
    displayName: "Tokyo",
    nested: {
      active: false,
    },
  });

  const landmarks = firestoreModule.collection(city, "landmarks.__");
  const landmark = await firestoreModule.addDoc(landmarks, {
    category: "tower",
    name: "Skytree",
  });
  assert.match(landmark.id, /^[A-Za-z0-9]{20}$/u);
  const landmarkSnapshot = await firestoreModule.getDoc(landmark);
  assert.equal(landmarkSnapshot.exists(), true);
  assert.deepEqual(landmarkSnapshot.data(), {
    category: "tower",
    name: "Skytree",
  });

  const cityResults = await firestoreModule.getDocs(
    firestoreModule.query(
      cities,
      firestoreModule.orderBy("count"),
      firestoreModule.limit(1),
      firestoreModule.startAt(2),
    ),
  );
  assert.equal(cityResults.empty, false);
  assert.equal(cityResults.size, 1);
  assert.equal(cityResults.metadata.fromCache, false);
  assert.equal(cityResults.metadata.hasPendingWrites, false);
  assert.equal(cityResults.docs[0].ref.path, "cities.v2/日本語 __.SF");
  assert.deepEqual(cityResults.docs[0].data(), {
    count: 2,
    displayName: "Tokyo",
    nested: {
      active: false,
    },
  });

  const landmarkResults = await firestoreModule.getDocs(
    firestoreModule.query(
      firestoreModule.collectionGroup(firestore, "landmarks.__"),
      firestoreModule.orderBy(firestoreModule.documentId()),
      firestoreModule.startAt(
        "projects/demo/databases/(default)/documents/cities.v2/日本語 __.SF/landmarks.__/00000000000000000000",
      ),
    ),
  );
  assert.equal(landmarkResults.size, 1);
  assert.equal(
    landmarkResults.docs[0].ref.path,
    `cities.v2/日本語 __.SF/landmarks.__/${landmark.id}`,
  );
  assert.deepEqual(landmarkResults.docs[0].data(), {
    category: "tower",
    name: "Skytree",
  });

  const emptyNestedResults = await firestoreModule.getDocs(
    firestoreModule.collection(
      firestoreModule.doc(firestoreModule.collection(firestore, "cities"), "missing"),
      "landmarks.__",
    ),
  );
  assert.equal(emptyNestedResults.empty, true);
  assert.equal(emptyNestedResults.size, 0);

  await firestoreModule.deleteDoc(city);
  const deleted = await firestoreModule.getDoc(city);
  assert.equal(deleted.exists(), false);

  const oakland = firestoreModule.doc(cities, "oak");
  const sanJose = firestoreModule.doc(cities, "sj");
  const smokeBatch = firestoreModule.writeBatch(firestore);
  smokeBatch.set(oakland, { name: "Oakland", visits: 1 });
  smokeBatch.set(sanJose, { name: "San Jose", visits: 2 });
  await smokeBatch.commit();
  const oaklandSnapshot = await firestoreModule.getDoc(oakland);
  const sanJoseSnapshot = await firestoreModule.getDoc(sanJose);
  assert.deepEqual(oaklandSnapshot.data(), { name: "Oakland", visits: 1 });
  assert.deepEqual(sanJoseSnapshot.data(), { name: "San Jose", visits: 2 });

  const transactionResult = await firestoreModule.runTransaction(
    firestore,
    async (transaction) => {
      const snapshot = await transaction.get(
        firestoreModule.query(cities, firestoreModule.where("name", "==", "Oakland")),
      );
      transaction.update(oakland, {
        visits: Number(snapshot.docs[0]?.data()?.visits ?? 0) + 1,
      });
      return snapshot.docs[0]?.data()?.name;
    },
    { maxAttempts: 2 },
  );
  assert.equal(transactionResult, "Oakland");
  const oaklandAfterTransaction = await firestoreModule.getDoc(oakland);
  assert.deepEqual(oaklandAfterTransaction.data(), { name: "Oakland", visits: 2 });

  const restTransformCity = firestoreModule.doc(cities, "transform-rest");
  await runFieldValueSmokeFlow(firestoreModule, firestore, restTransformCity);

  await assert.rejects(
    () =>
      firestoreModule.runTransaction(firestore, async (transaction) => {
        const snapshot = await transaction.get(sanJose);
        transaction.update(sanJose, {
          visits: Number(snapshot.data()?.visits ?? 0) + 5,
        });
        throw new Error("smoke rollback");
      }),
    /smoke rollback/u,
  );
  const sanJoseAfterRollback = await firestoreModule.getDoc(sanJose);
  assert.deepEqual(sanJoseAfterRollback.data(), { name: "San Jose", visits: 2 });

  await appModule.deleteApp(app);

  const grpcApp = appModule.initializeApp({ projectId: "demo" }, "smoke-grpc");
  const grpcFirestore = firestoreModule.initializeFirestore(grpcApp, {
    experimentalUnaryTransport: "grpc-web",
    host: baseUrl.host,
    ssl: false,
  });
  const grpcCities = firestoreModule.collection(grpcFirestore, "cities.v2");
  const grpcTransformCity = firestoreModule.doc(grpcCities, "transform-grpc");
  await runFieldValueSmokeFlow(firestoreModule, grpcFirestore, grpcTransformCity);
  await appModule.deleteApp(grpcApp);

  if (smokeAuthToken) {
    let parsedSmokeAuthToken;
    try {
      parsedSmokeAuthToken = JSON.parse(smokeAuthToken);
    } catch {
      parsedSmokeAuthToken = smokeAuthToken;
    }

    const secureCities = firestoreModule.collection(firestore, "secureSmoke");
    const secureCity = firestoreModule.doc(secureCities, "owned");
    const anonymousSecureSnapshot = await firestoreModule.getDoc(secureCity);
    assert.equal(
      anonymousSecureSnapshot.exists(),
      false,
      "Anonymous smoke client should not see protected documents.",
    );

    const authApp = appModule.initializeApp({ projectId: "demo" }, "smoke-auth");
    const authFirestore = firestoreModule.getFirestore(authApp);
    firestoreModule.connectFirestoreEmulator(
      authFirestore,
      baseUrl.hostname,
      Number.parseInt(baseUrl.port, 10),
      { mockUserToken: parsedSmokeAuthToken },
    );
    const authSecureCity = firestoreModule.doc(
      firestoreModule.collection(authFirestore, "secureSmoke"),
      "owned",
    );
    await firestoreModule.setDoc(authSecureCity, {
      owner: "user-1",
      name: "Authenticated Smoke City",
    });
    const authenticatedSecureSnapshot = await firestoreModule.getDoc(authSecureCity);
    assert.equal(authenticatedSecureSnapshot.exists(), true);
    assert.deepEqual(authenticatedSecureSnapshot.data(), {
      owner: "user-1",
      name: "Authenticated Smoke City",
    });

    const anonymousSecureSnapshotAfterWrite = await firestoreModule.getDoc(secureCity);
    assert.equal(
      anonymousSecureSnapshotAfterWrite.exists(),
      false,
      "Anonymous smoke client should stay filtered after authenticated writes.",
    );

    await appModule.deleteApp(authApp);
  }
}

async function typecheckFirebaseSurface() {
  const fixtureDir = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-firebase-ts-"));
  const normalize = (target) => path.relative(fixtureDir, target).replaceAll("\\", "/");
  const rootEntry = normalize(path.join(packageRoot, "src", "index.ts"));
  const appEntry = normalize(path.join(packageRoot, "src", "app.ts"));
  const firestoreEntry = normalize(path.join(packageRoot, "src", "firestore.ts"));

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
          lib: ["ES2022", "DOM"],
          paths: {
            "@nimbus/firebase": [rootEntry],
            "@nimbus/firebase/app": [appEntry],
            "@nimbus/firebase/firestore": [firestoreEntry],
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
import {
  deleteApp,
  getApp,
  getApps,
  initializeApp,
  type FirebaseApp,
  type FirebaseOptions,
} from "@nimbus/firebase/app";
import {
  addDoc,
  arrayRemove,
  arrayUnion,
  collection,
  collectionGroup,
  connectFirestoreEmulator,
  deleteField,
  deleteDoc,
  documentId,
  doc,
  endAt,
  endBefore,
  type FieldValue,
  getDoc,
  getDocs,
  getFirestore,
  increment,
  initializeFirestore,
  limit,
  onSnapshot,
  orderBy,
  query,
  runTransaction,
  serverTimestamp,
  setDoc,
  startAfter,
  startAt,
  terminate,
  updateDoc,
  writeBatch,
  where,
  type CollectionGroup,
  type CollectionReference,
  type DocumentSnapshot,
  type QueryDocumentSnapshot,
  type DocumentReference,
  FirestoreError,
  type FirestoreDataConverter,
  type Firestore,
  type Transaction,
  type TransactionOptions,
  type QuerySnapshot,
  type Query,
  type QueryConstraint,
  type FirestoreSettings,
  type SnapshotObserver,
  type SetOptions,
  type Unsubscribe,
  type WriteBatch,
} from "@nimbus/firebase/firestore";
import {
  getFirestore as getFirestoreFromRoot,
  initializeApp as initializeAppFromRoot,
} from "@nimbus/firebase";

const options: FirebaseOptions = {
  apiKey: "demo-key",
  projectId: "demo-project",
};

const app: FirebaseApp = initializeApp(options);
const namedApp = initializeAppFromRoot({ projectId: "other-project" }, "other");
const firestore: Firestore = getFirestore(app);
const settings: FirestoreSettings = {
  ignoreUndefinedProperties: true,
};
const setOptions: SetOptions = {
  merge: true,
};
const initialized: Firestore = initializeFirestore(namedApp, settings, "analytics");
const cities: CollectionReference = collection(firestore, "cities");
const city: DocumentReference = doc(cities, "SF");
const landmarks: CollectionReference = collection(city, "landmarks");
const group: CollectionGroup = collectionGroup(firestore, "landmarks");
const queryConstraint: QueryConstraint = where("state", "==", "CA");
const citiesQuery: Query = query(
  cities,
  queryConstraint,
  orderBy(documentId()),
  limit(10),
  startAt("projects/demo/databases/(default)/documents/cities/SF"),
  startAfter("projects/demo/databases/(default)/documents/cities/SEA"),
  endAt("projects/demo/databases/(default)/documents/cities/LA"),
  endBefore("projects/demo/databases/(default)/documents/cities/NYC"),
);
const snapshotPromise: Promise<DocumentSnapshot> = getDoc(city);
const querySnapshotPromise: Promise<QuerySnapshot> = getDocs(citiesQuery);
const transformValue: FieldValue = serverTimestamp();
const documentObserver: SnapshotObserver<DocumentSnapshot> = {
  next(snapshot) {
    void snapshot.exists();
  },
};
const queryObserver: SnapshotObserver<QuerySnapshot> = {
  next(snapshot) {
    void snapshot.size;
  },
};
const unsubscribeDocument: Unsubscribe = onSnapshot(city, documentObserver);
const unsubscribeQuery: Unsubscribe = onSnapshot(citiesQuery, queryObserver);
const addDocPromise: Promise<DocumentReference> = addDoc(cities, { name: "Paris" });
const setDocPromise: Promise<void> = setDoc(city, {
  deletedAt: deleteField(),
  name: "Paris",
  updatedAt: transformValue,
}, setOptions);
const updateDocPromise: Promise<void> = updateDoc(city, {
  "stats.tags": arrayUnion("metro"),
  "stats.visits": increment(1),
  archivedTags: arrayRemove("legacy"),
});
const deleteDocPromise: Promise<void> = deleteDoc(city);
const batch: WriteBatch = writeBatch(firestore);
const batchCommitPromise: Promise<void> = batch
  .set(city, {
    deletedAt: deleteField(),
    name: "Paris",
  }, setOptions)
  .update(city, { "stats.updatedAt": serverTimestamp() })
  .commit();
const transactionOptions: TransactionOptions = {
  maxAttempts: 2,
};
const transactionPromise: Promise<string> = runTransaction(
  firestore,
  async (transaction: Transaction) => {
    const snapshot = await transaction.get(city);
    const querySnapshot = await transaction.get(citiesQuery);
    transaction.update(city, {
      "stats.updatedAt": serverTimestamp(),
      "stats.visits": Number(snapshot.get("stats.visits") ?? 0) + 1,
    });
    return String(querySnapshot.docs[0]?.get("name") ?? snapshot.get("name") ?? "");
  },
  transactionOptions,
);
const firestoreError: FirestoreError = new FirestoreError("UNKNOWN", "message", 500);
const queryDocumentSnapshot: QueryDocumentSnapshot | null = null;
type City = {
  name: string;
  population: number;
};
const cityConverter: FirestoreDataConverter<City> = {
  toFirestore(model) {
    return {
      name: model.name,
      population: model.population,
    };
  },
  fromFirestore(snapshot) {
    const data = snapshot.data();
    return {
      name: String(data.name),
      population: Number(data.population),
    };
  },
};
const convertedCities: CollectionReference<City> = cities.withConverter(cityConverter);
const convertedCity: DocumentReference<City> = doc(convertedCities, "SEA");
const rawConvertedCity: DocumentReference = convertedCity.withConverter(null);
const convertedQuery: Query<City> = query(convertedCities, where("population", ">=", 1));
const reconvertedQuery: Query<City> = citiesQuery.withConverter(cityConverter);
const convertedSnapshotPromise: Promise<DocumentSnapshot<City>> = getDoc(convertedCity);
const convertedQuerySnapshotPromise: Promise<QuerySnapshot<City>> = getDocs(convertedQuery);
const convertedAddDocPromise: Promise<DocumentReference<City>> = addDoc(convertedCities, {
  name: "Seattle",
  population: 733000,
});
const convertedSetDocPromise: Promise<void> = setDoc(convertedCity, {
  name: "Seattle",
  population: 733000,
});

connectFirestoreEmulator(firestore, "127.0.0.1", 8080, {
  mockUserToken: {
    sub: "user-1",
  },
});

void getApp;
void getApps;
void deleteApp;
void terminate;
void initialized;
void cities;
void city;
void landmarks;
void group;
void citiesQuery;
void snapshotPromise;
void querySnapshotPromise;
void transformValue;
void unsubscribeDocument;
void unsubscribeQuery;
void addDocPromise;
void setDocPromise;
void updateDocPromise;
void deleteDocPromise;
void batchCommitPromise;
void transactionPromise;
void firestoreError;
void queryDocumentSnapshot;
void convertedCities;
void convertedCity;
void rawConvertedCity;
void convertedQuery;
void reconvertedQuery;
void convertedSnapshotPromise;
void convertedQuerySnapshotPromise;
void convertedAddDocPromise;
void convertedSetDocPromise;
void getFirestoreFromRoot(namedApp);
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
