---
title: SDK Resources
description: Reference for the root @nimbus/nimbus entry — the Nimbus control-plane client, credential discovery, and the services, sandboxes, and sessions APIs.
sidebar:
  label: Resources
  order: 4
---

The root entry point exports the `Nimbus` class — the control-plane client
for managing **services**, **sandboxes**, and **sessions** — along with the
request and resource types those APIs use. For concepts and worked examples,
see the [resource model guide](/developers/sdk/resource-model/).

```typescript
import { Nimbus } from "@nimbus/nimbus";

const nimbus = new Nimbus({ tenantId: "demo" });

const service = await nimbus.services.start({ name: "web", waitUntil: "ready" });
```

## The Nimbus class

```typescript
new Nimbus(options?: NimbusClientOptions)
```

| Member | Description |
| --- | --- |
| `services` | `NimbusServices` — service lifecycle and definitions. |
| `sandboxes` | `NimbusSandboxes` — sandbox creation and lifecycle. |
| `sessions` | `NimbusSessions` — interactive sessions against services or sandboxes. |
| `tenantId` | The default tenant from options, if set. |
| `static defaultClient(options?)` | `Promise<Nimbus>` — constructs a client and eagerly resolves endpoint and credential, so discovery failures surface immediately instead of on first call. |

The constructor itself never performs I/O; endpoint and credential discovery
runs lazily on the first request (or eagerly via `defaultClient`).

### NimbusClientOptions

| Option | Description |
| --- | --- |
| `endpoint` | Server origin, for example `http://127.0.0.1:8080`. |
| `tenantId` | Default tenant for calls that need one. |
| `token` | Bearer token (shorthand for `credential: { kind: "bearer", token }`). |
| `credential` | Explicit `NimbusCredential`. |
| `fetch` | Custom fetch implementation. |
| `headers` | Extra headers on every request. |

`NimbusCredential` is a tagged union:

```typescript
type NimbusCredential =
  | { kind: "bearer"; token: string }
  | { kind: "workload_identity"; token: string; issuer?: string; audience?: string; subject?: string };
```

Bearer and workload-identity credentials are sent as
`Authorization: Bearer …`.

### Endpoint and credential discovery

When `endpoint` or a credential is not passed explicitly, the client
discovers them in order:

**Endpoint:**

1. `options.endpoint`
2. `NIMBUS_ENDPOINT` environment variable
3. The `endpoint` field of the local credential file

**Credential:**

1. `options.credential` or `options.token`
2. Environment variables: `NIMBUS_TOKEN` or `NIMBUS_BEARER_TOKEN` (bearer),
   then `NIMBUS_WORKLOAD_IDENTITY_TOKEN` (with
   optional `NIMBUS_WORKLOAD_IDENTITY_ISSUER`,
   `NIMBUS_WORKLOAD_IDENTITY_AUDIENCE`, `NIMBUS_WORKLOAD_IDENTITY_SUBJECT`)
3. The local credential file:
   `~/.config/nimbus/application_default_credentials.json` (honors
   `XDG_CONFIG_HOME`; override the path with
   `NIMBUS_APPLICATION_CREDENTIALS`). The file may carry `endpoint`, a
   top-level `token`/`access_token`, or a nested
   `credential` object with `kind`, `token`, and workload-identity fields.
4. A token file named by `NIMBUS_WORKLOAD_IDENTITY_TOKEN_FILE` (read as a
   workload-identity token).

If no endpoint or no credential can be discovered, the client throws an
error naming the options and environment variables to set.

### Tenant resolution

Service and sandbox calls, plus `sessions.open` and `sessions.list`, need a
tenant. Each request accepts a `tenantId` that overrides the client-level
default; if neither is set, the call throws. `sessions.get` and
`sessions.close` address sessions by id alone.

## Services — `nimbus.services`

| Method | Returns | Description |
| --- | --- | --- |
| `start(input)` | `Promise<NimbusService>` | `POST .../services/{name}/start`. Optional `waitUntil: "ready" \| "healthy"` polls until the condition holds. |
| `stop(input)` | `Promise<NimbusService>` | `POST .../services/{name}/stop`. Optional `waitUntil: "stopped"`. |
| `restart(input)` | `Promise<NimbusService>` | `POST .../services/{name}/restart`. Optional `waitUntil: "ready" \| "healthy"`. |
| `get(selector)` | `Promise<NimbusService>` | `GET .../services/{name}`. |
| `create(input)` | `Promise<NimbusServiceDefinition>` | `POST .../services` with a backend spec and optional labels. |
| `update(input)` | `Promise<NimbusServiceDefinition>` | `PUT .../services/{name}`; requires `ifMatchGeneration`. |
| `delete(input)` | `Promise<void>` | `DELETE .../services/{name}?ifMatchGeneration=…`; optional `force`. |
| `list(input?)` | `Promise<NimbusServiceDefinitionCollection>` | `GET .../services` with optional `limit` and `pageToken`. |
| `wait(input)` | `Promise<NimbusService>` | Poll `get` until `until: "ready" \| "stopped" \| "healthy"` holds. |

