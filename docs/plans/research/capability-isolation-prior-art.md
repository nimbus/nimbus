# Capability Isolation Prior Art (research findings)

Source: deep-research harness run `wf_29a23c96-ff2` (2026-05-30). 6 angles,
23 sources fetched, 101 claims extracted, **25 verified** via 3-vote adversarial
verification (a claim is killed only on 2/3 refute): 23 confirmed, 2 killed,
9 findings after synthesis. 106 agent calls.

**Coverage is uneven (stated by the harness):** strong unanimous primary-source
coverage of *tenant isolation / capability gating* (deno_core ops,
deno_permissions, Cloudflare bindings, Convex runtime tiers). **Weak-to-no**
surviving primary coverage of *SES/LavaMoat* and *capability tokens
(macaroons/Biscuit/SPIFFE/IAM)* — those sources were fetched but few claims
survived verification, so the recommendations on them are **reasoned synthesis
grounded in the verified facts, not source-backed findings.** Flagged inline.

## Verbatim synthesized summary

> Multi-tenant serverless platforms isolate untrusted tenant code with a
> two-axis model that maps directly onto Nimbus's design: (1) an
> object-capability (ocap) surface where tenant code can only invoke what it
> holds a reference to, and (2) a separate ambient-permission/identity layer
> enforced at the host/process boundary. Convex runs queries/mutations in a
> Cloudflare-Workers-like V8 runtime that denies network/fs by tier; Cloudflare
> Workers gate platform-resource access exclusively through config-time bindings
> surfaced on the handler's env object; and Deno/deno_core lets a Rust embedder
> define exactly which native capabilities (ops) reach JS, with a separate
> deny-by-default permission-descriptor crate (deno_permissions) for
> per-instance/per-worker profiles. The verified evidence strongly validates
> Nimbus's core thesis: NOT registering a privileged "services" op/capability on
> the tenant (Convex/Firebase) isolate is a sound isolation boundary at the
> embedder level, because a Rust function with no V8 op binding is simply
> unreachable from JS — the capability surface is defined entirely by the host,
> not by JS code. The decisive caveat is that this boundary holds only at the
> op/binding layer (it does not stop V8 engine sandbox escapes, does not re-gate
> a capability that IS registered, and assumes no registered op indirectly
> dispatches to privileged code), and that within a single thread all modules
> share one privilege level, so privilege separation that needs ambient-authority
> differences must move to a worker/thread/process boundary, not module
> boundaries inside one isolate.

## Verified findings

1. **Convex gates by runtime tier, not a general ocap ctx model.** `high`, 3-0.
   Queries/mutations run in "a custom JavaScript runtime very similar to the
   Cloudflare Workers runtime"; `fetch`/network and Node.js are **Actions only**.
   The framing "determinism is the isolation mechanism" was **refuted 0-3** —
   determinism is a reactivity/correctness property, not the boundary; the
   boundary is the *denied capability set per tier*.
   — https://docs.convex.dev/functions/runtimes

2. **deno_core: the embedding Rust host fully controls the JS capability
   surface; an unregistered op has no V8 entry point.** `high`, 3-0 across four
   sub-claims. Ops (`#[op2]`/`extension!`) are the sole sanctioned native-call
   surface. Production embedders: Supabase Edge Runtime, deno_runtime, Secutils.
   Caveats: a base "core" extension is always installed (event loop/promises —
   plumbing); the boundary is op-layer only — does **not** stop V8 sandbox/JIT
   escapes, does **not** re-gate a registered op, and fails if a registered op
   dispatches to privileged code or leaks an over-broad reference. The stricter
   wording "JS cannot invoke ANY Rust capability not declared as an op" was
   **refuted 1-2** (base core ops exist) — scope the guarantee to *privileged
   platform capabilities*.
   — https://deno.com/blog/roll-your-own-javascript-runtime ; https://docs.rs/deno_core

3. **deno_core does NOT ship `--allow-net`/`--allow-read` descriptors; that
   model lives in the separate `deno_permissions` crate.** `high`, 3-0. Two
   distinct levers: op-registration (ocap surface) at the deno_core layer vs.
   descriptor profiles at the deno_permissions layer.
   — https://docs.rs/deno_core ; https://docs.rs/deno_runtime/.../PermissionsContainer.html

