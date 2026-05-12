# Nimbus Demos

This directory holds Nimbus demo apps split by ownership:

- `demos/nimbus/*` for Nimbus-native demos
- `demos/firebase/*` for Firebase/Firestore demos
- `demos/mongodb/*` for MongoDB wire protocol demos
- `demos/convex/*` for Convex-surface demos and fixtures

The Convex-side demos still mirror useful shapes from the official Convex demos repository:

- Official Convex demos: <https://github.com/get-convex/convex-demos>
- Convex backend: <https://github.com/get-convex/convex-backend>
- Axum static file example: <https://github.com/tokio-rs/axum/tree/main/examples/static-file-server>
- Axum chat/websocket example: <https://github.com/tokio-rs/axum/tree/main/examples/chat>

These are Nimbus ports and adaptations, not the official Convex demos running unchanged.

Current 4B note:

- compiled `ctx.db.patch(...)` and `ctx.db.delete(...)` are supported when the handler arg uses `v.id("table")`
- compiled `ctx.db.get(id)` is supported when the handler arg uses `v.id("table")`
- compiled `httpAction` routes are supported for the 4B declarative subset through `httpRouter`, `convex/http.ts`, request placeholders, and the tenant-scoped convex HTTP transport
- compiled `ctx.db.query(...).filter(...)` is supported for declarative filter chains
- compiled `ctx.db.query(...).first()` is supported for single-document query results
- compiled `ctx.db.query(...).unique()` is supported and returns an error when multiple documents match
- mixed `ctx.db.query(...).withIndex(...).filter(...).unique()` plans are supported for exact indexed lookups with residual filters
- runtime-only named query/mutation/action handlers now execute through the V8 bundle path for the first broader 4C slice
- runtime-only named `paginatedQuery` handlers now execute through that bundle path too when they return a live query builder
- named `paginatedQuery` refs now work with the convex WebSocket path, so `usePaginatedQuery` can refresh its loaded window after live invalidations
- compiled `paginatedQuery` handlers can now return `ctx.db.query(...)` directly, which is a closer match to natural Convex authoring
- the convex browser client now suppresses unchanged subscription payloads so React demos do not rerender on no-op invalidations
- reconnect/resubscribe also suppresses an unchanged initial replay payload, which avoids extra rerenders after transient socket drops
- generated `convex/_generated/api.ts` refs now carry typed args and common inferred result shapes, so the demos can lean on inference instead of manual casts
- generated action refs now infer common delegated return shapes too, so demo actions that call generated queries or mutations often do not need explicit `returns`
- `convex/react` now masks stale values and stale errors across arg changes and `"skip"` transitions, so hook loading/error behavior is much closer to Convex
- `useQueries` keeps failures local as `Error` values, while `useQuery` and `usePaginatedQuery` still throw into React error boundaries
- the React demo’s error-boundary panel now automatically recovers when the live underlying data stops violating `unique()`

Why this directory exists:

- keep a browser-facing test app in the repo
- exercise Nimbus through the same public HTTP and WebSocket surface real clients use
- make future demos easy to add in a predictable place

Current demos:

- `nimbus/html/`: Vite-based browser playground using `nimbus/rest` SDK for tenant setup, schema install, document inserts, scheduled inserts, and live WebSocket subscriptions
- `firebase/html/`: browser demo using `@nimbus/firebase` against a local Nimbus server
  - exercises `connectFirestoreEmulator`, `addDoc`, `getDocs`, `onSnapshot`, `writeBatch`, `runTransaction`, `deleteDoc`, and the supported `FieldValue` sentinels
  - unary calls can switch between REST and gRPC-Web, while live query updates use the documented WebSocket `Listen` bridge
