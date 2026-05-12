import fs from "node:fs/promises";
import path from "node:path";

const CLOUD_FUNCTIONS_INTERNAL_DIR = [".nimbus", "firebase"];
const DEFAULT_FIREBASE_FUNCTIONS_SOURCE = "functions";
const DEFAULT_FIREBASE_CODEBASE = "default";
const FUNCTIONS_FRAMEWORK_PACKAGE = "@google-cloud/functions-framework";
const DEFAULT_PACKAGE_ENTRYPOINTS = [
  "src/index.ts",
  "src/index.mts",
  "src/index.cts",
  "src/index.js",
  "src/index.mjs",
  "src/index.cjs",
  "index.ts",
  "index.mts",
  "index.cts",
  "index.js",
  "index.mjs",
  "index.cjs",
];
const DEFAULT_FRAMEWORK_ENTRYPOINTS = [
  "index.ts",
  "index.mts",
  "index.cts",
  "index.js",
  "index.mjs",
  "index.cjs",
];

async function detectCloudFunctionsProject(appDir) {
  const firebaseProject = await detectFirebaseCloudFunctionsProject(appDir);
  if (firebaseProject !== null) {
    return firebaseProject;
  }

  return detectFrameworkCloudFunctionsProject(appDir);
}

async function detectFirebaseCloudFunctionsProject(appDir) {
  const firebaseJsonPath = path.join(appDir, "firebase.json");
  if (!(await fileExists(firebaseJsonPath))) {
    return null;
  }

  const rawFirebaseJson = JSON.parse(await fs.readFile(firebaseJsonPath, "utf8"));
  const codebaseDescriptors = normalizeFirebaseCodebases(rawFirebaseJson.functions);
  const codebases = [];
  for (const descriptor of codebaseDescriptors) {
    const sourceDir = path.resolve(appDir, descriptor.source);
    if (!(await directoryExists(sourceDir))) {
      throw new Error(
        `Firebase Functions source directory ${path.relative(appDir, sourceDir) || "."} does not exist in ${appDir}.`,
      );
    }
    const entrypoint = await resolvePackageEntrypoint(sourceDir);
    codebases.push({
      name: descriptor.codebase,
      sourceDir,
      entrypoint,
    });
  }

  return {
    kind: "firebase_project",
    appDir,
    artifactDir: path.join(appDir, ...CLOUD_FUNCTIONS_INTERNAL_DIR),
    codebases,
  };
}

async function detectFrameworkCloudFunctionsProject(appDir) {
  const packageJsonPath = path.join(appDir, "package.json");
  if (!(await fileExists(packageJsonPath))) {
    return null;
  }

  const packageJson = JSON.parse(await fs.readFile(packageJsonPath, "utf8"));
  if (!packageDependsOn(packageJson, FUNCTIONS_FRAMEWORK_PACKAGE)) {
    return null;
  }

  const entrypoint = await resolvePackageEntrypoint(appDir, {
    packageJson,
    defaultEntrypoints: DEFAULT_FRAMEWORK_ENTRYPOINTS,
    surfaceLabel: "standalone Functions Framework package",
  });

  return {
    kind: "framework_package",
    appDir,
    artifactDir: path.join(appDir, ...CLOUD_FUNCTIONS_INTERNAL_DIR),
    entrypoint,
  };
}

function packageDependsOn(packageJson, packageName) {
  const dependencyKeys = [
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    "peerDependencies",
  ];
  return dependencyKeys.some((key) => {
    const value = packageJson[key];
    return value && typeof value === "object" && !Array.isArray(value) && packageName in value;
  });
}

