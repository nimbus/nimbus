# SUC5.1 — DynamoDB surface stops executing as the system principal

Branch: `codex/suc5-dynamodb-principal` (base `origin/main` @ `b567fef32`)
Crates: `crates/nimbus-dynamodb`, `crates/nimbus-tenant`, `crates/nimbus-engine`

## Outcome

The DynamoDB surface now executes every engine call on a user table as the
authenticated SigV4 access key, and every engine call on its own reserved
bookkeeping stores as an explicit system principal. Two defects were fixed on
the way; one of them is a real read-authorization bypass that the plan did not
predict.

## Current-state finding

The plan's assumption — "DynamoDB executes as the system principal" — is true
but incomplete. What was actually on `main`:

**1. One system-principal source, three re-assertions.** `tenant.rs::tenant_context()`
built every request's `TenantIsolationContext` with `TenantIsolationAuthority::System`,
and `item.rs`, `transact.rs`, and `stream.rs` additionally passed a literal
`PrincipalContext::system()` into `begin_transaction_session` /
`begin_mutation_execution_unit`. The authenticated access key id was parsed on
the dispatch path, used to resolve the tenant binding, and then discarded.

**2. Direct-path engine calls carried no principal at all.** Every adapter call
to `get_document`, `insert_document_with_id`, `update_document`,
`delete_document`, and `query_documents_structured` used the non-`_with_principal`
overload, which defaults to `MutationActor::anonymous()` / an anonymous read
principal. So the surface was not uniformly "system": the transaction/execution-unit
paths ran as system (bypassing policy) while the direct reads and writes ran as
anonymous (failing policy). Neither is the caller.

**3. The partition prefix scan bypassed `ReadAuthorization` entirely.** This is
the security-relevant one. `Engine::scan_documents_by_id_prefix_cancellable`
read straight from the store with no principal parameter and no policy filter.
DynamoDB `Query` (partition-key reads) routes through it, so a table whose
`TableAccessPolicy` denied a caller still returned that caller every row in the
partition. Fixing this required an engine change, not just an adapter change.

## Design

The mapping is the smallest one that reuses existing primitives. No new
registry, no new trait, nothing generic — `TenantBindingRegistry` stayed out of
scope as instructed.

### Identity material → `PrincipalContext`

A DynamoDB request's only identity material is the SigV4 access key id, already
extracted and verified on the dispatch path and already bound to a tenant by
`AccessKeyRegistry`. `tenant.rs::access_key_principal(access_key_id, tenant)`
turns that into a `PrincipalContext`:

- `claims`: `subject` = `sub` = `aws_access_key_id` = the access key id,
  `provider` = `"dynamodb"`
- `verified_claims`: `tenant_id` = the bound tenant

Three claim spellings for the same value so a `TableAccessPolicy` can name the
caller with whichever `PrincipalClaimSource::Identity` claim its author reaches
for; `tenant_id` goes in `verified_claims` because the binding is server-side
truth, not caller-asserted. This mirrors how MongoDB and Firestore map their
authenticated callers.

`tenant_context()` was **deleted**, not deprecated — pre-launch posture, no
compat shim. It is replaced by three explicit constructors:

| function | authority | used by |
| --- | --- | --- |
| `request_context(tenant, principal, surface)` | `Application { principal }` | every authenticated request, built once in `dispatch.rs::finish_authentication` |
| `maintenance_context(tenant, surface)` | `System` | TTL sweeper drivers, the key store |
| `caller_principal(&context)` | — | reads the application principal back out, falling back to system for a maintenance context |
| `adapter_principal()` | — | `PrincipalContext::system()`, for the adapter's own reserved stores |

`request_context` calls `require_matching_principal_claim` so a principal can
never be threaded into a context for a tenant it is not bound to; that is an
internal invariant, defended by a unit test.

`nimbus-tenant` gained one accessor, `TenantIsolationContext::application_principal()
-> Option<&PrincipalContext>`, returning `Some` only for `Application` authority.
`caller_principal` is built on it.

### Where each principal is used

