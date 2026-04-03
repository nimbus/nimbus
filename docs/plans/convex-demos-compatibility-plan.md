# Plan: Convex Demos Compatibility

## Scope

This repo already has a substantial Convex compatibility baseline:

- `packages/convex` ships in-repo `convex/browser`, `convex/react`,
  `convex/server`, and `convex/values`
- `packages/codegen` emits generated refs plus the runtime bundle
- `demos/convex/html` and `demos/convex/http` already exercise the current
  supported surface
- `packages/convex/src/differential.mjs` already compares Neovex's supported
  subset against an official Convex browser client oracle

This plan is therefore not "make Convex demos exist." It is the remaining work
to support the specific upstream-style `node`, `html`, and `http` demo authoring
patterns cleanly in Neovex.

Because `convex-demos` has no license, treat it as a behavior reference only:

1. Fix missing compat-layer behavior in Neovex
2. Add or refine repo-owned demos in `demos/convex/`
3. Add verification that matches how this repo actually resolves generated refs,
   browser bundles, and external demo fixtures

## Current Baseline

Already present in the current codebase:

- `.collect()` without an explicit limit
- `ctx.runQuery`, `ctx.runMutation`, and `ctx.runAction`
- `httpRouter` and compiled `httpAction` routes
- generated refs for the supported declarative subset
- `ConvexHttpClient.query()`, `ConvexClient.onUpdate()`, and React hooks such as
  `useQuery`, `useMutation`, `useQueries`, and `usePaginatedQuery`
- `convex codegen --app <external-dir>` for arbitrary app directories
- in-repo Convex demo apps at `demos/convex/html` and `demos/convex/http`
- external differential coverage for the currently documented supported subset

## Remaining Gaps

| # | Gap | Blocks | Severity |
|---|-----|--------|----------|
| 1 | WebSocket constructor injection for Node clients | node demo | High |
| 2 | String-based function refs on the in-repo `convex/browser` surface | upstream-style html ergonomics | High |
| 3 | `anyApi` proxy for upstream-style query/mutation/action refs | upstream-style html ergonomics | Medium |
| 4 | Served browser bundle for plain `<script>` usage | plain html demo | High |
| 5 | `ActionCtx`, `QueryCtx`, and `MutationCtx` convenience exports | type parity | Low |
| 6 | Repo-owned Node demo | node demo | Medium |
| 7 | External upstream-demo runner with correct package resolution | DX | Medium |

---

## Phase 1: WebSocket Constructor Injection

**Goal:** Allow Node.js clients to pass a WebSocket implementation such as the
`ws` package without mutating `globalThis.WebSocket`.

**Files:**

- `packages/neovex/src/browser.ts`
  - Add `webSocket?: { new (url: string): WebSocket }` to the `NeovexClient`
    constructor options.
  - Store the constructor on the client instance.
  - Replace the hardcoded `new WebSocket(...)` socket creation site with
    `new (this.webSocketImpl ?? WebSocket)(...)`.
- `packages/convex/src/browser.ts`
  - Add `webSocket?` to `ConvexClientOptions`.
  - Thread it through `withConvexDeploymentUrlCheck(...)`.
  - Pass it through to `ConvexClient` and `ConvexReactClient`.

**Verify:**

- Add a selftest that passes a fake WebSocket constructor through client
  options instead of monkey-patching `globalThis.WebSocket`.
- `npm run convex:test`
- `npm run convex:build`

---

## Phase 2: String-Based Function Refs

**Goal:** Make the in-repo `convex/browser` surface accept `"module:export"`
strings for the upstream-style query, mutation, action, and standard live-query
flows.

**Scope guardrails:**

- This first slice covers `query()`, `mutation()`, `action()`, `scheduleAfter()`,
  `scheduleAt()`, and non-paginated `onUpdate()`.
- `paginatedQuery()` and paginated `onUpdate()` continue to require generated
  refs or `makePaginatedQueryReference(...)` until we add an explicit paginated
  string-ref design. The client currently uses ref kind on the WebSocket path,
  so we should not pretend every named ref is interchangeable.

**Files:**

- `packages/convex/src/browser.ts`
  - Add a `coerceRef(input, kind)` helper for raw strings and supported proxy
    refs.
  - Add overloads to `query()`, `mutation()`, `action()`, `scheduleAfter()`,
    and `scheduleAt()` that accept `string`.
  - Add an overload to `onUpdate()` for standard query subscriptions accepting
    `string`.
