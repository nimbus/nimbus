---
title: SDK Clients
description: Reference for @nimbus/nimbus/browser and @nimbus/nimbus/transports/rest — deployment clients, subscriptions, and the native REST client.
sidebar:
  label: Clients
  order: 3
---

Two entry points provide client classes:

- `@nimbus/nimbus/browser` — the data-plane clients (`NimbusClient`,
  `NimbusHttpClient`, `NimbusReactClient`) that call deployment functions and
  subscribe to live query results.
- `@nimbus/nimbus/transports/rest` — the native-surface clients
  (`NimbusRestClient`, `NimbusSubscriptionClient`) for tenant administration,
  document operations, scheduling, and shape-based subscriptions.

## Deployment URL

`NimbusClient` and `NimbusHttpClient` take a deployment URL of the form
`{origin}/convex/{tenant}`:

```typescript
import { NimbusClient } from "@nimbus/nimbus/browser";

const client = new NimbusClient("http://127.0.0.1:8080/convex/demo");
```

The constructor validates that the address is an absolute URL unless
`skipDeploymentUrlCheck: true` is passed. Function calls go to paths under
the deployment URL (`/query`, `/mutation`, …) and subscriptions use the
WebSocket endpoint at `{deploymentUrl}/ws`.

## Function references

Clients call functions through typed references. Named references address
functions deployed on the server using `"module:function"` format:

```typescript
import { makeQueryReference, makeMutationReference } from "@nimbus/nimbus/browser";

const listMessages = makeQueryReference<{ channel: string }>("messages:list");
const sendMessage = makeMutationReference<{ channel: string; body: string }>(
  "messages:send",
);
```

| Export | Produces |
| --- | --- |
| `makeQueryReference(name, visibility?)` | `QueryReference` |
| `makePaginatedQueryReference(name, visibility?)` | `PaginatedQueryReference` |
| `makeMutationReference(name, visibility?)` | `MutationReference` |
| `makeActionReference(name, visibility?)` | `ActionReference` |

The `define*` family builds client-resolved references instead: each takes a
`name` and a `resolve(args)` function that produces a raw query or mutation
shape, which the client sends directly instead of a function name.

| Export | Resolver returns |
| --- | --- |
| `defineQuery(name, resolve)` | A query shape (`{ table, filters, order, limit }`). |
| `definePaginatedQuery(name, resolve)` | A query shape, paginated by the client. |
| `defineMutation(name, resolve)` | A mutation shape (insert, update, or delete). |
| `defineAction(name, resolve)` | An action shape wrapping a query or mutation. |

