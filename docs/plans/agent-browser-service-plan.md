# Plan: Agent Browser Service

Canonical deferred design and execution plan for adding a first-class
**browser session** resource to Nimbus that agent workloads can consume
ergonomically through the runtime host bridge.

This document owns the durable forward-looking context for the
`BrowserService`, the `BrowserProvider` trait, the `ctx.browser` host
op, browser-session storage state in `nimbus-storage`, and the
sandbox-tier supervision of Chrome/Brave behind `nimbus-sandbox`.

Paired research: `docs/plans/research/agent-browser-service-prior-art.md`.

---

## Status

- **Status:** `deferred`
- **Primary owner:** this plan
- **Activation gate:** all three must hold before promoting from
  `deferred` to active:
  1. Product direction commits to a real browser as a Nimbus function
     capability (not just `fetch()`). The loudest consumer is agent
     workloads, but the capability is not agent-restricted — any
     function may opt in. This gate is *independent* of the
     `wasi-agent-capabilities-plan.md` activation gate; the two plans
     are siblings, not parent/child (see Relationship To Other Plans).
  2. A consumer is named — at least one demo or partner workload that
     needs a real browser.
  3. The Chrome-vs-Brave default and the Playwright-compatible-API
     question (see Open Decisions) are resolved.

## How To Use This Plan

- Read this before starting any browser-supervision or agent-browser
  capability work.
- Treat it as the canonical control plane for the agent BrowserService
  workstream once promoted.
- Do not start implementation until the activation gate is met.
- When promoted, implement exactly one phase at a time and record
  verification in the Execution Log before marking a phase `done`.
- For background and architectural reasoning, read the research note
  at `docs/plans/research/agent-browser-service-prior-art.md` first.

## Control Plan Rules

This document is the durable control plane for the agent BrowserService
workstream. The source of truth is:

1. the current git worktree
2. this plan's `Phase Status Ledger`, `Implementation Checkpoints`, and
   `Execution Log`
3. `ARCHITECTURE.md` for the landed runtime architecture
4. `docs/architecture/sandbox/microvm-service-baseline.md` for the
   sandbox tier this plan integrates with
5. `docs/plans/research/agent-browser-service-prior-art.md` for the
   north-star research and prior-art survey

Do not rely on prior chat transcripts as progress state.

### Status model

- `todo`: not started; eligible when hard dependencies and gate notes are
  satisfied
- `in_progress`: actively being implemented; keep exactly one phase in this
  state per autonomous execution run
- `blocked`: cannot proceed until the recorded blocker is resolved
- `done`: acceptance criteria are met and verification has been recorded
- `deferred`: intentionally parked behind a product or benchmarking gate

### Recovery loop for every new session

1. Reread this `Control Plan Rules` section, `Phase Status Ledger`,
   `Implementation Checkpoints`, `Phase Order and Dependencies`, and
   `Execution Log`.
2. Inspect the current git worktree and reconcile it against this plan
   before picking new scope.
3. If any phase is already `in_progress`, resume that phase first.
4. If the worktree is dirty, identify which phase owns the changes and
   update that phase's checkpoint or log entry before starting new work.
5. Implement exactly one phase by default.
6. Record verification in `Execution Log` before marking a phase `done`.
7. If blocked, record the blocker here before stopping.

---

## Why This Plan Exists

Agents inside `nimbus-runtime` cannot today do real browser work. They
can call `fetch()` for static HTML/JSON, but anything JS-rendered,
behind login, or requiring interaction is out of reach. The three
viable workarounds are all wrong for Nimbus:

| Workaround | Why it's wrong |
|---|---|
| Ad-hoc `chrome` subprocess spawned by user code | Bypasses sandbox + audit. Agents step on each other. Cold start every call. |
| External managed service (Browserbase, Browserless, Cloudflare) | Sits outside Nimbus's trust boundary. Per-session-minute cost. Network dependency. |
| `fetch()` plus DOM parsing | Fails on every modern web app. |

Adding a BrowserService inside Nimbus gives agents a real browser as a
first-class resource — addressable as `ctx.browser`, audited through
the engine mutation journal, sandboxed behind `nimbus-sandbox`,
persisted in `nimbus-storage`. Same trust and observability story as
`ctx.db`.

This is additive. Functions that do not import the browser capability
never see it. The default tenant trust posture is "no browser" — the
capability is opt-in per tenant, identical in shape to the agent
capabilities admission gate in `docs/plans/wasi-agent-capabilities-plan.md`.

## Architecture Boundary

### What this plan owns

- A new `nimbus-browser` crate sitting between `nimbus-engine` and
  `nimbus-sandbox`.
- The `BrowserProvider` trait and at least one concrete provider
  (`SubprocessChromeProvider`).
- The session registry, BrowserContext multiplexing, warm pool, and
  recycle policy.
- The `ctx.browser` host bridge op family.
- The storage state schema for durable named sessions in
  `nimbus-storage`.
