# Node LTS Runtime And Deno Fork Strategy

Date: 2026-05-27
Status: research baseline
Owner: runtime / node-compat

## Scope

This memo evaluates Nimbus' Deno-family Cargo dependencies, the
`nimbus/deno` fork dependency shape, the current Node.js compatibility posture,
and the runtime permission model. It compares Nimbus against the local
reference repos under `~/src/github.com/*` and current upstream project docs.

The goal is a reliable, enterprise-trustworthy Node LTS support program: support
active Node LTS lines where they make sense, avoid permanently favoring one
major line in the evidence model, and keep the runtime permission boundary
clearer than raw Node compatibility implies.

## Executive Findings

1. Nimbus does not have two competing Deno runtime stacks in Cargo. It has
   normal workspace dependency declarations plus a `[patch.crates-io]` source
   override that resolves key Deno-family crates from the pinned `nimbus/deno`
   fork. That shape is the right Cargo mechanism, but it needs a provenance
   verifier so humans do not have to mentally prove the patch closure.
2. As of 2026-05-27, Node20 is EOL, Node22 is Maintenance LTS, Node24 is Active
   LTS, and Node26 is Current with planned LTS in October 2026. Nimbus docs and
   harness language still treat Node22 as the default center. Product defaults
   are fine, but evidence and support posture should treat active LTS lanes as
   peers.
3. Nimbus has unusually strong evidence infrastructure for a young runtime:
   upstream Node fixtures, lane-specific manifests, supplementary probes,
   generated dashboards, canaries, and explicit gap language. The weak point is
   stale prose and hard-coded lane metadata that can contradict the generated
   evidence.
4. The current runtime process metadata is not enterprise-trustworthy yet.
   `nimbus/deno` hardcodes a Node-like `process.version` (`v24.2.0` in the
   fork), while Nimbus overlays `v{major}.0.0-nimbus`. That is better than a
   single Node22 value, but it is not a truthful patch-level LTS identity and is
   already tracked as an active supplementary failure.
5. The permission architecture is directionally strong: compatibility target is
   separate from grants, server production admission rejects unsafe in-process
   Node authority, and tenant isolation defaults to production. The remaining
   smell is that `RuntimeGrants::application_node()` contains local-development
   style loopback/listen/worker grants and relies on a later production gate.
   That is safe, but not as obvious as an enterprise-facing contract should be.

## Current Nimbus Dependency Shape

The workspace dependency section declares Deno-family crate versions such as
`deno_core = 0.401.0`, `deno_node = 0.186.0`, `deno_resolver = 0.79.0`,
`node_resolver = 0.86.0`, and `v8 = 149.0.0`.

The `[patch.crates-io]` section redirects the patch-sensitive Deno-family crates
to:

- `https://github.com/nimbus/deno`, tag `v2.8.0-nimbus.5`
- `https://github.com/nimbus/rusty_v8`, tag `v149.0.0-nimbus.1`

Local proof from `cargo tree -p nimbus-runtime` shows the important runtime
crates resolving to the fork, for example:

- `deno_core v0.401.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)`
- `deno_node v0.186.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)`
- `node_resolver v0.86.0 (https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a)`
- `v8 v149.0.0 (https://github.com/nimbus/rusty_v8?tag=v149.0.0-nimbus.1#9b775538)`

That is not duplication. The version declarations keep Cargo's package identity
and semver requirements clear. The patch table swaps source repositories so the
whole workspace shares the same forked implementation. Removing one half would
either lose version clarity or lose fork control.

The gap is operational: some Deno-adjacent crates remain from crates.io, such as
`deno_ast`, `deno_error`, `deno_semver`, `deno_path_util`, and `deno_graph`
through transitive dependencies. That may be intentional, but it should be
verified by a scripted "Deno patch closure" check with an allowlist and reasons.

## Current Node Support And Evidence

Generated public evidence currently reports:

| Lane | Current role in Nimbus docs | Fixture denominator | Passed | Classified gaps/skips | Pass rate |
| --- | --- | ---: | ---: | ---: | ---: |
| Node20 | supported selectable target | 1308 | 904 | 404 | 69.1% |
| Node22 | default selectable target | 1283 | 876 | 407 | 68.3% |
| Node24 | supported selectable target | 1495 | 925 | 570 | 61.9% |

