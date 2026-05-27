import assert from "node:assert/strict";

const targets = {
  nimbus: {
    uriEnv: "NIMBUS_MONGODB_DUAL_TARGET_URI",
  },
  mongodb_cloud: {
    uriEnv: "MONGODB_CLOUD_DUAL_TARGET_URI",
  },
};

const targetName = process.env.NIMBUS_TEST_TARGET ?? "nimbus";
const target = targets[targetName];
assert.ok(
  target,
  `mongodb dual-target test does not define NIMBUS_TEST_TARGET=${targetName}. Known targets: ${Object.keys(targets).join(", ")}`,
);

const uri = process.env[target.uriEnv] ?? "";
if (process.env.NIMBUS_DUAL_TARGET_DRY_RUN === "1") {
  console.log(
    `dual-target dry-run: mongodb/${targetName} SCRAM auth probe via ${target.uriEnv}`,
  );
} else if (!uri) {
  throw new Error(
    `mongodb ${targetName} target requires ${target.uriEnv}. Set NIMBUS_DUAL_TARGET_DRY_RUN=1 to validate only the target contract.`,
  );
} else {
  const { MongoClient } = await import("mongodb");
  const badCredentialUri = new URL(uri);
  badCredentialUri.username = process.env.MONGODB_DUAL_TARGET_BAD_USER ?? "dual-target";
  badCredentialUri.password =
    process.env.MONGODB_DUAL_TARGET_BAD_PASSWORD ?? "definitely-wrong";

  const client = new MongoClient(badCredentialUri.toString(), {
    serverSelectionTimeoutMS: 5_000,
  });
  try {
    await client.connect();
    await client.db("admin").command({ ping: 1 });
    assert.fail("MongoDB dual-target auth probe unexpectedly authenticated");
  } catch (error) {
    assert.match(
      String(error?.message ?? error),
      /auth|credential|AuthenticationFailed|bad auth/i,
    );
    if (error && typeof error === "object" && "code" in error) {
      assert.equal(error.code, 18, "MongoDB auth failures should use code 18");
    }
  } finally {
    await client.close().catch(() => {});
  }
}