function normalizeFirebaseCodebases(rawFunctionsConfig) {
  if (rawFunctionsConfig === undefined) {
    return [{
      source: DEFAULT_FIREBASE_FUNCTIONS_SOURCE,
      codebase: DEFAULT_FIREBASE_CODEBASE,
    }];
  }

  if (typeof rawFunctionsConfig === "string") {
    return [{
      source: rawFunctionsConfig,
      codebase: DEFAULT_FIREBASE_CODEBASE,
    }];
  }

  if (Array.isArray(rawFunctionsConfig)) {
    return normalizeFirebaseCodebaseDescriptors(rawFunctionsConfig);
  }

  if (rawFunctionsConfig && typeof rawFunctionsConfig === "object") {
    return normalizeFirebaseCodebaseDescriptors([rawFunctionsConfig]);
  }

  throw new Error("firebase.json functions configuration must be a string, object, or array.");
}

function normalizeFirebaseCodebaseDescriptors(descriptors) {
  const seen = new Set();
  return descriptors.map((descriptor) => {
    if (!descriptor || typeof descriptor !== "object" || Array.isArray(descriptor)) {
      throw new Error("firebase.json functions descriptors must be objects.");
    }
    const source = normalizeNonEmptyString(
      descriptor.source ?? DEFAULT_FIREBASE_FUNCTIONS_SOURCE,
      "firebase.json functions source",
    );
    const codebase = normalizeNonEmptyString(
      descriptor.codebase ?? DEFAULT_FIREBASE_CODEBASE,
      "firebase.json functions codebase",
    );
    if (seen.has(codebase)) {
      throw new Error(`firebase.json reuses Functions codebase "${codebase}".`);
    }
    seen.add(codebase);
    return { source, codebase };
  });
}

function normalizeNonEmptyString(value, label) {
  if (typeof value !== "string" || value.trim().length === 0) {
    throw new Error(`${label} must be a non-empty string.`);
  }
  return value.trim();
}

async function resolvePackageEntrypoint(
  packageDir,
  { packageJson = null, defaultEntrypoints = DEFAULT_PACKAGE_ENTRYPOINTS, surfaceLabel = "Cloud Functions" } = {},
) {
  const resolvedPackageJson = packageJson ?? await loadOptionalPackageJson(packageDir);
  if (resolvedPackageJson && typeof resolvedPackageJson.main === "string" && resolvedPackageJson.main.trim().length > 0) {
    const packageMain = resolvedPackageJson.main.trim();
    const candidates = [
      packageMain,
      ...sourceAlternativesForPackageMain(packageMain),
    ];
    for (const candidate of candidates) {
      const resolved = path.resolve(packageDir, candidate);
      if (await fileExists(resolved)) {
        return resolved;
      }
    }
  }

  for (const candidate of defaultEntrypoints) {
    const resolved = path.resolve(packageDir, candidate);
    if (await fileExists(resolved)) {
      return resolved;
    }
  }

  throw new Error(
    `Could not find a ${surfaceLabel} entrypoint in ${packageDir}. Expected package.json main or one of: ${defaultEntrypoints.join(", ")}.`,
  );
}

async function loadOptionalPackageJson(packageDir) {
  const packageJsonPath = path.join(packageDir, "package.json");
  if (!(await fileExists(packageJsonPath))) {
    return null;
  }
  return JSON.parse(await fs.readFile(packageJsonPath, "utf8"));
}

function sourceAlternativesForPackageMain(packageMain) {
  const ext = path.extname(packageMain);
  const stem = ext ? packageMain.slice(0, -ext.length) : packageMain;
  const alternatives = new Set();

  for (const candidateExt of [".ts", ".mts", ".cts", ".js", ".mjs", ".cjs"]) {
    alternatives.add(`${stem}${candidateExt}`);
  }

  for (const compiledPrefix of ["lib/", "dist/", "build/"]) {
    if (packageMain.startsWith(compiledPrefix)) {
      const sourceStem = `src/${packageMain.slice(compiledPrefix.length).replace(/\.[^.]+$/, "")}`;
      for (const candidateExt of [".ts", ".mts", ".cts", ".js", ".mjs", ".cjs"]) {
        alternatives.add(`${sourceStem}${candidateExt}`);
      }
    }
  }

  return [...alternatives];
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

export { detectCloudFunctionsProject };