All service paths are rooted at `/api/tenants/{tenant}/services`.

- Passing an unsupported `waitUntil` value (for example `"stopped"` to
  `start`) throws before any request is sent.
- `wait` accepts `timeoutMs` (default `30000`) and `intervalMs` (default
  `250`), both required to be positive finite numbers. On timeout it throws
  an error that includes the last observed status.
- Lifecycle calls and `get` return `NimbusService` — a loose runtime view
  (`name`, plus optional `lifecycleState`, `readiness`, `health`,
  `sandboxId`, `backend`, `endpoints`, and other fields). `create`,
  `update`, and `list` return `NimbusServiceDefinition` resources with
  `metadata` / `spec` / `status` blocks.

### Optimistic concurrency

`update` and `delete` require `ifMatchGeneration` — the `metadata.generation`
of the definition you read. If the resource changed in the meantime, the
server rejects the write and the call throws; re-read and retry. See the
[resource model guide](/developers/sdk/resource-model/) for the full
read-modify-write pattern.

### Backend specs

`NimbusServiceCreateRequest.backend` takes a `NimbusServiceBackendSpec`:

```typescript
type NimbusServiceBackendSpec =
  | { kind: "sandbox"; sandbox: NimbusSandboxSpec }
  | { kind: "builtIn"; provider: "loadBalancer" | "serviceDiscovery" | "browser" | "modelGateway" }
  | {
      kind: "external";
      endpoint: { url: string };          // NimbusExternalEndpointPolicy
      auth: { kind: "none" };             // NimbusExternalAuthPolicy
      health: { kind: "http"; path: string }; // NimbusHealthCheckPolicy
    };
```

Responses use `NimbusServiceBackendResponse`, which adds a `redacted`
variant: callers without inspect permission see
`{ kind: "redacted", backend, redacted: true, reason: "requiresInspectPermission" }`.

## Sandboxes — `nimbus.sandboxes`

| Method | Returns | Description |
| --- | --- | --- |
| `create(input)` | `Promise<NimbusSandboxResource>` | `POST .../sandboxes` with `profile`, `spec`, optional `labels`. |
| `get(selector)` | `Promise<NimbusSandboxResource>` | `GET .../sandboxes/{id}`. |
| `list(input?)` | `Promise<NimbusSandboxCollection>` | `GET .../sandboxes` with optional `limit`, `pageToken`, `status`, `labelKey`, `labelValue`. |
| `stop(selector)` | `Promise<NimbusSandboxResource>` | `POST .../sandboxes/{id}/stop`. |

Sandboxes are id-addressed (`NimbusSandboxSelector` is `{ id, tenantId? }`).
`NimbusSandboxProfile` is `"worker" | "desktop"`.

A `NimbusSandboxSpec` describes what runs:

```typescript
const sandbox = await nimbus.sandboxes.create({
  profile: "worker",
  spec: {
    owner: { kind: "standalone", displayName: "batch-job" },
    backend: "krun",
    root: {
      kind: "oci_image",
      source: { kind: "reference", reference: "docker.io/library/node:22" },
    },
    process: { argv: ["node", "run.js"], cwd: "/app" },
  },
});
```

- `owner` (`NimbusSandboxOwnerSpec`) is `{ kind: "service", serviceName? }`
  or `{ kind: "standalone", displayName? }`.
- `backend` (`NimbusSandboxBackendKind`) is `"krun"` or `"container"`.
- `root` (`NimbusSandboxRootSpec`) currently takes an OCI image reference
  (`NimbusSandboxOciImageReferenceSource`).
- `process` (`NimbusSandboxProcessSpec`) has optional `argv`, `args`,
  `entrypoint`, `command`, `env`, `cwd`, `user`, `terminal`.

Responses redact launch inputs: `NimbusSandboxProcessResponse` returns
`argv`, `entrypoint`, `command`, and `environment` as `NimbusRedactedValues`
(`{ redacted: true, valueCount }`), and `NimbusSandboxRootResponse` may be
`{ kind: "redacted", redacted: true, reason: "operatorOnlyLaunchInput" }`.

