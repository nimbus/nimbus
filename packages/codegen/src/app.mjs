import { createHash } from "node:crypto";
import fs from "node:fs/promises";
import path from "node:path";

function sha256Hex(contents) {
  return createHash("sha256").update(contents).digest("hex");
}

function resolveAppDirectory(args) {
  let app = ".";
  for (let index = 0; index < args.length; index += 1) {
    if (args[index] === "--app") {
      app = args[index + 1] ?? ".";
      index += 1;
    }
  }
  return path.resolve(process.cwd(), app);
}

async function collectModuleFiles(convexDir) {
  const files = [];
  await walk(convexDir, files);
  return files
    .filter((filePath) => {
      const relative = path.relative(convexDir, filePath);
      return (
        !relative.startsWith("_generated") &&
        relative !== "schema.ts" &&
        relative !== "schema.js" &&
        relative !== "http.ts" &&
        relative !== "http.js" &&
        relative !== "auth.config.ts" &&
        relative !== "auth.config.js" &&
        (filePath.endsWith(".ts") || filePath.endsWith(".tsx")) &&
        !filePath.endsWith(".d.ts")
      );
    })
    .sort();
}

async function walk(directory, files) {
  const entries = await fs.readdir(directory, { withFileTypes: true });
  for (const entry of entries) {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      await walk(entryPath, files);
      continue;
    }
    files.push(entryPath);
  }
}

export { collectModuleFiles, resolveAppDirectory, sha256Hex };
