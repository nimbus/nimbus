# CFA6 - Durable Object Primitive Proof

Captured 2026-06-28 on branch `codex/nkv-cloudflare-foundation`.

## Scope landed

- Extended `crates/nimbus-services/src/catalog.rs` with typed Durable Object
  catalog primitives:
  - `DurableObjectNamespace`
  - `DurableObjectId`
  - `DurableObjectInstanceKey`
  - `DurableObjectStorageHandle`
  - `DurableObjectActivationLease`
  - `DurableObjectInstance`
- Exported the new primitives from `crates/nimbus-services/src/lib.rs`.
- Added deterministic `idFromName`-style addressing by hashing
  `(do_namespace, name)` to a canonical 64-hex `DurableObjectId`.
- Added `newUniqueId`-style addressing by hashing `(do_namespace, ulid)` to the
  same canonical 64-hex id shape.
- Added `idFromString`-style validation: exactly 64 ASCII hex characters,
  canonicalized to lowercase.
- Added a `DurableObjectInstanceKey` whose directory key is
  `(tenant_id, do_namespace, do_id)`, not `service_name`.
- Added a `DurableObjectStorageHandle` derived from the typed instance key and
  lease epoch. Storage handle construction uses the authenticated tenant in the
  key; it is not constructible from a wire-supplied 64-hex object id alone.

## Directory and isolation model

The CFA6 directory key is:

```rust
DurableObjectInstanceKey {
    tenant_id,
    namespace,
    id,
}
```

The lead `tenant_id` component is the isolation boundary. A forged
`idFromString` value can only form a storage handle under the caller's
authenticated tenant and namespace. CFA7 will bind the Worker stub constructor
to that same authenticated `(tenant_id, do_namespace)` and add the cross-tenant
RPC/storage denial test.

The per-instance storage prefix is derived from the typed key:

```text
cloudflare/durable-object/<tenant_id>/<do_namespace>/<do_id>
```

That prefix is a skeleton handle for CFA7. CFA6 does not add DO storage
behavior; it fixes the key shape so CFA7/CFA8 do not need a re-keying rewrite.

## Recorded decisions

1. HS5 handoff requires per-DO-ID placement and leasing beneath the tenant
   ownership layer. A tenant-wide lease is insufficient because it collapses a
   tenant's many Durable Objects onto one node and defeats scatter. The HS5
   handoff key is the same `(tenant_id, do_namespace, do_id)` directory key
   introduced here.
2. Serialization granularity is per Durable Object id. CFA7 must give every
   `(tenant_id, do_namespace, do_id)` an independent write lane. The existing
   per-tenant engine journal remains the storage transaction boundary, but it
   must not become the DO write lane because that would serialize unrelated DOs
   in the same tenant.
3. The duplicate-activation contract is epoch-fenced, not absolute. During
   ungraceful failover a transient duplicate activation can exist. Correctness
   rests on a per-instance monotonically increasing `lease_epoch` stored with
   the DO's durable state; every DO write must carry the activation epoch and
   validate it transactionally before commit. CFA7 owns the stale-epoch loser
   write rejection test.

## Tests added

`crates/nimbus-services/src/catalog.rs` covers:

- deterministic `DurableObjectId::from_name` within a namespace;
- `DurableObjectId::from_hex_string` length, hex, and lowercase
  canonicalization;
- namespace rejection for empty, path-containing, and NUL-containing values;
- tenant and namespace participation in `DurableObjectInstanceKey`;
- storage-handle derivation from the typed instance key and lease epoch.

## Verification

- `cargo fmt --all --check`
  - passed.
- `cargo test -p nimbus-services catalog -- --nocapture`
  - `10 passed; 0 failed; 0 ignored; 0 measured; 67 filtered out`.