## Sessions — `nimbus.sessions`

| Method | Returns | Description |
| --- | --- | --- |
| `open(input)` | `Promise<NimbusSessionResource>` | `POST /api/sessions` with `target`, `channels`, optional `requestedTtlMs`. |
| `get(selector)` | `Promise<NimbusSessionResource>` | `GET /api/sessions/{id}`. |
| `list(input?)` | `Promise<NimbusSessionCollection>` | `GET /api/sessions?tenantId=…` with optional `limit`, `pageToken`, `state`. |
| `close(input)` | `Promise<NimbusSessionResource>` | `POST /api/sessions/{id}/close` with optional `reason`. |

```typescript
const session = await nimbus.sessions.open({
  target: { sandbox: { id: sandbox.metadata.id } },
  channels: ["stdio"],
});
```

- `NimbusSessionTarget` is `{ service: { name } }` or `{ sandbox: { id } }`.
- `NimbusSessionChannel` is `"cdp" | "page" | "stdio" | "files"`.
- Session state (`status.lifecycleState`) is `"open" | "closed" | "expired"`;
  `spec.expiresAt` carries the expiry. Sessions cannot be renewed or
  extended — open a new one.
- `spec.targetSnapshot` records what the session bound to at open time
  (service name/generation/backend, or sandbox id/generation/profile).

## Resource shapes

Service definitions, sandboxes, and sessions share a `metadata` / `spec` /
`status` layout:

- `metadata` — `tenantId`, the identifier (`name` for services, `id` for
  sandboxes and sessions), `generation`, `resourceVersion`, `createdAt`,
  `updatedAt`, and `labels` (services and sandboxes).
- `status.conditions` — an array of `NimbusCondition`:
  `{ type, status: "True" | "False" | "Unknown", reason, message, observedGeneration, lastTransitionTime }`.
- Collections (`NimbusServiceDefinitionCollection`, `NimbusSandboxCollection`,
  `NimbusSessionCollection`) wrap `items` with collection `metadata`
  including `limit`, `remainingCount`, and an optional `nextPageToken` for
  the next `list` call.

## Exports — `@nimbus/nimbus`

Class: `Nimbus` — documented above.

Types, grouped (all documented in the sections above):

| Group | Exports |
| --- | --- |
| Client | `NimbusClientOptions`, `NimbusCredential` |
| Shared | `NimbusCondition`, `NimbusRedactedValues`, `NimbusServiceEndpoint` |
| Service requests | `NimbusServiceSelector`, `NimbusServiceStartRequest`, `NimbusServiceStopRequest`, `NimbusServiceLifecycleRequest`, `NimbusServiceWaitRequest`, `NimbusServiceCreateRequest`, `NimbusServiceUpdateRequest`, `NimbusServiceDeleteRequest`, `NimbusServiceListRequest` |
| Service wait conditions | `NimbusServiceWaitCondition`, `NimbusServiceActivationWaitCondition`, `NimbusServiceStopWaitCondition` |
| Service backends | `NimbusServiceBackendSpec`, `NimbusServiceBackendResponse`, `NimbusBuiltInProviderId`, `NimbusExternalAuthPolicy`, `NimbusExternalEndpointPolicy`, `NimbusHealthCheckPolicy` |
| Service resources | `NimbusService`, `NimbusServiceDefinition`, `NimbusServiceDefinitionCollection` |
| Sandbox requests | `NimbusSandboxCreateRequest`, `NimbusSandboxSelector`, `NimbusSandboxListRequest`, `NimbusSandboxProfile` |
| Sandbox specs | `NimbusSandboxSpec`, `NimbusSandboxOwnerSpec`, `NimbusSandboxBackendKind`, `NimbusSandboxRootSpec`, `NimbusSandboxOciImageReferenceSource`, `NimbusSandboxProcessSpec` |
| Sandbox responses | `NimbusSandboxResource`, `NimbusSandboxCollection`, `NimbusSandboxSpecResponse`, `NimbusSandboxRootResponse`, `NimbusSandboxProcessResponse` |
| Session requests | `NimbusSessionOpenRequest`, `NimbusSessionSelector`, `NimbusSessionListRequest`, `NimbusSessionCloseRequest`, `NimbusSessionTarget`, `NimbusSessionChannel` |
| Session resources | `NimbusSessionResource`, `NimbusSessionCollection` |
