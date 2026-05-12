import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";

import {
  readCloudFunctionsFile,
  readCloudFunctionsJson,
  runCli,
} from "./helpers.mjs";

async function runCloudFunctionsFixtures() {
  await testFirebaseDocumentTriggersGenerateCloudFunctionsArtifacts();
  await testFirebaseGlobalOptionsValidateAndApplyToDocumentTriggers();
  await testFirebaseDocumentTriggersMaterializeFirestoreEventShapes();
  await testFirebaseOnRequestTargetsUseSharedHttpPathContract();
  await testFirebaseOnCallTargetsUseCallableEnvelopeAndErrorContract();
  await testFirebaseDeferredRootSurfaceAndCallableUnsupportedOptionsFailFast();
  await testFrameworkPackageTargetsUseExplicitBindingManifest();
  await testFrameworkHttpTargetsMaterializeExpressRequestAndResponse();
  await testFrameworkCloudEventTargetsMaterializeStandardCloudEventShape();
  await testFrameworkPackageRequiresBindingManifestForDiscoveredTargets();
  await testFirebaseAdminAppLifecycleAndFirestoreHandleAcquisition();
  await testFirebaseAdminFirestoreDocumentOperationsUseCoveredHostBridge();
  await testFirebaseAdminFirestoreDeferredOperationsFailFast();
}