4. **A Rust host can build deny-by-default, per-instance permission profiles
   programmatically.** `high`, 3-0. `deno_permissions` exposes `Permissions` /
   thread-shareable `PermissionsContainer`; `PermissionsOptions` with all
   `allow_* = None`, `prompt=false` → deny-by-default. Typed per category (Net,
   Read, Write, Run, Env, Sys, FFI, Import), each gated by `check_*`. **Nimbus
   already calls these** (`crates/nimbus-runtime/src/runtime_capabilities.rs`)
   and pins `deno_permissions` 0.107.0 (one minor behind 0.108.0). Caveat:
   `PermissionsContainer` is process/worker-scoped; *per-invocation* gating
   means the embedder builds a fresh profile per call (not automatic).
   — https://docs.rs/deno_permissions/ ; https://docs.deno.com/runtime/fundamentals/security/

5. **Deno is deny-by-default and resource-scopable** (paths, hosts, env, program
   names). `high`, 3-0. Caveat: default-deny is the *CLI* default; in an embedded
   `deno_core` host it's the host's responsibility. A granted capability is not
   itself sandboxed (an allowed subprocess runs unconfined).
   — https://docs.deno.com/runtime/fundamentals/security/

6. **Within one thread, all Deno code shares one privilege level — modules
   cannot have different privilege levels in the same thread.** `high`, 3-0.
   Verbatim from docs. Bounds the *ambient-permission* axis only (orthogonal to
   ocap gating). Implication: if adapter code and privileged code ever need
   *different ambient authority*, that split must be at the worker/thread/process
   boundary — which is exactly where the privileged services/libkrun path lives
   (Rust host path, never the tenant isolate).
   — https://docs.deno.com/runtime/fundamentals/security/ ; .../workers/

7. **Deno Workers give attenuation-only separation:** inherit parent perms by
   default, `deno.permissions` only restricts, `'none'` = zero-perm worker,
   per-category `'inherit' | false | list`. `high`, 3-0. The canonical mechanism
   for running code at reduced ambient privilege; no escalation path.
   — https://docs.deno.com/runtime/fundamentals/workers/

8. **Cloudflare Workers = bindings-as-capabilities, the production precedent for
   the ctx/env approach.** `high`, 3-0. A Worker reaches platform resources only
   through bindings declared at config time (wrangler) and surfaced on `env`;
   an unbound resource is simply absent — no ambient path. Qualifications (2-1 on
   phrasing): arbitrary outbound `fetch()` to the public internet is **not**
   binding-gated (bindings gate *platform resources*); legacy Service Worker
   format exposes bindings as ambient globals. The capability-surface point
   stands; "only via the handler param" is format-conditional.
   — https://developers.cloudflare.com/workers/runtime-apis/bindings/

9. **Recommended defense-in-depth layering + SES/LavaMoat verdict.** `medium`
   (synthesis, not a single voted claim). Reproduced and applied in the next
   section.

## Refuted (do not rely on)
- "Determinism is Convex's primary isolation mechanism." **0-3.**
- "JS cannot invoke ANY Rust capability not declared as an op." **1-2** (base
  core ops always present — scope to *privileged* capabilities).

## What this drives in the Nimbus plan

**Core thesis validated (high, 3-0).** Withholding the privileged service
op/capability from ungranted runtime isolates is a *sound* boundary at the
embedder/op layer — the same mechanism Convex, Cloudflare, and Deno embedders
use in production. → Confirms plan **layer 3** (ocap `ctx` +
`RuntimeServiceCapabilityHost`).

**The op-layer caveats become explicit plan items:**
- does not stop a V8 escape → microVM isolation (`nimbus-libkrun`, **layer 5**)
  stays the blast-radius backstop;
- does not re-gate a *registered* op → privileged service ops must not be
  registered for ungranted isolates, service-capable bridges must expose only an
  exact-granted `RuntimeServiceCapabilityHost`, refusal-only bridges must keep no
  positive service capability path, and any *shared* op must not internally
  dispatch to privileged paths (add a review/test guard);
- one thread = one privilege level → a Convex tenant and the privileged path
  must never co-inhabit one isolate; `services` stays in the Rust host path /
  separate execution context (**layer 2/L2**).

**Per-tier deny-by-default permission profiles are real and already partly
wired** (`runtime_capabilities.rs`). → Adopt **layer 3'**: query/mutation =
no-net/no-fs; action = widened; native = services. Mirrors Convex's tier model
and the `deno_permissions` deny-by-default default. Decide per-isolate vs
per-invocation (open question 4).