Type helpers: `FunctionReference` (union of reference kinds), `InferArgs`
and `InferResult` (extract a reference's argument and result types),
`QueryReference`, `QueryEntry`, and the `queryEntry(ref, args)` helper that
pairs a reference with its arguments.

## NimbusClient

The full-featured client: HTTP calls plus WebSocket subscriptions.

```typescript
new NimbusClient(address, {
  skipDeploymentUrlCheck?: boolean;
  auth?: string;
  fetch?: FetchLike;
  disabled?: boolean;
  authRefreshTokenLeewaySeconds?: number;  // default 10
  webSocket?: WebSocketConstructor;
})
```

| Option | Description |
| --- | --- |
| `skipDeploymentUrlCheck` | Skip absolute-URL validation of `address`. |
| `auth` | Initial bearer token for requests. |
| `fetch` | Custom `fetch` implementation. |
| `disabled` | Start closed: subscriptions never connect; HTTP methods still work. |
| `authRefreshTokenLeewaySeconds` | Seconds before JWT expiry at which the client refreshes the socket token. |
| `webSocket` | Custom WebSocket constructor (for runtimes without a global `WebSocket`). |

### Methods

| Method | Returns | Description |
| --- | --- | --- |
| `url` (getter) | `string` | The deployment URL. |
| `setAuth(value, onChange?)` | `void` | Set a bearer token or an `AuthTokenFetcher`; reconnects the socket. |
| `clearAuth()` | `void` | Drop credentials; reconnects the socket. |
| `connectionState()` | `ConnectionState` | Current connection snapshot. |
| `subscribeToConnectionState(callback)` | `() => void` | Listen for connection-state changes; returns an unsubscribe function. |
| `query(ref, args?)` | `Promise<Result>` | Run a query once over HTTP. |
| `mutation(ref, args?)` | `Promise<Result>` | Run a mutation (tracked in `inflightMutations`). |
| `action(ref, args?)` | `Promise<Result>` | Run an action (tracked in `inflightActions`). |
| `paginatedQuery(ref, args, pageSize, cursor)` | `Promise<Page<Item>>` | Fetch one page: `{ data, next_cursor, has_more }`. |
| `scheduleAfter(ref, args, runAfterMs)` | `Promise<string>` | Schedule a mutation after a delay; resolves to a job id. |
| `scheduleAt(ref, args, runAtMs)` | `Promise<string>` | Schedule a mutation at an absolute time; resolves to a job id. |
| `cancelScheduledFunction(jobId)` | `Promise<void>` | Cancel a scheduled job. |
| `onUpdate(ref, args, callback, onError?, options?)` | `Unsubscribe<Result>` | Subscribe to live results over WebSocket. |
| `close()` | `void` | Close the socket and drop all subscriptions. |

`onUpdate` accepts `options: { pageSize?, cursor? }` for paginated query
references. The returned `Unsubscribe<T>` is a callable function that also
exposes `unsubscribe()`, `getCurrentValue()` (last received value, or
`undefined`), and `getQueryLogs()` (always `undefined` in the current
implementation).

```typescript
const unsubscribe = client.onUpdate(
  listMessages,
  { channel: "general" },
  (messages) => render(messages),
  (error) => console.error(error),
);

// later
unsubscribe();
```

### WebSocket behavior

- The socket negotiates the `nimbus.v2` subprotocol and opens with a
  `client_hello` declaring the `queries.v1` and `subscriptions.v1`
  capabilities. See the
  [WebSocket protocol reference](/reference/native/websocket-protocol/).
- If credentials are set, the client authenticates before subscribing (2
  second timeout, with one retry using a force-refreshed token).
- Named references subscribe with `subscribe_named`; client-resolved
  (`define*`) references subscribe with `subscribe` and a raw query shape.
- Identical consecutive results are deduplicated by JSON equality, so
  callbacks fire only on changes.
- On disconnect the client automatically reconnects (50 ms timer) and
  resubscribes every active subscription.
- When auth was set with an `AuthTokenFetcher`, the client decodes the JWT
  `iat`/`exp` claims and schedules a token refresh
  `authRefreshTokenLeewaySeconds` before expiry.

### ConnectionState

| Field | Type |
| --- | --- |
| `hasInflightRequests` | `boolean` |
| `isWebSocketConnected` | `boolean` |
| `timeOfOldestInflightRequest` | `Date \| null` |
| `hasEverConnected` | `boolean` |
| `connectionCount` | `number` |
| `connectionRetries` | `number` |
| `inflightMutations` | `number` |
| `inflightActions` | `number` |

## NimbusHttpClient

HTTP-only client with the same call methods and no WebSocket. `NimbusClient`
uses one internally.

```typescript
new NimbusHttpClient(address, {
  skipDeploymentUrlCheck?: boolean;
  auth?: string;
  fetch?: FetchLike;
})
```

Calls map to deployment routes: `query` → `POST /query`, `mutation` →
`POST /mutation`, `action` → `POST /action`, `paginatedQuery` →
`POST /query/paginated`, `scheduleAfter` → `POST /schedule/run_after`,
`scheduleAt` → `POST /schedule/run_at`, `cancelScheduledFunction` →
`DELETE /schedule/{jobId}`.

Additional members beyond the `NimbusClient` call surface:

| Member | Description |
| --- | --- |
| `getAuthToken(forceRefreshToken)` | Resolve the current token, invoking the fetcher when set. |
| `notifyAuthState(isAuthenticated)` | Invoke the registered auth-change listener. |
| `canRefreshAuthToken()` | `true` when an `AuthTokenFetcher` is registered. |

When a request receives `401` and an `AuthTokenFetcher` is registered, the
client refetches the token with `forceRefreshToken: true` and retries once.

`AuthTokenFetcher` is
`(args: { forceRefreshToken: boolean }) => Promise<string | null | undefined>`.

## NimbusReactClient

`class NimbusReactClient extends NimbusClient {}` — identical surface,
exported for use with the React providers. See
[React](/reference/sdk/react/).

## Custom WebSocket implementations

`WebSocketLike` is the minimal socket interface the client needs
(`send`, `close`, and either `addEventListener` or `on`), and
`WebSocketConstructor` is its constructor type. Pass a constructor via the
`webSocket` option to run `NimbusClient` in environments without a global
`WebSocket`.

## NimbusRestClient — `@nimbus/nimbus/transports/rest`

A thin client for the [native HTTP API](/reference/native/http-api/). It
takes the server origin, not a deployment URL:

```typescript
import { NimbusRestClient } from "@nimbus/nimbus/transports/rest";

const rest = new NimbusRestClient("http://127.0.0.1:8080", {
  token: process.env.NIMBUS_TOKEN,
});
```

Options (`NimbusRestClientOptions`): `fetch` (custom fetch), `headers`
(extra default headers), `token` (sent as `Authorization: Bearer …`), and
`apiKey` (sent as an `X-Nimbus-Api-Key` header — note that the current
server authenticates bearer tokens; prefer `token`).

### request

```typescript
request<T = unknown>(path: string, options?: RequestOptions): Promise<T>
```

The core escape hatch: sends a JSON request to `{baseUrl}{path}` with the
default headers, parses JSON responses, returns `null` for `204`, and throws
an `Error` with the server's message on non-2xx responses. `RequestOptions`
is `RequestInit` with a plain-object `headers` field. Any route in the
[native HTTP API](/reference/native/http-api/) can be called this way.

### Convenience methods

| Method | Route |
| --- | --- |
| `health()` | `GET /health` |
| `createTenant(id)` | `POST /api/tenants` |
| `listTenants()` | `GET /api/tenants` |
| `insertDocument(tenantId, table, fields)` | `POST /api/tenants/{t}/documents` |
| `query(tenantId, query)` | `POST /api/tenants/{t}/query` |
| `scheduleMutation(tenantId, request)` | `POST /api/tenants/{t}/schedule` |
| `listScheduledJobs(tenantId)` | `GET /api/tenants/{t}/schedule` |
| `getScheduledJobResult(tenantId, jobId)` | `GET /api/tenants/{t}/schedule/history/{jobId}` |
| `listCronJobs(tenantId)` | `GET /api/tenants/{t}/crons` |
| `deleteCronJob(tenantId, name)` | `DELETE /api/tenants/{t}/crons/{name}` |

`scheduleMutation` resolves to `{ job_id }` (`ScheduleMutationRequest` is
`{ run_after_ms, mutation }`). When calling `query`, always pass `filters`
explicitly (use `[]` for no filters) — the server requires the field even
though the `SubscribeQuery` type marks it optional.

### Methods to avoid

Several convenience methods predate the current server routes and do not
work against the current server. Use `request()` with the routes documented
in the [native HTTP API reference](/reference/native/http-api/) instead:

- `getDocument`, `listDocuments`, `updateDocument`, `deleteDocument` — these
  target document paths the server does not expose in that shape (document
  reads and updates are addressed by `{table}/{documentId}`, and updates send
  a `{ patch }` body).
- `createCronJob` — the route exists, but the `CronJobRequest.schedule`
  field is typed as a `string` while the server expects a schedule object
  such as `{ "type": "interval", "seconds": 60 }`.
- `setTableSchema` — the route exists, but the `TableSchema` type's
  `indexes` entries (`{ name, field }`) do not match the server's expected
  shape (`{ name, fields: [...] }`, with `indexes` required).

## NimbusSubscriptionClient — `@nimbus/nimbus/transports/rest`

A standalone WebSocket client for shape-based subscriptions against the
native `/ws` endpoint (it appends `?tenant_id={tenant}` to the server
origin).

```typescript
import { NimbusSubscriptionClient } from "@nimbus/nimbus/transports/rest";

const subs = new NimbusSubscriptionClient("http://127.0.0.1:8080", "demo");
await subs.connect();

const subscription = await subs.subscribe(
  { table: "messages", filters: [], limit: 50 },
  { onResult: (rows) => render(rows) },
);

// later
subscription.unsubscribe();
subs.close();
```

| Member | Description |
| --- | --- |
| `connect()` | Open the socket (subprotocol `nimbus.v2`) and send `client_hello`. |
| `subscribe(query, { onResult?, onError? })` | Send a shape subscription; resolves to a `Subscription` after the first result. |
| `unsubscribe(subscriptionId)` | Stop a subscription. |
| `close()` | Close the socket; pending requests reject. |

Constructor options (`SubscriptionClientOptions`): `onLog?: (message) => void`
for connection/protocol logging.

Notes:

- The native `/ws` endpoint accepts shape subscriptions only; named-function
  subscriptions are available through `NimbusClient` against a deployment
  URL.
- Always pass `filters` (use `[]`) in `SubscribeQuery`, as with `query`
  above.
- `Subscription.subscriptionId` is typed `string`, but the wire value is a
  number; treat it as opaque and pass it back to `unsubscribe` unchanged.

## Exports — `@nimbus/nimbus/browser`

Classes and functions: `NimbusClient`, `NimbusHttpClient`,
`NimbusReactClient`, `defineQuery`, `definePaginatedQuery`, `defineMutation`,
`defineAction`, `makeQueryReference`, `makePaginatedQueryReference`,
`makeMutationReference`, `makeActionReference`, `queryEntry` — documented
above.

Types: `AuthTokenFetcher`, `ConnectionState`, `FunctionReference`,
`InferArgs`, `InferResult`, `QueryEntry`, `QueryReference`, `Unsubscribe`,
`WebSocketConstructor`, `WebSocketLike` — documented above.

## Exports — `@nimbus/nimbus/transports/rest`

Classes: `NimbusRestClient`, `NimbusSubscriptionClient` — documented above.

Types:

| Type | Description |
| --- | --- |
| `CronJobRequest` | Cron creation body; see the caveat under "Methods to avoid". |
| `FetchLike` | Fetch-compatible function type. |
| `NimbusRestClientOptions` | Constructor options for `NimbusRestClient`. |
| `RequestOptions` | `RequestInit` with plain-object headers. |
| `ScheduleMutationRequest` | `{ run_after_ms, mutation }`. |
| `SubscribeQuery` | Query shape: `{ table, filters?, order?, limit? }` (always pass `filters`). |
| `Subscription` | `{ subscriptionId, unsubscribe }`. |
| `SubscriptionClientOptions` | `{ onLog? }`. |
| `TableSchema` | Schema body; see the caveat under "Methods to avoid". |