- The browser-tenant admission flag and ACL on session ownership.
- The browser-op extension to the mutation journal.
- The JS facade in `packages/nimbus/` (and a thin Convex-compat shim if
  needed in `packages/convex/`).

### What this plan does NOT own

- The `nimbus-sandbox` boundary itself — it is consumed, not changed.
  If the BrowserService needs a sandbox capability that does not exist
  (e.g., GPU passthrough), open a separate plan against the sandbox
  layer.
- The `nimbus-runtime` V8 surface — only the new host bridge op is
  added; no new V8 features.
- The agent DOM extraction format itself — this plan emits a stable
  serialised form, but agent loops that consume it (a Browser-Use-like
  library) are user space.
- LLM integration. No model-aware code in `nimbus-browser`.
- Anti-fingerprint / CAPTCHA / residential-proxy work beyond plain
  proxy URL support. Scaffolded as policy hooks; not implemented.

## Reference Implementations

See `docs/plans/research/agent-browser-service-prior-art.md` for the
full survey. Short summary of what each contributes here:

| System | What this plan borrows |
|---|---|
| Browserless v2 | Single-Chrome-many-BrowserContexts production proof. CDP WebSocket proxying as a workable seam. |
| Browserbase + Stagehand | Stateful named sessions as the user-facing primitive. Storage-state durability. Session video replay as an audit precedent. |
| Cloudflare Browser Rendering (containers retrospective) | Runtime-binding ergonomics (`ctx.browser` shape). Warm pool to hide cold start. Composite "Quick Actions" host ops to collapse round-trips. **Negative lesson:** eventually-consistent KV is wrong for hot-path session claiming; use a strongly-consistent path. **Negative lesson:** the right isolation unit is the *trust boundary*, not a fixed level of the browser hierarchy. |
| Steel.dev | Open-source single-binary supervisor pattern. Multi-protocol surface. |
| Playwright | `BrowserContext` abstraction. `storageState` serialisation format. |
| Chrome DevTools Protocol | The wire protocol the provider speaks. |

## Proposed Internal Shape

### Crate layout

```text
crates/nimbus-browser/
  src/
    lib.rs              // BrowserService entry, public types
    provider.rs         // BrowserProvider trait
    provider/
      subprocess.rs     // SubprocessChromeProvider
      pool.rs           // warm-pool wrapper around any provider
    session.rs          // SessionId, SessionHandle, registry
    context.rs          // BrowserContext multiplexing over CDP
    storage_state.rs    // Playwright-compatible storage state schema
    extractor.rs        // a11y tree + cleaned DOM extraction
    cdp/
      client.rs         // minimal CDP client (or wrapper around chromiumoxide)
    bridge.rs           // host bridge op definitions
```

`nimbus-browser` may depend on `nimbus-core`, `nimbus-storage`, and
`nimbus-sandbox`. It must not depend on `nimbus-runtime` (runtime
crosses the bridge to it, not the other way).

### BrowserProvider trait

```text
BrowserProvider (trait, Send + Sync + 'static)
  - launch(opts: LaunchOpts) -> Result<BrowserHandle>
  - new_context(browser: &BrowserHandle, opts: ContextOpts)
        -> Result<ContextHandle>
  - close_context(ContextHandle) -> Result<StorageState>
  - new_page(ContextHandle) -> Result<PageHandle>
  - goto(PageHandle, url, opts) -> Result<NavigationResult>
  - eval(PageHandle, script) -> Result<JsonValue>
  - click / type / select / scroll  (CDP Input domain wrappers)
  - extract_a11y(PageHandle) -> Result<A11yTree>
  - screenshot(PageHandle, opts) -> Result<Vec<u8>>
  - close_browser(BrowserHandle) -> Result<()>
```

All ops receive `tenant_id` + `session_id` for scoping. Trait is
`Send + Sync` for sharing across engine workers.

### Concrete providers (delivery order)

| Provider | Backing | Phase |
|---|---|---|
| `SubprocessChromeProvider` | Local `chrome` or `chromium` subprocess on the host, no sandbox | B2 — dev/test |
| `SandboxedBrowserProvider` | Chrome inside `nimbus-sandbox` (krun microVM on Linux, Virtualization.framework on macOS) | B5 — production |
| `BraveProvider` *(optional)* | Same as sandboxed but with Brave binary; gated on a per-session policy flag | not in initial scope |

The provider is selected per-tenant or globally via configuration. The
host bridge contract is identical regardless of which provider is
active.

### `ctx.browser` host bridge surface

Sketch only. Exact API resolves at B3.

