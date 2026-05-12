# Native HTTP / WebSocket Adapter

```bash
nimbus start --port 8080

curl -s -X POST http://localhost:8080/api/tenants -H "Content-Type: application/json" -d '{"id": "demo"}'
curl -s -X POST http://localhost:8080/api/tenants/demo/documents -H "Content-Type: application/json" \
  -d '{"table": "messages", "fields": {"text": "hello", "author": "you"}}'
curl -s http://localhost:8080/api/tenants/demo/query -H "Content-Type: application/json" \
  -d '{"table": "messages", "filters": []}'
```

The most direct path to Nimbus. REST for documents, WebSocket for live
subscriptions. No SDK needed -- just curl or any HTTP client. ~1 minute
from install to query.

## Quick start

```bash
# 1. Start the server
nimbus start --port 8080

# 2. Create a tenant
curl -s -X POST http://localhost:8080/api/tenants \
  -H "Content-Type: application/json" \
  -d '{"id": "demo"}'

# 3. Insert a document
curl -s -X POST http://localhost:8080/api/tenants/demo/documents \
  -H "Content-Type: application/json" \
  -d '{"table": "messages", "fields": {"text": "hello", "author": "you"}}'

# 4. Query it back
curl -s -X POST http://localhost:8080/api/tenants/demo/query \
  -H "Content-Type: application/json" \
  -d '{"table": "messages", "filters": []}'
```

## Client package

`nimbus`

- `nimbus/rest` -- `NimbusRestClient` (HTTP) and `NimbusSubscriptionClient` (WebSocket) for the native REST API
- `nimbus/browser` -- `NimbusHttpClient` and `NimbusClient` for server-function references (Convex-style)
- `nimbus/server`, `nimbus/react`, `nimbus/values` -- server functions, React hooks, value types

## Project Layout

For HTTP-only usage (no server functions):

```
my-app/
├── src/
│   └── main.ts
├── package.json
└── index.html
```

For server functions with the native `nimbus/` source root (experimental):

```
my-app/
├── nimbus/
│   ├── schema.ts
│   ├── messages.ts
│   └── _generated/
│       ├── api.ts
│       ├── dataModel.d.ts
│       └── server.ts
├── src/
│   └── main.tsx
├── .nimbus/
│   └── convex/                # Internal build artifacts
├── package.json
└── vite.config.ts
```

## Example Code

### Using the `nimbus/rest` SDK

The `nimbus/rest` export provides `NimbusRestClient` for HTTP operations and `NimbusSubscriptionClient` for live WebSocket subscriptions.

```typescript
import { NimbusRestClient, NimbusSubscriptionClient } from "nimbus/rest";

const client = new NimbusRestClient("http://localhost:8080");

// Health check
await client.health();

// Create a tenant
await client.createTenant("my-tenant");

// Install a table schema
await client.setTableSchema("my-tenant", "tasks", {
  table: "tasks",
  fields: [
    { name: "title", field_type: "string", required: true },
    { name: "status", field_type: "string", required: true },
    { name: "priority", field_type: "number", required: false },
  ],
  indexes: [
    { name: "by_status", field: "status" },
    { name: "by_priority", field: "priority" },
  ],
});

// Insert a document
await client.insertDocument("my-tenant", "tasks", {
  title: "Ship MVP",
  status: "open",
  priority: 1,
});

// Query documents
const tasks = await client.query("my-tenant", { table: "tasks", filters: [] });
```

### Schedule a mutation

```typescript
const { job_id } = await client.scheduleMutation("my-tenant", {
  run_after_ms: 5000,
  mutation: {
    type: "insert",
    table: "tasks",
    fields: { title: "Follow-up", status: "queued", priority: 2 },
  },
});

// Check the result later
const result = await client.getScheduledJobResult("my-tenant", job_id);
```

### Live subscriptions (WebSocket)

```typescript
import { NimbusSubscriptionClient } from "nimbus/rest";

const ws = new NimbusSubscriptionClient("http://localhost:8080", "my-tenant", {
  onLog: (msg) => console.log(msg),
});

await ws.connect();

const subscription = await ws.subscribe(
  { table: "tasks", filters: [], limit: 25 },
  {
    onResult: (documents) => console.log("live update:", documents),
    onError: (error) => console.error("subscription error:", error),
  },
);

// Later: unsubscribe and close
subscription.unsubscribe();
ws.close();
```

The native WebSocket uses the `nimbus.v2` protocol. See the [WebSocket protocol reference](websocket-protocol.md) for the full framing contract.

### Server functions with the `nimbus` SDK

When using server functions with a `nimbus/` source root, the `nimbus/browser` package provides typed clients for function references (distinct from the REST surface above):

```typescript
import { NimbusHttpClient, NimbusClient } from "nimbus/browser";
import { api } from "./nimbus/_generated/api.ts";

// HTTP client for one-shot queries
const http = new NimbusHttpClient("http://localhost:8080/convex/my-tenant");
const tasks = await http.query(api.tasks.list, {});

// WebSocket client for live subscriptions
const live = new NimbusClient("http://localhost:8080/convex/my-tenant");
live.onUpdate(api.tasks.list, {}, (results) => console.log(results));
```

## REST Endpoints

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/health` | Health check |
| `POST` | `/api/tenants` | Create tenant |
| `GET` | `/api/tenants` | List tenants |
| `POST` | `/api/tenants/{id}/documents` | Insert document |
| `GET` | `/api/tenants/{id}/documents` | List documents |
| `GET` | `/api/tenants/{id}/documents/{doc_id}` | Get document |
| `PATCH` | `/api/tenants/{id}/documents/{doc_id}` | Update document |
| `DELETE` | `/api/tenants/{id}/documents/{doc_id}` | Delete document |
| `POST` | `/api/tenants/{id}/query` | Execute query |
| `PUT` | `/api/tenants/{id}/schema/{table}` | Set table schema |
| `POST` | `/api/tenants/{id}/schedule` | Schedule mutation |
| `POST` | `/api/tenants/{id}/crons` | Create cron job |
| `WS` | `/ws?tenant_id={id}` | WebSocket (nimbus.v2) |

See the [HTTP and WebSocket API reference](http-api.md) for the full route catalog.

### Tenant creation security

> **Important:** `POST /api/tenants` creates tenants on demand without authentication. This is convenient for local development but is a security concern in production. Pre-provision tenants via the admin API. A `--auto-create-tenants` flag (default off, opt-in for development) is planned.

## `nimbus/` Source Root (Experimental)

The native `nimbus/` source root is the preferred authoring mode for new Nimbus-native projects. When codegen detects both `nimbus/` and `convex/` directories, `nimbus/` takes priority. Generated files import from `nimbus/*` instead of `convex/*`.

This is experimental. See the [source directory story](../../plans/stories/support-nimbus-source-directory.md) for the full contract.

## Related Docs

- [HTTP and WebSocket API reference](http-api.md)
- [WebSocket protocol](websocket-protocol.md)
- [Demo: nimbus/html](../../demos/nimbus/html/)
