# SI1 Spec — enforce the `identity` grant at the mint seam

Design authority: `docs/private/plans/service-identity-provider-auth-plan.md`
SI1 row ("Promote `identity` grant to enforced capability") + SI0 spec
(`si0-spec.md`, sibling file). Inventory facts this design rests on:

- The `identity` grant is `RuntimeGrants.identity: Vec<String>`
  (`nimbus-runtime/src/limits/grants.rs:38`) — today a pure placeholder:
  Restricted-mode ceiling rejects it (`grants.rs:200`), admission routes
  it to the trusted tier (`nimbus-tenant/src/runtime_admission.rs:226`),
  and it is recorded as audit facts — but it maps to NO capability
  (`runtime_capabilities/permissions.rs` has no identity entry).
- No guest-facing identity API exists (zero identity/auth references in
  nimbus-bridge host_calls/abi). `ctx.auth.*` is request-owned by design.
- The ONLY identity API is SI0's mint seam:
  `nimbus_workload_identity::authorize_mint`.
- The decision carries grants via `decision.runtime().grants().identity`
  (`runtime_admission.rs:96,137`).

## SI1 decision (normative)

The `identity` grant becomes a hard precondition of mint authorization:
`authorize_mint` DENIES unless the admitted decision's runtime grants
include at least one `identity` entry. Grant entries are recorded in the
audit event. Audience/subject scoping stays owned by `ProviderAuthPolicy`
(server-owned); the grant is the per-workload capability opt-in, not a
second allow-list. Guest-visible behavior does NOT change — declaring an
identity grant still synthesizes nothing in-guest (that projection is
SI6). Do not touch `runtime_capabilities/permissions.rs`.

## Hard constraints

1. ALL changes live in `crates/nimbus-workload-identity` (+ its tests).
   Do NOT modify nimbus-tenant, nimbus-runtime, nimbus-bridge, or any
   other crate. Do NOT add new dependencies: read the grant through the
   existing public accessors (`decision.runtime().grants().identity`) —
   field access on a transitive-dep type is fine; do not `use` any
   nimbus_runtime path (that would require a new direct dependency; if
   you find you need one, STOP and report).
2. Deny-by-default and unskippable audit are preserved: the new deny
   variant flows through the same `MintAuthorization { outcome, audit }`.
3. The check is capability-presence: empty `identity` grants ⇒ deny.
   Do not invent a grant-string grammar (no parsing of entries like
   `"service:agent-prod"`); entries are recorded opaquely for audit.
4. Check ordering: grant-missing is evaluated FIRST (before subject/
   audience/TTL) — a workload without the capability learns nothing about
   policy shape from the deny reason.

## API changes (exact)

In `mint.rs`:
- `IdentityMintRequest::for_decision` additionally captures (privately)
  `identity_grants: Vec<String>` — a sorted, deduped copy of
  `decision.runtime().grants().identity`.
- New error variant:
  `IdentityMintError::IdentityGrantMissing` with message
  `"workload has no identity grant; identity minting requires an explicit identity grant"`.
- `authorize_claims` returns `IdentityGrantMissing` when the captured
  grants are empty, before any policy matching.

In `audit.rs`:
- `IdentityAuditEvent` gains field `identity_grants: Vec<String>`
  (serialized; tenant-safe opaque strings; empty vec when none). Wire it
  through `IdentityAuditEventParts` and the accessor set.

`lib.rs` docs: update the crate doc to state the SI1 rule (mint requires
an explicit identity grant on the admitted decision).

## Tests (required)

Existing SI0 tests construct decisions via the crate's test fixture —
they will start failing once the grant check lands. Update the fixture so
the default test decision carries `identity: vec!["service:test"]` (find
how the fixture builds `TenantIsolationPolicyInput`/`RuntimeGrants` and
set the field there), keeping all 9 SI0 tests green.

New tests:
1. `mint_denies_without_identity_grant` — decision admitted with EMPTY
   identity grants, valid policy/audience/TTL ⇒
   `Err(IdentityGrantMissing)`; audit event `Denied` with
   `identity_grants: []`; deny reason does NOT mention the policy's
   subjects or audiences.
2. `grant_check_precedes_policy_matching` — empty grants AND a policy
   that would not match either ⇒ error is `IdentityGrantMissing` (not
   `NoMatchingSubjectRule`).
3. `mint_succeeds_with_identity_grant_and_records_grants_in_audit` —
   granted decision mints; audit event carries the sorted grant entries.
4. Serialization: audit JSON includes the `identity_grants` key on both
   mint and deny events; still no secret-shaped keys.

## Verification gates (from the worktree root, in order)

```
cargo fmt --all --check
cargo clippy -p nimbus-workload-identity --all-targets -- -D warnings
cargo test -p nimbus-workload-identity     # 12+ integration tests + compile_fail doctest
cargo check -p nimbus-server
```

Record actual test counts. (The worktree already contains
`packages/nimbus-ui/dist` + `.nimbus` artifacts? If NOT and gate 4 fails
on missing `packages/nimbus-ui/dist/index.html` or `.nimbus/convex/*`,
report it as the known environment gap and continue — the main-checkout
copies live at `/Users/jack/src/github.com/nimbus/nimbus/packages/nimbus-ui/{dist,.nimbus}`
and MAY be copied into the worktree's `packages/nimbus-ui/` to unblock
the gate; that copy is a build artifact, never committed.)

## As built (PR #128, squash-merged `4b1245b8f`, 2026-07-06)

Landed to contract; no deviations.

- `authorize_mint` denies with `IdentityMintError::IdentityGrantMissing`
  unless the admitted decision's runtime grants carry at least one
  `identity` entry. The grant check runs BEFORE any policy matching, so
  an ungranted workload learns nothing about provider-policy shape from
  its deny reason.
- `IdentityMintRequest::for_decision` captures a sorted/deduped private
  copy of the identity grants, read through nimbus-tenant's public
  accessors; zero production `nimbus_runtime` coupling (dev-dependency
  only, for test fixtures).
- `IdentityAuditEvent` gained `identity_grants`, recorded on both mint
  and deny paths; entries stay opaque (no grant-string grammar) —
  audience/subject/TTL scoping remains `ProviderAuthPolicy`-owned.
- Guest-visible behavior unchanged: an identity grant synthesizes
  nothing in-guest (`ctx.auth` stays request-owned); projection is SI6.

Evidence: 13 integration tests + 1 `compile_fail` doctest (the 9 SI0
tests kept green via the fixture grant; 4 new SI1 tests); fmt/clippy
clean; `cargo check -p nimbus-server`; autoreview (Codex) clean
("patch is correct (0.9)").
