import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const packageRoot = fileURLToPath(new URL("../", import.meta.url));
const packageJsonPath = fileURLToPath(
  new URL("../package.json", import.meta.url),
);
const tscPath = fileURLToPath(
  new URL("../../../node_modules/typescript/bin/tsc", import.meta.url),
);
const buildOnly = process.argv.includes("--build-only");
const typecheckOnly = process.argv.includes("--typecheck-only");
// Smoke-only mode skips the build/typecheck stages so callers (e.g. the
// Rust dev round-trip test) need only the `mongodb` driver installed.
const smokeOnly = process.argv.includes("--smoke-only");
const smokePort = optionalFlagValue("--smoke-port");
const smokeUsername = optionalFlagValue("--smoke-username");
const smokePassword = optionalFlagValue("--smoke-password");

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
  if (smokeOnly) {
    await runSmokeSuite();
    return;
  }

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
  await testUriBuilder();
  await typecheckSurface();

  if (smokePort) {
    await runSmokeSuite();
  }
}

async function runSmokeSuite() {
  assert.ok(
    smokePort && smokeUsername && smokePassword,
    "the smoke suite requires --smoke-port, --smoke-username, and --smoke-password",
  );
  const port = parseInt(smokePort, 10);
  await smokeTestCrud(port, smokeUsername, smokePassword);
  await smokeTestAggregation(port, smokeUsername, smokePassword);
  await smokeTestWrongPasswordRejected(port, smokeUsername);
}

async function assertPackageExports() {
  const packageJson = JSON.parse(await fs.readFile(packageJsonPath, "utf8"));
  assert.equal(packageJson.name, "@nimbus/mongodb");
  assert.deepEqual(packageJson.exports, {
    ".": "./src/index.ts",
  });
  console.log("  ✓ package.json exports verified");
}

async function buildPackageSurface() {
  // Lazy so --smoke-only runs never load esbuild.
  const { build } = await import("esbuild");
  const outDir = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-mongodb-"));
  await build({
    entryPoints: [path.join(packageRoot, "src/index.ts")],
    bundle: true,
    format: "esm",
    outdir: outDir,
    platform: "node",
    external: ["mongodb"],
    logLevel: "silent",
  });

  const bundlePath = path.join(outDir, "index.js");
  const stat = await fs.stat(bundlePath);
  assert.ok(stat.size > 0, "bundle should be non-empty");
  console.log(`  ✓ ESM bundle built (${stat.size} bytes)`);
  return outDir;
}

async function testUriBuilder() {
  const { mongoUri } = await import("./uri.ts");

  const defaultUri = mongoUri();
  assert.equal(
    defaultUri,
    "mongodb://127.0.0.1:27017/default?directConnection=true",
  );

  const customUri = mongoUri({
    host: "localhost",
    port: 27018,
    database: "mydb",
  });
  assert.equal(
    customUri,
    "mongodb://localhost:27018/mydb?directConnection=true",
  );

  const authUri = mongoUri({
    username: "admin",
    password: "s3cret",
    database: "testdb",
  });
  assert.equal(
    authUri,
    "mongodb://admin:s3cret@127.0.0.1:27017/testdb?directConnection=true",
  );

  const specialCharsUri = mongoUri({
    username: "user@domain",
    password: "p@ss:word",
  });
  assert.ok(specialCharsUri.includes("user%40domain"));
  assert.ok(specialCharsUri.includes("p%40ss%3Aword"));

  console.log("  ✓ mongoUri builder tests passed");
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

async function smokeTestCrud(port, username, password) {
  const { mongoUri } = await import("./uri.ts");
  const { MongoClient } = await import("mongodb");
  const client = new MongoClient(
    mongoUri({ port, database: "smoketest", username, password }),
  );
  await client.connect();

  try {
    const db = client.db("smoketest");
    const coll = db.collection("smoke_crud");

    await coll.deleteMany({});

    const insertResult = await coll.insertOne({ name: "Alice", age: 30 });
    assert.ok(insertResult.insertedId, "insertOne should return an insertedId");

    const insertManyResult = await coll.insertMany([
      { name: "Bob", age: 25 },
      { name: "Carol", age: 35 },
    ]);
    assert.equal(insertManyResult.insertedCount, 2);

    const found = await coll.findOne({ name: "Alice" });
    assert.equal(found.name, "Alice");
    assert.equal(found.age, 30);

    const all = await coll.find({}).sort({ name: 1 }).toArray();
    assert.ok(all.length >= 3, `expected at least 3 docs, got ${all.length}`);

    const updateResult = await coll.updateOne(
      { name: "Alice" },
      { $set: { age: 31 } },
    );
    assert.equal(updateResult.modifiedCount, 1);

    const updated = await coll.findOne({ name: "Alice" });
    assert.equal(updated.age, 31);

    const deleteResult = await coll.deleteOne({ name: "Carol" });
    assert.equal(deleteResult.deletedCount, 1);

    const afterDelete = await coll.find({}).toArray();
    assert.ok(
      !afterDelete.some((d) => d.name === "Carol"),
      "Carol should be deleted",
    );

    const count = await coll.countDocuments({});
    assert.ok(count >= 2, `expected at least 2 remaining docs, got ${count}`);

    const distinct = await coll.distinct("name");
    assert.ok(distinct.includes("Alice"));
    assert.ok(distinct.includes("Bob"));

    console.log("  ✓ smoke test: CRUD operations passed");
  } finally {
    await client.close();
  }
}

async function smokeTestAggregation(port, username, password) {
  const { mongoUri } = await import("./uri.ts");
  const { MongoClient } = await import("mongodb");
  const client = new MongoClient(
    mongoUri({ port, database: "smoketest", username, password }),
  );
  await client.connect();

  try {
    const db = client.db("smoketest");
    const coll = db.collection("smoke_agg");

    await coll.deleteMany({});
    await coll.insertMany([
      { dept: "eng", salary: 100 },
      { dept: "eng", salary: 120 },
      { dept: "sales", salary: 80 },
    ]);

    const pipeline = [
      { $group: { _id: "$dept", total: { $sum: "$salary" } } },
      { $sort: { _id: 1 } },
    ];
    const results = await coll.aggregate(pipeline).toArray();
    assert.ok(results.length >= 2, "expected at least 2 groups");

    const matchPipeline = [{ $match: { dept: "eng" } }, { $count: "total" }];
    const matchResults = await coll.aggregate(matchPipeline).toArray();
    assert.equal(matchResults[0].total, 2);

    console.log("  ✓ smoke test: aggregation pipeline passed");
  } finally {
    await client.close();
  }
}

async function smokeTestWrongPasswordRejected(port, username) {
  const { mongoUri } = await import("./uri.ts");
  const { MongoClient } = await import("mongodb");
  const client = new MongoClient(
    mongoUri({
      port,
      database: "smoketest",
      username,
      password: "wrong-password",
    }),
    { serverSelectionTimeoutMS: 2000 },
  );
  try {
    await assert.rejects(async () => {
      await client.connect();
      await client.db("smoketest").collection("smoke_crud").findOne({});
    }, "a wrong password must not authenticate");
    console.log("  ✓ smoke test: wrong password rejected");
  } finally {
    await client.close();
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