The contract correctly says "not full Node built-in compatibility." The concern
is not overclaiming in the latest evidence page; it is that older prose such as
`docs/architecture/runtime/deno-vs-neovex-node-compat.md` can sound more final
than the generated evidence, and the active supplementary failure inventory
still records process-release-shape gaps.

The runtime itself also contains Node22-shaped names and defaults:

- `RuntimeCompatibilityTarget` is an enum with `Node20`, `Node22`, and `Node24`.
- `RuntimeLimits::tooling_node22()` only permits tooling on Node22.
- `RuntimeLimits::application_node22()` remains the product default.
- `node22_runtime_bootstrap.js` is the bootstrap composition root even when the
  lane is Node20 or Node24.
- `RuntimeCompatibilityTarget::node_runtime_version()` returns synthetic
  `v20.0.0-nimbus`, `v22.0.0-nimbus`, and `v24.0.0-nimbus`.

Those are manageable, but they should be moved behind a data-driven LTS lane
registry before Node26 reaches LTS.

## Current Node Release Reality

Primary upstream sources:

- Node Release Working Group schedule: `https://github.com/nodejs/release`
- Node release schedule JSON: `https://github.com/nodejs/Release/blob/main/schedule.json`
- Node releases page: `https://nodejs.org/en/about/releases/`
- Node EOL page: `https://nodejs.org/en/about/eol`

As of 2026-05-27:

| Major | Status | Codename | LTS start | Maintenance start | End of life | Nimbus implication |
| --- | --- | --- | --- | --- | --- | --- |
| 20 | End-of-Life | Iron | 2023-10-24 | 2024-10-22 | 2026-04-30 | Legacy/grace only, not active enterprise LTS |
| 22 | Maintenance LTS | Jod | 2024-10-29 | 2025-10-21 | 2027-04-30 | Supported LTS lane |
| 24 | Active LTS | Krypton | 2025-10-28 | 2026-10-20 | 2028-04-30 | Supported LTS lane |
| 26 | Current | none yet | 2026-10-28 planned | 2027-10-27 planned | 2029-04-30 planned | Preview lane until LTS |

Enterprise support should mean "all active non-EOL LTS lines" plus an explicit,
time-boxed legacy grace policy if Nimbus chooses to keep Node20 fixtures for
customer transition.

## Local Reference Repo Findings

### Deno And `nimbus/deno`

Local repos reviewed:

- `/Users/jack/src/github.com/nimbus/deno` at `v2.8.0-nimbus.5`
- `/Users/jack/src/github.com/denoland/deno` at `v2.7.11-22-g3ec851496`

Patterns worth keeping:

- Deno keeps Node compatibility tests in `tests/node_compat/`.
- The test suite has a structured `config.jsonc` and `schema.json`.
- `tests/node_compat/runner/suite/node_version.ts` is the upstream test corpus
  version source of truth.
- Ignored or special fixtures carry reasons in structured config.
- The latest Deno fork has many ext/node compatibility fixes, including
  `worker_threads MessagePort` work in release notes.

Patterns Nimbus should not copy blindly:

- Deno currently hardcodes process metadata to a single Node-like line. That is
  acceptable for Deno's single compatibility target, but Nimbus wants multiple
  Node LTS targets.
- Deno's public support table is module-level. Nimbus needs module-level and
  LTS-lane-level evidence because its public API exposes selectable Node
  targets.

### Bun

Local repo reviewed:

- `/Users/jack/src/github.com/oven-sh/bun`

Useful patterns:

- Bun treats Node compatibility bugs as product bugs and runs thousands of
  Node tests before release, per its public docs.
- Bun has explicit build metadata for the reported Node version and ABI:
  `scripts/build/deps/nodejs-headers.ts` defines `NODEJS_VERSION = "24.3.0"`
  and `NODEJS_ABI_VERSION = "137"`.
- Tests assert `process.versions.node`, `process.versions.modules`, and N-API
  metadata directly.

Nimbus takeaway: runtime version and ABI metadata should be first-class build
or lane data, not hard-coded ad hoc bootstrap strings.

### Cloudflare workerd

Local repo reviewed:

- `/Users/jack/src/github.com/cloudflare/workerd`

Useful patterns:

- Node compatibility is explicitly flag-gated (`nodejs_compat`,
  `nodejs_compat_v2`) and tied to compatibility dates.
- Workerd keeps permanent compatibility flags and has review guidance saying
  existing flags must not be removed or inverted.
