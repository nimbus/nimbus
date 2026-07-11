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

async function directoryExists(directoryPath) {
  try {
    const stat = await fs.stat(directoryPath);
    return stat.isDirectory();
  } catch (error) {
    if (error && typeof error === "object" && error.code === "ENOENT") {
      return false;
    }
    throw error;
  }
}

async function fileExists(filePath) {
  try {
    const stat = await fs.stat(filePath);
    return stat.isFile();
  } catch (error) {
    if (error && typeof error === "object" && error.code === "ENOENT") {
      return false;
    }
    throw error;
  }
}

async function readUtf8FileIfExists(filePath) {
  if (!await fileExists(filePath)) {
    return null;
  }
  try {
    return await fs.readFile(filePath, "utf8");
  } catch (error) {
    if (error && typeof error === "object" && error.code === "ENOENT") {
      return null;
    }
    throw error;
  }
}

async function resolveSourceRoot(appDir, { functionsOverride } = {}) {
  // convex.json's "functions" setting relocates the source directory (e.g.
  // Create React App projects that cannot import from outside src/). It is
  // a Convex-specific setting, so an override always resolves to the
  // "convex" package namespace rather than re-running the nimbus/convex
  // dual-root heuristic below.
  if (functionsOverride != null) {
    const overrideDir = path.resolve(appDir, functionsOverride);
    if (!await directoryExists(overrideDir)) {
      const relativeOverride = path.relative(appDir, overrideDir) || ".";
      throw new Error(
        `convex.json declares "functions": ${JSON.stringify(functionsOverride)}, but ` +
        `${relativeOverride} is not a directory in ${appDir}. Create that directory with ` +
        `your Convex functions inside it, or remove "functions" from convex.json.`,
      );
    }
    return {
      sourceDirName: path.basename(overrideDir),
      sourceDirPath: overrideDir,
      packageNamespace: "convex",
      detectedBothRoots: false,
    };
  }

  const nimbusDir = path.join(appDir, "nimbus");
  const convexDir = path.join(appDir, "convex");
  const nimbusExists = await directoryExists(nimbusDir);
  const convexExists = await directoryExists(convexDir);

  if (nimbusExists && convexExists) {
    return {
      sourceDirName: "nimbus",
      sourceDirPath: nimbusDir,
      packageNamespace: "@nimbus/nimbus",
      detectedBothRoots: true,
    };
  }

  if (nimbusExists) {
    return {
      sourceDirName: "nimbus",
      sourceDirPath: nimbusDir,
      packageNamespace: "@nimbus/nimbus",
      detectedBothRoots: false,
    };
  }

  if (convexExists) {
    return {
      sourceDirName: "convex",
      sourceDirPath: convexDir,
      packageNamespace: "convex",
      detectedBothRoots: false,
    };
  }

  throw new Error(
    `No nimbus/ or convex/ directory found in ${appDir}. ` +
    `Create one of those directories and place your app functions there.`,
  );
}

async function tryResolveSourceRoot(appDir, options) {
  try {
    return await resolveSourceRoot(appDir, options);
  } catch (error) {
    if (
      error instanceof Error
      && error.message.startsWith("No nimbus/ or convex/ directory found in ")
    ) {
      return null;
    }
    // A declared-but-missing "functions" override is a real misconfiguration,
    // not "no Convex app here" — surface it rather than silently falling
    // through to Cloud Functions detection.
    throw error;
  }
}

async function collectModuleFiles(sourceDir) {
  const files = [];
  await walk(sourceDir, files);
  return files
    .filter((filePath) => {
      const relative = path.relative(sourceDir, filePath);
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

export {
  collectModuleFiles,
  fileExists,
  readUtf8FileIfExists,
  resolveAppDirectory,
  resolveSourceRoot,
  sha256Hex,
  tryResolveSourceRoot,
};
