export const NIMBUS_CONTROL_PLANE_ROUTES = {
  "services.get": {
    verb: "GET",
    path: "/api/tenants/{tenant_id}/services/{service_name}",
  },
  "services.create": {
    verb: "POST",
    path: "/api/tenants/{tenant_id}/services",
  },
  "services.update": {
    verb: "PUT",
    path: "/api/tenants/{tenant_id}/services/{service_name}",
  },
  "services.delete": {
    verb: "DELETE",
    path: "/api/tenants/{tenant_id}/services/{service_name}",
  },
  "services.list": {
    verb: "GET",
    path: "/api/tenants/{tenant_id}/services",
  },
  "services.start": {
    verb: "POST",
    path: "/api/tenants/{tenant_id}/services/{service_name}/start",
  },
  "services.stop": {
    verb: "POST",
    path: "/api/tenants/{tenant_id}/services/{service_name}/stop",
  },
  "sandboxes.create": {
    verb: "POST",
    path: "/api/tenants/{tenant_id}/sandboxes",
  },
  "sandboxes.get": {
    verb: "GET",
    path: "/api/tenants/{tenant_id}/sandboxes/{sandbox_id}",
  },
  "sandboxes.list": {
    verb: "GET",
    path: "/api/tenants/{tenant_id}/sandboxes",
  },
  "sandboxes.stop": {
    verb: "POST",
    path: "/api/tenants/{tenant_id}/sandboxes/{sandbox_id}/stop",
  },
  "sessions.open": {
    verb: "POST",
    path: "/api/sessions",
  },
  "sessions.get": {
    verb: "GET",
    path: "/api/sessions/{session_id}",
  },
  "sessions.list": {
    verb: "GET",
    path: "/api/sessions",
  },
  "sessions.close": {
    verb: "POST",
    path: "/api/sessions/{session_id}/close",
  },
} as const;

export type NimbusControlPlaneRouteName =
  keyof typeof NIMBUS_CONTROL_PLANE_ROUTES;
export type NimbusControlPlaneRouteParams = Record<string, string | undefined>;

const PARAMETER_LABELS: Record<string, string> = {
  tenant_id: "tenant",
  service_name: "service",
  sandbox_id: "sandbox",
  session_id: "session",
};

export function controlPlaneRouteVerb(
  route: NimbusControlPlaneRouteName,
): string {
  return NIMBUS_CONTROL_PLANE_ROUTES[route].verb;
}

export function controlPlaneRoutePath(
  route: NimbusControlPlaneRouteName,
  params: NimbusControlPlaneRouteParams = {},
  query?: URLSearchParams,
): string {
  const template = NIMBUS_CONTROL_PLANE_ROUTES[route].path;
  const path = template.replace(/\{([a-z_]+)\}/g, (_, name: string) =>
    encodeControlPlaneRouteParameter(template, name, params[name]),
  );
  const suffix = query?.toString();
  return suffix ? `${path}?${suffix}` : path;
}

function encodeControlPlaneRouteParameter(
  template: string,
  name: string,
  value: string | undefined,
): string {
  if (value === undefined) {
    throw new Error(`missing path parameter \`${name}\` for ${template}`);
  }
  const trimmed = value.trim();
  const label = PARAMETER_LABELS[name] ?? name;
  if (!trimmed) throw new Error(`Nimbus ${label} name must not be empty`);
  return encodeURIComponent(trimmed);
}
