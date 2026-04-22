#!/usr/bin/env node

import { runCliFromArgs } from "./main.mjs";

runCliFromArgs(process.argv.slice(2), {
  onInfo(message) {
    console.error(message);
  },
}).catch((error) => {
  console.error(error.message);
  process.exit(1);
});