### SES / LavaMoat — verdict: UNNECESSARY for this threat model
Decision rule (from finding 9, synthesis — flagged non-source-backed): SES/
LavaMoat neutralize prototype pollution, ambient authority, and a compromised
transitive dependency *that shares the same JS realm as privileged code*. In
Nimbus the privileged `services` capability runs only in the Rust host path and
is never in the tenant isolate's realm, and privileged JS (`nimbus/rest`) never
shares a realm with adapter/tenant code — so there is no in-realm privileged
target. **Adopt realm separation (L1 ocap + L2 Rust-path) instead of paying the
SES perf/DX cost.** Re-enter scope only if a future design co-locates privileged
JS and untrusted JS in one realm — make that invariant an explicit, tested rule.
→ The JS package split (`@nimbus/core` vs `nimbus/rest`) is retained for
**developer ergonomics/clarity, not as a security boundary.**

### Server-side route gating — baseline confirmed, token choice deferred
Principle (AWS-Lambda style): client never trusted; authority attached
server-side to the caller's identity, checked per call. → Keep **layer 4**:
identity/role gate on `/api/.../services/*`. **Open:** attenuated capability
tokens (macaroon/Biscuit) vs identity gating (SPIFFE/mTLS/IAM) — *no primary
evidence survived verification*; needs a dedicated follow-up pass. Default to
identity/role gating now; attenuated tokens only if delegation / short-lived
narrow third-party grants become a requirement.

## Open questions (carried into the plan)
1. Privileged-route auth — **DECISION MADE: identity/role gating now**
   (AWS-Lambda baseline confirmed). Attenuated tokens (macaroon/Biscuit) are out
   of scope unless a delegation / short-lived-grant requirement appears; only the
   token-mechanism *detail* lacked surviving citations and would need follow-up
   research *at that point*, not before.
2. Make "privileged JS and untrusted JS never share a V8 realm" an explicit,
   **tested** architecture invariant (gates whether SES ever re-enters scope).
3. The **positive** grant path for a *native* Nimbus app needing `services` —
   **DECIDED: native-only op extension** (a privileged `deno_core` extension
   registered only for native isolates; absent from adapter isolates). Withholding
   was already validated; this picks the grant mechanism.
4. Per-invocation vs per-isolate deny-by-default profiles — **DECIDED:
   per-isolate** (one profile per runtime tier; revisit per-invocation only if a
   tier needs it).

## Time-sensitivity
`deno_core`/`deno_permissions` are 0.x crates evolving in lockstep with Deno;
`PermissionState` variants and `PermissionsContainer` methods can shift between
versions. Cloudflare's `env` access changed 2025-03 (importable from
`cloudflare:workers`), so "only via handler param" is version-conditional.

## Primary sources
- Convex runtimes — https://docs.convex.dev/functions/runtimes
- deno_core / "roll your own runtime" — https://deno.com/blog/roll-your-own-javascript-runtime ; https://docs.rs/deno_core
- deno_permissions — https://docs.rs/deno_permissions/ ; https://docs.rs/deno_runtime/latest/deno_runtime/deno_permissions/struct.PermissionsContainer.html
- Deno security / workers — https://docs.deno.com/runtime/fundamentals/security/ ; https://docs.deno.com/runtime/fundamentals/workers/
- Cloudflare Workers bindings / security model — https://developers.cloudflare.com/workers/runtime-apis/bindings/ ; https://developers.cloudflare.com/workers/reference/security-model/
- AWS Lambda execution role — https://docs.aws.amazon.com/lambda/latest/dg/lambda-intro-execution-role.html
- ocap / Capsicum — https://en.wikipedia.org/wiki/Object-capability_model ; https://www.cl.cam.ac.uk/research/security/capsicum/
- SES / LavaMoat (fetched; few claims survived) — https://hardenedjs.org/ ; https://github.com/endojs/endo/blob/master/packages/ses/README.md ; https://github.com/LavaMoat/LavaMoat
- Tokens (fetched; few claims survived) — https://research.google/pubs/macaroons-cookies-with-contextual-caveats-for-decentralized-authorization-in-the-cloud/ ; https://www.biscuitsec.org/ ; https://spiffe.io/docs/latest/spiffe-about/overview/
