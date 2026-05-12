import fs from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { readUtf8FileIfExists, sha256Hex } from "./app.mjs";
import { buildCloudFunctionsRuntimeBundle } from "./cloud_functions/bundle.mjs";
import { detectCloudFunctionsProject } from "./cloud_functions/project.mjs";

const CLOUD_FUNCTIONS_ARTIFACT_MANIFEST = Object.freeze({
  version: 1,
  family: "cloud_functions",
  runtime_bundle: {
    entry_file: "bundle.mjs",
    sha256_file: "bundle.sha256",
  },
  targets_manifest: "targets.json",
  import_resolution: {
    strategy: "deploy_alias_layer",
    covered_specifiers: [
      "@google-cloud/functions-framework",
      "firebase-admin/app",
      "firebase-admin/firestore",
      "firebase-functions/v2",
      "firebase-functions/v2/firestore",
      "firebase-functions/v2/https",
    ],
  },
});

async function generateCloudFunctionsArtifacts({ appDir, onInfo } = {}) {
  const project = await detectCloudFunctionsProject(appDir);
  if (project === null) {
    return null;
  }

  if (project.kind === "firebase_project") {
    onInfo?.(`Detected Firebase Cloud Functions app at ${appDir}.`);
  } else {
    onInfo?.(`Detected standalone Functions Framework app at ${appDir}.`);
  }
  await fs.mkdir(project.artifactDir, { recursive: true });
  const runtimeBundle = await buildCloudFunctionsRuntimeBundle(project);
  const bundlePath = path.join(project.artifactDir, "bundle.mjs");
  await fs.writeFile(bundlePath, runtimeBundle, "utf8");
  await fs.writeFile(
    path.join(project.artifactDir, "bundle.sha256"),
    `${sha256Hex(runtimeBundle)}\n`,
    "utf8",
  );
  const bundleModule = await import(
    `${pathToFileURL(bundlePath).href}?generatedAt=${Date.now()}`
  );
  const discoveredTargets = bundleModule.__nimbusTargets;
  if (!Array.isArray(discoveredTargets)) {
    throw new Error(
      `Generated Cloud Functions runtime bundle for ${appDir} did not expose __nimbusTargets.`,
    );
  }
  const targets = await finalizeCloudFunctionsTargets(project, discoveredTargets);
  await fs.writeFile(
    path.join(project.artifactDir, "artifact.json"),
    `${JSON.stringify(CLOUD_FUNCTIONS_ARTIFACT_MANIFEST, null, 2)}\n`,
    "utf8",
  );
  await fs.writeFile(
    path.join(project.artifactDir, "targets.json"),
    `${JSON.stringify({ version: 1, targets }, null, 2)}\n`,
    "utf8",
  );

  return {
    appDir,
    project,
    targets,
  };
}

async function finalizeCloudFunctionsTargets(project, discoveredTargets) {
  if (project.kind === "firebase_project") {
    return discoveredTargets;
  }

  return mergeFrameworkTargetBindings(project, discoveredTargets);
}

async function mergeFrameworkTargetBindings(project, discoveredTargets) {
  const targetsPath = path.join(project.artifactDir, "targets.json");
  const existingManifest = await readOptionalJson(targetsPath);
  if (existingManifest === null) {
    if (discoveredTargets.length === 0) {
      return [];
    }
    throw new Error(
      `Standalone Functions Framework app ${project.appDir} requires ${targetsPath} to bind discovered targets: ${discoveredTargets
        .map((target) => target.name)
        .sort((left, right) => left.localeCompare(right))
        .join(", ")}.`,
    );
  }

  if (existingManifest.version !== 1 || !Array.isArray(existingManifest.targets)) {
    throw new Error(
      `Standalone Functions Framework targets manifest at ${targetsPath} must use {"version":1,"targets":[...]}.`,
    );
  }

  const manifestTargets = new Map();
  for (const manifestTarget of existingManifest.targets) {
    if (!manifestTarget || typeof manifestTarget !== "object" || Array.isArray(manifestTarget)) {
      throw new Error(
        `Standalone Functions Framework targets manifest at ${targetsPath} must contain object targets.`,
      );
    }
    if (typeof manifestTarget.name !== "string" || manifestTarget.name.trim().length === 0) {
      throw new Error(
        `Standalone Functions Framework targets manifest at ${targetsPath} contains a target without a non-empty name.`,
      );
    }
    const targetName = manifestTarget.name.trim();
    if (manifestTargets.has(targetName)) {
      throw new Error(
        `Standalone Functions Framework targets manifest at ${targetsPath} duplicates target "${targetName}".`,
      );
    }
    manifestTargets.set(targetName, manifestTarget);
  }

  const finalizedTargets = [];
  for (const discoveredTarget of discoveredTargets) {
    const manifestTarget = manifestTargets.get(discoveredTarget.name);
    if (!manifestTarget) {
      throw new Error(
        `Standalone Functions Framework target "${discoveredTarget.name}" is missing a binding entry in ${targetsPath}.`,
      );
    }
    if (manifestTarget.authoring_surface !== "functions_framework") {
      throw new Error(
        `Standalone Functions Framework target "${discoveredTarget.name}" in ${targetsPath} must declare "authoring_surface": "functions_framework".`,
      );
    }
    if (manifestTarget.signature_type !== discoveredTarget.signature_type) {
      throw new Error(
        `Standalone Functions Framework target "${discoveredTarget.name}" in ${targetsPath} must use signature_type "${discoveredTarget.signature_type}".`,
      );
    }
    if (
      typeof manifestTarget.entrypoint === "string"
      && manifestTarget.entrypoint.trim().length > 0
      && manifestTarget.entrypoint !== discoveredTarget.entrypoint
    ) {
      throw new Error(
        `Standalone Functions Framework target "${discoveredTarget.name}" in ${targetsPath} must use entrypoint "${discoveredTarget.entrypoint}".`,
      );
    }
    if (
      manifestTarget.binding === null
      || typeof manifestTarget.binding !== "object"
      || Array.isArray(manifestTarget.binding)
    ) {
      throw new Error(
        `Standalone Functions Framework target "${discoveredTarget.name}" in ${targetsPath} must include a binding object.`,
      );
    }
    finalizedTargets.push({
      name: discoveredTarget.name,
      entrypoint: discoveredTarget.entrypoint,
      authoring_surface: "functions_framework",
      signature_type: discoveredTarget.signature_type,
      binding: manifestTarget.binding,
    });
    manifestTargets.delete(discoveredTarget.name);
  }

  if (manifestTargets.size > 0) {
    throw new Error(
      `Standalone Functions Framework targets manifest at ${targetsPath} declares unknown targets: ${[...manifestTargets.keys()].join(", ")}.`,
    );
  }

  return finalizedTargets;
}

async function readOptionalJson(filePath) {
  const source = await readUtf8FileIfExists(filePath);
  return source === null ? null : JSON.parse(source);
}

export { detectCloudFunctionsProject, generateCloudFunctionsArtifacts };
