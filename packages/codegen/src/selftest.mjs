import path from "node:path";
import { fileURLToPath } from "node:url";

import { runActionFixtures } from "./selftest/action_fixtures.mjs";
import { runCoreFixtures } from "./selftest/core_fixtures.mjs";
import { runDatabaseFixtures } from "./selftest/database_fixtures.mjs";
import { runRuntimeFixtures } from "./selftest/runtime_fixtures.mjs";

const isDirectExecution =
  !!process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);

async function main() {
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