```text
ctx.browser.session(name: string, opts?: SessionOpts) -> Session

// Primitive ops — one host-bridge crossing each, used for fine-grained agent loops
Session:
  goto(url, opts?) -> NavigationResult
  click(selector | a11y_id, opts?) -> void
  type(selector | a11y_id, text, opts?) -> void
  eval<T>(fn | script) -> T
  extract(opts?: { a11y?, html?, screenshot? }) -> View
  download(url, opts?) -> StorageRef
  commit() -> void          // snapshot storage state to nimbus-storage
  close() -> void           // close context; final snapshot
  pages: PageHandle[]

// Composite ops — single host-bridge crossing for canonical multi-step flows
// (Cloudflare's "Quick Actions" lesson: round-trips dominate latency)
ctx.browser.snapshot(url, opts?) -> View          // launch+goto+extract+close
ctx.browser.screenshot(url, opts?) -> bytes       // launch+goto+screenshot+close
ctx.browser.pdf(url, opts?) -> bytes              // launch+goto+pdf+close
ctx.browser.run(session, steps[], opts?) -> R     // batched step list, one crossing
```

Composite ops are not sugar — they are the fast path. An agent that issues
`goto`/`click`/`extract` as three separate calls incurs three host-bridge
crossings; the same flow through `run` executes inside `BrowserService` with
one. Provide both; primitives for interactive agent loops, composites for
canonical flows.

`ctx.browser.session("research")` is idempotent — first call creates,
subsequent calls in the same or future invocations resume the same
storage state. Sessions are tenant-scoped; cross-tenant session
references error.

### Engine mutation-journal integration

Browser ops flow through the existing single mutation path. A new
mutation variant `BrowserOp { session_id, op, args_hash, result_hash,
ts }` records what happened without storing the full DOM (which lives
in the extractor cache). Storage-state checkpoints are first-class
mutations so they replay deterministically.

### Storage state schema

```text
table _nimbus.browser_sessions {
  session_id:       SessionId    (PK)
  tenant_id:        TenantId
  display_name:     string
  storage_state:    bytes        // Playwright-compatible JSON, zstd-compressed
  context_options:  json         // viewport, ua, locale, timezone, proxy
  acl:              json         // who/what can open this session
  created_ms:       u64
  last_used_ms:     u64
  last_snapshot_ms: u64
}

table _nimbus.browser_extraction_cache {
  cache_key:   bytes  (PK)       // hash(session_id, page_url, dom_revision)
  payload:     bytes             // serialised a11y tree or screenshot
  created_ms:  u64
  ttl_ms:      u64
}
```

Both tables live under the `_nimbus` system tenant, not the user's
tenant — they are infrastructure, not user data.

### Tenant admission

```text
TenantCapabilities (extended):
  browser_enabled: bool         // gate: tenant may use ctx.browser at all
  browser_proxy:   Option<Url>  // forced proxy, if any
  browser_quota:   BrowserQuota // max concurrent sessions, max storage, etc.
```

A tenant without `browser_enabled` calling `ctx.browser.*` gets a
capability-denied error, not a runtime panic. This matches the
`nimbus:agent` admission model exactly.

### Sandbox integration

The `SandboxedBrowserProvider` consumes the sandbox layer's existing
"run a process inside an isolated VM" surface. The browser is
*supervised by* `nimbus-browser` but *runs inside* a sandbox unit. CDP
traffic crosses the boundary over loopback or vsock; storage state
materialises into the sandbox unit at context-create time and flushes
back on close.

Open question: one sandbox VM per Nimbus deployment hosting many
Chromes, or one VM per Chrome. Resolved at the sandbox integration
phase (B5) based on memory and security tradeoffs measured then.

### Isolation rule: one Chrome per trust unit

