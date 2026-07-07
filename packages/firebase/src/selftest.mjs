import { assert } from "./selftest/support.mjs";
import { assertCodegenDeterminism } from "./selftest/codegen_determinism.mjs";
import {
  assertGeneratedProtoSurface,
  assertPackageExports,
  buildPackageSurface,
  typecheckFirebaseSurface,
} from "./selftest/package_surface.mjs";
import { testRoundTripSurface } from "./selftest/round_trip_surface.mjs";
import { testRuntimeSurface } from "./selftest/runtime_surface.mjs";
import { testSmokeSurface } from "./selftest/smoke_surface.mjs";

const buildOnly = process.argv.includes("--build-only");
const typecheckOnly = process.argv.includes("--typecheck-only");
const smokeBaseUrl = optionalFlagValue("--smoke-base-url");
const roundTripBaseUrl = optionalFlagValue("--round-trip-base-url");
const roundTripProjectId = optionalFlagValue("--round-trip-project-id");

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
  await assertCodegenDeterminism();
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
  if (roundTripBaseUrl) {
    assert.ok(
      roundTripProjectId,
      "--round-trip-base-url requires --round-trip-project-id.",
    );
    await testRoundTripSurface(bundleDir, roundTripBaseUrl, roundTripProjectId);
    return;
  }

  await testRuntimeSurface(bundleDir);
  await typecheckFirebaseSurface();
}

await main();
