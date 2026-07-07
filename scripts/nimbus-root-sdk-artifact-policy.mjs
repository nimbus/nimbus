export const NIMBUS_ROOT_SDK_ARTIFACT_PATHS = [
  "packages/nimbus/dist/control-plane/client.js",
  "packages/nimbus/dist/control-plane/client.d.ts",
  "crates/nimbus-assets/embedded/packages/@nimbus/nimbus/control-plane/client.js",
  "crates/nimbus-assets/embedded/packages/@nimbus/nimbus/control-plane/client.d.ts",
];

export const NIMBUS_ROOT_SDK_ROUTE_ARTIFACT_PATHS = [
  "packages/nimbus/dist/control_plane_routes.js",
  "packages/nimbus/dist/control_plane_routes.d.ts",
  "crates/nimbus-assets/embedded/packages/@nimbus/nimbus/control_plane_routes.js",
  "crates/nimbus-assets/embedded/packages/@nimbus/nimbus/control_plane_routes.d.ts",
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
  "#controlPlaneRequest",
  "#resolveRestClient",
];

export const NIMBUS_ROOT_SDK_CONTROL_PLANE_ROUTE_FRAGMENTS = [
  "NIMBUS_CONTROL_PLANE_ROUTES",
  "services.get",
  "services.create",
  "services.update",
  "services.delete",
  "services.list",
  "services.start",
  "services.stop",
  "services.restart",
  "sandboxes.create",
  "sandboxes.get",
  "sandboxes.list",
  "sandboxes.stop",
  "sessions.open",
  "sessions.get",
  "sessions.list",
  "sessions.close",
  "/api/tenants/{tenant_id}/services/{service_name}",
  "/api/tenants/{tenant_id}/services",
  "/api/tenants/{tenant_id}/sandboxes/{sandbox_id}",
  "/api/tenants/{tenant_id}/sandboxes",
  "/api/sessions/{session_id}/close",
  "/api/sessions",
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

export function assertNimbusRootSdkRouteArtifactText(artifactPath, artifact) {
  const errors = [];
  for (const fragment of NIMBUS_ROOT_SDK_CONTROL_PLANE_ROUTE_FRAGMENTS) {
    if (!artifact.includes(fragment)) {
      errors.push(
        `${artifactPath} is missing canonical control-plane route fragment: ${fragment}`,
      );
    }
  }
  for (const fragment of NIMBUS_ROOT_SDK_FORBIDDEN_FRAGMENTS) {
    if (artifact.includes(fragment)) {
      errors.push(
        `${artifactPath} contains stale or public root SDK fragment: ${fragment}`,
      );
    }
  }
  if (errors.length > 0) {
    throw new Error(errors.join("\n"));
  }
}
