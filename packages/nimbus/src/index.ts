import {
  NimbusRestClient,
  type FetchLike,
  type RequestOptions,
} from "./transports/rest.ts";

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

type ProcessLike = {
  env?: Record<string, string | undefined>;
};

type LocalCredentialFile = {
  endpoint?: unknown;
  token?: unknown;
  access_token?: unknown;
  credential?: {
    kind?: unknown;
    type?: unknown;
    token?: unknown;
    access_token?: unknown;
    issuer?: unknown;
    audience?: unknown;
    subject?: unknown;
  };
};

type FsPromises = {
  readFile(path: string, encoding: "utf8"): Promise<string>;
};

const dynamicImport = new Function(
  "specifier",
  "return import(specifier)",
) as (specifier: string) => Promise<unknown>;

type ControlPlaneRequest = <T = unknown>(
  path: string,
  options?: RequestOptions,
) => Promise<T>;

export class Nimbus {
  readonly services: NimbusServices;
  readonly sandboxes: NimbusSandboxes;
  readonly sessions: NimbusSessions;
  readonly tenantId?: string;

  readonly #options: NimbusClientOptions;
  #clientPromise: Promise<NimbusRestClient> | null = null;

  constructor(options: NimbusClientOptions = {}) {
    this.#options = { ...options };
    this.tenantId = stringOrUndefined(options.tenantId);
    this.services = new NimbusServices(
      this,
      this.#controlPlaneRequest.bind(this),
    );
    this.sandboxes = new NimbusSandboxes(
      this,
      this.#controlPlaneRequest.bind(this),
    );
    this.sessions = new NimbusSessions(
      this,
      this.#controlPlaneRequest.bind(this),
    );
  }

  static async defaultClient(options: NimbusClientOptions = {}): Promise<Nimbus> {
    const client = new Nimbus(options);
    await client.#resolveRestClient();
    return client;
  }

  async #controlPlaneRequest<T = unknown>(
    path: string,
    options: RequestOptions = {},
  ): Promise<T> {
    const client = await this.#resolveRestClient();
    return client.request<T>(path, options);
  }

  async #resolveRestClient(): Promise<NimbusRestClient> {
    this.#clientPromise ??= createDefaultRestClient(this.#options);
    return this.#clientPromise;
  }
}

class NimbusServices {
  constructor(
    private readonly client: Nimbus,
    private readonly sendControlPlaneRequest: ControlPlaneRequest,
  ) {}

  async start(input: NimbusServiceStartRequest): Promise<NimbusService> {
    assertLifecycleWaitUntil("start", input.waitUntil, ["ready", "healthy"]);
    const service = await this.lifecycle("start", input);
    if (!input.waitUntil) return service;
    return this.wait({
      name: input.name,
      tenantId: input.tenantId,
      until: input.waitUntil,
    });
  }

  async stop(input: NimbusServiceStopRequest): Promise<NimbusService> {
    assertLifecycleWaitUntil("stop", input.waitUntil, ["stopped"]);
    const service = await this.lifecycle("stop", input);
    if (!input.waitUntil) return service;
    return this.wait({
      name: input.name,
      tenantId: input.tenantId,
      until: input.waitUntil,
    });
  }

  async restart(input: NimbusServiceStartRequest): Promise<NimbusService> {
    assertLifecycleWaitUntil("restart", input.waitUntil, ["ready", "healthy"]);
    const service = await this.lifecycle("restart", input);
    if (!input.waitUntil) return service;
    return this.wait({
      name: input.name,
      tenantId: input.tenantId,
      until: input.waitUntil,
    });
  }

  get(selector: NimbusServiceSelector): Promise<NimbusService> {
    return this.sendControlPlaneRequest(serviceResourcePath(this.client, selector), {
      method: "GET",
    });
  }

  create(input: NimbusServiceCreateRequest): Promise<NimbusServiceDefinition> {
    return this.sendControlPlaneRequest(serviceCollectionPath(this.client, input), {
      method: "POST",
      body: JSON.stringify(serviceDefinitionRequestBody(input, input.name)),
    });
  }