- Node modules have registration gates, module-specific tests, and sidecar
  network test targets for host-sensitive APIs.

Nimbus takeaway: compatibility dates are not the right user model for Nimbus'
Node LTS goal, but the discipline is useful: new compatibility behavior should
be tied to a named, immutable lane/contract record with tests and docs.

### Convex

Local repo reviewed:

- `/Users/jack/src/github.com/get-convex/convex-backend`

Useful patterns:

- Convex treats `"use node"` as an action-only escape hatch rather than the
  default runtime for queries and mutations.
- The self-hosted/local executor is an external Node process reached through
  Unix sockets or named pipes.
- The executor has health checks, restarts after crashes, request timeouts,
  payload limits, metrics, source-map error filtering, and package download
  caching keyed by source package identity.
- Local support checks currently allow Node 18, 20, 22, and 24, with repo-wide
  tooling notes centered on Node20.

Nimbus takeaway: Convex does not solve multi-LTS emulation inside one embedded
runtime; it delegates Node actions to real Node. Nimbus' embedded approach can
offer tighter integration, but only if version metadata, permission admission,
and conformance evidence are correspondingly stronger.

## Permission Model Review

Strong points:

- `RuntimeCompatibilityTarget` is documented as API shape, not permission.
- `RuntimeGrants` separate read/write, net, env, secret, identity, service,
  run, sys, FFI, worker, and tool authority.
- `RuntimeMode::Restricted` rejects sensitive grant families.
- Production tenant isolation defaults to `Production`.
- Server admission rejects generic loopback/wildcard `net_connect`, all
  `net_listen`, worker grants, run grants, FFI, inspector, and broad package
  filesystem grants for in-process untrusted production routes.
- The production fallback is explicit: route to `MicroVmService` or trusted-only
  depending on the rejected grant family.

Trust gaps:

- `RuntimeGrants::application_node()` includes `127.0.0.1`, `localhost`,
  wildcard listen grants, `inspector`, and `worker = ["thread"]` by default.
  Production admission rejects this, but the lower-level preset looks too broad.
- `RuntimeGrants::application_web_standard()` allows reading
  `NODE_TLS_REJECT_UNAUTHORIZED`. Reading an ambient host value that can disable
  TLS verification is surprising in an application preset unless it is virtual
  or local-dev scoped.
- Worker threads are granted by default for Node applications even though recent
  VM MessagePort fixtures have exposed lifecycle risk.
- The grant strings are ergonomic but not as typed as the architecture docs.
  Service-bound network grants should be distinguishable from raw host strings.

Conclusion: the architecture is not papering over an unsafe production path, but
the API shape is overly defensive in one layer and overly permissive-looking in
another. Enterprise trust would improve if local-dev Node grants, production
application Node grants, and microVM-required Node grants were separate named
profiles.

## Recommended Architecture

### 1. Node LTS Lane Registry

Introduce a checked-in data file such as `crates/nimbus-runtime/node_lts_lanes.toml`
or `docs/runtimes/nodejs/lts-lanes.json` with one record per supported or
preview major:

```toml
[[lane]]
name = "node22"
major = 22
support_phase = "maintenance_lts"
codename = "Jod"
upstream_version = "22.x.y"
upstream_tag = "v22.x.y"
eol = "2027-04-30"
product_default = true

[[lane]]
name = "node24"
major = 24
support_phase = "active_lts"
codename = "Krypton"
upstream_version = "24.x.y"
upstream_tag = "v24.x.y"
eol = "2028-04-30"
product_default = false
```

The important split is:

- `product_default`: only answers what happens when the user omits config.
- `support_phase`: answers whether the lane is active LTS, maintenance LTS,
  current preview, EOL legacy, or removed.
- `evidence_required`: derived from support phase, not from product default.

### 2. Truthful Runtime Metadata

The runtime should derive `process.version`, `process.versions.node`,
`process.release.lts`, ABI/module metadata, and fixture provenance from the lane
registry. Avoid synthetic values like `v22.0.0-nimbus` in supported LTS lanes.

When Nimbus intentionally differs from upstream Node component versions, expose
the truth rather than masquerading:

- `process.versions.node`: selected Node API contract version.
- `process.versions.v8`: actual embedded V8 version.
- optional diagnostic metadata outside the Node API surface: Nimbus runtime
  backend, Deno fork tag, and compatibility lane.

### 3. Equal-Lane Evidence Model