**Caller principal** (`caller_principal(context)`) — everything that touches a
user's table: `item.rs` single-item transactions and `read_item`, `query.rs`
`enumerate` and `enumerate_query_partition`, `transact.rs` at both transaction
sites, `stream.rs` user-table writes (`execute_atomic_write_batch_with_streams`,
`stage_base_writes`), `ttl.rs` `sweep_table`'s query over the user table.

**Adapter principal** (`adapter_principal()`) — the reserved stores the adapter
owns and users cannot address: `_ddb_catalog` (`control_plane.rs`), `_ddb_ttl`
(`ttl.rs`), `_ddb_tags` (`tag.rs`), `_ddb_stream_*` / `_ddb_streamseq_*`
(`stream.rs` sidecar work: `delete_all`, `next_sequence_value`,
`set_sequence_value`, `reclaim_expired_events`), and `_nimbus_ddb_system`
(`key_management.rs`). These previously ran **anonymous**, so making them
explicitly system is a strict improvement, and it keeps control-plane
bookkeeping out of reach of user-authored policies.

### Engine change

`scan_documents_by_id_prefix_cancellable` now takes `principal: &PrincipalContext`,
resolves `ReadAuthorization::for_table(schema.get_table(table), principal)`,
returns empty on `authorization.impossible`, and post-filters the scanned
documents through `allows_document`. This is the same shape the other read APIs
already use.

## Deviations

Three deliberate departures from a naive "everything runs as the caller" rule.
Each is documented in the code at the call site.

**`reclaim_table_items` (DeleteTable) runs as the adapter, not the deleting
caller.** The table's access policy is being torn down with it; honoring it
during reclamation would leave behind exactly the rows the caller cannot see or
delete — orphaned storage that a table later recreated under the same name would
inherit. Whether the caller may delete the table at all is the control-plane
question, and it is answered earlier by the access key's tenant binding.

**The TTL sweeper runs as system.** Its drivers build a `maintenance_context`,
so `caller_principal` yields system inside `sweep_table`. Expiry is a contract of
the table, not an act of a caller; a delete policy must not be able to pin
expired items in storage forever. There is no authenticated caller on that path
in any case.

**`scan_documents_by_id_starting_at_cancellable` was left unchanged.** It is
limit-bearing, and post-filtering a limited page would silently truncate results
— a correctness bug traded for a security fix that path does not need: its only
caller is the adapter-owned `_ddb_stream_*` sidecar, which legitimately runs as
the adapter. Making it policy-aware requires filter-then-fill paging, which is
out of scope here.

One more note on scope: `key_management.rs::lookup_async` still uses
`get_document_async`, because no `_with_principal` async read variant exists. It
reads the key store in the reserved `_nimbus` tenant on the authentication path
*before* any caller identity exists, so system authority is correct there
regardless.

## Fail-before evidence

`crates/nimbus-dynamodb/tests/principal_authorization.rs` binds two access keys,
`AKIAOWNER` and `AKIAOTHER`, to the **same** tenant `acme` in
`AuthMode::LookupOnly`. Same tenant on purpose: nothing in these tests is
provable by tenant scoping alone, so a pass can only come from the principal
actually reaching the engine.

Against the unmodified `main` code, 4 of 5 tests fail:

```
Summary [   0.755s] 5 tests run: 1 passed, 4 failed, 0 skipped
   FAIL [   0.669s] (1/5) nimbus-dynamodb::principal_authorization scan_runs_as_the_calling_access_key
   FAIL [   0.683s] (2/5) nimbus-dynamodb::principal_authorization get_item_runs_as_the_calling_access_key
   FAIL [   0.687s] (3/5) nimbus-dynamodb::principal_authorization put_item_runs_as_the_calling_access_key
   PASS [   0.748s] (4/5) nimbus-dynamodb::principal_authorization an_unauthenticated_request_never_reaches_the_engine
   FAIL [   0.749s] (5/5) nimbus-dynamodb::principal_authorization query_runs_as_the_calling_access_key
```

The four failures are two distinct defects.

**Anonymous, not the caller** — the named access key is refused its own data:

```
put_item_runs_as_the_calling_access_key
  assertion `left == right` failed: the access key the create policy names must be able to write:
  {"__type":"com.amazonaws.dynamodb.v20120810#AccessDeniedException","message":"permission denied: create access denied"}
    left: 400
   right: 200

get_item_runs_as_the_calling_access_key
  assertion `left == right` failed: the access key the read policy names must be admitted,
  which requires the adapter to call the engine as that caller: {}
    left: Null
   right: "s"

scan_runs_as_the_calling_access_key
  assertion `left == right` failed: the named caller must see its own rows in a Scan:
  {"Items":[],"Count":0,"ScannedCount":0}
    left: Number(0)
   right: 1
```

**Unauthorized read** — the *wrong* access key is served the partition anyway,
because the prefix scan never consulted the policy:

```
query_runs_as_the_calling_access_key
  assertion `left == right` failed: the partition read must enforce the table's read policy
  against the caller, not scan storage unauthorized:
  {"Items":[{"pk":{"S":"a"},"secret":{"S":"s"},"sk":{"S":"1"}},
            {"pk":{"S":"a"},"secret":{"S":"s"},"sk":{"S":"2"}}],"Count":2,"ScannedCount":2}
    left: Number(2)
   right: 0
```

`an_unauthenticated_request_never_reaches_the_engine` passed before the change
and after it: the adapter's SigV4 contract already rejected unsigned requests at
dispatch. It is pinned here so that stays true.

Full RED capture (314 lines, `rc=100`) was taken before any fix landed.

The engine-level fail-before for the prefix-scan bypass was attempted by
temporarily stripping the `ReadAuthorization` filter out of `query_api.rs`; that
edit was **blocked by the permission classifier** and was not worked around.
`query_api.rs` was verified byte-identical to its pre-probe copy afterwards. The
same defect is already demonstrated end-to-end by
`query_runs_as_the_calling_access_key` above, which is the stronger evidence
anyway — it exhibits the bypass through the real DynamoDB surface rather than a
synthetic engine call.

## Regression pins added

- `crates/nimbus-dynamodb/tests/principal_authorization.rs` — 5 tests across
  PutItem, GetItem, Query, Scan, and the unauthenticated path.
- `crates/nimbus-engine/src/tests/policy.rs::engine_read_policy_filters_the_id_prefix_scan`
  — seeds `part-a#1`/`part-a#2`/`part-b#1` owned by `user-123`/`user-456`/`user-123`
  under `read_only_owner_policy()`, then asserts the owner sees only its own row
  from prefix `part-a#`, a stranger sees none, and `PrincipalContext::anonymous()`
  sees none.
- `crates/nimbus-dynamodb/src/tenant.rs` — `request_context_carries_the_calling_access_key`,
  `request_context_refuses_a_principal_bound_to_another_tenant`,
  `maintenance_context_has_no_caller_and_runs_as_system`.
- `crates/nimbus-tenant/src/tests.rs` — `application_principal()` returns the
  principal for `Application` authority and `None` for Operator/System.

## Verification

All commands run in `/Users/jack/src/github.com/nimbus/nimbus-suc5` with
`set -o pipefail` and an explicit exit code.

| command | result |
| --- | --- |
| `cargo check --workspace --all-targets` | rc=0 |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo nextest run -p nimbus-dynamodb --test principal_authorization` | **5 tests run: 5 passed, 0 skipped** — rc=0 (was 1 passed / 4 failed) |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo nextest run -p nimbus-dynamodb` | **277 tests run: 277 passed, 0 skipped** (14.095s) — rc=0 |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo nextest run -p nimbus-tenant -p nimbus-engine` | **757 tests run: 757 passed (2 slow), 5 skipped** (81.343s) — rc=0 |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo nextest run -p nimbus-server -E 'test(dynamodb) or test(ddb)'` | **15 tests run: 15 passed (1 leaky), 590 skipped** — rc=0 |
| `cargo clippy -p nimbus-dynamodb -p nimbus-tenant -p nimbus-engine --all-targets -- -D warnings` | rc=0, zero crate-local findings |
| `cargo fmt --all --check` | rc=0 |

The 5 skips in the engine/tenant lane and the 590 in the server lane are the
lanes' own pre-existing filters, not skips introduced here.
