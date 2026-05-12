#!/usr/bin/env node

import { runCliFromArgs } from "@nimbus/codegen";

const HELP_TEXT = `Usage: convex <command> [options]

Commands:
  codegen   Generate _generated files and the compatible runtime bundle

Supported today:
  convex codegen --app <dir>

Source roots:
  uses nimbus/ when present, otherwise convex/
`;

async function main() {
  const [command, ...rest] = process.argv.slice(2);

  if (!command || command === "help" || command === "--help" || command === "-h") {
    console.log(HELP_TEXT);
    return;
  }

  if (command === "codegen") {
    await runCliFromArgs(rest, {
      onInfo(message) {
        console.error(message);
      },
    });
    return;
  }

  console.error(
    `Unsupported convex command "${command}". This CLI currently supports "convex codegen".`,
  );
  process.exit(1);
}

main().catch((error) => {
  console.error(error.message);
  process.exit(1);
});
