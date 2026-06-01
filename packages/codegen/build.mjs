// Prebundle @nimbus/codegen into a single JS payload for the in-binary Nimbus
// V8 tooling runner (BPD4). The codegen runtime path imports only `typescript`
// (inlined here) + node builtins + a lazy `esbuild` import. esbuild is kept
// external because it is staged SEPARATELY as a tooling binary under the V8
// tooling runtime (RuntimeLimits::tooling_node22 + $discovered_tooling) — NOT
// because it cannot run in V8 (it can, as a staged tooling target) and NOT via
// external Node. The bundle loads and runs codegen in V8 with no app
// `node_modules/@nimbus/codegen`; the esbuild paths resolve the staged tooling
// binary lazily.

import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { build } from "esbuild";

const root = fileURLToPath(new URL("./", import.meta.url));
const outfile = path.join(root, "dist", "codegen.bundle.mjs");

export async function buildCodegenBundle() {
  const result = await build({
    entryPoints: [path.join(root, "src", "main.mjs")],
    outfile,
    bundle: true,
    format: "esm",
    platform: "node",
    target: "node20",
    // Kept external: esbuild is staged as a separate tooling binary under the
    // V8 tooling runtime and resolved lazily (not inlined, not external Node).
    external: ["esbuild"],
    logLevel: "silent",
    metafile: true,
    // typescript (CJS) does dynamic `require("fs")` etc.; provide a real
    // createRequire so the bundle resolves node builtins through the host's
    // Node-compatible module system instead of esbuild's throwing __require shim.
    banner: {
      js: [
        "// Prebundled @nimbus/codegen for the in-binary V8 codegen runner (BPD4).",
        'import { createRequire as __nimbusCreateRequire } from "node:module";',
        'import { fileURLToPath as __nimbusFileURLToPath } from "node:url";',
        'import { dirname as __nimbusDirname } from "node:path";',
        "const require = __nimbusCreateRequire(import.meta.url);",
        "const __filename = __nimbusFileURLToPath(import.meta.url);",
        "const __dirname = __nimbusDirname(__filename);",
      ].join("\n"),
    },
  });
  return { outfile, result };
}

async function main() {
  const { outfile: out } = await buildCodegenBundle();
  console.log(`wrote ${path.relative(root, out)}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error);
    process.exit(1);
  });
}
