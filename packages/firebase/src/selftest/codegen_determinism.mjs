import crypto from "node:crypto";

import { assert, fs, os, path } from "./support.mjs";
import { generateProtos } from "../codegen-protos.mjs";

// DE16 required test: regenerating the buf/protoc-gen-es output must be
// byte-identical run to run. Generates into two disposable directories (never
// the real src/gen tree) and diffs the full file set + content hashes.
export async function assertCodegenDeterminism() {
  const firstRun = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-firebase-codegen-a-"));
  const secondRun = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus-firebase-codegen-b-"));
  try {
    await generateProtos(firstRun);
    await generateProtos(secondRun);

    const firstDigest = await hashTree(firstRun);
    const secondDigest = await hashTree(secondRun);

    const firstPaths = [...firstDigest.keys()].sort();
    const secondPaths = [...secondDigest.keys()].sort();
    assert.deepEqual(
      firstPaths,
      secondPaths,
      "codegen-protos.mjs produced a different file set across two runs (non-deterministic generation).",
    );

    const mismatched = firstPaths.filter((relativePath) => firstDigest.get(relativePath) !== secondDigest.get(relativePath));
    assert.deepEqual(
      mismatched,
      [],
      `codegen-protos.mjs produced non-deterministic output for: ${mismatched.join(", ")}`,
    );
    assert.ok(firstPaths.length > 0, "codegen-protos.mjs produced no output to compare.");
  } finally {
    await fs.rm(firstRun, { recursive: true, force: true });
    await fs.rm(secondRun, { recursive: true, force: true });
  }
}

async function hashTree(root) {
  const digests = new Map();
  await walk(root, "");
  return digests;

  async function walk(dir, relativeDir) {
    for (const entry of await fs.readdir(dir, { withFileTypes: true })) {
      const absolute = path.join(dir, entry.name);
      const relative = relativeDir ? `${relativeDir}/${entry.name}` : entry.name;
      if (entry.isDirectory()) {
        await walk(absolute, relative);
        continue;
      }
      const content = await fs.readFile(absolute);
      digests.set(relative, crypto.createHash("sha256").update(content).digest("hex"));
    }
  }
}
