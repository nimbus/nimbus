import crypto from "node:crypto";

import { ConvexHttpClient } from "convex/browser";

import { api } from "./convex/_generated/api.ts";
import type { Doc } from "./convex/_generated/dataModel.d.ts";

type Digest = Doc<"digests">;

const nativeUrl = process.env.NIMBUS_NATIVE_URL ?? "http://localhost:8080";
const adminToken = process.env.NIMBUS_ADMIN_TOKEN;
const tenantId = process.env.NIMBUS_TENANT_ID ?? "demo";
const convexUrl = process.env.NIMBUS_CONVEX_URL ?? `${nativeUrl}/convex/${tenantId}`;
let activeAnchor = "setup";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function pass(anchor: string) {
  console.log(`PASS ${anchor}`);
}

async function ensureTenant() {
  const response = await fetch(`${nativeUrl}/api/tenants`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      ...(adminToken ? { Authorization: `Bearer ${adminToken}` } : {}),
    },
    body: JSON.stringify({ id: tenantId }),
  });
  if (!response.ok && response.status !== 409) {
    throw new Error(`failed to ensure smoke tenant: ${response.status}`);
  }
}

function assertDigest(digest: Digest | undefined, message: string): asserts digest is Digest {
  assert(digest !== undefined, message);
  assert(typeof digest._id === "string" && digest._id.length > 0, "digest must have a stable id");
  assert(Number.isFinite(digest.createdAt), "digest must have a finite createdAt");
}

async function main() {
  await ensureTenant();

  const http = new ConvexHttpClient(convexUrl);
  const text = "nimbus runtimes example";
  // Cross-check both runtimes against a digest computed independently by the
  // smoke script itself, so a passing test proves the two runtimes actually
  // agree on the algorithm rather than merely both returning *some* hex string.
  const expected = crypto.createHash("sha256").update(text, "utf8").digest("hex");

  // digests.hashWithDefaultRuntime — default V8 runtime, Web Crypto SubtleCrypto
  activeAnchor = "digests.hashWithDefaultRuntime";
  const defaultId = await http.action(api.digests.hashWithDefaultRuntime, { text });
  assert(
    typeof defaultId === "string" && defaultId.length > 0,
    "hashWithDefaultRuntime must return a stable id",
  );
  const afterDefault = await http.query(api.digests.list, {});
  const defaultRow = afterDefault.find((digest) => digest._id === defaultId);
  assertDigest(defaultRow, "hashWithDefaultRuntime must persist its digest row");
  assert(defaultRow.runtime === "default", 'hashWithDefaultRuntime must tag runtime as "default"');
  assert(defaultRow.input === text, "hashWithDefaultRuntime must preserve the input text");
  assert(
    defaultRow.output === expected,
    `hashWithDefaultRuntime digest mismatch: got ${defaultRow.output}, expected ${expected}`,
  );
  pass("digests.hashWithDefaultRuntime");

  // nodeDigests.hashWithNodeRuntime — "use node" runtime, node:crypto
  activeAnchor = "nodeDigests.hashWithNodeRuntime";
  const nodeId = await http.action(api.nodeDigests.hashWithNodeRuntime, { text });
  assert(
    typeof nodeId === "string" && nodeId.length > 0,
    "hashWithNodeRuntime must return a stable id",
  );
  assert(
    nodeId !== defaultId,
    "hashWithNodeRuntime must create a distinct row from the default-runtime action",
  );
  const afterNode = await http.query(api.digests.list, {});
  const nodeRow = afterNode.find((digest) => digest._id === nodeId);
  assertDigest(nodeRow, "hashWithNodeRuntime must persist its digest row");
  assert(nodeRow.runtime === "node", 'hashWithNodeRuntime must tag runtime as "node"');
  assert(nodeRow.input === text, "hashWithNodeRuntime must preserve the input text");
  assert(
    nodeRow.output === expected,
    `hashWithNodeRuntime digest mismatch: got ${nodeRow.output}, expected ${expected}`,
  );
  pass("nodeDigests.hashWithNodeRuntime");

  // digests.list — both runtimes' rows are visible through the shared query
  activeAnchor = "digests.list";
  assert(afterNode.length >= 2, `digests.list expected at least two rows, got ${afterNode.length}`);
  assert(
    afterNode.some((digest) => digest._id === defaultId) &&
      afterNode.some((digest) => digest._id === nodeId),
    "digests.list must include rows written by both runtimes",
  );
  pass("digests.list");

  // shareIds.create — default runtime, real third-party npm package (nanoid),
  // proving a browser-compatible package works from a default-runtime
  // function, not just a "use node" one.
  activeAnchor = "shareIds.create";
  const shareRowId = await http.mutation(api.shareIds.create, {});
  assert(
    typeof shareRowId === "string" && shareRowId.length > 0,
    "shareIds.create must return a stable id",
  );
  const shareRows = await http.query(api.shareIds.list, {});
  const shareRow = shareRows.find((row) => row._id === shareRowId);
  assert(shareRow !== undefined, "shareIds.create must persist its row");
  // nanoid's default alphabet/size: 21 URL-safe characters.
  assert(
    /^[A-Za-z0-9_-]{21}$/.test(shareRow.id),
    `shareIds.create must generate a nanoid-shaped id, got ${JSON.stringify(shareRow.id)}`,
  );
  pass("shareIds.create");
}

main().catch((error) => {
  console.error(`FAIL ${activeAnchor}: ${(error as Error).message}`);
  process.exitCode = 1;
});