- `packages/convex/src/selftest.mjs`
  - Add coverage for string refs across query, mutation, action, scheduling,
    and non-paginated subscriptions.

**Design note:** The server already resolves named functions via the existing
`{ name, args }` request path. The client still needs to stamp the correct ref
kind for any behavior that branches locally.

**Verify:**

- `npm run convex:test`
- Keep paginated selftests on generated refs so the supported surface stays
  explicit

---

## Phase 3: `anyApi` Proxy

**Goal:** Export an upstream-style `anyApi` proxy for the same first-slice
surface as Phase 2.

**Scope guardrails:**

- First slice is query, mutation, action, scheduling, and non-paginated live
  query use.
- Do not claim paginated `anyApi` support until we define how paginated refs
  encode `kind: "paginated_query"` on the client side.

**Files:**

- `packages/convex/src/browser.ts`
  - Export `anyApi` as a recursive `Proxy` that records property access and
    resolves to `"module:export"` names when consumed by `coerceRef(...)`.
- `packages/convex/src/selftest.mjs`
  - Add coverage for `anyApi.messages.list`, `anyApi.messages.send`, and a live
    query subscription path.

**Verify:**

- `npm run convex:test`

---

## Phase 4: Served Browser Bundle

**Goal:** Produce an IIFE bundle that plain HTML pages can load through the
existing `/demos/` static serving path.

**Current codebase constraint:** The server serves the repo's `demos/` tree, not
`packages/convex/`, so `vanilla.html` cannot point at a file under
`packages/convex/dist/` and expect it to work through Neovex.

**Files:**

- `packages/convex/build.mjs` (new)
  - Bundle `packages/convex/src/browser.ts` as an IIFE with
    `globalName: "convex"`.
  - Write the canonical artifact to `packages/convex/dist/browser.bundle.js`.
  - Copy the served artifact to `demos/convex/vendor/browser.bundle.js`.
- `packages/convex/package.json`
  - Add `"build": "node ./build.mjs"`.
- `.gitignore`
  - Keep `dist` ignored.
  - Add `demos/convex/vendor/browser.bundle.js` if we generate it into the
    served demos tree.

**Verify:**

- `npm run build --workspace convex`
- Confirm both bundle outputs exist
- Add a selftest that loads the IIFE bundle and checks
  `typeof convex.ConvexClient === "function"`

---

## Phase 5: Context Type Aliases

**Goal:** Export `ActionCtx`, `QueryCtx`, and `MutationCtx` from
`convex/server` and the generated `_generated/server.ts` barrel.

**Files:**

- `packages/convex/src/server.ts`
  - Add:
    - `export type QueryCtx = GenericQueryCtx;`
    - `export type MutationCtx = GenericMutationCtx;`
    - `export type ActionCtx = GenericActionCtx;`
- `packages/codegen/src/emit/generated_files.mjs`
  - Add `ActionCtx`, `QueryCtx`, and `MutationCtx` to the generated re-export
    list.

**Verify:**

- `convex codegen --app demos/convex/http`
- Confirm `convex/_generated/server.ts` re-exports the aliases
- `npm run convex:test`

---

## Phase 6: Repo-Owned Node Demo

**Goal:** Add `demos/convex/node/` as the repo-owned upstream-style Node demo.

**Current codebase constraint:** Codegen emits TypeScript source files such as
`convex/_generated/api.ts`, not runtime-ready `.js` files. The demo runtime
entry therefore needs either TypeScript execution or a small build step.

**Files:**

- `demos/convex/node/package.json`
  - Name: `convex-node`
  - Dependencies: `convex: "*"`, `ws: "^8.0.0"`
  - Dev dependencies: `typescript`, `tsx`
  - Scripts:
    - `codegen`
    - `test`
    - `demo` -> `npm run codegen && tsx ./script.ts`
- `demos/convex/node/tsconfig.json`
  - Minimal TS config for codegen output plus the demo script
- `demos/convex/node/convex/schema.ts`
  - `messages` table with `author` and `body`
- `demos/convex/node/convex/messages.ts`
  - `list` query using `.collect()`
  - `send` mutation using `db.insert(...)`
