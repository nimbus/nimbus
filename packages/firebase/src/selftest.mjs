import { assert } from "./selftest/support.mjs";
import {
  assertGeneratedProtoSurface,
  assertPackageExports,
  buildPackageSurface,
  typecheckFirebaseSurface,
} from "./selftest/package_surface.mjs";
import { testRuntimeSurface } from "./selftest/runtime_surface.mjs";
import { testSmokeSurface } from "./selftest/smoke_surface.mjs";

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

await main();
