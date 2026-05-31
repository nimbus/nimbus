import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { build } from "esbuild";

const packageRoot = fileURLToPath(new URL("../", import.meta.url));
const packageJsonPath = fileURLToPath(
  new URL("../package.json", import.meta.url),
);
const tscPath = fileURLToPath(
  new URL("../../../node_modules/typescript/bin/tsc", import.meta.url),
);
const buildOnly = process.argv.includes("--build-only");
const typecheckOnly = process.argv.includes("--typecheck-only");
const smokePort = optionalFlagValue("--smoke-port");

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
  if (buildOnly) {
    await buildPackageSurface();
    return;
  }
  if (typecheckOnly) {
    await typecheckSurface();
    return;
  }

  await buildPackageSurface();
  await testConnectionHelpers();
  await typecheckSurface();

  if (smokePort) {
    await smokeTestCrud(parseInt(smokePort, 10));
  }
}

async function assertPackageExports() {
  const packageJson = JSON.parse(await fs.readFile(packageJsonPath, "utf8"));
  assert.equal(packageJson.name, "@nimbus/dynamodb");
  assert.deepEqual(packageJson.exports, {
    ".": "./src/index.ts",
  });
  console.log("  ✓ package.json exports verified");
}

async function buildPackageSurface() {
  const outDir = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-dynamodb-"));
  await build({
    entryPoints: [path.join(packageRoot, "src/index.ts")],
    bundle: true,
    format: "esm",
    outdir: outDir,
    platform: "node",
    // The AWS SDK is a peer the consumer provides; never bundle it.
    external: ["@aws-sdk/client-dynamodb"],
    logLevel: "silent",
  });

  const bundlePath = path.join(outDir, "index.js");
  const stat = await fs.stat(bundlePath);
  assert.ok(stat.size > 0, "bundle should be non-empty");
  console.log(`  ✓ ESM bundle built (${stat.size} bytes)`);
  return outDir;
}

async function testConnectionHelpers() {
  const { clientConfig, endpoint } = await import("./client.ts");

  // Defaults: local DynamoDB-Local-style endpoint, us-east-1, nimbus creds.
  assert.equal(endpoint(), "http://127.0.0.1:8000");
  const defaults = clientConfig();
  assert.equal(defaults.endpoint, "http://127.0.0.1:8000");
  assert.equal(defaults.region, "us-east-1");
  assert.equal(defaults.credentials.accessKeyId, "nimbus");
  assert.equal(defaults.credentials.secretAccessKey, "nimbus");

  // host/port override.
  assert.equal(endpoint({ host: "localhost", port: 9001 }), "http://localhost:9001");

  // Explicit endpoint wins over host/port.
  assert.equal(
    endpoint({ endpoint: "https://ddb.example:443", host: "ignored", port: 1 }),
    "https://ddb.example:443",
  );

  // Access key id selects the tenant; region + secret flow through.
  const scoped = clientConfig({
    accessKeyId: "AKIAACME",
    secretAccessKey: "shhh",
    region: "eu-west-1",
    port: 8123,
  });
  assert.equal(scoped.endpoint, "http://127.0.0.1:8123");
  assert.equal(scoped.region, "eu-west-1");
  assert.equal(scoped.credentials.accessKeyId, "AKIAACME");
  assert.equal(scoped.credentials.secretAccessKey, "shhh");

  console.log("  ✓ connection-helper tests passed");
}

async function typecheckSurface() {
  const result = spawnSync(
    process.execPath,
    [tscPath, "--project", path.join(packageRoot, "tsconfig.json")],
    { stdio: "pipe", encoding: "utf8" },
  );
  if (result.status !== 0) {
    console.error(result.stdout);
    console.error(result.stderr);
    throw new Error("typecheck failed");
  }
  console.log("  ✓ typecheck passed");
}

// Smoke test against a running Nimbus DynamoDB listener (opt-in via
// `--smoke-port`). Requires `@aws-sdk/client-dynamodb` to be installed and the
// server to have an access key bound to a tenant (default "nimbus").
async function smokeTestCrud(port) {
  const { clientConfig } = await import("./client.ts");
  const {
    DynamoDBClient,
    CreateTableCommand,
    PutItemCommand,
    GetItemCommand,
  } = await import("@aws-sdk/client-dynamodb");

  const client = new DynamoDBClient(clientConfig({ port }));
  try {
    await client.send(
      new CreateTableCommand({
        TableName: "SmokeTable",
        KeySchema: [{ AttributeName: "pk", KeyType: "HASH" }],
        AttributeDefinitions: [{ AttributeName: "pk", AttributeType: "S" }],
        BillingMode: "PAY_PER_REQUEST",
      }),
    );

    await client.send(
      new PutItemCommand({
        TableName: "SmokeTable",
        Item: { pk: { S: "a" }, v: { N: "1" } },
      }),
    );

    const got = await client.send(
      new GetItemCommand({
        TableName: "SmokeTable",
        Key: { pk: { S: "a" } },
      }),
    );
    assert.equal(got.Item?.v?.N, "1", "GetItem should round-trip the value");

    console.log("  ✓ smoke test: CreateTable/PutItem/GetItem passed");
  } finally {
    client.destroy();
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
