import { NimbusRestClient, type RequestOptions } from "../transports/rest.ts";
import {
  controlPlaneRoutePath,
  controlPlaneRouteVerb,
  type NimbusControlPlaneRouteName,
  type NimbusControlPlaneRouteParams,
} from "../control_plane_routes.ts";
import { createDefaultRestClient, stringOrUndefined } from "./discovery.ts";
import type {
  NimbusClientOptions,
  NimbusSandboxCollection,
  NimbusSandboxCreateRequest,
  NimbusSandboxListRequest,
  NimbusSandboxResource,
  NimbusSandboxSelector,
  NimbusService,
  NimbusServiceCreateRequest,
  NimbusServiceDefinition,
  NimbusServiceDefinitionCollection,
  NimbusServiceDeleteRequest,
  NimbusServiceLifecycleRequest,
  NimbusServiceListRequest,
  NimbusServiceRestartRequest,
  NimbusServiceRestartSubmission,
  NimbusServiceSelector,
  NimbusServiceStartRequest,
  NimbusServiceStopRequest,
  NimbusServiceUpdateRequest,
  NimbusServiceWaitCondition,
  NimbusServiceWaitRequest,
  NimbusSessionCloseRequest,
  NimbusSessionCollection,
  NimbusSessionListRequest,
  NimbusSessionOpenRequest,
  NimbusSessionResource,
  NimbusSessionSelector,
} from "./types.ts";

type ControlPlaneRouteRequestOptions = {
  params?: NimbusControlPlaneRouteParams;
  query?: URLSearchParams;
  body?: unknown;
};

type ControlPlaneRouteRequest = <T = unknown>(
  route: NimbusControlPlaneRouteName,
  options?: ControlPlaneRouteRequestOptions,
) => Promise<T>;

export interface NimbusServiceWaitTiming {
  monotonicNow(): number;
  sleep(ms: number): Promise<void>;
}

const systemServiceWaitTiming: NimbusServiceWaitTiming = {
  monotonicNow: () => globalThis.performance.now(),
  sleep,
};

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
      this.#controlPlaneRouteRequest.bind(this),
    );
    this.sandboxes = new NimbusSandboxes(
      this,
      this.#controlPlaneRouteRequest.bind(this),
    );
    this.sessions = new NimbusSessions(
      this,
      this.#controlPlaneRouteRequest.bind(this),
    );
  }

  static async defaultClient(
    options: NimbusClientOptions = {},
  ): Promise<Nimbus> {
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

  async #controlPlaneRouteRequest<T = unknown>(
    route: NimbusControlPlaneRouteName,
    options: ControlPlaneRouteRequestOptions = {},
  ): Promise<T> {
    const requestOptions: RequestOptions = {
      method: controlPlaneRouteVerb(route),
    };
    if (options.body !== undefined) {
      requestOptions.body = JSON.stringify(options.body);
    }
    return this.#controlPlaneRequest<T>(
      controlPlaneRoutePath(route, options.params, options.query),
      requestOptions,
    );
  }

  async #resolveRestClient(): Promise<NimbusRestClient> {
    this.#clientPromise ??= createDefaultRestClient(this.#options);
    return this.#clientPromise;
  }
}

