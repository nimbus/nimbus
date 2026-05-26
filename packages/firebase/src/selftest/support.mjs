import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";

import { encodeEnvelope } from "@connectrpc/connect/protocol";
import { trailerFlag, trailerSerialize } from "@connectrpc/connect/protocol-grpc-web";
import { build } from "esbuild";

const require = createRequire(import.meta.url);
const packageRoot = fileURLToPath(new URL("../../", import.meta.url));
const packageJsonPath = fileURLToPath(new URL("../../package.json", import.meta.url));
const tscPath = fileURLToPath(
  new URL("../../../../node_modules/typescript/bin/tsc", import.meta.url),
);

export {
  assert,
  build,
  encodeEnvelope,
  fileURLToPath,
  fs,
  os,
  packageJsonPath,
  packageRoot,
  path,
  pathToFileURL,
  require,
  spawnSync,
  trailerFlag,
  trailerSerialize,
  tscPath,
};
