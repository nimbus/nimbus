import type { FetchLike } from "../transports/rest.ts";

export type NimbusCredential =
  | { kind: "bearer"; token: string }
  | {
      kind: "workload_identity";
      token: string;
      issuer?: string;
      audience?: string;
      subject?: string;
    };

export interface NimbusClientOptions {
  endpoint?: string;
  tenantId?: string;
  token?: string;
  credential?: NimbusCredential;
  fetch?: FetchLike;
  headers?: Record<string, string>;
}

export interface NimbusServiceSelector {
  name: string;
  tenantId?: string;
}

export type NimbusServiceWaitCondition = "ready" | "stopped" | "healthy";

export type NimbusServiceActivationWaitCondition = Extract<
  NimbusServiceWaitCondition,
  "ready" | "healthy"
>;

export type NimbusServiceStopWaitCondition = Extract<
  NimbusServiceWaitCondition,
  "stopped"
>;

export interface NimbusServiceStartRequest extends NimbusServiceSelector {
  waitUntil?: NimbusServiceActivationWaitCondition;
}

export interface NimbusServiceStopRequest extends NimbusServiceSelector {
  waitUntil?: NimbusServiceStopWaitCondition;
}

export type NimbusServiceLifecycleRequest =
  | NimbusServiceStartRequest
  | NimbusServiceStopRequest;

export interface NimbusServiceWaitRequest extends NimbusServiceSelector {
  until: NimbusServiceWaitCondition;
  timeoutMs?: number;
  intervalMs?: number;
}

export type NimbusBuiltInProviderId =
  | "loadBalancer"
  | "serviceDiscovery"
  | "browser"
  | "modelGateway";

export type NimbusExternalAuthPolicy = { kind: "none" };

export type NimbusHealthCheckPolicy = { kind: "http"; path: string };

export type NimbusExternalEndpointPolicy = { url: string };

export type NimbusSandboxOwnerSpec =
  | { kind: "service"; serviceName?: string }
  | { kind: "standalone"; displayName?: string };

export type NimbusSandboxBackendKind = "krun" | "container";

export type NimbusSandboxRootSpec = {
  kind: "oci_image";
  source: NimbusSandboxOciImageReferenceSource;
};

export type NimbusSandboxOciImageReferenceSource = {
  kind: "reference";
  reference: string;
};

export interface NimbusSandboxProcessSpec {
  argv?: string[];
  args?: string[];
  entrypoint?: string[];
  command?: string[];
  env?: string[];
  cwd?: string;
  user?: string;
  terminal?: boolean;
}

export interface NimbusRedactedValues {
  redacted: true;
  valueCount: number;
}

export interface NimbusSandboxSpec {
  tenantId?: string;
  owner: NimbusSandboxOwnerSpec;
  backend: NimbusSandboxBackendKind;
  root: NimbusSandboxRootSpec;
  process: NimbusSandboxProcessSpec;
}

export type NimbusSandboxRootResponse =
  | NimbusSandboxRootSpec
  | {
      kind: "redacted";
      redacted: true;
      reason: "operatorOnlyLaunchInput";
    };

export interface NimbusSandboxProcessResponse {
  argv: NimbusRedactedValues;
  entrypoint?: NimbusRedactedValues;
  command?: NimbusRedactedValues;
  environment: NimbusRedactedValues;
  cwd: string;
  user?: string;
  terminal: boolean;
}

export interface NimbusSandboxSpecResponse {
  tenantId: string;
  owner: NimbusSandboxOwnerSpec;
  backend: NimbusSandboxBackendKind;
  root: NimbusSandboxRootResponse;
  process: NimbusSandboxProcessResponse;
}

export type NimbusServiceBackendSpec =
  | { kind: "sandbox"; sandbox: NimbusSandboxSpec }
  | { kind: "builtIn"; provider: NimbusBuiltInProviderId }
  | {
      kind: "external";
      endpoint: NimbusExternalEndpointPolicy;
      auth: NimbusExternalAuthPolicy;
      health: NimbusHealthCheckPolicy;
    };

export type NimbusServiceBackendResponse =
  | { kind: "sandbox"; sandbox: NimbusSandboxSpecResponse }
  | { kind: "builtIn"; provider: NimbusBuiltInProviderId }
  | {
      kind: "external";
      endpoint: NimbusExternalEndpointPolicy;
      auth: NimbusExternalAuthPolicy;
      health: NimbusHealthCheckPolicy;
    }
  | {
      kind: "redacted";
      backend: "sandbox" | "builtIn" | "external";
      redacted: true;
      reason: "requiresInspectPermission";
    };

export interface NimbusServiceCreateRequest extends NimbusServiceSelector {
  backend: NimbusServiceBackendSpec;
  labels?: Record<string, string>;
}