async function testFirebaseDocumentTriggersGenerateCloudFunctionsArtifacts() {
  const appDir = await createFirebaseProjectFixture({
    "src/index.ts": `
import { onDocumentCreated, onDocumentDeleted } from "firebase-functions/v2/firestore";

export const syncUser = onDocumentCreated("users/{userId}", async (event) => ({
  status: "created",
  id: event?.firestore?.params?.userId ?? null,
}));

export const cleanupUser = onDocumentDeleted(
  { document: "users/{userId}", database: "(default)" },
  async () => ({ status: "deleted" }),
);
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const artifact = await readCloudFunctionsJson(appDir, "artifact.json");
  assert.equal(artifact.family, "cloud_functions");
  assert.equal(artifact.targets_manifest, "targets.json");

  const targets = await readCloudFunctionsJson(appDir, "targets.json");
  assert.deepEqual(
    targets.targets
      .map((target) => ({
        name: target.name,
        entrypoint: target.entrypoint,
        event_type: target.binding.event_type,
        document: target.binding.document,
      }))
      .sort((left, right) => left.name.localeCompare(right.name)),
    [
      {
        name: "cleanupUser",
        entrypoint: "exports.cleanupUser",
        event_type: "google.cloud.firestore.document.v1.deleted",
        document: "users/{userId}",
      },
      {
        name: "syncUser",
        entrypoint: "exports.syncUser",
        event_type: "google.cloud.firestore.document.v1.created",
        document: "users/{userId}",
      },
    ],
  );

  const runtimeBundle = await readCloudFunctionsFile(appDir, "bundle.mjs");
  assert.match(runtimeBundle, /globalThis\.__nimbusInvoke = createInvocationDispatcher/);
  assert.match(runtimeBundle, /defineFirestoreDocumentTarget/);

  const runtimeBundleHash = (await readCloudFunctionsFile(appDir, "bundle.sha256")).trim();
  assert.equal(
    runtimeBundleHash,
    createHash("sha256").update(runtimeBundle).digest("hex"),
  );
}

async function testFirebaseGlobalOptionsValidateAndApplyToDocumentTriggers() {
  const appDir = await createFirebaseProjectFixture({
    "src/index.ts": `
import { setGlobalOptions } from "firebase-functions/v2";
import { onDocumentWritten } from "firebase-functions/v2/firestore";

setGlobalOptions({ retry: true });

export const syncUser = onDocumentWritten("users/{userId}", async (event) => event);
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const targets = await readCloudFunctionsJson(appDir, "targets.json");
  assert.equal(targets.targets.length, 1);
  assert.equal(targets.targets[0].binding.document, "users/{userId}");

  const invalidAppDir = await createFirebaseProjectFixture({
    "src/index.ts": `
import { setGlobalOptions } from "firebase-functions/v2";
import { onDocumentWritten } from "firebase-functions/v2/firestore";

setGlobalOptions({ region: "us-central1" });

export const syncUser = onDocumentWritten("users/{userId}", async (event) => event);
`,
  });
  const invalid = runCli(invalidAppDir);
  assert.notEqual(invalid.status, 0, invalid.stdout);
  assert.match(
    invalid.stderr || invalid.stdout,
    /global option "region" is not covered for firestore_document in the first cloud functions slice/,
  );
}

async function testFirebaseDocumentTriggersMaterializeFirestoreEventShapes() {
  const appDir = await createFirebaseProjectFixture({
    "src/index.ts": `
import {
  onDocumentCreated,
  onDocumentDeleted,
  onDocumentUpdated,
  onDocumentWritten,
} from "firebase-functions/v2/firestore";

export const inspectCreatedUser = onDocumentCreated("users/{userId}", async (event) => ({
  id: event.id,
  source: event.source,
  specversion: event.specversion,
  subject: event.subject,
  type: event.type,
  time: event.time,
  project: event.project,
  database: event.database,
  document: event.document,
  params: event.params,
  exists: event.data?.exists,
  data: event.data?.data(),
}));

export const inspectDeletedUser = onDocumentDeleted("users/{userId}", async (event) => ({
  id: event.id,
  source: event.source,
  specversion: event.specversion,
  subject: event.subject,
  type: event.type,
  time: event.time,
  project: event.project,
  database: event.database,
  document: event.document,
  params: event.params,
  exists: event.data?.exists,
  data: event.data?.data(),
}));

export const inspectUpdatedUser = onDocumentUpdated("users/{userId}", async (event) => ({
  id: event.id,
  source: event.source,
  specversion: event.specversion,
  subject: event.subject,
  type: event.type,
  time: event.time,
  project: event.project,
  database: event.database,
  document: event.document,
  params: event.params,
  beforeExists: event.data.before.exists,
  beforeData: event.data.before.data(),
  afterExists: event.data.after.exists,
  afterData: event.data.after.data(),
}));

export const inspectWrittenUser = onDocumentWritten("users/{userId}", async (event) => ({
  id: event.id,
  source: event.source,
  specversion: event.specversion,
  subject: event.subject,
  type: event.type,
  time: event.time,
  project: event.project,
  database: event.database,
  document: event.document,
  params: event.params,
  beforeExists: event.data.before.exists,
  beforeData: event.data.before.data(),
  afterExists: event.data.after.exists,
  afterData: event.data.after.data(),
}));
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  delete globalThis.__nimbusCloudFunctionsState;
  delete globalThis.__nimbusInvoke;
  await import(
    `${pathToFileURL(path.join(appDir, ".nimbus", "firebase", "bundle.mjs")).href}?t=${Date.now()}`
  );

  const rawTime = 1712345678901;
  const commonIdentity = {
    id: "evt-users-alice",
    source: "//firestore.googleapis.com/projects/demo-project/databases/(default)",
    specversion: "1.0",
    subject: "documents/users/alice",
    time: new Date(rawTime).toISOString(),
    project: "demo-project",
    database: "(default)",
    document: "users/alice",
    params: {
      userId: "alice",
    },
  };

  assert.deepEqual(
    await globalThis.__nimbusInvoke({
      function_name: "exports.inspectCreatedUser",
      args: sampleCreatedTriggerEvent({ time: rawTime }),
    }),
    {
      ...commonIdentity,
      type: "google.cloud.firestore.document.v1.created",
      exists: true,
      data: {
        name: "after",
        nested: {
          count: 2,
        },
      },
    },
  );

  assert.deepEqual(
    await globalThis.__nimbusInvoke({
      function_name: "exports.inspectDeletedUser",
      args: sampleDeletedTriggerEvent({ time: rawTime }),
    }),
    {
      ...commonIdentity,
      type: "google.cloud.firestore.document.v1.deleted",
      exists: true,
      data: {
        name: "before",
        nested: {
          count: 1,
        },
      },
    },
  );

  assert.deepEqual(
    await globalThis.__nimbusInvoke({
      function_name: "exports.inspectUpdatedUser",
      args: sampleUpdatedTriggerEvent({ time: rawTime }),
    }),
    {
      ...commonIdentity,
      type: "google.cloud.firestore.document.v1.updated",
      beforeExists: true,
      beforeData: {
        name: "before",
        nested: {
          count: 1,
        },
      },
      afterExists: true,
      afterData: {
        name: "after",
        nested: {
          count: 2,
        },
      },
    },
  );

  assert.deepEqual(
    await globalThis.__nimbusInvoke({
      function_name: "exports.inspectWrittenUser",
      args: sampleWrittenTriggerEvent({ time: rawTime }),
    }),
    {
      ...commonIdentity,
      type: "google.cloud.firestore.document.v1.written",
      beforeExists: true,
      beforeData: {
        name: "before",
        nested: {
          count: 1,
        },
      },
      afterExists: true,
      afterData: {
        name: "after",
        nested: {
          count: 2,
        },
      },
    },
  );
}

async function testFrameworkPackageTargetsUseExplicitBindingManifest() {
  const appDir = await createFrameworkPackageFixture({
    "src/index.ts": `
import functions from "@google-cloud/functions-framework";

functions.cloudEvent("syncUser", async (event) => ({ event }));
functions.http("helloWorld", async (req, res) => ({ req, res }));
`,
    ".nimbus/firebase/targets.json": JSON.stringify({
      version: 1,
      targets: [
        {
          name: "helloWorld",
          entrypoint: "registry.helloWorld",
          authoring_surface: "functions_framework",
          signature_type: "http",
          binding: {
            binding_kind: "https",
            exposure: "http",
            path: "/hello",
            execution: "request",
          },
        },
        {
          name: "syncUser",
          entrypoint: "registry.syncUser",
          authoring_surface: "functions_framework",
          signature_type: "cloud_event",
          binding: {
            binding_kind: "firestore_document",
            event_type: "google.cloud.firestore.document.v1.written",
            database: "(default)",
            document: "users/{userId}",
            execution: "service",
          },
        },
      ],
    }, null, 2),
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const targets = await readCloudFunctionsJson(appDir, "targets.json");
  assert.deepEqual(
    targets.targets.map((target) => ({
      name: target.name,
      entrypoint: target.entrypoint,
      signature_type: target.signature_type,
      binding_kind: target.binding.binding_kind,
    })),
    [
      {
        name: "syncUser",
        entrypoint: "registry.syncUser",
        signature_type: "cloud_event",
        binding_kind: "firestore_document",
      },
      {
        name: "helloWorld",
        entrypoint: "registry.helloWorld",
        signature_type: "http",
        binding_kind: "https",
      },
    ],
  );

  const runtimeBundle = await readCloudFunctionsFile(appDir, "bundle.mjs");
  assert.match(runtimeBundle, /registerFrameworkTarget/);
}

async function testFrameworkHttpTargetsMaterializeExpressRequestAndResponse() {
  const appDir = await createFrameworkPackageFixture({
    "src/index.ts": `
import functions from "@google-cloud/functions-framework";

functions.http("helloWorld", async (req, res) => {
  res
    .status(201)
    .set("x-handler", req.get("x-test") ?? "missing")
    .json({
      method: req.method,
      path: req.path,
      originalUrl: req.originalUrl,
      header: req.header("x-test"),
      query: req.query,
      body: req.body,
      rawBody: req.rawBody,
    });
});

functions.http("fallback", async (req, res) => ({
  ok: req.path,
  body: req.body,
}));
`,
    ".nimbus/firebase/targets.json": JSON.stringify({
      version: 1,
      targets: [
        {
          name: "helloWorld",
          entrypoint: "registry.helloWorld",
          authoring_surface: "functions_framework",
          signature_type: "http",
          binding: {
            binding_kind: "https",
            exposure: "http",
            path: "/hello",
            execution: "request",
          },
        },
        {
          name: "fallback",
          entrypoint: "registry.fallback",
          authoring_surface: "functions_framework",
          signature_type: "http",
          binding: {
            binding_kind: "https",
            exposure: "http",
            path: "/fallback",
            execution: "request",
          },
        },
      ],
    }, null, 2),
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  delete globalThis.__nimbusCloudFunctionsState;
  delete globalThis.__nimbusInvoke;
  await import(
    `${pathToFileURL(path.join(appDir, ".nimbus", "firebase", "bundle.mjs")).href}?t=${Date.now()}`
  );

  assert.deepEqual(
    await globalThis.__nimbusInvoke({
      function_name: "registry.helloWorld",
      args: {
        method: "POST",
        path: "/hello",
        original_url: "http://localhost/hello?name=jack",
        query: {
          name: "jack",
        },
        headers: {
          "content-type": "application/json",
          "x-test": "present",
        },
        body: {
          hello: "world",
        },
        raw_body: "{\"hello\":\"world\"}",
      },
    }),
    {
      status: 201,
      headers: {
        "content-type": "application/json",
        "x-handler": "present",
      },
      body_kind: "json",
      body: {
        method: "POST",
        path: "/hello",
        originalUrl: "http://localhost/hello?name=jack",
        header: "present",
        query: {
          name: "jack",
        },
        body: {
          hello: "world",
        },
        rawBody: "{\"hello\":\"world\"}",
      },
    },
  );

  assert.deepEqual(
    await globalThis.__nimbusInvoke({
      function_name: "registry.fallback",
      args: {
        method: "GET",
        path: "/fallback",
        original_url: "http://localhost/fallback",
        query: {},
        headers: {},
        body: null,
        raw_body: "",
      },
    }),
    {
      status: 200,
      headers: {
        "content-type": "application/json",
      },
      body_kind: "json",
      body: {
        ok: "/fallback",
        body: null,
      },
    },
  );
}

async function testFrameworkCloudEventTargetsMaterializeStandardCloudEventShape() {
  const appDir = await createFrameworkPackageFixture({
    "src/index.ts": `
import functions from "@google-cloud/functions-framework";

functions.cloudEvent("syncUser", async (event) => ({
  id: event.id,
  source: event.source,
  specversion: event.specversion,
  subject: event.subject,
  type: event.type,
  time: event.time,
  data: event.data,
}));
`,
    ".nimbus/firebase/targets.json": JSON.stringify({
      version: 1,
      targets: [
        {
          name: "syncUser",
          entrypoint: "registry.syncUser",
          authoring_surface: "functions_framework",
          signature_type: "cloud_event",
          binding: {
            binding_kind: "firestore_document",
            event_type: "google.cloud.firestore.document.v1.written",
            database: "(default)",
            document: "users/{userId}",
            execution: "service",
          },
        },
      ],
    }, null, 2),
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  delete globalThis.__nimbusCloudFunctionsState;
  delete globalThis.__nimbusInvoke;
  await import(
    `${pathToFileURL(path.join(appDir, ".nimbus", "firebase", "bundle.mjs")).href}?t=${Date.now()}`
  );

  const rawTime = 1712345678901;
  assert.deepEqual(
    await globalThis.__nimbusInvoke({
      function_name: "registry.syncUser",
      args: sampleWrittenTriggerEvent({ time: rawTime }),
    }),
    {
      id: "evt-users-alice",
      source: "//firestore.googleapis.com/projects/demo-project/databases/(default)",
      specversion: "1.0",
      subject: "documents/users/alice",
      type: "google.cloud.firestore.document.v1.written",
      time: new Date(rawTime).toISOString(),
      data: {
        value: sampleDocumentEventDocument("alice", {
          name: "after",
          nested: {
            count: 2,
          },
        }),
        oldValue: sampleDocumentEventDocument("alice", {
          name: "before",
          nested: {
            count: 1,
          },
        }),
        updateMask: {
          fieldPaths: ["name", "nested.count"],
        },
      },
    },
  );
}

async function testFirebaseOnRequestTargetsUseSharedHttpPathContract() {
  const appDir = await createFirebaseProjectFixture({
    "src/index.ts": `
import { setGlobalOptions } from "firebase-functions/v2";
import { onRequest } from "firebase-functions/v2/https";

setGlobalOptions({ retry: true });

export const hello = onRequest(async (req, res) => {
  res.status(202).set("x-nimbus-http", req.path).json({
    method: req.method,
    path: req.path,
    originalUrl: req.originalUrl,
    query: req.query,
    body: req.body,
    rawBody: req.rawBody,
    header: req.get("x-test"),
  });
});

export const empty = onRequest({}, async () => ({ ok: true }));
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const targets = await readCloudFunctionsJson(appDir, "targets.json");
  assert.deepEqual(
    targets.targets
      .map((target) => ({
        name: target.name,
        entrypoint: target.entrypoint,
        authoring_surface: target.authoring_surface,
        signature_type: target.signature_type,
        binding_kind: target.binding.binding_kind,
        exposure: target.binding.exposure,
        path: target.binding.path,
        execution: target.binding.execution,
      }))
      .sort((left, right) => left.name.localeCompare(right.name)),
    [
      {
        name: "empty",
        entrypoint: "exports.empty",
        authoring_surface: "firebase_v2",
        signature_type: "http",
        binding_kind: "https",
        exposure: "http",
        path: "/empty",
        execution: "request",
      },
      {
        name: "hello",
        entrypoint: "exports.hello",
        authoring_surface: "firebase_v2",
        signature_type: "http",
        binding_kind: "https",
        exposure: "http",
        path: "/hello",
        execution: "request",
      },
    ],
  );

  delete globalThis.__nimbusCloudFunctionsState;
  delete globalThis.__nimbusInvoke;
  await import(
    `${pathToFileURL(path.join(appDir, ".nimbus", "firebase", "bundle.mjs")).href}?t=${Date.now()}`
  );

  assert.deepEqual(
    await globalThis.__nimbusInvoke({
      function_name: "exports.hello",
      args: {
        method: "POST",
        path: "/hello",
        original_url: "http://localhost/hello?name=jack",
        query: {
          name: "jack",
        },
        headers: {
          "content-type": "application/json",
          "x-test": "present",
        },
        body: {
          hello: "world",
        },
        raw_body: "{\"hello\":\"world\"}",
      },
    }),
    {
      status: 202,
      headers: {
        "content-type": "application/json",
        "x-nimbus-http": "/hello",
      },
      body_kind: "json",
      body: {
        method: "POST",
        path: "/hello",
        originalUrl: "http://localhost/hello?name=jack",
        query: {
          name: "jack",
        },
        body: {
          hello: "world",
        },
        rawBody: "{\"hello\":\"world\"}",
        header: "present",
      },
    },
  );

  const invalidOptionsAppDir = await createFirebaseProjectFixture({
    "src/index.ts": `
import { onRequest } from "firebase-functions/v2/https";

export const hello = onRequest({ region: "us-central1" }, async () => ({ ok: true }));
`,
  });

  const invalidOptions = runCli(invalidOptionsAppDir);
  assert.notEqual(invalidOptions.status, 0, invalidOptions.stdout);
  assert.match(
    invalidOptions.stderr || invalidOptions.stdout,
    /HTTPS option "region" is not covered in the first cloud functions slice/,
  );
}

async function testFirebaseOnCallTargetsUseCallableEnvelopeAndErrorContract() {
  const appDir = await createFirebaseProjectFixture({
    "src/index.ts": `
import { HttpsError, onCall } from "firebase-functions/v2/https";

export const hello = onCall(async (request, response) => {
  if (request.data?.fail) {
    throw new HttpsError("invalid-argument", "bad input", {
      reason: "fail",
    });
  }
  return {
    acceptsStreaming: request.acceptsStreaming,
    app: request.app ?? null,
    auth: request.auth ?? null,
    data: request.data,
    instanceIdToken: request.instanceIdToken ?? null,
    path: request.rawRequest.path,
    sendChunkType: typeof response.sendChunk,
  };
});

export const empty = onCall({}, async () => null);
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  const targets = await readCloudFunctionsJson(appDir, "targets.json");
  assert.deepEqual(
    targets.targets
      .map((target) => ({
        name: target.name,
        entrypoint: target.entrypoint,
        authoring_surface: target.authoring_surface,
        signature_type: target.signature_type,
        binding_kind: target.binding.binding_kind,
        exposure: target.binding.exposure,
        path: target.binding.path,
        execution: target.binding.execution,
      }))
      .sort((left, right) => left.name.localeCompare(right.name)),
    [
      {
        name: "empty",
        entrypoint: "exports.empty",
        authoring_surface: "firebase_v2",
        signature_type: "http",
        binding_kind: "https",
        exposure: "callable",
        path: "/empty",
        execution: "request",
      },
      {
        name: "hello",
        entrypoint: "exports.hello",
        authoring_surface: "firebase_v2",
        signature_type: "http",
        binding_kind: "https",
        exposure: "callable",
        path: "/hello",
        execution: "request",
      },
    ],
  );

  delete globalThis.__nimbusCloudFunctionsState;
  delete globalThis.__nimbusInvoke;
  await import(
    `${pathToFileURL(path.join(appDir, ".nimbus", "firebase", "bundle.mjs")).href}?t=${Date.now()}`
  );

  assert.deepEqual(
    await globalThis.__nimbusInvoke({
      function_name: "exports.hello",
      args: {
        method: "POST",
        path: "/hello",
        original_url: "http://localhost/hello",
        query: {},
        headers: {
          authorization: "Bearer token",
          "content-type": "application/json",
          "firebase-instance-id-token": "iid-token",
        },
        body: {
          data: {
            name: "Ada",
          },
        },
        raw_body: "{\"data\":{\"name\":\"Ada\"}}",
        callable: {
          data: {
            name: "Ada",
          },
          auth: {
            uid: "user-123",
            token: {
              sub: "user-123",
              role: "admin",
            },
          },
          instance_id_token: "iid-token",
        },
      },
    }),
    {
      status: 200,
      headers: {
        "content-type": "application/json",
      },
      body_kind: "json",
      body: {
        data: {
          acceptsStreaming: false,
          app: null,
          auth: {
            uid: "user-123",
            token: {
              sub: "user-123",
              role: "admin",
            },
          },
          data: {
            name: "Ada",
          },
          instanceIdToken: "iid-token",
          path: "/hello",
          sendChunkType: "function",
        },
      },
    },
  );

  assert.deepEqual(
    await globalThis.__nimbusInvoke({
      function_name: "exports.hello",
      args: {
        method: "POST",
        path: "/hello",
        original_url: "http://localhost/hello",
        query: {},
        headers: {
          "content-type": "application/json",
        },
        body: {
          data: {
            fail: true,
          },
        },
        raw_body: "{\"data\":{\"fail\":true}}",
        callable: {
          data: {
            fail: true,
          },
        },
      },
    }),
    {
      status: 400,
      headers: {
        "content-type": "application/json",
      },
      body_kind: "json",
      body: {
        error: {
          status: "INVALID_ARGUMENT",
          message: "bad input",
          details: {
            reason: "fail",
          },
        },
      },
    },
  );
}

async function testFirebaseDeferredRootSurfaceAndCallableUnsupportedOptionsFailFast() {
  const onInitAppDir = await createFirebaseProjectFixture({
    "src/index.ts": `
import { onInit } from "firebase-functions/v2";

onInit(() => {});
`,
  });

  const onInitResult = runCli(onInitAppDir);
  assert.notEqual(onInitResult.status, 0, onInitResult.stdout);
  assert.match(
    onInitResult.stderr || onInitResult.stdout,
    /firebase-functions\/v2 root API "onInit\(\)" is deferred/,
  );

  const invalidCallableOptionsAppDir = await createFirebaseProjectFixture({
    "src/index.ts": `
import { onCall } from "firebase-functions/v2/https";

export const hello = onCall({ region: "us-central1" }, async () => ({ ok: true }));
`,
  });

  const onCallResult = runCli(invalidCallableOptionsAppDir);
  assert.notEqual(onCallResult.status, 0, onCallResult.stdout);
  assert.match(
    onCallResult.stderr || onCallResult.stdout,
    /Callable option "region" is not covered in the first cloud functions slice/,
  );
}

async function testFirebaseAdminAppLifecycleAndFirestoreHandleAcquisition() {
  const appDir = await createFirebaseProjectFixture({
    "src/index.ts": `
import { onDocumentWritten } from "firebase-functions/v2/firestore";
import { deleteApp, getApp, getApps, initializeApp } from "firebase-admin/app";
import { getFirestore } from "firebase-admin/firestore";

const defaultApp = initializeApp({ projectId: "demo-project" });
const secondaryApp = initializeApp({ projectId: "secondary-project" }, "secondary");

export const inspectAdmin = onDocumentWritten("users/{userId}", async () => {
  const firestore = getFirestore(defaultApp, "custom-db");
  const beforeDelete = getApps().map((app) => app.name);
  await deleteApp(secondaryApp);
  return {
    defaultAppName: getApp().name,
    defaultProjectId: defaultApp.options.projectId,
    beforeDelete,
    afterDelete: getApps().map((app) => app.name),
    firestoreAppName: firestore.app.name,
    firestoreDatabaseId: firestore.databaseId,
  };
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  delete globalThis.__nimbusCloudFunctionsState;
  delete globalThis.__nimbusAdminApps;
  delete globalThis.__nimbusInvoke;
  await import(
    `${pathToFileURL(path.join(appDir, ".nimbus", "firebase", "bundle.mjs")).href}?t=${Date.now()}`
  );

  assert.deepEqual(
    await globalThis.__nimbusInvoke({
      function_name: "exports.inspectAdmin",
      args: sampleWrittenTriggerEvent({ time: 1712345678901 }),
    }),
    {
      defaultAppName: "[DEFAULT]",
      defaultProjectId: "demo-project",
      beforeDelete: ["[DEFAULT]", "secondary"],
      afterDelete: ["[DEFAULT]"],
      firestoreAppName: "[DEFAULT]",
      firestoreDatabaseId: "custom-db",
    },
  );
}

async function testFirebaseAdminFirestoreDocumentOperationsUseCoveredHostBridge() {
  const appDir = await createFirebaseProjectFixture({
    "src/index.ts": `
import { onDocumentWritten } from "firebase-functions/v2/firestore";
import { Timestamp, getFirestore } from "firebase-admin/firestore";

export const inspectAdmin = onDocumentWritten("users/{userId}", async () => {
  const firestore = getFirestore();
  const users = firestore.collection("users");
  const userRef = users.doc("alice");
  const snapshot = await userRef.get();
  const setResult = await firestore.doc("audit/alice").set({
    beforeName: snapshot.get("profile.name"),
    recordedAt: Timestamp.fromMillis(42),
  });
  const updateResult = await userRef.update({
    processed: true,
  });
  const deleteResult = await firestore.doc("trash/alice").delete();
  return {
    beforeExists: snapshot.exists,
    beforeId: snapshot.id,
    beforeName: snapshot.get("profile.name"),
    beforeData: snapshot.data(),
    usersCollectionId: users.id,
    usersCollectionParent: users.parent,
    userParentPath: userRef.parent.path,
    nestedCollectionPath: userRef.collection("messages").path,
    setWriteTime: setResult.writeTime.toMillis(),
    updateWriteTime: updateResult.writeTime.toMillis(),
    deleteWriteTime: deleteResult.writeTime.toMillis(),
  };
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  delete globalThis.__nimbusCloudFunctionsState;
  delete globalThis.__nimbusAdminApps;
  delete globalThis.__nimbusInvoke;
  const hostCalls = [];
  globalThis.__nimbusAsyncHostValue = async (opName, payload) => {
    hostCalls.push({ opName, payload: JSON.parse(JSON.stringify(payload)) });
    if (
      opName === "op_nimbus_runtime_extension_call"
      && payload?.operation === "firebase_admin.firestore.get_document"
    ) {
      return {
        path: "users/alice",
        id: "alice",
        fields: {
          profile: {
            name: "before",
          },
          count: 1,
        },
        create_time_ms: 11,
        update_time_ms: 12,
      };
    }
    if (
      opName === "op_nimbus_runtime_extension_call"
      && payload?.operation === "firebase_admin.firestore.set_document"
    ) {
      return { write_time_ms: 101 };
    }
    if (
      opName === "op_nimbus_runtime_extension_call"
      && payload?.operation === "firebase_admin.firestore.update_document"
    ) {
      return { write_time_ms: 102 };
    }
    if (
      opName === "op_nimbus_runtime_extension_call"
      && payload?.operation === "firebase_admin.firestore.delete_document"
    ) {
      return { write_time_ms: 103 };
    }
    throw new Error(`unexpected host op ${opName}`);
  };

  await import(
    `${pathToFileURL(path.join(appDir, ".nimbus", "firebase", "bundle.mjs")).href}?t=${Date.now()}`
  );

  assert.deepEqual(
    await globalThis.__nimbusInvoke({
      function_name: "exports.inspectAdmin",
      args: sampleWrittenTriggerEvent({ time: 1712345678901 }),
    }),
    {
      beforeExists: true,
      beforeId: "alice",
      beforeName: "before",
      beforeData: {
        profile: {
          name: "before",
        },
        count: 1,
      },
      usersCollectionId: "users",
      usersCollectionParent: null,
      userParentPath: "users",
      nestedCollectionPath: "users/alice/messages",
      setWriteTime: 101,
      updateWriteTime: 102,
      deleteWriteTime: 103,
    },
  );

  assert.deepEqual(hostCalls, [
    {
      opName: "op_nimbus_runtime_extension_call",
      payload: {
        namespace: "cloud_functions",
        operation: "firebase_admin.firestore.get_document",
        payload: {
          database_id: "(default)",
          document_path: "users/alice",
        },
      },
    },
    {
      opName: "op_nimbus_runtime_extension_call",
      payload: {
        namespace: "cloud_functions",
        operation: "firebase_admin.firestore.set_document",
        payload: {
          database_id: "(default)",
          document_path: "audit/alice",
          fields: {
            beforeName: "before",
            recordedAt: 42,
          },
        },
      },
    },
    {
      opName: "op_nimbus_runtime_extension_call",
      payload: {
        namespace: "cloud_functions",
        operation: "firebase_admin.firestore.update_document",
        payload: {
          database_id: "(default)",
          document_path: "users/alice",
          patch: {
            processed: true,
          },
        },
      },
    },
    {
      opName: "op_nimbus_runtime_extension_call",
      payload: {
        namespace: "cloud_functions",
        operation: "firebase_admin.firestore.delete_document",
        payload: {
          database_id: "(default)",
          document_path: "trash/alice",
        },
      },
    },
  ]);

  delete globalThis.__nimbusAsyncHostValue;
}

async function testFirebaseAdminFirestoreDeferredOperationsFailFast() {
  const appDir = await createFirebaseProjectFixture({
    "src/index.ts": `
import { onDocumentWritten } from "firebase-functions/v2/firestore";
import { getFirestore } from "firebase-admin/firestore";

export const invalidDoc = onDocumentWritten("users/{userId}", async () => {
  return getFirestore().collection("users").doc();
});

export const invalidSet = onDocumentWritten("users/{userId}", async () => {
  return getFirestore().doc("users/alice").set({ ok: true }, { merge: true });
});

export const invalidDelete = onDocumentWritten("users/{userId}", async () => {
  return getFirestore().doc("users/alice").delete({ exists: true });
});
`,
  });

  const result = runCli(appDir);
  assert.equal(result.status, 0, result.stderr || result.stdout);

  delete globalThis.__nimbusCloudFunctionsState;
  delete globalThis.__nimbusAdminApps;
  delete globalThis.__nimbusInvoke;
  await import(
    `${pathToFileURL(path.join(appDir, ".nimbus", "firebase", "bundle.mjs")).href}?t=${Date.now()}`
  );

  await assert.rejects(
    () => globalThis.__nimbusInvoke({
      function_name: "exports.invalidDoc",
      args: sampleWrittenTriggerEvent({ time: 1712345678901 }),
    }),
    /collection\(\)\.doc\(\) requires an explicit document path/,
  );

  await assert.rejects(
    () => globalThis.__nimbusInvoke({
      function_name: "exports.invalidSet",
      args: sampleWrittenTriggerEvent({ time: 1712345678901 }),
    }),
    /DocumentReference\.set\(\) currently supports only set\(data\)/,
  );

  await assert.rejects(
    () => globalThis.__nimbusInvoke({
      function_name: "exports.invalidDelete",
      args: sampleWrittenTriggerEvent({ time: 1712345678901 }),
    }),
    /DocumentReference\.delete\(\) does not yet support delete options/,
  );
}

async function testFrameworkPackageRequiresBindingManifestForDiscoveredTargets() {
  const appDir = await createFrameworkPackageFixture({
    "src/index.ts": `
import functions from "@google-cloud/functions-framework";

functions.cloudEvent("syncUser", async (event) => event);
`,
  });

  const result = runCli(appDir);
  assert.notEqual(result.status, 0, result.stdout);
  assert.match(
    result.stderr || result.stdout,
    /requires .*\.nimbus\/firebase\/targets\.json to bind discovered targets: syncUser/,
  );
}

async function createFirebaseProjectFixture(files) {
  const appDir = await fs.mkdtemp(path.join(os.tmpdir(), "tmp_firebase_codegen_"));
  await fs.mkdir(path.join(appDir, "functions", "src"), { recursive: true });
  await fs.writeFile(
    path.join(appDir, "firebase.json"),
    JSON.stringify({ functions: { source: "functions" } }, null, 2),
    "utf8",
  );
  await fs.writeFile(
    path.join(appDir, "functions", "package.json"),
    JSON.stringify({ main: "lib/index.js" }, null, 2),
    "utf8",
  );
  for (const [fileName, source] of Object.entries(files)) {
    await fs.writeFile(path.join(appDir, "functions", fileName), source, "utf8");
  }
  return appDir;
}

async function createFrameworkPackageFixture(files) {
  const appDir = await fs.mkdtemp(path.join(os.tmpdir(), "tmp_framework_codegen_"));
  await fs.writeFile(
    path.join(appDir, "package.json"),
    JSON.stringify({
      main: "dist/index.js",
      dependencies: {
        "@google-cloud/functions-framework": "^3.4.5",
      },
    }, null, 2),
    "utf8",
  );
  for (const [fileName, source] of Object.entries(files)) {
    const targetPath = path.join(appDir, fileName);
    await fs.mkdir(path.dirname(targetPath), { recursive: true });
    await fs.writeFile(targetPath, source, "utf8");
  }
  return appDir;
}

function sampleWrittenTriggerEvent({ time }) {
  return {
    cloud_event: {
      id: "evt-users-alice",
      source: "//firestore.googleapis.com/projects/demo-project/databases/(default)",
      specversion: "1.0",
      type: "google.cloud.firestore.document.v1.written",
      time,
      subject: "documents/users/alice",
    },
    firestore: {
      project_id: "demo-project",
      database_id: "(default)",
      document_path: {
        collection_path: {
          root: "users",
          descendants: [],
        },
        document_id: "alice",
      },
      params: {
        userId: "alice",
      },
    },
    data: {
      value: sampleDocumentEventDocument("alice", {
        name: "after",
        nested: {
          count: 2,
        },
      }),
      oldValue: sampleDocumentEventDocument("alice", {
        name: "before",
        nested: {
          count: 1,
        },
      }),
      updateMask: {
        fieldPaths: ["name", "nested.count"],
      },
    },
    commit: {
      sequence: 42,
      timestamp: time,
    },
    execution: {
      Service: {
        principal: {
          kind: "system",
          subject: "cloud-functions",
        },
      },
    },
  };
}

function sampleCreatedTriggerEvent({ time }) {
  return {
    ...sampleWrittenTriggerEvent({ time }),
    cloud_event: {
      ...sampleWrittenTriggerEvent({ time }).cloud_event,
      type: "google.cloud.firestore.document.v1.created",
    },
    data: {
      value: sampleDocumentEventDocument("alice", {
        name: "after",
        nested: {
          count: 2,
        },
      }),
    },
  };
}

function sampleDeletedTriggerEvent({ time }) {
  return {
    ...sampleWrittenTriggerEvent({ time }),
    cloud_event: {
      ...sampleWrittenTriggerEvent({ time }).cloud_event,
      type: "google.cloud.firestore.document.v1.deleted",
    },
    data: {
      oldValue: sampleDocumentEventDocument("alice", {
        name: "before",
        nested: {
          count: 1,
        },
      }),
    },
  };
}

function sampleUpdatedTriggerEvent({ time }) {
  return {
    ...sampleWrittenTriggerEvent({ time }),
    cloud_event: {
      ...sampleWrittenTriggerEvent({ time }).cloud_event,
      type: "google.cloud.firestore.document.v1.updated",
    },
  };
}

function sampleDocumentEventDocument(documentId, fields) {
  return {
    path: {
      collection_path: {
        root: "users",
        descendants: [],
      },
      document_id: documentId,
    },
    document: {
      id: documentId,
      table: "users",
      creation_time: 1712345000000,
      fields,
    },
  };
}

export { runCloudFunctionsFixtures };
