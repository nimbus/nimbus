# HG4 threat model: lane-oracle metadata

Status: **RESOLVED (hardened).** The owner ratified harden over accept-as-low
(see RECOMMENDATION below) and the boolean-collapse fix described there is
implemented: `invoke_ctx_resolve_callee_lane`
(`crates/nimbus-server/src/adapters/convex/host_bridge/function_ops/ctx_ops/runtime_calls.rs`)
now returns `Value::Bool` — "locally dispatchable: yes/no" — instead of the
three-way `"node"`/`"bun"`/`"default"`/`null` lane string. See
`docs/private/plans/runtime-guest-trust-global-hardening-plan.md`'s HG4/Band F
row for the DONE status and verification record. The rest of this document
(surface, inference, precedent) is retained as the historical analysis that
justified the fix; only the RECOMMENDATION section below reflects the final
disposition.

## The surface (pre-hardening)

This section describes the surface as it stood before the fix, since it is
the analysis that justified hardening. See the RECOMMENDATION section for
the implemented fix.

The nested `ctx.run*` dispatcher resolves each callee's runtime lane
host-side via `op_nimbus_ctx_resolve_callee_lane`, backed by
`invoke_ctx_resolve_callee_lane`
(`crates/nimbus-server/src/adapters/convex/host_bridge/function_ops/ctx_ops/runtime_calls.rs:140`),
which calls `ConvexRegistry::runtime_environment_for_function`
(`crates/nimbus-convex/src/registry/resolution/runtime_access.rs:90`). A
guest handler body can invoke this op with an arbitrary `{name, visibility}`
payload it constructs itself. The host answered with one of `"node"`,
`"bun"`, `"default"`, or `null` (unknown / not locally dispatchable) —
**without checking `visibility`**. `null` was also returned, by design, for
any name that does not resolve to a locally-dispatchable runtime function,
so the response failed safe to host dispatch either way (see the comment at
`runtime_calls.rs:133-139`); the guest never got to force local dispatch by
supplying a crafted answer, since the actual local-vs-host decision was
resolved from this same host call, not fabricated client-side.

## What a guest could infer (pre-hardening)

For any function name in the guest's own app/tenant registry — public or
**internal** — a guest handler could learn:

1. Whether that name resolves to a locally-dispatchable runtime function at
   all (a `null` response reveals nothing beyond "not this kind of
   function," which also covers "does not exist").
2. If it does resolve, which of three buckets its `compatibility_target`
   falls into: Node-family, Bun/JSC, or default (WebStandard/Wasm).

This was a same-tenant, same-app oracle only. `ConvexRegistry` is bound to
one tenant's function tree; there was no cross-tenant reach through this op.
After hardening, the op still answers a same-tenant question, but the answer
collapses cases 1 and 2 above into a single boolean, so neither the callee's
lane bucket nor a definite "not locally dispatchable" reason is distinguishable
from any other `false` case.

## Is this new exposure, or already-leaked information?

Nimbus already has an accepted existence/visibility/kind oracle on this same
call surface: `should_use_nested_runtime`
(`crates/nimbus-server/src/adapters/convex/host_bridge/function_ops/nested_runtime/selection.rs:4-40`)
throws a descriptive `Error::InvalidInput` — `"convex function not found:
{name}"`, `"convex function {name} is {actual_visibility}, not
{requested_visibility}"`, `"convex function {name} is a {actual_kind}, not a
{requested_kind}"` — when a guest actually *attempts* a nested call with a
name/visibility/kind that doesn't match. That path already reveals
existence, true visibility, and function kind for any name a guest probes,
by design (it is how `ctx.runQuery`/`ctx.runMutation`/`ctx.runAction`
produce useful error messages).

HG4's lane-oracle is **not a strict subset** of that existing surface: it
adds one new fact — Node vs. Bun vs. default runtime-lane bucketing — that
the error-oracle path does not reveal, and it reveals that fact via a quiet
lookup rather than a failed call attempt. So this is a small, genuinely
incremental disclosure, not a re-statement of an already-shipped leak.

## Attacker gain

Runtime-lane bucketing is deployment/build topology, not application data:
it says nothing about a function's arguments, return values, auth
requirements, or business logic. The most a guest learns is "this internal
function is implemented against the Node-compat runtime" — useful, at most,
for a same-tenant guest deciding whether some Node-specific behavior
(polyfill quirks, `process.env` subset, etc.) might apply to a function it
cannot otherwise call successfully (visibility is still enforced on every
*real* invocation path; this op only leaks lane, it does not bypass
visibility for execution). There is no path from this oracle to code
execution, data access, or forged local dispatch — the dispatch decision
itself stays host-resolved per invocation, immune to guest tampering
(that is HG2's fix, already landed).

## Is this a real boundary violation?

By the trust model this plan enforces elsewhere (SAME-TENANT
cross-invocation integrity: no guest-reassignable state may drive a trust
decision or be read by the trusted host path), HG4 does not fit the pattern
the other nine findings share. HG0/HG1/HG2/HG3/HG5–HG9 are all cases where a
guest could **write** state the trusted host or preamble later **trusts**.
HG4 is a **read-only host-authoritative** oracle: the guest asks a question,
the host answers truthfully from data it alone owns, and the answer cannot
be used to influence any later trust decision. It is an information-disclosure
question (how much internal topology should a same-tenant guest be able to
map), not an integrity question.

## Precedent

- **Convex** itself treats function existence, kind, and visibility as
  observable via `ctx.runQuery`/`ctx.runMutation`/`ctx.runAction` error
  messages (`"Could not find function"` style errors) — deployment function
  topology is not treated as a secret from same-deployment code.
- **Cloudflare Workers** service bindings return typed errors when a bound
  target doesn't support the requested call shape, revealing binding-target
  capability shape to the calling Worker.
- **Deno Deploy** isolates surface `"no such export"`/module-resolution
  errors that reveal a target module's export shape to same-isolate-group
  callers.

None of these treat internal-function/runtime-topology metadata as
confidential from code that already runs inside the same
tenant/deployment/isolate group. Nimbus's own error-oracle (above) already
sets the same precedent internally.

## RECOMMENDATION

**Ratified: harden.** The owner chose to harden rather than accept the
three-way lane disclosure as low, even though the analysis above shows it
carries no attacker-actionable gain and is a strict subset of what the
existing error-oracle already reveals. Confidentiality of internal
deployment/runtime topology was judged worth closing outright rather than
relying on a "no gain today" argument that could not bind future product
surfaces.

**Implemented fix.** `invoke_ctx_resolve_callee_lane`
(`crates/nimbus-server/src/adapters/convex/host_bridge/function_ops/ctx_ops/runtime_calls.rs`)
now resolves BOTH lanes host-side — the callee's lane (from `payload.name`,
as before) and, new, the calling invocation's own lane (from
`ConvexHostBridge::current_function_name()`, itself sourced from
`ConvexHostBridgeInvocation.function_name`) — and answers a bare
`Value::Bool`: `true` only when both lanes resolve and match, `false` for
every other case (cross-lane callee, unresolvable callee, or unresolvable
current lane). The three-way `"node"`/`"bun"`/`"default"`/`null` string is
gone from this op's guest-visible return value entirely; a real cross-lane
callee and a nonexistent one are now indistinguishable to the guest (both
`false`).

Guest-side, `nimbus_context_contract.js`'s `__nimbusRunNamedFunction` was
updated to consume the boolean directly (`locallyDispatchable === true`)
instead of comparing a host-returned lane string against the isolate's own
frozen `globalThis.__nimbusRuntimeEnvironmentLane`. The dispatch decision
itself is unchanged in effect: local dispatch is taken if and only if caller
and callee share a lane, exactly as before — only the shape of the
guest-visible answer changed, matching how earlier HG bands removed
lane/callee lookups from guest reach entirely rather than adding
guest-side filtering.

A previously-latent correctness gap surfaced and was fixed during this work:
nested host dispatch (`ConvexHostBridge::invoke_nested_runtime_function_*`,
`crates/nimbus-server/src/adapters/convex/host_bridge/function_ops/nested_runtime/dispatch.rs`)
reuses the calling bridge's session/bootstrap state across a nested hop
rather than building a fresh bridge, so a naive "current function" field
would have stayed frozen at the top-level entrypoint's name for every hop of
a multi-level nested-dispatch chain — corrupting the SECOND hop's lane
comparison in a chain like default -> node -> default. Fixed via
`ConvexHostBridge::retargeted_for_nested_invocation` (`bridge.rs`), which
retargets a per-hop clone's `function_name` to the nested callee before it
becomes the new invocation's host object.
