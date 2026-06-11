export const NIMBUS_ROOT_SDK_ARTIFACT_PATHS = [
  "packages/nimbus/dist/index.js",
  "packages/nimbus/dist/index.d.ts",
  "crates/nimbus-assets/embedded/packages/@nimbus/nimbus/index.js",
  "crates/nimbus-assets/embedded/packages/@nimbus/nimbus/index.d.ts",
];

export const NIMBUS_ROOT_SDK_FORBIDDEN_FRAGMENTS = [
  "ensureRunning",
  "/api/services/",
  "NimbusSessionCreateRequest",
  "sessions.create",
  "sessions.renew",
  "sessions.extend",
  "async request(path",
  "async resolveRestClient",
];

export const NIMBUS_ROOT_SDK_METHOD_FRAGMENTS = [
  "start(input",
  "stop(input",
  "restart(input",
  "get(selector",
  "wait(input",
  "open(input",
  "close(input",
];

export const NIMBUS_ROOT_SDK_RUNTIME_FRAGMENTS = [
  "/api/tenants/",
  "/services/",
  "/api/sessions",
  "#controlPlaneRequest",
  "#resolveRestClient",
];

export function isNimbusRootSdkRuntimeArtifact(artifactPath) {
  return (
    artifactPath.endsWith(".js") ||
    (artifactPath.endsWith(".ts") && !artifactPath.endsWith(".d.ts"))
  );
}

export function assertNimbusRootSdkArtifactText(
  artifactPath,
  artifact,
  options = {},
) {
  const runtime = options.runtime ?? isNimbusRootSdkRuntimeArtifact(artifactPath);
  const errors = [];

  for (const fragment of NIMBUS_ROOT_SDK_FORBIDDEN_FRAGMENTS) {
    if (artifact.includes(fragment)) {
      errors.push(
        `${artifactPath} contains stale or public root SDK fragment: ${fragment}`,
      );
    }
  }
  for (const fragment of NIMBUS_ROOT_SDK_METHOD_FRAGMENTS) {
    if (!artifact.includes(fragment)) {
      errors.push(
        `${artifactPath} is missing canonical service API fragment: ${fragment}`,
      );
    }
  }
  if (runtime) {
    for (const fragment of NIMBUS_ROOT_SDK_RUNTIME_FRAGMENTS) {
      if (!artifact.includes(fragment)) {
        errors.push(
          `${artifactPath} is missing canonical service route/runtime fragment: ${fragment}`,
        );
      }
    }
  }

  if (errors.length > 0) {
    throw new Error(errors.join("\n"));
  }
}