  update(input: NimbusServiceUpdateRequest): Promise<NimbusServiceDefinition> {
    return this.sendControlPlaneRequest(serviceResourcePath(this.client, input), {
      method: "PUT",
      body: JSON.stringify(serviceDefinitionRequestBody(input, input.name, input.ifMatchGeneration)),
    });
  }

  delete(input: NimbusServiceDeleteRequest): Promise<void> {
    const query = new URLSearchParams({
      ifMatchGeneration: String(input.ifMatchGeneration),
    });
    if (input.force !== undefined) {
      query.set("force", String(input.force));
    }
    return this.sendControlPlaneRequest(
      `${serviceResourcePath(this.client, input)}?${query.toString()}`,
      { method: "DELETE" },
    );
  }

  list(input: NimbusServiceListRequest = {}): Promise<NimbusServiceDefinitionCollection> {
    const query = new URLSearchParams();
    if (input.limit !== undefined) query.set("limit", String(input.limit));
    if (input.pageToken !== undefined) query.set("pageToken", input.pageToken);
    const suffix = query.size > 0 ? `?${query.toString()}` : "";
    return this.sendControlPlaneRequest(
      `${serviceCollectionPath(this.client, input)}${suffix}`,
      { method: "GET" },
    );
  }

  async wait(input: NimbusServiceWaitRequest): Promise<NimbusService> {
    const timeoutMs = positiveFiniteNumber(input.timeoutMs, 30_000, "timeoutMs");
    const intervalMs = positiveFiniteNumber(input.intervalMs, 250, "intervalMs");
    const deadline = Date.now() + timeoutMs;
    let latest: NimbusService | null = null;

    while (Date.now() <= deadline) {
      latest = await this.get(input);
      if (serviceMatchesCondition(latest, input.until)) {
        return latest;
      }
      await sleep(intervalMs);
    }

    const observed = latest
      ? normalizeStatusString(latest.readiness)
        ?? normalizeStatusString(latest.health)
        ?? normalizeStatusString(latest.lifecycleState)
        ?? normalizeStatusString(latest.state)
        ?? "unknown"
      : "unknown";
    throw new Error(
      `Nimbus service ${input.name} did not reach ${input.until} within ${timeoutMs}ms; last observed status was ${observed}.`,
    );
  }

  private lifecycle(
    verb: "start" | "stop" | "restart",
    input: NimbusServiceLifecycleRequest,
  ): Promise<NimbusService> {
    return this.sendControlPlaneRequest(serviceResourcePath(this.client, input, `/${verb}`), {
      method: "POST",
    });
  }
}

class NimbusSandboxes {
  constructor(
    private readonly client: Nimbus,
    private readonly sendControlPlaneRequest: ControlPlaneRequest,
  ) {}

  create(input: NimbusSandboxCreateRequest): Promise<NimbusSandboxResource> {
    return this.sendControlPlaneRequest(sandboxCollectionPath(this.client, input), {
      method: "POST",
      body: JSON.stringify({
        profile: input.profile,
        spec: input.spec,
        labels: input.labels ?? {},
      }),
    });
  }

  get(input: NimbusSandboxSelector): Promise<NimbusSandboxResource> {
    return this.sendControlPlaneRequest(sandboxResourcePath(this.client, input), {
      method: "GET",
    });
  }

  list(input: NimbusSandboxListRequest = {}): Promise<NimbusSandboxCollection> {
    const query = new URLSearchParams();
    if (input.limit !== undefined) query.set("limit", String(input.limit));
    if (input.pageToken !== undefined) query.set("pageToken", input.pageToken);
    if (input.status !== undefined) query.set("status", input.status);
    if (input.labelKey !== undefined) query.set("labelKey", input.labelKey);
    if (input.labelValue !== undefined) query.set("labelValue", input.labelValue);
    const encoded = query.toString();
    const suffix = encoded ? `?${encoded}` : "";
    return this.sendControlPlaneRequest(
      `${sandboxCollectionPath(this.client, input)}${suffix}`,
      { method: "GET" },
    );
  }

  stop(input: NimbusSandboxSelector): Promise<NimbusSandboxResource> {
    return this.sendControlPlaneRequest(`${sandboxResourcePath(this.client, input)}/stop`, {
      method: "POST",
    });
  }
}