- `demos/convex/node/script.ts`
  - Import `ConvexHttpClient` and `ConvexClient` from `convex/browser`
  - Import `api` from `./convex/_generated/api`
  - Import `WebSocket` from `ws`
  - Run a point-in-time query through `ConvexHttpClient`
  - Run a live subscription through `ConvexClient` with `{ webSocket: WebSocket }`
  - Send a mutation
  - Handle `SIGINT` cleanly

**Workspace wiring:**

- `package.json` (root)
  - Add `demos/convex/node` to `workspaces`
  - Add:
    - `"convex:codegen:node": "convex codegen --app demos/convex/node"`
    - `"convex:server:node": "cargo run -p neovex-bin -- --port 8080 --convex-app-dir ./demos/convex/node"`
    - `"convex:demo:node": "npm run demo --workspace convex-node"`

**Verify:**

- `npm run convex:codegen:node`
- `npm run convex:server:node`
- `npm run convex:demo:node`

---

## Phase 7: Vanilla HTML Demo Variant

**Goal:** Add a plain HTML variant that uses the served browser bundle plus the
Phase 2 and 3 string-ref ergonomics.

**Files:**

- `demos/convex/html/vanilla.html`
  - Load `/demos/convex/vendor/browser.bundle.js`
  - Use `convex.ConvexClient`, string refs, and `convex.anyApi`
  - Keep the first slice to standard queries and mutations; do not introduce
    paginated `anyApi` behavior here yet

**Verify:**

- Build the bundle first
- Open `http://localhost:8080/demos/convex/html/vanilla.html`
- Confirm messages load and mutate successfully

---

## Phase 8: External Upstream-Demo Runner

**Goal:** Let developers validate upstream demo shapes against Neovex without
mutating their local `convex-demos` clone and without accidentally resolving
runtime imports to the official `convex` package.

**Current codebase constraint:** Running Neovex directly against a user's clone
is not enough. Generated files and app runtime code import `convex/*` at
runtime, so a direct in-place runner would exercise the official package rather
than this repo's compat layer.

**Approach:** Build a temporary overlay app for the selected upstream demo.

**Files:**

- `.env.example`
  - Document `CONVEX_DEMOS_DIR=/absolute/path/to/convex-demos`
- `.gitignore`
  - Add `.env`
- `scripts/convex-demo-overlay.mjs` (new)
  - Copy the selected upstream demo into a temp workspace
  - Symlink or otherwise force `convex`, `neovex`, and `@neovex/codegen`
    resolution to this repo's workspace packages
  - Run `convex codegen --app <overlay-dir>`
  - Print or return the prepared overlay path
- `Makefile`
  - Add `-include .env`
  - Add `convex-demo`, `convex-demo-node`, `convex-demo-html`,
    and `convex-demo-http`
  - Use the overlay helper instead of pointing `--convex-app-dir` at the raw
    user clone
- `demos/README.md`
  - Document the overlay workflow and its purpose

**Verify:**

- Set `CONVEX_DEMOS_DIR`
- Run `make convex-demo-node`
- Confirm the prepared app resolves `convex/*` to this repo's compat package
- Start Neovex against the prepared overlay app and connect a client

---

## Phase Order

```text
Phase 1 (WebSocket injection) -----------+
Phase 2 (String refs) -------------------+--> Phase 3 (`anyApi`)
Phase 5 (Ctx aliases) -------------------+

Phase 4 (served browser bundle) ---------+--> Phase 7 (vanilla html)
Phase 6 (node demo) --------------------- depends on Phase 1
Phase 8 (external overlay runner) ------- after Phases 1-7 are stable
```

Phases 1, 2, 4, and 5 are independent. Phase 3 depends on Phase 2's coercion
story. Phase 7 depends on Phases 2, 3, and 4. Phase 8 should come last so the
overlay workflow validates an already-defined supported surface.

## Verification

1. `npm run convex:test`
2. `npm run test:differential --workspace convex -- --neovex-only`
3. `npm run convex:build`
4. Existing demos still work:
   - `npm run convex:demo:html`
   - `npm run convex:demo:http`
5. New Node demo works:
   - `npm run convex:server:node`
   - `npm run convex:demo:node`
6. Vanilla HTML works:
   - `http://localhost:8080/demos/convex/html/vanilla.html`
7. Optional external overlay runner works:
   - `make convex-demo-node`
8. Run normal workspace formatting and linting for any code changes:
   - `cargo fmt --all --check`
   - `make clippy`