export class NimbusServices {
  constructor(
    private readonly client: Nimbus,
    private readonly sendControlPlaneRequest: ControlPlaneRouteRequest,
    private readonly waitTiming: NimbusServiceWaitTiming = systemServiceWaitTiming,
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

  restart(
    input: NimbusServiceRestartRequest,
  ): Promise<NimbusServiceRestartSubmission> {
    return this.sendControlPlaneRequest("services.restart", {
      params: serviceResourceParams(this.client, input),
      body: {
        sourceGeneration: input.sourceGeneration,
        requestId: input.requestId,
      },
    });
  }

  get(selector: NimbusServiceSelector): Promise<NimbusService> {
    return this.sendControlPlaneRequest("services.get", {
      params: serviceResourceParams(this.client, selector),
    });
  }

  create(input: NimbusServiceCreateRequest): Promise<NimbusServiceDefinition> {
    return this.sendControlPlaneRequest("services.create", {
      params: serviceCollectionParams(this.client, input),
      body: serviceDefinitionRequestBody(input, input.name),
    });
  }

  update(input: NimbusServiceUpdateRequest): Promise<NimbusServiceDefinition> {
    return this.sendControlPlaneRequest("services.update", {
      params: serviceResourceParams(this.client, input),
      body: serviceDefinitionRequestBody(
        input,
        input.name,
        input.ifMatchGeneration,
      ),
    });
  }

  delete(input: NimbusServiceDeleteRequest): Promise<void> {
    const query = new URLSearchParams({
      ifMatchGeneration: String(input.ifMatchGeneration),
    });
    if (input.force !== undefined) {
      query.set("force", String(input.force));
    }
    return this.sendControlPlaneRequest("services.delete", {
      params: serviceResourceParams(this.client, input),
      query,
    });
  }

  list(
    input: NimbusServiceListRequest = {},
  ): Promise<NimbusServiceDefinitionCollection> {
    const query = new URLSearchParams();
    if (input.limit !== undefined) query.set("limit", String(input.limit));
    if (input.pageToken !== undefined) query.set("pageToken", input.pageToken);
    return this.sendControlPlaneRequest("services.list", {
      params: serviceCollectionParams(this.client, input),
      query,
    });
  }

  async wait(input: NimbusServiceWaitRequest): Promise<NimbusService> {
    const timeoutMs = positiveFiniteNumber(
      input.timeoutMs,
      30_000,
      "timeoutMs",
    );
    const intervalMs = positiveFiniteNumber(
      input.intervalMs,
      250,
      "intervalMs",
    );
    const deadline = this.waitTiming.monotonicNow() + timeoutMs;
    let latest: NimbusService | null = null;

    while (this.waitTiming.monotonicNow() <= deadline) {
      latest = await this.get(input);
      if (serviceMatchesCondition(latest, input.until)) {
        return latest;
      }
      await this.waitTiming.sleep(intervalMs);
    }

    const observed = latest
      ? (normalizeStatusString(latest.readiness) ??
        normalizeStatusString(latest.health) ??
        normalizeStatusString(latest.lifecycleState) ??
        normalizeStatusString(latest.state) ??
        "unknown")
      : "unknown";
    throw new Error(
      `Nimbus service ${input.name} did not reach ${input.until} within ${timeoutMs}ms; last observed status was ${observed}.`,
    );
  }

  private lifecycle(
    verb: "start" | "stop",
    input: NimbusServiceLifecycleRequest,
  ): Promise<NimbusService> {
    return this.sendControlPlaneRequest(SERVICE_LIFECYCLE_ROUTES[verb], {
      params: serviceResourceParams(this.client, input),
    });
  }
}

export class NimbusSandboxes {
  constructor(
    private readonly client: Nimbus,
    private readonly sendControlPlaneRequest: ControlPlaneRouteRequest,
  ) {}

  create(input: NimbusSandboxCreateRequest): Promise<NimbusSandboxResource> {
    return this.sendControlPlaneRequest("sandboxes.create", {
      params: sandboxCollectionParams(this.client, input),
      body: {
        profile: input.profile,
        spec: input.spec,
        labels: input.labels ?? {},
      },
    });
  }

  get(input: NimbusSandboxSelector): Promise<NimbusSandboxResource> {
    return this.sendControlPlaneRequest("sandboxes.get", {
      params: sandboxResourceParams(this.client, input),
    });
  }

  list(input: NimbusSandboxListRequest = {}): Promise<NimbusSandboxCollection> {
    const query = new URLSearchParams();
    if (input.limit !== undefined) query.set("limit", String(input.limit));
    if (input.pageToken !== undefined) query.set("pageToken", input.pageToken);
    if (input.status !== undefined) query.set("status", input.status);
    if (input.labelKey !== undefined) query.set("labelKey", input.labelKey);
    if (input.labelValue !== undefined)
      query.set("labelValue", input.labelValue);
    return this.sendControlPlaneRequest("sandboxes.list", {
      params: sandboxCollectionParams(this.client, input),
      query,
    });
  }

