# HG4 threat model: lane-oracle metadata

Status: **non-binding recommendation, pending owner ratification.** This note
makes no runtime change. See
`docs/private/plans/runtime-guest-trust-global-hardening-plan.md`'s HG4 row
and fix-approach item 6 ("decide in the threat model — document lane metadata
as non-secret, or design uniform failure behavior against an explicit
confidentiality requirement").

## The surface

The nested `ctx.run*` dispatcher resolves each callee's runtime lane
host-side via `op_nimbus_ctx_resolve_callee_lane`, backed by
`invoke_ctx_resolve_callee_lane`
(`crates/nimbus-server/src/adapters/convex/host_bridge/function_ops/ctx_ops/runtime_calls.rs:140`),
which calls `ConvexRegistry::runtime_environment_for_function`
(`crates/nimbus-convex/src/registry/resolution/runtime_access.rs:90`). A
guest handler body can invoke this op with an arbitrary `{name, visibility}`
payload it constructs itself. The host answers with one of `"node"`,
`"bun"`, `"default"`, or `null` (unknown / not locally dispatchable) —
**without checking `visibility`**. `null` is also returned, by design, for
any name that does not resolve to a locally-dispatchable runtime function,
so the response fails safe to host dispatch either way (see the comment at
`runtime_calls.rs:133-139`); the guest never gets to force local dispatch by
supplying a crafted answer, since the actual local-vs-host decision is
resolved from this same host call, not fabricated client-side.

## What a guest can infer

For any function name in the guest's own app/tenant registry — public or
**internal** — a guest handler can learn:

1. Whether that name resolves to a locally-dispatchable runtime function at
   all (a `null` response reveals nothing beyond "not this kind of
   function," which also covers "does not exist").
2. If it does resolve, which of three buckets its `compatibility_target`
   falls into: Node-family, Bun/JSC, or default (WebStandard/Wasm).

This is a same-tenant, same-app oracle only. `ConvexRegistry` is bound to
one tenant's function tree; there is no cross-tenant reach through this op.

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

**Accept as low, document as non-secret.** Runtime-lane bucketing for a
guest's own app functions (public or internal) is deployment metadata, not
application data or a capability; it does not enable any bypass of
visibility, auth, or dispatch integrity; and Nimbus's own error-oracle
already discloses strictly more (existence + true visibility + kind) on the
same call surface without objection. Building uniform failure behavior for
HG4 (e.g., collapsing all three lane values into a single opaque answer, or
requiring visibility-checked calls before answering) would add complexity
and a second lookup round-trip to the nested-dispatch hot path for a
disclosure that is already implied by the accepted error-oracle and carries
no attacker-actionable gain.

This is a recommendation for owner ratification, not a decision — the
owner may weigh confidentiality of internal deployment topology differently
(e.g., if a future product surface treats "which runtime a function targets"
as sensitive competitive/deployment information rather than debug metadata).
If ratified as "harden" instead, the minimal fix is to have
`invoke_ctx_resolve_callee_lane` return a boolean ("locally dispatchable:
yes/no") instead of the three-way lane string, which removes the
Node/Bun/default distinction while preserving the fail-safe-to-host-dispatch
behavior HG2 already guarantees.