Cloudflare's container retrospective (`https://blog.cloudflare.com/browser-run-containers/`)
runs **one browser per user** ("once we assign a browser to a user, it's
exclusively theirs"). Browserless and Steel run **one Chrome per
container, many BrowserContexts inside**. Both are correct — for their
respective trust boundaries. The general rule:

> **One Chrome process per trust unit. Many BrowserContexts within for
> the workloads inside that trust unit.**

For Nimbus the trust unit is the **tenant**:

- Tenant A and Tenant B must never share a Chrome process. A Chrome
  0-day breaks past BrowserContext but not past the sandbox VM, and a
  shared Chrome would let one tenant's compromise reach another's
  storage state, cookies, and in-flight tabs.
- Two of Tenant A's agents *do* share a Chrome process. They are
  mutually trusted (same admin, same data). BrowserContext isolation is
  sufficient; the cost of a dedicated Chrome per session is not
  warranted.
- Inside one tenant, exceeding a per-Chrome context cap (default soft
  cap: 32 contexts; tuned at B7) starts a second Chrome in the same
  tenant's sandbox VM.

This is more isolated than Browserless's stance and less isolated than
Cloudflare's, because Nimbus's trust boundary sits in between theirs.

### Why this plan does not need a KV or Durable-Object equivalent

Cloudflare's architecture needed a strongly-consistent shared store
because their browser fleet runs across many independent Workers and
many container hosts. They tried Workers KV, hit a 30-second
eventual-consistency window that caused double-claim races on the hot
path, and moved authoritative claim state into Durable Objects (one DO
per browser-container pair, single-writer, serializable).

Nimbus does not have either problem at the layer this plan operates on:

| Cloudflare's need | Cloudflare's mechanism | Nimbus equivalent |
|---|---|---|
| Strongly-consistent claim authority | Durable Object as serializable single-writer | The engine's mutation path is already serialized. `SessionRegistry::claim` is an ordinary mutation. |
| Live coordinator process per session | DO instance holding the WebSocket | `SessionHandle` struct inside `BrowserService` holding the CDP client. Same process as the engine. |
| Affinity routing for stateful sessions | Workers routes the same DO ID to the same DO instance | No routing problem — single Nimbus process; the request lands wherever the engine lives. |
| Persistent session metadata + storage state | D1 (after KV was abandoned) | `nimbus-storage` tables `_nimbus.browser_sessions` and `_nimbus.browser_extraction_cache`. |

If Nimbus later runs multi-node (see
`docs/architecture/horizontal-scaling.md`), the engine itself acquires
a sharding/leader story; BrowserService inherits whatever the engine
inherits. No browser-specific KV or DO is introduced by this plan in
either case.

### Horizontal-scaling and cross-node state

The MVP targets single-node Nimbus. In the multi-node future committed
by `docs/architecture/horizontal-scaling.md` (iroh QUIC mesh + iroh-gossip
+ iroh-blobs + openraft), a browser session is *single-writer state with
node affinity*: a `BrowserContext` cannot have two CDP clients writing
to it simultaneously without corruption. Three cross-node coordination
problems appear; each is solved by an iroh-stack primitive that already
exists in the horizontal-scaling architecture, **no new primitives in
this plan**.

| Cross-node problem | Cloudflare's answer (DO + KV→D1) | Nimbus's answer (already in `horizontal-scaling.md`) |
|---|---|---|
| Single-writer claim — which node owns this session? | DO per browser-container pair, serializable single-writer (after KV's 30-second eventual-consistency window failed) | openraft-replicated session-registry table. `(tenant_id, session_id) → node_id` is a Raft-committed mapping; one truth per cluster, no race. |
| Affinity routing — how does another node reach the session? | Workers routes a DO ID to the same DO instance | Engine reads the Raft mapping; iroh full-mesh QUIC delivers the request to the owner node directly (full mesh holds under 20 nodes per `horizontal-scaling.md` §3). |
| Durability + migration — how does the session survive node failure? | DO + D1 replicate the small bits; container is replaceable | iroh-blobs for the Playwright storage-state blob (BLAKE3-verified, P2P, content-addressed). On node death, Raft reassigns ownership; the new owner pulls the latest storage-state blob over iroh-blobs and hydrates a fresh `BrowserContext`. |
| Cross-node CDP traffic (last resort) | DO holds the WebSocket; same node sees all CDP | Dedicated iroh QUIC stream per forwarded session. CDP is itself a multiplexed protocol — it must ride its own iroh stream, **never wrapped inside another framed RPC channel**. |

#### Routing-over-proxying rule

> **Prefer routing the request to the owner node. Proxy CDP across nodes
> only when routing is impossible.**

Cloudflare's SASE-mode QUIC retrospective
(`https://blog.cloudflare.com/faster-sase-proxy-mode-quic/`) documents
the cost of tunneling stream-multiplexed traffic through another framed
stream: their original L4-stream → L3-WireGuard → L4-stream conversion
imposed enough head-of-line blocking and conversion tax that switching
to native QUIC streams doubled throughput. CDP is structurally similar
(multiplexed target/session/method/id messages). Two consequences for
us:

1. When an HTTP/WebSocket request lands on a non-owner node, the engine
   redirects the *user request* to the owner node (a single
   redirect/forward in `nimbus-server`), not the underlying CDP traffic.
   The user-facing request crosses iroh once; CDP never crosses the
   cluster boundary.
2. When forwarding *is* necessary (e.g., a long-running attach), the
   forwarded CDP traffic uses its own iroh bidirectional QUIC stream,
   not a multiplexed JSON-RPC wrapper inside one of the engine's
   existing channels. iroh's stream-per-logical-channel model is
   load-bearing for performance and must be honored.

#### What this plan adds for the multi-node path

Nothing in the V8/single-node MVP. When the cluster substrate lands, the
session registry gains:

- A Raft-committed entry per `(tenant_id, session_id) → node_id`.
- A storage-state replication policy: `_nimbus.browser_sessions` rows
  are openraft-replicated metadata; the storage-state blob itself rides
  iroh-blobs with the row holding the BLAKE3 hash.
- A snapshot trigger policy: the storage-state blob is rewritten on
  session checkpoint — explicit `Session.checkpoint()` calls, session
  close, idle-timeout, and after a configurable number of CDP-writes
  since the last snapshot (default: every 30 seconds of activity, or
  100 mutations, whichever fires first). Per-CDP-write snapshotting
  would dominate the cluster; never-snapshotting loses too much state
  on node death. The threshold lives in `BrowserService` config, not
  in function code.
- A reassignment handler: on node loss, the new owner pulls the blob
  and creates a fresh `BrowserContext` from the latest snapshot. In-
  flight requests at the moment of failure error out — sessions are
  durable across failures but individual CDP commands are not.

These extensions are additive and live behind the same `BrowserService`
API. Function code calling `ctx.browser.session("research")` is
unchanged.

#### Gossip and Raft topic conventions

Cluster-state signals use the canonical topic naming from
`horizontal-scaling.md` §3. Browser-session-specific topics:

- `topic:<tenant_id>:browser_sessions` — session-registry change
  signals (a session was claimed, released, reassigned). The durable
  mapping rides openraft; gossip carries only invalidation.
- `topic:cluster:state` — node capacity for browser placement (engine-
  wide; no browser-specific channel).

CDP traffic does **not** ride gossip. Per the routing-over-proxying
rule above, cross-node CDP rides a dedicated iroh QUIC stream.

#### Multi-Raft forward note

`horizontal-scaling.md` Open Question 1 calls out that at 10+ nodes
the cluster partitions tenants into separate Raft groups. The
session-registry table is tenant-scoped (the row PK starts with
`tenant_id`), so each session naturally migrates into its tenant's
Raft group when partitioning lands; no plan-level change is required.
The cross-tenant invariant ("two nodes cannot simultaneously hold
live handles for the same `session_id`") survives the migration
because session IDs are tenant-scoped — there is no global
session-ID namespace.

### Secret-management dependency

A durable browser session is in part an *identity* — it holds cookies
that authenticate it as a logged-in user. The plan exposes three
classes of secret-handling, only two of which are safe to keep MVP-
internal:

| Class | Example | MVP-safe? |
|---|---|---|
| **In-storage-state credentials** | Login cookies, OAuth refresh tokens, CSRF tokens that Chrome itself sets and reads | Yes — these live inside the encrypted `_nimbus.browser_sessions.storage_state` blob, never leave Chrome, and are wrapped by the existing KMS DEK envelope from `docs/architecture/storage/encryption.md`. |
| **Per-session policy credentials** | Proxy auth (`http://user:pass@proxy:8080`), TLS client cert/key, CAPTCHA-service API key, residential-proxy auth | **No.** These are set on `BrowserContext` at create time and must come from somewhere. Caller-supplied literal strings would leak them into journal entries and source code. |
| **Function-level secrets the agent reads at runtime** | `ctx.secret.get("openai_api_key")` to call an LLM mid-flow | **No.** Out of scope for this plan but the same gap. |

Nimbus does not yet have a tenant-scoped secret-store API. The
`secret` permission grant in
`docs/architecture/runtime/permission-model.md` is explicitly an
audit/declaration placeholder until a future plan delivers
materialization. Quoting the permission model verbatim:

> "Secret and identity grants are declaration and audit inputs until a
> future secret-store or service-identity API exists."

The execution plan for that materialisation lives at
`docs/plans/secret-management-plan.md`, with prior-art research at
`docs/plans/research/secret-management-prior-art.md`. The browser plan
does *not* block on the full secret-management plan landing — but it
does require two things:

1. **MVP**: per-session policy credentials may be supplied via the
   tenant's secret-grant-allowlisted environment indirection
   (`ctx.env.get("BROWSER_PROXY_AUTH")` returns from a tenant-scoped
   table that is at least KMS-encrypted at rest), and must never be
   journaled, never logged, never returned to the function once set on
   a context.
2. **Once a real secret store lands**: migrate per-session policy
   credentials to first-class secret references
   (`secret_ref: "proxy/research"`) and drop the env-indirection
   workaround. This is a clean breaking change consistent with the repo
   pre-launch policy in `AGENTS.md`.

## Required Invariants

- The browser capability is strictly additive. Tenants without
  `browser_enabled` must never reach a `ctx.browser` code path.
- Cross-tenant session access must be impossible — session IDs are
  unique per tenant and the engine rejects cross-tenant references at
  the bridge boundary.
- The default production provider must be the sandboxed one. The
  subprocess provider is dev/test only and must refuse to start when
  `NIMBUS_ENV=production`.
- Storage state must be encrypted at rest with the same key material as
  other `nimbus-storage` blobs.
- Browser ops must journal through the same mutation path as DB writes.
  No side-channel writes to the cache or storage-state tables.
- A session's storage state must be writable by exactly one running
  context at a time. Concurrent opens of the same `session_id` from
  different invocations queue.
- Cold-start latency for a warm-pool session must be under 200 ms once
  the pool is steady-state. Cold-pool latency must be under 3 s.
- The provider must support graceful Chrome restart without losing the
  storage state of in-flight sessions (hydrate-from-snapshot on
  recovery).
- Network egress from the browser must be subject to the tenant's
  outbound HTTP policy, identical to the policy enforced on the
  `nimbus:agent/http-client` capability.
- Per-session policy credentials (proxy auth, client certs, CAPTCHA
  keys) must never appear in journal entries, log lines, extraction
  cache payloads, or any function-readable return value. The function
  supplies a *reference* to a credential; the BrowserService is the
  only component that resolves it.
- Cross-node CDP traffic, when forwarding is unavoidable, must ride a
  dedicated iroh QUIC stream. Wrapping CDP inside another framed RPC
  channel is forbidden — see the Cloudflare SASE-mode-QUIC
  retrospective and the routing-over-proxying rule above.
- Session ownership at multi-node scale is a Raft-committed mapping.
  Two nodes simultaneously holding live `BrowserContext` handles for
  the same `session_id` is a correctness violation, not an
  optimisation opportunity.

## Open Decisions

These must resolve before promoting this plan from `deferred`:

1. **Default browser binary:** Chrome or Brave? Default position:
   Chrome for the MVP; Brave as a per-session policy override later.
2. **Wire-protocol fidelity:** Playwright-compatible at the host
   surface, or Nimbus-native? Default position: Nimbus-native
   `ctx.browser` shape, with the *provider* speaking CDP under the
   hood. A Playwright-compatible facade can layer on top later.
3. **Per-Chrome sandbox unit vs shared:** decided at B5 with
   measurements, not in advance.
4. **Extractor ordering:** a11y tree first (cheap, broadly sufficient)
   then set-of-marks screenshot? Default position: yes — a11y first.
5. **Crate location:** confirm `crates/nimbus-browser/` rather than
   folding into `nimbus-engine` or `nimbus-sandbox`. Default position:
   own crate; the surface is large enough.
6. **Capability namespace:** `ctx.browser` vs `ctx.agent.browser` vs a
   separate `nimbus:browser` WIT package. Default position:
   `ctx.browser` for the V8 surface; reserve `nimbus:browser` WIT
   package for the wasmtime path.

## Phase Status Ledger

| Phase | Status | Summary | Hard Dependencies | Gate Note |
|-------|--------|---------|-------------------|-----------|
| B0 | `todo` | Decision gate — resolve every item in `Open Decisions` and record outcomes in this plan before any code lands | activation gate met | this is a no-code phase; output is a decision log appended below |
| B1 | `todo` | Scaffolding — `crates/nimbus-browser/` crate with `BrowserProvider` trait, types, no provider impl | B0 | crate compiles, trait shape reviewed against the prior-art research |
| B2 | `todo` | `SubprocessChromeProvider` — local Chrome subprocess, CDP client, single BrowserContext per call, no pool, no sandbox | B1 | host-only dev provider; refuses to start in production; smoke test: launch Chrome, open page, extract title |
| B3 | `todo` | Session registry + BrowserContext multiplexing — named sessions, context per session, ephemeral storage state, tenant scoping enforced at the bridge | B2 | two concurrent sessions in one Chrome with cookie isolation verified; tenant cross-access rejected |
| B4 | `todo` | `ctx.browser` host bridge op + JS facade — `Session`, `goto`, `click`, `type`, `eval`, `extract` exposed to `nimbus-runtime`; admission gated on `browser_enabled` | B3 | demo agent reaches a page and extracts text; tenant without capability errors at deploy/invocation time |
| B5 | `todo` | `SandboxedBrowserProvider` — Chrome behind `nimbus-sandbox` (krun on Linux, VZ.framework on macOS); CDP over vsock or loopback; per-Chrome sandbox-unit policy decided here with measurements | B4 | sandboxed provider runs the same smoke and tenant tests as B2/B3; isolation verified by attempt to read host fs from inside Chrome |
| B6 | `todo` | Durable storage state — snapshot on close, periodic snapshot, `commit()` host op; hydration on session open; encrypted at rest | B3 | named session survives Chrome restart and Nimbus restart with cookies intact |
| B7 | `todo` | Warm pool + recycle policy — pre-launched Chromes (and/or pre-created BrowserContexts) sized by `BrowserQuota`; recycle on context close or N pages or M minutes | B6 | warm-pool latency under 200 ms steady-state proven via bench |
| B8 | `todo` | Extraction cache — a11y tree extractor first, cleaned-DOM extractor next; cache keyed by `(session_id, page_url, dom_revision)`; serialised LLM-friendly format | B4 | cache hit on repeated extract of the same page; format documented |
| B9 | `todo` | Mutation-journal integration + ACL — browser ops as `BrowserOp` journal entries; session ACL per tenant; replay-safe ordering | B4, B6 | journal replay reproduces session lifecycle; ACL enforced at the bridge |
| B10 | `todo` | Network policy hooks — proxy per session, outbound URL allowlist/denylist sharing the agent http-client policy machinery | B5 | denylist violations blocked at the CDP `Network.enable` layer |
| B11 | `todo` | End-to-end agent scenario — demo agent runs a multi-page flow, logs in, extracts structured data, returns through a Nimbus function | B4, B5, B6, B8 | scenario green in CI; verification artifacts captured |

## Phase Order and Dependencies

```text
Activation gate met
  └── B0 Decision gate (no code)
        └── B1 nimbus-browser scaffolding + BrowserProvider trait
              └── B2 SubprocessChromeProvider (dev only)
                    └── B3 Session registry + context multiplexing
                          ├── B4 ctx.browser host bridge + JS facade
                          │     ├── B5 SandboxedBrowserProvider (production)
                          │     ├── B8 Extraction cache
                          │     └── B9 Mutation-journal + ACL
                          │           └── B11 End-to-end agent scenario
                          ├── B6 Durable storage state
                          │     └── B7 Warm pool + recycle
                          └── B10 Network policy hooks (after B5)
```

Recommended delivery order: B0 → B1 → B2 → B3 → B4 → B6 → B5 → B7 →
B8 → B9 → B10 → B11.

B5 and B6 can swap; B6 first lets B7 build on top. B8, B9, B10 are
independent and can run in parallel once B4/B5/B6 land.

## Implementation Checkpoints

| Phase | Checkpoint | Next Step |
|-------|------------|-----------|
| B0 | none yet | resolve `Open Decisions` and append a decision log entry per item |
| B1 | none yet | |
| B2 | none yet | |
| B3 | none yet | |
| B4 | none yet | |
| B5 | none yet | |
| B6 | none yet | |
| B7 | none yet | |
| B8 | none yet | |
| B9 | none yet | |
| B10 | none yet | |
| B11 | none yet | |

## Execution Log

| Date | Phase | Outcome | Summary | Verification | Next Step |
|------|-------|---------|---------|--------------|-----------|
| 2026-05-18 | meta | documented | Initial plan authored. Paired research note authored at `docs/plans/research/agent-browser-service-prior-art.md`. Plan adopts the single-Chrome-many-BrowserContexts shape that the prior-art survey shows has converged across Browserless, Steel, Browserbase, and Cloudflare. Sandbox tier reuses `nimbus-sandbox` (krun/VZ) rather than introducing a new isolation surface. Capability admission mirrors `wasi-agent-capabilities-plan.md`. | review against `docs/plans/research/agent-browser-service-prior-art.md`, `ARCHITECTURE.md`, `docs/architecture/sandbox/microvm-service-baseline.md`, and the `wasi-agent-capabilities-plan.md` admission model | hold deferred until activation gate is met |
| 2026-05-19 | meta | refined | Decoupled activation gate from `wasi-agent-capabilities-plan.md` after recognising the two are siblings, not parent/child (different substrates, different gates). Folded in lessons from Cloudflare's `blog.cloudflare.com/browser-run-containers/` container retrospective: (a) tightened the isolation rule to "one Chrome per trust unit (tenant), many BrowserContexts within"; (b) added composite/Quick-Action host ops alongside primitives to collapse host-bridge round-trips; (c) added an explicit "Why this plan does not need a KV or Durable-Object equivalent" section explaining that the engine's mutation path already provides what Cloudflare's DO layer provides. | review against the Cloudflare blog post and `docs/plans/research/agent-browser-service-prior-art.md`; no code changes | hold deferred until activation gate is met |
| 2026-05-19 | meta | refined | Added explicit "Horizontal-scaling and cross-node state" section mapping single-writer-with-affinity browser sessions onto the iroh+openraft+iroh-blobs primitives already committed in `docs/architecture/horizontal-scaling.md` (no new cluster primitives introduced by this plan). Added the "routing-over-proxying" rule citing Cloudflare's SASE-mode-QUIC retrospective (`blog.cloudflare.com/faster-sase-proxy-mode-quic/`) — cross-node CDP must ride a dedicated iroh QUIC stream, never wrapped inside another framed RPC channel. Added "Secret-management dependency" section documenting the three classes of secrets the plan exposes, the MVP workaround via the existing `secret`-grant + KMS-encrypted env table, and the breaking-change migration to first-class `secret_ref` references when a real secret-store plan lands. Authored the paired research note `docs/plans/research/secret-management-shape.md` capturing the gap. Added four matching `Required Invariants` covering credential-leak prevention, cross-node CDP transport, single-writer session ownership, and the no-wrap rule. | review against `docs/architecture/horizontal-scaling.md` §§3–4, the Cloudflare SASE post, and `docs/plans/research/secret-management-shape.md`; no code changes | hold deferred until activation gate is met |
| 2026-05-19 | meta | refined | Horizontal-scaling coherence audit. Re-pointed secret-management references to the new canonical `docs/plans/secret-management-plan.md` (the shape note was superseded). Added a storage-state snapshot trigger policy (explicit checkpoint, close, idle, or 30s/100-mutations-since-last threshold) to close an ambiguity in the multi-node section. Added explicit gossip topic conventions (`topic:<tenant_id>:browser_sessions`, `topic:cluster:state`) matching the canonical naming in `horizontal-scaling.md` §3. Added a multi-Raft forward note tracking Open Question 1 in `horizontal-scaling.md`: tenant-Raft partitioning at 10+ nodes requires no plan-level change because session IDs are already tenant-scoped. | cross-checked against the new Consumer Plans section of `horizontal-scaling.md` and against `secret-management-plan.md`'s identical multi-Raft note; no code changes | hold deferred until activation gate is met |

## Verification Expectations

When promoted, the BrowserService should not be considered viable
without:

- `BrowserProvider` trait conformance tests (one shared suite, run
  against every concrete provider).
- `SubprocessChromeProvider` unit + integration tests (launch, navigate,
  extract, close; refuse-to-start in production).
- `SandboxedBrowserProvider` integration tests (same suite plus a host-fs
  isolation probe and a network egress policy probe).
- Session registry tests (cross-tenant access rejected, same-tenant
  reuse idempotent, concurrent opens of one session serialised).
- Storage-state durability tests (cookies survive Chrome restart;
  cookies survive Nimbus restart; encryption at rest verified).
- Warm pool latency benchmark (steady-state under 200 ms p95; cold-pool
  under 3 s p95).
- Extraction cache hit/miss tests; serialised a11y-tree format snapshot
  test.
- Mutation-journal replay test (a recorded session reproduces
  byte-identical journal entries).
- ACL admission tests (tenant without capability rejected at deploy or
  invocation, never at runtime panic).
- End-to-end agent scenario (multi-page login + extract + return)
  green in CI with verification artefacts under
  `docs/plans/proof/agent-browser-service/`.
- V8 backend regression suite green after every phase.
- `make ci` green at every phase boundary.

## Relationship To Other Plans

- **`docs/plans/research/agent-browser-service-prior-art.md`**:
  load-bearing research baseline. Reread before any decision and update
  if new prior art changes the consensus.
- **`docs/plans/wasi-agent-capabilities-plan.md`**: sibling plan, not
  a parent. The relationship is *philosophical*, not *substrate-level*:
  - **Shared:** the capability admission model (per-tenant opt-in
    flag, default deny, capability-denied-at-link-time rather than
    runtime panic), the "strictly additive host surface" invariant,
    and the plan-document structure.
  - **NOT shared:** the activation gate, the delivery substrate, or
    the sequencing. `wasi-agent-capabilities-plan.md` is gated on
    `wasmtime-backend-plan.md` Phase W3 because it ships WIT interfaces
    and linker bindings on the wasmtime path. This plan rides V8's
    existing `HostBridge` and has no wasmtime dependency. Either plan
    may promote and land before the other.
  - **Possible future convergence:** if browser access is also
    delivered on the wasmtime path, a `nimbus:browser` WIT package
    would land under that path's umbrella following the same shape as
    `nimbus:agent`. That convergence is additive and not assumed by
    this plan.
- **`docs/plans/wasmtime-backend-plan.md`**: no hard dependency for the
  V8 path. If browser access is also delivered on the wasmtime path, a
  `nimbus:browser` WIT package and linker bindings are added under that
  plan's umbrella, following the same shape as `nimbus:agent`.
- **`docs/architecture/sandbox/microvm-service-baseline.md`**: the
  sandbox tier the production provider sits behind. No changes to the
  sandbox layer are assumed by this plan; if any are required they
  open a separate plan against the sandbox layer.
- **`docs/architecture/runtime/adapter-boundary.md`**: the host-bridge
  boundary `ctx.browser` crosses. Update when B4 lands to document the
  new op family.
- **`docs/architecture/horizontal-scaling.md`**: the cluster substrate
  this plan defers to for the multi-node future. BrowserService
  introduces no new cluster primitives; it is a consumer of iroh
  (mesh + QUIC streams), iroh-blobs (storage-state replication),
  iroh-gossip (cluster-state liveness), and openraft (session
  ownership). The "Horizontal-scaling and cross-node state" section
  of this plan documents the mapping in detail. Any future change to
  the cluster substrate that affects single-writer state with
  affinity must be reflected in this plan's cross-node section.
- **`docs/plans/secret-management-plan.md`**: the canonical execution
  plan for tenant-scoped secret management. This plan has a soft
  dependency on the secret-management plan landing — see the
  "Secret-management dependency" section above for the MVP workaround
  and the breaking-change migration path. When the secret-management
  plan reaches Phase S4 (host bridge live) plus one provider, update
  this plan to consume `SecretRef` URIs in `ContextOpts` and remove
  the env-indirection fallback.
- **`docs/plans/research/secret-management-prior-art.md`**: prior-art
  research that informs the secret-management plan; useful background
  for any browser-plan work touching per-session credentials.
- **`docs/architecture/storage/encryption.md`**: KMS DEK envelope
  contract used to encrypt `_nimbus.browser_sessions.storage_state` at
  rest. No new encryption primitives are introduced.
- **`ARCHITECTURE.md`**: update when each phase lands, documenting the
  browser capability surface and tenant admission model.
