import path from "node:path";
import { fileURLToPath } from "node:url";

import { runActionFixtures } from "./selftest/action_fixtures.mjs";
import { runCodegenChecks } from "./selftest/check_fixtures.mjs";
import { runCoreFixtures } from "./selftest/core_fixtures.mjs";
import { runDatabaseFixtures } from "./selftest/database_fixtures.mjs";
import { runRuntimeFixtures } from "./selftest/runtime_fixtures.mjs";

const isDirectExecution =
  !!process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
const typecheckOnly = process.argv.includes("--typecheck-only");

async function main() {
  await runCodegenChecks();
  if (typecheckOnly) {
    return;
  }
  await runCoreFixtures();
  await runDatabaseFixtures();
  await runActionFixtures();
  await runRuntimeFixtures();
}

export { main };

if (isDirectExecution) {
  main().catch((error) => {
    console.error(error);
    process.exit(1);
  });
}
