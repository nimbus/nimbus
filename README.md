# neovex

[![CI](https://github.com/agentstation/neovex/actions/workflows/ci.yml/badge.svg)](https://github.com/agentstation/neovex/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/agentstation/neovex/graph/badge.svg)](https://codecov.io/gh/agentstation/neovex)
[![Release](https://img.shields.io/github/v/release/agentstation/neovex)](https://github.com/agentstation/neovex/releases/latest)
[![Homebrew](https://img.shields.io/badge/homebrew-agentstation%2Ftap%2Fneovex-orange)](https://github.com/agentstation/homebrew-tap)

Self-hosted JavaScript backend runtime powered by V8.

## Install

### Homebrew (macOS and Linux)

```bash
brew install agentstation/tap/neovex
```

Homebrew automatically verifies the SHA256 checksum of the downloaded archive.

### Download binary

Download the latest release for your platform from [GitHub Releases](https://github.com/agentstation/neovex/releases/latest).

| Platform | Architecture | Archive |
|----------|-------------|---------|
| Linux | x86_64 | `neovex_linux_x86_64.tar.gz` |
| Linux | ARM64 | `neovex_linux_arm64.tar.gz` |
| macOS | Intel | `neovex_darwin_x86_64.tar.gz` |
| macOS | Apple Silicon | `neovex_darwin_arm64.tar.gz` |
| Windows | x86_64 | `neovex_windows_x86_64.zip` |

```bash
# Example: download and install on macOS Apple Silicon
curl -LO https://github.com/agentstation/neovex/releases/latest/download/neovex_darwin_arm64.tar.gz
tar xzf neovex_darwin_arm64.tar.gz
sudo mv neovex /usr/local/bin/
```

### Build from source

Requires [Rust](https://rustup.rs/) stable toolchain.

```bash
git clone https://github.com/agentstation/neovex.git
cd neovex
cargo install --path crates/neovex-bin
```

## Verify

Every release includes SHA256 checksums and [build provenance attestations](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations/using-artifact-attestations-to-establish-provenance-for-builds) signed via [Sigstore](https://www.sigstore.dev/). These provide cryptographic proof that each binary was built by our GitHub Actions CI from this repository's source code.

### Checksum verification

Each release includes a `checksums-sha256.txt` file:

```bash
# Download the binary and checksums
curl -LO https://github.com/agentstation/neovex/releases/latest/download/neovex_darwin_arm64.tar.gz
curl -LO https://github.com/agentstation/neovex/releases/latest/download/checksums-sha256.txt

# Verify
sha256sum --check --ignore-missing checksums-sha256.txt
```

On macOS, use `shasum -a 256 --check` instead of `sha256sum`.

### Build provenance attestation

Verify that a binary was built by GitHub Actions from this repository:

```bash
gh attestation verify neovex_darwin_arm64.tar.gz --owner agentstation
```

This checks the Sigstore-signed attestation against the [GitHub attestation ledger](https://github.com/agentstation/neovex/attestations). It confirms the exact workflow, commit, and runner that produced the artifact. Requires the [GitHub CLI](https://cli.github.com/).

## Licensing

- source-available under the [Neovex Community License](LICENSE)
- plain-English summary in [LICENSING.md](LICENSING.md)
- commercial terms overview in [COMMERCIAL.md](COMMERCIAL.md)
- contributor policy in [CONTRIBUTING.md](CONTRIBUTING.md)
- optional runtime license loading via `--license-file`, `NEOVEX_LICENSE_FILE`, or `./.neovex/license.json`
- current in-product license status exposed at `GET /debug/license/status`

Workspace notes:

- `crates/neovex` is the facade crate for embedders and external consumers.
- `crates/neovex-bin` is the CLI/binary entrypoint that produces the `neovex` executable.
- `packages/neovex` is the Neovex-native JavaScript SDK surface.
- `packages/convex` is the Convex compatibility package.

The current vertical slice proves:

- explicit tenant creation
- optional per-table schema validation over HTTP
- document insert, update, and delete over HTTP
- explicit query and paginated query endpoints
- backfilled single-field indexes for explicit query paths
- durable scheduled mutations and recurring cron jobs
- scheduled job result lookup by `job_id`
- query subscriptions over WebSocket
- post-commit re-evaluation with automatic push
- startup recovery for claimed-but-unfinished scheduled jobs

Current data-layer features:

- schema CRUD per tenant and per table
- cursor-based paginated queries with opaque cursors
- single-field indexes maintained atomically with writes
- explicit query-path acceleration for indexed equality filters
- explicit query-path range planning for indexed string and number filters
- index-aware subscription evaluation for initial results and re-evaluation
- durable at-least-once scheduled job execution
- persisted scheduled job completion/failure results for observability
- recurring interval-based cron jobs
- schemaless behavior preserved for tables without schema

Core routes:

- `POST /api/tenants`
- `GET /api/tenants`
- `DELETE /api/tenants/{tenant_id}`
- `POST /api/tenants/{tenant_id}/schedule`
- `GET /api/tenants/{tenant_id}/schedule`
- `GET /api/tenants/{tenant_id}/schedule/history/{job_id}`
- `POST /api/tenants/{tenant_id}/crons`
- `GET /api/tenants/{tenant_id}/crons`
- `DELETE /api/tenants/{tenant_id}/crons/{name}`
- `GET /api/tenants/{tenant_id}/schema`
- `PUT /api/tenants/{tenant_id}/schema/{table}`
- `GET /api/tenants/{tenant_id}/schema/{table}`
- `DELETE /api/tenants/{tenant_id}/schema/{table}`
- `GET /api/tenants/{tenant_id}/commits?after=<sequence>`
- `POST /api/tenants/{tenant_id}/documents`
- `GET /api/tenants/{tenant_id}/documents/{table}/{document_id}`
- `PATCH /api/tenants/{tenant_id}/documents/{table}/{document_id}`
- `DELETE /api/tenants/{tenant_id}/documents/{table}/{document_id}`
- `GET /api/tenants/{tenant_id}/documents/{table}`
- `POST /api/tenants/{tenant_id}/query`
- `POST /api/tenants/{tenant_id}/query/paginated`
- `GET /ws` with `X-Tenant-Id` or `?tenant_id=` for browser demos

Demo app:

- `GET /demos/`
- `GET /demos/neovex/html/`

Convex support demos:

- `npm run convex:server:html`
- `npm run convex:demo:html`
- the convex server loads generated functions from `demos/convex/html/.neovex/convex/functions.json`
- the React demo now exercises live `send`, delayed `ctx.scheduler.runAfter(...)`, `patch`, `delete`, `ctx.db.get(id)` detail queries, a live-invalidated `usePaginatedQuery` feed, `useQueries`, and query-error recovery through a React error boundary
- `npm run convex:server:http`
- `npm run convex:demo:http`
- the HTTP demo loads generated functions from `demos/convex/http/.neovex/convex/functions.json`
- the HTTP demo now submits through a Convex-style action that delegates to an internal mutation, schedules that internal mutation with `ctx.scheduler.runAfter(...)`, loads single documents through compiled `ctx.db.get(id)`, shows exact indexed lookups via mixed `withIndex(...).filter(...).unique()`, and exercises compiled `httpAction` routes through `httpRouter`

Convex note:

- Neovex now includes an in-repo `convex` Convex support package plus generated named-function manifests for a supported 4B subset.
- Current Convex support is still partial: generated `convex/_generated/api`, generated `convex/_generated/server`, generated `convex/_generated/dataModel.d.ts`, generated `convex/_generated/scheduled_functions.ts`, `convex/react` hooks, `convex/browser`, `convex/server` wrappers, `convex/values`, declarative `convex/schema.ts`, compiled `ctx.db.query(...).withIndex(...).order(...).collect()/take()/first()/unique()` queries, compiled `ctx.db.query(...).filter(...).collect()/take()/first()/unique()` queries, compiled `ctx.db.get(id)` reads, compiled `ctx.db.insert(...)`, `ctx.db.patch(...)`, and `ctx.db.delete(...)` mutations, compiled `httpAction` routes through `httpRouter`, named HTTP calls, handler-side scheduled mutation commands, and named live query subscriptions work for the supported declarative subset.
- Mixed indexed queries such as `ctx.db.query(...).withIndex(...).filter(...).unique()` now compile and execute for exact-match lookups with residual filters.
- The `convex/browser` and `convex/react` convex clients now automatically reconnect and resubscribe live queries after a dropped WebSocket connection.
- The `convex/react` hooks now mask stale values and stale errors when query args change or switch to `"skip"`, which brings loading and boundary behavior closer to real Convex hook semantics.
- Named `paginatedQuery` refs now participate in the live WebSocket subscription path, and `usePaginatedQuery` refreshes the currently loaded window when those subscriptions invalidate.
- `useQueries` keeps query failures local as `Error` values instead of throwing, while `useQuery` and `usePaginatedQuery` still throw into React error boundaries.
- Compiled `paginatedQuery` handlers can now return `ctx.db.query(...)` builders directly instead of needing a `.collect()` workaround.
- The convex browser/react client now suppresses unchanged subscription payloads, which reduces extra rerenders when Neovex’s table-level invalidation re-evaluates to the same result.
- Reconnect/resubscribe now also suppresses an unchanged initial replay result, so apps do not rerender just because the socket dropped and immediately came back with the same data.
- The `useMutation` and `useAction` hooks now keep stable callable identities while always dispatching through the latest client and generated ref.
- Generated `convex/_generated/api.ts` refs now preserve typed args and the common compiled result shapes, so the demo apps can rely on inferred `Doc<...>`, `Id<...>`, and paginated item types instead of local casts.
- Generated `convex/_generated/scheduled_functions.ts` refs now preserve typed args and return shapes too, so `ctx.scheduler.runAfter/runAt` can use the same typed mutation references as the rest of the convex surface.
- Scheduled convex mutations now deduplicate crash-replayed execution by scheduled job id, so recovered running jobs do not double-apply their write path.
- Generated action refs now infer common delegated return shapes as well, so compiled `ctx.runMutation(...)`, `ctx.runQuery(...)`, and `ctx.runAction(...)` flows do not need explicit `returns` in many cases.
- Compiled `patch` and `delete` currently require id arguments declared with `v.id("table")` so the compiler can keep the Convex call shape while still lowering to Neovex mutation plans.
- Action handlers now support compiled `ctx.runQuery(...)`, `ctx.runMutation(...)`, and `ctx.runAction(...)` when they target generated refs from `convex/_generated/api`.
- Public vs internal function visibility is now preserved in generated refs, and public convex endpoints reject internal functions.
- Neovex now also includes a new `crates/neovex-runtime` crate that
boots a V8 runtime through `deno_core`, loads a bundled ESM entrypoint, and
bridges Rust host calls for the first 4C slice.
- `packages/codegen` now emits `.neovex/convex/bundle.mjs` as the
runtime handoff artifact for that new runtime crate, while
`packages/convex` exposes the Convex-compatible CLI and client surface.
- `packages/codegen` now also emits `.neovex/convex/bundle.sha256`,
and the convex runtime re-checks that hash before every bundle invocation.
- When that bundle is present, named convex HTTP query, paginated query,
mutation, and action calls now execute through the runtime bundle path before
delegating into Rust-owned declarative execution.
- Named live query subscriptions now re-evaluate through that runtime bundle
path too.
- The first runtime `ctx.db` host-binding slice is now in place for compiled
read paths, so generated query and paginated-query handlers can execute
through dedicated runtime host operations instead of the coarse
`convex.invoke` path.
- Generated mutation and action handlers can now execute through direct runtime
host operations too, and compiled scheduled commands can flow through that
same runtime mutation/action path.
- Convex `httpAction` routes can now execute through that runtime bundle path
too when a named route manifest entry and bundle are present.
- Named paginated live subscriptions can now re-evaluate through the runtime
bundle path too when the convex client supplies the subscribed window size.
- The runtime bootstrap now exposes a shared JS-level
`globalThis.__neovexCreateContext()` API with `ctx.db`,
`ctx.scheduler`, and `ctx.run*` helpers, and generated bundles now execute
through that shared surface instead of only generated direct host-call glue.
- Convex-compatible runtime code should keep using `ctx.auth.getUserIdentity()`.
Neovex also now exposes a richer runtime-only extension,
`ctx.auth.getVerifiedIdentity()`, which returns normalized verified auth
fields plus the auth-provider kind for Neovex-native code without changing
the Convex compatibility contract.
- `neovex/server` now types that richer extension natively for Neovex apps,
while `convex/server` stays aligned to Convex’s compatibility surface.
- The runtime sandbox now enforces per-isolate heap limits, wall-clock
execution timeouts, bundle-root-only module imports, `globalThis.Deno`
removal after bootstrap, and a shared top-level isolate concurrency cap.
- Async runtime bundle handlers are now awaited correctly by the V8 runtime.
- A first broader arbitrary-runtime slice is now working too: codegen can emit
runtime-only named query/mutation/action handlers when 4B lowering is not
possible, and those handlers execute through the bundle path.
- Runtime-only named paginated-query handlers now work through the same bundle
path when they return a live `ctx.db.query(...)` builder, backed by a
runtime query-builder paginate host op.
- Runtime handlers can now compose through `ctx.runQuery`,
`ctx.runMutation`, and `ctx.runAction` without dropping out of the bundle
path, so runtime-only callees can execute through the same runtime bridge.
- Generated runtime bundles now expose a local named-function dispatcher too,
so nested `ctx.run*` calls prefer same-isolate execution instead of spinning
up a fresh nested runtime when the callee is available in the current bundle.
- Nested runtime composition now also has a per-request invocation budget, so
one request cannot fan out unbounded internal runtime executions.
- The follow-on runtime executor/async bridge work is now materially in place:
a shared local-thread runtime worker pool backs async HTTP/WS runtime usage,
typed async host ops cover read/write/scheduler and `ctx.run*` fallback
paths, and request-scoped cancellation now propagates through the worker
queue and cooperative query evaluation instead of stopping only at isolate
termination.
- Generated/runtime normal execution now also uses dedicated typed sync ops for
query-builder setup, same-isolate nested-call entry, and convex `httpAction`
route dispatch, so the old raw host op is no longer part of the normal
generated bundle path.
- A small read-only runtime diagnostics endpoint now exists at
`/debug/runtime/metrics`, exposing the active runtime limits plus live
executor/runtime counters such as worker dispatches, queue depth,
cancellations, same-isolate nested dispatches, and cross-isolate fallback
dispatches.
- Runtime-only named queries can now bootstrap live convex subscriptions by
taking an initial runtime read trace and synthesizing one conservative
Neovex base subscription query per traced table.
- Runtime-only named paginated queries can now bootstrap live convex
paginated subscriptions the same way, including multi-table traced reads.
- The first runtime read-set and narrower-invalidation slice is now working:
runtime-backed document reads such as `ctx.db.get(...)` can suppress
unrelated same-table writes, and runtime query subscriptions now track
returned document ids so indexed `ctx.db.query(...).withIndex(...)` results
still re-run when a previously returned row leaves the result set. Filtered
runtime queries can now also skip obvious non-matching inserts/updates, and
transactional delete snapshots let them stay quiet for unrelated deletes,
even without an explicit index-range hit. Ordered paginated runtime
subscriptions now track visible-window boundaries too, and ordered limited
runtime queries such as `take(...)` / `first()` / `unique()` now reuse the
same boundary model, so matching writes above or below the visible window
can stay quiet instead of forcing a refresh. Runtime subscription planning
now also keeps disjoint same-table reads separate instead of collapsing them
back to one broad table subscription.
- Phase 4C is now complete against its success criteria: runtime-backed V8
handler execution is in place, Neovex now has materially narrower
Convex-style read tracking than plain table-level invalidation, and the
in-repo convex apps run against the Convex-shaped surface without app-source
rewrites.
- Further Convex support polish beyond Phase 4 can still deepen mixed
multi-table range/window invalidation and broaden arbitrary runtime handler
coverage.

Runtime flags:

- `--runtime-heap-mb`
- `--runtime-initial-heap-mb`
- `--runtime-timeout-secs`
- `--runtime-max-isolates`
- `--runtime-max-nested-calls`