export interface NimbusServiceUpdateRequest extends NimbusServiceSelector {
  backend: NimbusServiceBackendSpec;
  ifMatchGeneration: number;
  labels?: Record<string, string>;
}

export interface NimbusServiceDeleteRequest extends NimbusServiceSelector {
  ifMatchGeneration: number;
  force?: boolean;
}

export interface NimbusServiceListRequest {
  tenantId?: string;
  limit?: number;
  pageToken?: string;
}

export interface NimbusServiceDefinition {
  metadata: {
    tenantId: string;
    name: string;
    generation: number;
    resourceVersion: string;
    createdAt: string;
    updatedAt: string;
    labels: Record<string, string>;
    source: "staticCatalog" | "dynamic";
  };
  spec: {
    backend: NimbusServiceBackendResponse;
  };
  status: {
    backend: "sandbox" | "builtIn" | "external";
    lifecycleState: string;
    readiness: string;
    health: string;
    conditions: NimbusCondition[];
  };
}

export interface NimbusCondition {
  type: string;
  status: "True" | "False" | "Unknown";
  reason: string;
  message: string;
  observedGeneration: number;
  lastTransitionTime: string;
}

export interface NimbusServiceDefinitionCollection {
  metadata: {
    tenantId: string;
    resourceVersion: string;
    limit: number;
    nextPageToken?: string;
    remainingCount: number;
  };
  items: NimbusServiceDefinition[];
}

export type NimbusSandboxProfile = "worker" | "desktop";

export interface NimbusSandboxCreateRequest {
  tenantId?: string;
  profile: NimbusSandboxProfile;
  spec: NimbusSandboxSpec;
  labels?: Record<string, string>;
}

export interface NimbusSandboxSelector {
  tenantId?: string;
  id: string;
}

export interface NimbusSandboxListRequest {
  tenantId?: string;
  limit?: number;
  pageToken?: string;
  status?: string;
  labelKey?: string;
  labelValue?: string;
}

export interface NimbusSandboxResource {
  metadata: {
    tenantId: string;
    id: string;
    generation: number;
    resourceVersion: string;
    createdAt: string;
    updatedAt: string;
    labels: Record<string, string>;
  };
  spec: {
    profile: NimbusSandboxProfile;
    sandbox: NimbusSandboxSpecResponse;
  };
  status: {
    lifecycleState: string;
    readiness: string;
    health: string;
    backend: string;
    endpoints: NimbusServiceEndpoint[];
    conditions: NimbusCondition[];
  };
}

export interface NimbusSandboxCollection {
  metadata: {
    tenantId: string;
    resourceVersion: string;
    limit: number;
    nextPageToken?: string;
    remainingCount: number;
  };
  items: NimbusSandboxResource[];
}

export type NimbusSessionChannel = "cdp" | "page" | "stdio" | "files";

export type NimbusSessionTarget =
  | { service: { name: string } }
  | { sandbox: { id: string } };

export interface NimbusSessionOpenRequest {
  tenantId?: string;
  target: NimbusSessionTarget;
  channels: NimbusSessionChannel[];
  requestedTtlMs?: number;
}

export interface NimbusSessionSelector {
  id: string;
}

export interface NimbusSessionListRequest {
  tenantId?: string;
  limit?: number;
  pageToken?: string;
  state?: "open" | "closed" | "expired";
}

export interface NimbusSessionCloseRequest extends NimbusSessionSelector {
  reason?: string;
}

export interface NimbusSessionResource {
  metadata: {
    tenantId: string;
    id: string;
    generation: number;
    resourceVersion: string;
    createdAt: string;
    updatedAt: string;
  };
  spec: {
    target: NimbusSessionTarget;
    targetSnapshot:
      | {
          service: {
            name: string;
            generation: number;
            backend: "sandbox" | "builtIn" | "external";
            provider?: NimbusBuiltInProviderId;
          };
        }
      | {
          sandbox: {
            id: string;
            generation: number;
            profile: NimbusSandboxProfile;
            backend: string;
          };
        };
    channels: NimbusSessionChannel[];
    expiresAt: string;
  };
  status: {
    lifecycleState: "open" | "closed" | "expired";
    expiresAt: string;
    closedAt?: string;
    closeReason?: string;
    conditions: NimbusCondition[];
  };
}

export interface NimbusSessionCollection {
  metadata: {
    tenantId: string;
    resourceVersion: string;
    limit: number;
    nextPageToken?: string;
    remainingCount: number;
  };
  items: NimbusSessionResource[];
}

export interface NimbusServiceEndpoint {
  name: string;
  protocol: string;
  host: string;
  port: number;
}

export interface NimbusService {
  name: string;
  tenantId?: string;
  state?: string;
  lifecycleState?: string;
  readiness?: string;
  health?: string;
  sandboxId?: string;
  backend?: string;
  endpoints?: NimbusServiceEndpoint[];
  status?: string;
  [key: string]: unknown;
}