The evidence dashboard should not privilege Node22 except as a product default.
For every active LTS lane:

- zero unclassified fixture results in published support slices;
- generated provenance for upstream Node fixture tag/version;
- per-fixture timeout and hang classification;
- package canaries per lane, not only the default lane;
- trend non-regression checks per lane and per API family;
- stale prose checks that prevent hand-written docs from claiming more than
  generated evidence.

### 4. Deno Fork Provenance And Patch Closure

Add a verifier that checks:

- all patch-sensitive Deno crates resolve to the expected `nimbus/deno` tag and
  SHA;
- all intentional crates.io Deno-family crates are allowlisted with reasons;
- `nimbus/deno` and `nimbus/rusty_v8` tags exist and match `Cargo.lock`;
- the local canonical fork worktree is clean before release proof;
- the fork tag includes a changelog entry mapping Nimbus-visible fixes to
  upstream Deno commits or Nimbus-only patch reasons.

### 5. Permission Profiles By Deployment Intent

Replace the single broad application Node grant preset with at least three
named profiles:

- `application_node_local_dev`: loopback connect/listen and inspector allowed
  for local tooling.
- `application_node_production_in_process`: no raw loopback, no listen, no
  worker, no inspector, no host env leakage beyond virtualized values.
- `application_node_production_service`: service-bound networking and worker
  authority only when routed to the appropriate outer isolation tier.

Production admission should remain as a fail-closed backstop, but the first
profile chosen should already communicate the deployment intent.

### 6. Upstream-First Fork Policy

Nimbus-specific host integration belongs in Nimbus. General Node API semantics
belong in the Deno fork, and ideally upstream Deno. A fix should move to
`nimbus/deno` when the alternative is a long-lived JavaScript shim that
duplicates Deno ext/node behavior or adds hot-path overhead.

## Decision

Keep the current Cargo version-plus-patch shape. Do not try to "choose between"
Cargo Deno crates and the `nimbus/deno` fork; they serve different purposes.

Do change the Node support architecture: introduce a data-driven LTS lane
registry, make runtime metadata truthful per lane, split permission profiles by
deployment intent, and make generated evidence the only source of public support
claims. This is the path that makes Nimbus look less like a clever shim and more
like an enterprise runtime product with a release discipline.

## Sources

Local sources:

- `Cargo.toml`
- `Cargo.lock`
- `crates/nimbus-runtime/src/limits/axes.rs`
- `crates/nimbus-runtime/src/limits/grants.rs`
- `crates/nimbus-runtime/src/limits/resources.rs`
- `crates/nimbus-runtime/src/runtime/bootstrap/source.rs`
- `docs/architecture/runtime/permission-model.md`
- `docs/runtimes/nodejs/evidence/latest.md`
- `docs/runtimes/nodejs/compatibility.md`
- `docs/architecture/runtime/node-compat-supplementary-failures.md`
- `/Users/jack/src/github.com/nimbus/deno/tests/node_compat/`
- `/Users/jack/src/github.com/nimbus/deno/ext/node/polyfills/_process/process.ts`
- `/Users/jack/src/github.com/get-convex/convex-backend/crates/node_executor/`
- `/Users/jack/src/github.com/cloudflare/workerd/src/workerd/io/compatibility-date.capnp`
- `/Users/jack/src/github.com/oven-sh/bun/scripts/build/deps/nodejs-headers.ts`

Web sources:

- Node Release Working Group: `https://github.com/nodejs/release`
- Node schedule JSON: `https://github.com/nodejs/Release/blob/main/schedule.json`
- Node releases page: `https://nodejs.org/en/about/releases/`
- Node EOL page: `https://nodejs.org/en/about/eol`
- Node permissions docs: `https://nodejs.org/api/permissions.html`
- Deno Node APIs docs: `https://docs.deno.com/runtime/reference/node_apis/`
- Deno Node/npm compatibility docs: `https://docs.deno.com/runtime/fundamentals/node/`
- Cloudflare Workers Node compatibility docs:
  `https://developers.cloudflare.com/workers/runtime-apis/nodejs/`
- Cloudflare Workers compatibility flags:
  `https://developers.cloudflare.com/workers/configuration/compatibility-flags/`
- Bun Node compatibility docs: `https://bun.sh/docs/runtime/nodejs-compat`
- Electron release cadence docs:
  `https://www.electronjs.org/docs/latest/tutorial/electron-timelines`