class NimbusSessions {
  constructor(
    private readonly client: Nimbus,
    private readonly sendControlPlaneRequest: ControlPlaneRequest,
  ) {}

  open(input: NimbusSessionOpenRequest): Promise<NimbusSessionResource> {
    const tenantId = resolveTenantId(this.client, input, "session open request");
    return this.sendControlPlaneRequest(sessionCollectionPath(), {
      method: "POST",
      body: JSON.stringify({
        tenantId,
        target: input.target,
        channels: input.channels,
        ...(input.requestedTtlMs === undefined ? {} : { requestedTtlMs: input.requestedTtlMs }),
      }),
    });
  }

  get(input: NimbusSessionSelector): Promise<NimbusSessionResource> {
    return this.sendControlPlaneRequest(sessionResourcePath(input), {
      method: "GET",
    });
  }

  list(input: NimbusSessionListRequest = {}): Promise<NimbusSessionCollection> {
    const tenantId = resolveTenantId(this.client, input, "session list request");
    const query = new URLSearchParams({ tenantId });
    if (input.limit !== undefined) query.set("limit", String(input.limit));
    if (input.pageToken !== undefined) query.set("pageToken", input.pageToken);
    if (input.state !== undefined) query.set("state", input.state);
    return this.sendControlPlaneRequest(
      `${sessionCollectionPath()}?${query.toString()}`,
      { method: "GET" },
    );
  }

  close(input: NimbusSessionCloseRequest): Promise<NimbusSessionResource> {
    return this.sendControlPlaneRequest(`${sessionResourcePath(input)}/close`, {
      method: "POST",
      body: JSON.stringify({
        ...(input.reason === undefined ? {} : { reason: input.reason }),
      }),
    });
  }
}

async function createDefaultRestClient(options: NimbusClientOptions): Promise<NimbusRestClient> {
  const env = getEnv();
  const explicitCredential = normalizeExplicitCredential(options);
  const envCredential = normalizeEnvCredential(env);
  const explicitEndpoint = stringOrUndefined(options.endpoint);
  const envEndpoint = stringOrUndefined(env.NIMBUS_ENDPOINT);
  const localCredentials =
    (!explicitEndpoint && !envEndpoint) || (!explicitCredential && !envCredential)
      ? await readLocalCredentialFile(env)
      : null;
  const endpoint = explicitEndpoint
    ?? envEndpoint
    ?? stringOrUndefined(localCredentials?.endpoint);
  if (!endpoint) {
    throw new Error(
      "Nimbus endpoint discovery failed. Set new Nimbus({ endpoint }), NIMBUS_ENDPOINT, or endpoint in ~/.config/nimbus/application_default_credentials.json.",
    );
  }

  const credential =
    explicitCredential
    ?? envCredential
    ?? normalizeLocalCredential(localCredentials)
    ?? await resolveWorkloadIdentityCredential(env);
  if (!credential) {
    throw new Error(
      "Nimbus credential discovery failed. Set new Nimbus({ token }), NIMBUS_TOKEN, a local Nimbus application_default_credentials.json file, or a workload identity token.",
    );
  }

  return new NimbusRestClient(endpoint, {
    fetch: options.fetch,
    headers: {
      ...headersForCredential(credential),
      ...(options.headers ?? {}),
    },
  });
}

function normalizeExplicitCredential(options: NimbusClientOptions): NimbusCredential | null {
  if (options.credential) return options.credential;
  if (options.token) return { kind: "bearer", token: options.token };
  return null;
}

function normalizeEnvCredential(env: Record<string, string | undefined>): NimbusCredential | null {
  const token = stringOrUndefined(env.NIMBUS_TOKEN)
    ?? stringOrUndefined(env.NIMBUS_BEARER_TOKEN);
  if (token) return { kind: "bearer", token };

  const workloadToken = stringOrUndefined(env.NIMBUS_WORKLOAD_IDENTITY_TOKEN);
  if (workloadToken) {
    return {
      kind: "workload_identity",
      token: workloadToken,
      issuer: stringOrUndefined(env.NIMBUS_WORKLOAD_IDENTITY_ISSUER),
      audience: stringOrUndefined(env.NIMBUS_WORKLOAD_IDENTITY_AUDIENCE),
      subject: stringOrUndefined(env.NIMBUS_WORKLOAD_IDENTITY_SUBJECT),
    };
  }

  return null;
}

