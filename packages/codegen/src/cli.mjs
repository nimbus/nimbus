#!/usr/bin/env node

import { runCliFromArgs } from "./main.mjs";

runCliFromArgs(process.argv.slice(2)).catch((error) => {
  console.error(error.message);
  process.exit(1);
});
