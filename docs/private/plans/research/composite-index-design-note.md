# Composite Index Design Note

This note is the `SA4` design gate from
`docs/plans/archive/scalability-and-architecture-follow-on-plan.md`.

It describes the intended shape for composite indexes before broad
implementation starts.

## Current State

Today Nimbus supports only single-field secondary indexes:

- schema shape: `IndexDefinition { name, field }`
- storage key shape: `table\0index\0<encoded-value><doc-id>`
- planner shapes:
  - exact equality on one indexed field
  - range scan on one indexed field
  - otherwise full table scan
- pagination cursors carry one optional sort value plus document id
- dependency tracking can record one-field index ranges, predicates, and
  paginated windows

This is enough for `by_status == "open"` or `by_rank >= 10`, but not for
common shapes like `status == "open" ORDER BY rank`.

## Goals

- make composite indexes first-class schema objects
- preserve the existing single-field encoding and scan semantics as the
  one-field case of the new representation
- allow Stage A to land without forcing planner and cursor rewrites in the same
  patch
- keep correctness conservative when planner support lands, even if dependency
  narrowing comes later

## Non-Goals

- no attempt to support arbitrary SQL-style index planning
- no multiple-range-field plans in the first composite slice
- no backwards-compatibility shims for old schema payloads beyond whatever
  serde transition is needed while this branch is in flight

## Proposed Schema Shape

Use one schema type for both single-field and composite indexes:

```rust
pub struct IndexDefinition {
    pub name: String,
    pub fields: Vec<String>,
}
```

Rules:

- `fields` must be non-empty
- field order is significant
- field names must be unique within one index
- every referenced field must exist on the table schema
- every referenced field must be scalar and indexable
  - `string`
  - `number`
  - `boolean`
- a one-field index is represented as `fields.len() == 1`

Example:

```json
{
  "name": "by_status_rank",
  "fields": ["status", "rank"]
}
```

## Storage Key Encoding

Keep the existing logical prefix:

```text
<table>\0<index-name>\0
```

After that prefix, append the encoded value for each indexed field in order,
then append the document id as the final tie-breaker:

```text
<table>\0<index>\0<field-1><field-2>...<field-n><doc-id>
```

This works because the current per-field scalar encodings are
order-preserving and self-delimiting:

- `null`, `bool`, and `number` are fixed-width after the type tag
- strings are terminated with the existing escaped `0x00 0x00` sentinel

Consequences:

- single-field indexes keep the exact same key layout they already use
- exact-prefix scans can reuse the existing prefix scan pattern
- a composite scan can target:
  - full exact tuples
  - exact prefix on leading fields
  - one range on the next field after the exact prefix

## Missing-Field Semantics

Index entries should only exist when all indexed fields are present on the
document.

Chosen behavior:

- missing any indexed field: omit the index entry entirely
- explicit `null`: index it as a real scalar value

This matches the current single-field behavior and keeps range and prefix scans
predictable.

## Write Maintenance And Backfill

All write paths should share one helper that derives the full composite key for
one document:

- returns `None` when any indexed field is missing
- otherwise returns the concatenated encoded tuple key

That helper should be used by:

- insert with indexes
- update with indexes
- delete with indexes
- execution-unit batch apply
- schema replacement and backfill

Backfill behavior stays transactional:

- replacing a table schema removes old index keys for that table
- scans all table documents
- rebuilds keys for the new index definitions
- writes schema payload plus rebuilt keys in the same redb transaction

## Planner Matching Rules

The first composite planner slice should support only the shapes we can reason
about cleanly:

1. Exact prefix equality
   - index `["status", "rank"]`
   - query filters `status == "open"`
2. Exact prefix equality plus one range on the next field
   - index `["status", "rank"]`
   - query filters `status == "open" AND rank >= 10`
3. Exact prefix equality plus `ORDER BY` on the next field
   - index `["status", "rank"]`
   - query filters `status == "open"`
   - query order `rank ASC|DESC`

Initial exclusions:

- skipping leading index fields
- multiple range fields
- range on one field plus ordering on a later field
- planner shapes that need statistics or selectivity estimation

Planning policy:

- prefer the longest exact-prefix match
- prefer plans that satisfy requested ordering over plans that still require an
  in-memory resort
- strip residual filters only for predicates fully consumed by the exact prefix
  and one supported range field
- keep unsupported trailing predicates as residual filters

## Cursor Encoding And Ordering Stability

Current cursor payloads carry one `sort_value` plus document id.

Composite plans need a generalized boundary payload:

- `sort_values: Vec<Option<Value>>`
- `doc_id`
- `query_signature`

`sort_values` represents the effective ordered tuple used by the plan, not
just the user-visible `ORDER BY` field.

Examples:

- full scan ordered by `rank`: `[rank]`
- composite scan for `status == "open" ORDER BY rank`: `[status, rank]`

Document id remains the final tie-breaker so pagination stays deterministic
when indexed values are duplicated.

Stage C can switch existing one-field cursors to the generalized tuple format
directly because the project is still pre-launch.

## Dependency Tracking And Invalidation

Correctness comes first; narrowing can lag planner support.

Initial conservative rule when composite plans land:

- continue recording predicate dependencies and paginated-window dependencies
  exactly as today
- do not rely on composite-specific narrowing for correctness in the first
  planner slice

That means:

- subscriptions stay safe even if invalidation is broader than ideal
- execution-unit OCC stays safe even if conflict checks read more commit log
  entries than strictly necessary

Once planner behavior is stable, a follow-on narrowing can extend
`IndexRangeDependency` or add a composite-aware dependency type carrying:

- `index_name`
- ordered `fields`
- exact-prefix values for leading fields
- optional lower/upper bound on the next range field

Until then, composite-index-backed reads should remain conservative.

## Stage Breakdown

### Stage A

- change schema representation to `fields: Vec<String>`
- update validation rules
- update storage key construction and backfill helpers
- preserve current single-field scan behavior

### Stage B

- add composite planner support for exact-prefix and one-range-next-field
  shapes
- strip only the filters the chosen composite index fully satisfies
- keep unsupported shapes on fallback or single-field planning

### Stage C

- generalize cursor payloads to ordered tuples
- add composite planner pagination coverage
- keep dependency tracking conservative at minimum
- optionally add composite-specific invalidation narrowing if it stays clearly
  correct and scoped

If Stage B or Stage C expands materially beyond this note, split `SA4` into
explicit sub-items in the plan before continuing.

## Main Risks

- schema churn is wide because `IndexDefinition` appears across core, storage,
  engine, tests, and Convex-compatible surfaces
- cursor generalization is the biggest semantic change because it affects
  pagination stability and compatibility-style differential tests
- dependency narrowing is easy to get subtly wrong; it should lag planner
  adoption rather than block it