function normalizeLocalCredential(file: LocalCredentialFile | null): NimbusCredential | null {
  if (!file) return null;

  const nested = file.credential;
  if (nested) {
    const kind = stringOrUndefined(nested.kind) ?? stringOrUndefined(nested.type);
    const token = stringOrUndefined(nested.token) ?? stringOrUndefined(nested.access_token);
    if (kind === "workload_identity" && token) {
      return {
        kind: "workload_identity",
        token,
        issuer: stringOrUndefined(nested.issuer),
        audience: stringOrUndefined(nested.audience),
        subject: stringOrUndefined(nested.subject),
      };
    }
    if ((kind === "bearer" || kind === "access_token") && token) {
      return { kind: "bearer", token };
    }
  }

  const token = stringOrUndefined(file.token) ?? stringOrUndefined(file.access_token);
  if (token) return { kind: "bearer", token };

  return null;
}

async function resolveWorkloadIdentityCredential(
  env: Record<string, string | undefined>,
): Promise<NimbusCredential | null> {
  const tokenFile = stringOrUndefined(env.NIMBUS_WORKLOAD_IDENTITY_TOKEN_FILE);
  if (!tokenFile) return null;

  const token = (await readTextFileIfExists(tokenFile))?.trim();
  if (!token) return null;

  return {
    kind: "workload_identity",
    token,
    issuer: stringOrUndefined(env.NIMBUS_WORKLOAD_IDENTITY_ISSUER),
    audience: stringOrUndefined(env.NIMBUS_WORKLOAD_IDENTITY_AUDIENCE),
    subject: stringOrUndefined(env.NIMBUS_WORKLOAD_IDENTITY_SUBJECT),
  };
}

async function readLocalCredentialFile(
  env: Record<string, string | undefined>,
): Promise<LocalCredentialFile | null> {
  const explicitPath = stringOrUndefined(env.NIMBUS_APPLICATION_CREDENTIALS);
  const defaultPath = defaultCredentialFilePath(env);
  const raw = await readTextFileIfExists(explicitPath ?? defaultPath);
  if (!raw) return null;
  try {
    return JSON.parse(raw) as LocalCredentialFile;
  } catch (error) {
    throw new Error(
      `Nimbus credential discovery failed: ${(explicitPath ?? defaultPath) || "credential file"} is not valid JSON: ${(error as Error).message}`,
    );
  }
}

async function readTextFileIfExists(filePath: string | undefined): Promise<string | null> {
  if (!filePath) return null;
  const fs = await dynamicImport("node:fs/promises") as FsPromises;
  try {
    return await fs.readFile(filePath, "utf8");
  } catch (error) {
    if (
      error
      && typeof error === "object"
      && "code" in error
      && error.code === "ENOENT"
    ) {
      return null;
    }
    throw error;
  }
}

function defaultCredentialFilePath(env: Record<string, string | undefined>): string | undefined {
  const configHome = stringOrUndefined(env.XDG_CONFIG_HOME);
  if (configHome) return `${stripTrailingSlash(configHome)}/nimbus/application_default_credentials.json`;

  const home = stringOrUndefined(env.HOME) ?? stringOrUndefined(env.USERPROFILE);
  return home ? `${stripTrailingSlash(home)}/.config/nimbus/application_default_credentials.json` : undefined;
}

function headersForCredential(credential: NimbusCredential): Record<string, string> {
  switch (credential.kind) {
    case "bearer":
    case "workload_identity":
      return { Authorization: `Bearer ${credential.token}` };
  }
}

function getEnv(): Record<string, string | undefined> {
  return ((globalThis as typeof globalThis & { process?: ProcessLike }).process?.env ?? {});
}

function clientTenantId(client: Nimbus): string | undefined {
  return client.tenantId;
}

function serviceResourcePath(
  client: Nimbus,
  selector: NimbusServiceSelector,
  suffix = "",
): string {
  const tenantId = resolveTenantId(client, selector, "service request");
  const tenant = encodeResourceName(tenantId, "tenant");
  const name = encodeResourceName(selector.name, "service");
  return `/api/tenants/${tenant}/services/${name}${suffix}`;
}