- `convex/node/`: Convex-style Node demo using generated refs, an injected Node WebSocket implementation, point-in-time reads, and live subscriptions
- `convex/html/`: Convex-style React demo using `convex/react`, generated `_generated/api.ts`, and Nimbus's convex transport
  - the demo now authors functions through `convex/_generated/server`, `convex/values`, and `convex/schema.ts`
  - codegen now emits `_generated/dataModel.d.ts` and `_generated/scheduled_functions.ts` for the supported subset
  - the app exercises live `ctx.db.insert(...)`, delayed `ctx.scheduler.runAfter(...)`, `ctx.db.patch(...)`, `ctx.db.delete(...)`, a runtime-only list query, `ctx.db.query(...).first()`, `ctx.db.query(...).unique()`, `ctx.db.get(id)`, `useQueries`, and `usePaginatedQuery` against a runtime-only `paginatedQuery`
- `mongodb/node/`: MongoDB wire protocol demo using `@nimbus/mongodb` URI helper with the stock `mongodb` driver for CRUD operations
- `convex/http/`: Convex-style browser HTTP demo using `convex/browser` and generated refs without React
  - the demo authors queries with a runtime-only filtered list, compiled `ctx.db.query(...).withIndex(...).filter(...).unique()`, `ctx.db.get(id)`, and a runtime-only multi-step mutation that writes immediately and schedules a follow-up write
  - the composer path now goes through a Convex-style action that delegates to an internal mutation via generated refs, it can also schedule that same internal mutation with `ctx.scheduler.runAfter(...)`, and it includes compiled `httpAction` routes for POST and GET flows

Security note:

- several demos call `POST /api/tenants` from frontend code to create tenants on demand
- this is fine for local development but is a security concern in production
- in production, pre-provision tenants via the admin API or CLI

Browser note:

- native browser `WebSocket` clients cannot send the custom `X-Tenant-Id` header
- the demo uses `GET /ws?tenant_id=...` while non-browser clients can still use the header form
- the convex browser client now reconnects and resubscribes live queries automatically after a dropped socket
- the plain HTML variant at `/demos/convex/html/vanilla.html` is served directly by Nimbus and expects `npm run build --workspace convex` to have generated `/demos/convex/vendor/browser.bundle.js`

Planned next demos:

- `pagination/`: explicit paginated query exercise
- `scheduling/`: schedule and cron workflow demo

Run the Nimbus server:

```bash
cargo run -p nimbus-bin -- start --port 8080
```

Run the Firebase HTML demo:

```bash
npm run firebase:demo:html
```

Run the MongoDB Node demo (requires MongoDB wire protocol listener):

```bash
npm run demo --workspace mongodb-node
```

Run the Convex support server for the React demo:

```bash
npm run convex:server:html
```

Run the Convex support server for the Node demo:

```bash
npm run convex:server:node
```

Run the Convex support server for the HTTP demo:

```bash
npm run convex:server:http
```

Run the Nimbus native HTML demo (Vite dev server):

```bash
npm run nimbus:demo:html
```

Then open:

- <http://localhost:8080/demos/>
- <http://127.0.0.1:5177/> for the Nimbus native HTML demo
- <http://127.0.0.1:5176/> for the Firebase HTML demo

For the React convex demo, in a second terminal run:

```bash
npm run convex:demo:html
```

For the Node convex demo, in a second terminal run:

```bash
npm run convex:demo:node
```

For the HTTP convex demo, in a second terminal run:

```bash
npm run convex:demo:http
```

For the plain HTML variant, build the browser bundle first and then open:

```bash
npm run build --workspace convex
```

- <http://localhost:8080/demos/convex/html/vanilla.html>

Running upstream `convex-demos` against Nimbus:

1. Clone <https://github.com/get-convex/convex-demos> somewhere on your machine.
2. Copy `.env.example` to `.env` and set `CONVEX_DEMOS_DIR` to that clone path.
3. Run `make convex-demo-node`, `make convex-demo-html`, or `make convex-demo-http`.

Those targets build a temporary overlay app that forces `convex/*` imports to
resolve to this repo's workspace packages before running codegen and Nimbus.