  stop(input: NimbusSandboxSelector): Promise<NimbusSandboxResource> {
    return this.sendControlPlaneRequest("sandboxes.stop", {
      params: sandboxResourceParams(this.client, input),
    });
  }
}

export class NimbusSessions {
  constructor(
    private readonly client: Nimbus,
    private readonly sendControlPlaneRequest: ControlPlaneRouteRequest,
  ) {}

  open(input: NimbusSessionOpenRequest): Promise<NimbusSessionResource> {
    const tenantId = resolveTenantId(
      this.client,
      input,
      "session open request",
    );
    return this.sendControlPlaneRequest("sessions.open", {
      body: {
        tenantId,
        target: input.target,
        channels: input.channels,
        ...(input.requestedTtlMs === undefined
          ? {}
          : { requestedTtlMs: input.requestedTtlMs }),
      },
    });
  }

  get(input: NimbusSessionSelector): Promise<NimbusSessionResource> {
    return this.sendControlPlaneRequest("sessions.get", {
      params: sessionResourceParams(input),
    });
  }

  list(input: NimbusSessionListRequest = {}): Promise<NimbusSessionCollection> {
    const tenantId = resolveTenantId(
      this.client,
      input,
      "session list request",
    );
    const query = new URLSearchParams({ tenantId });
    if (input.limit !== undefined) query.set("limit", String(input.limit));
    if (input.pageToken !== undefined) query.set("pageToken", input.pageToken);
    if (input.state !== undefined) query.set("state", input.state);
    return this.sendControlPlaneRequest("sessions.list", { query });
  }

  close(input: NimbusSessionCloseRequest): Promise<NimbusSessionResource> {
    return this.sendControlPlaneRequest("sessions.close", {
      params: sessionResourceParams(input),
      body: {
        ...(input.reason === undefined ? {} : { reason: input.reason }),
      },
    });
  }
}

function clientTenantId(client: Nimbus): string | undefined {
  return client.tenantId;
}

const SERVICE_LIFECYCLE_ROUTES = {
  start: "services.start",
  stop: "services.stop",
} as const;

function serviceResourceParams(
  client: Nimbus,
  selector: NimbusServiceSelector,
): NimbusControlPlaneRouteParams {
  return {
    ...serviceCollectionParams(client, selector),
    service_name: selector.name,
  };
}

function serviceCollectionParams(
  client: Nimbus,
  input: NimbusServiceListRequest,
): NimbusControlPlaneRouteParams {
  const tenantId = resolveTenantId(client, input, "service collection request");
  return { tenant_id: tenantId };
}

function sandboxCollectionParams(
  client: Nimbus,
  input: { tenantId?: string },
): NimbusControlPlaneRouteParams {
  const tenantId = resolveTenantId(client, input, "sandbox collection request");
  return { tenant_id: tenantId };
}

function sandboxResourceParams(
  client: Nimbus,
  selector: NimbusSandboxSelector,
): NimbusControlPlaneRouteParams {
  const tenantId = resolveTenantId(client, selector, "sandbox request");
  return {
    tenant_id: tenantId,
    sandbox_id: selector.id,
  };
}

function sessionResourceParams(
  selector: NimbusSessionSelector,
): NimbusControlPlaneRouteParams {
  return { session_id: selector.id };
}

function resolveTenantId(
  client: Nimbus,
  selector: { tenantId?: string },
  context: string,
): string {
  const tenantId =
    stringOrUndefined(selector.tenantId) ?? clientTenantId(client);
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

function positiveFiniteNumber(
  value: unknown,
  fallback: number,
  label: string,
): number {
  if (value === undefined) return fallback;
  if (typeof value === "number" && Number.isFinite(value) && value > 0) {
    return value;
  }
  throw new Error(
    `Nimbus service wait ${label} must be a positive finite number.`,
  );
}

function serviceMatchesCondition(
  service: NimbusService,
  condition: NimbusServiceWaitCondition,
): boolean {
  const state =
    normalizeStatusString(service.lifecycleState) ??
    normalizeStatusString(service.state) ??
    normalizeStatusString(service.status);
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
  verb: "start" | "stop",
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
  return typeof value === "string" && value.length > 0
    ? value.toLowerCase()
    : undefined;
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