function serviceCollectionPath(
  client: Nimbus,
  input: NimbusServiceListRequest,
): string {
  const tenantId = resolveTenantId(client, input, "service collection request");
  const tenant = encodeResourceName(tenantId, "tenant");
  return `/api/tenants/${tenant}/services`;
}

function sandboxCollectionPath(
  client: Nimbus,
  input: { tenantId?: string },
): string {
  const tenantId = resolveTenantId(client, input, "sandbox collection request");
  const tenant = encodeResourceName(tenantId, "tenant");
  return `/api/tenants/${tenant}/sandboxes`;
}

function sandboxResourcePath(
  client: Nimbus,
  selector: NimbusSandboxSelector,
): string {
  const tenantId = resolveTenantId(client, selector, "sandbox request");
  const tenant = encodeResourceName(tenantId, "tenant");
  const id = encodeResourceName(selector.id, "sandbox");
  return `/api/tenants/${tenant}/sandboxes/${id}`;
}

function sessionCollectionPath(): string {
  return "/api/sessions";
}

function sessionResourcePath(selector: NimbusSessionSelector): string {
  const id = encodeResourceName(selector.id, "session");
  return `/api/sessions/${id}`;
}

function resolveTenantId(
  client: Nimbus,
  selector: { tenantId?: string },
  context: string,
): string {
  const tenantId = stringOrUndefined(selector.tenantId) ?? clientTenantId(client);
  if (!tenantId) {
    throw new Error(
      `Nimbus ${context} requires a tenantId. Pass tenantId in the service call or construct the client with new Nimbus({ tenantId }).`,
    );
  }
  return tenantId;
}

function serviceDefinitionRequestBody(
  input: NimbusServiceCreateRequest | NimbusServiceUpdateRequest,
  name: string,
  generation?: number,
): unknown {
  return {
    metadata: {
      tenantId: stringOrUndefined(input.tenantId),
      name,
      ...(generation === undefined ? {} : { generation }),
      labels: input.labels ?? {},
    },
    spec: {
      backend: input.backend,
    },
  };
}

function encodeResourceName(value: string, label: string): string {
  const trimmed = value.trim();
  if (!trimmed) throw new Error(`Nimbus ${label} name must not be empty`);
  return encodeURIComponent(trimmed);
}

function positiveFiniteNumber(value: unknown, fallback: number, label: string): number {
  if (value === undefined) return fallback;
  if (typeof value === "number" && Number.isFinite(value) && value > 0) {
    return value;
  }
  throw new Error(`Nimbus service wait ${label} must be a positive finite number.`);
}

function serviceMatchesCondition(
  service: NimbusService,
  condition: NimbusServiceWaitCondition,
): boolean {
  const state = normalizeStatusString(service.lifecycleState)
    ?? normalizeStatusString(service.state)
    ?? normalizeStatusString(service.status);
  const readiness = normalizeStatusString(service.readiness);
  const health = normalizeStatusString(service.health);

  switch (condition) {
    case "ready":
      return readiness === "ready" || state === "ready";
    case "stopped":
      return state === "stopped" || readiness === "stopped";
    case "healthy":
      return health === "healthy";
  }
}

function assertLifecycleWaitUntil(
  verb: "start" | "stop" | "restart",
  waitUntil: NimbusServiceWaitCondition | undefined,
  allowed: readonly NimbusServiceWaitCondition[],
): void {
  if (!waitUntil || allowed.includes(waitUntil)) return;
  throw new Error(
    `Nimbus services.${verb}({ waitUntil }) only supports ${formatAllowedValues(allowed)}; received ${waitUntil}.`,
  );
}

function formatAllowedValues(values: readonly string[]): string {
  if (values.length === 1) return values[0];
  return values.slice(0, -1).join(", ") + `, or ${values[values.length - 1]}`;
}

function normalizeStatusString(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value.toLowerCase() : undefined;
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function stringOrUndefined(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function stripTrailingSlash(value: string): string {
  return value.endsWith("/") ? value.slice(0, -1) : value;
}
