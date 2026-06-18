import path from "node:path";
import { fileURLToPath } from "node:url";

import { runActionFixtures } from "./selftest/action_fixtures.mjs";
import { runCapabilityBoundaryFixtures } from "./selftest/capability_boundary_fixtures.mjs";
import { runCodegenChecks } from "./selftest/check_fixtures.mjs";
import { runCloudFunctionsFixtures } from "./selftest/cloud_functions_fixtures.mjs";
import { runCoreFixtures } from "./selftest/core_fixtures.mjs";
import { runDatabaseFixtures } from "./selftest/database_fixtures.mjs";
import { runRuntimeFixtures } from "./selftest/runtime_fixtures.mjs";
import { runRuntimeRemapFixtures } from "./selftest/runtime_remap_fixtures.mjs";
import { runTypeInferenceFixtures } from "./selftest/type_inference_fixtures.mjs";

const isDirectExecution =
  !!process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
const typecheckOnly = process.argv.includes("--typecheck-only");

async function main() {
  await runCodegenChecks();
  if (typecheckOnly) {
    return;
  }
  await runCapabilityBoundaryFixtures();
  await runCloudFunctionsFixtures();
  await runCoreFixtures();
  await runDatabaseFixtures();
  await runActionFixtures();
  await runRuntimeFixtures();
  await runRuntimeRemapFixtures();
  await runTypeInferenceFixtures();
}

export { main };

if (isDirectExecution) {
  main().catch((error) => {
    console.error(error);
    process.exit(1);
  });
}
