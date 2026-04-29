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

async function resolveSourceRoot(appDir) {
  const neovexDir = path.join(appDir, "neovex");
  const convexDir = path.join(appDir, "convex");
  const neovexExists = await directoryExists(neovexDir);
  const convexExists = await directoryExists(convexDir);

  if (neovexExists && convexExists) {
    return {
      sourceDirName: "neovex",
      sourceDirPath: neovexDir,
      packageNamespace: "neovex",
      detectedBothRoots: true,
    };
  }

  if (neovexExists) {
    return {
      sourceDirName: "neovex",
      sourceDirPath: neovexDir,
      packageNamespace: "neovex",
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
    `No neovex/ or convex/ directory found in ${appDir}. ` +
    `Create one of those directories and place your app functions there.`,
  );
}

async function tryResolveSourceRoot(appDir) {
  try {
    return await resolveSourceRoot(appDir);
  } catch (error) {
    if (
      error instanceof Error
      && error.message.startsWith("No neovex/ or convex/ directory found in ")
    ) {
      return null;
    }
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
