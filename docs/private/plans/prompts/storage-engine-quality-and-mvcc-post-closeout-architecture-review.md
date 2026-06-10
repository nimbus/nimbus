# Post-Closeout Prompt - Storage Engine Quality And MVCC Architecture Review

Use this prompt after `docs/plans/storage-engine-quality-and-mvcc-plan.md`
reaches SEQ14 closeout and the implementation branch has final proof, verifier
output, architecture docs, and a pull request.

This is not a resume prompt for executing the SEQ plan. It is a follow-up
architecture challenge prompt for a fresh Codex agent to audit the completed end
state from first principles before Nimbus treats the storage architecture as an
enterprise-ready baseline.

---

## Prompt

You are reviewing the completed Nimbus storage-engine quality and MVCC work.
Your job is to challenge the final architecture, not to assume the plan was
correct because it completed.

Work from repo evidence, not chat history.

Start by verifying that you are auditing the intended completed work, not
`main`, a stale worktree, or a mismatched PR checkout:

```text
git status --short --branch
git rev-parse HEAD
git rev-parse origin/codex/storage-engine-quality-and-mvcc
gh pr view 13 --json url,state,isDraft,baseRefName,headRefName,headRefOid
```

If `gh` is unavailable, use the GitHub connector or PR page to identify the PR
head. Do not continue silently if local `HEAD`, the remote branch, and PR #13's
head differ. Either switch to the PR head or report the mismatch as the first
finding.

Read these Nimbus docs first:

- `AGENTS.md`
- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `docs/plans/storage-engine-quality-and-mvcc-plan.md`
- `docs/plans/proof/storage-engine-quality-and-mvcc/seq14-closeout.md`
- `docs/architecture/storage/persistence-engine-baseline.md`
- `docs/architecture/storage/provider-topologies.md`
- `docs/architecture/storage/consistency-routing.md`
- `docs/architecture/storage/table-identity.md`
- `docs/architecture/storage/trait-segregation.md`
- `docs/architecture/storage/typed-key-columns.md`
- `docs/architecture/storage/encryption.md`
- `docs/operating/storage-backends.md`
- `docs/adapters/convex/ai-guidelines.md`
- `docs/adapters/convex/compatibility.md`
- `docs/adapters/firebase/compatibility.md`
- `docs/adapters/cloud-functions/compatibility.md`
- `docs/adapters/dynamodb/enterprise-readiness.md`
- `docs/adapters/dynamodb/feature-coverage.md`
- `docs/adapters/mongodb/operations.md`
- `docs/adapters/native/README.md`
- `docs/adapters/native/http-api.md`
- `docs/adapters/native/websocket-protocol.md`
- `docs/adapters/native/errors.md`

Then inspect the final code and verifier evidence that implement the SEQ plan.

Also re-check the local comparison repositories that shaped the plan. Do not
assume the SEQ0 refs are still current without looking:

| Source | Local path | What to re-check |
| --- | --- | --- |
| Convex | `~/src/github.com/get-convex/convex-backend` | application-level snapshots, repeatable timestamps, table/index identity, authorization/dependency tracking |
| CockroachDB | `~/src/github.com/cockroachdb/cockroach` | MVCC timestamp vocabulary, GC/retention errors, metamorphic/iterator testing |
| TigerBeetle | `~/src/github.com/tigerbeetle/tigerbeetle` | deterministic checkpoint/replay discipline and digest-style correctness proofs |
| ElectricSQL | `~/src/github.com/electric-sql/electric` | snapshot-plus-log handoff, shape handles, replica freshness boundaries |
| ExtendDB | `~/src/github.com/ExtendDB/extenddb` | compatibility transparency and backend-owned physical layout behind protocol semantics |

For each source, record `git -C <path> rev-parse --short HEAD` and
`git -C <path> status --short --branch`. If the local source has moved since
`SEQ0`, state whether the newer source changes any conclusion. If a source is
missing, report the missing evidence instead of treating the comparison as
complete.

## Core Question

From first principles, did Nimbus need the architecture that was built?

The intended answer should be evidence-backed, not ideological. Backend MVCC in
Postgres, MySQL, SQLite, libSQL, or redb may provide physical transaction
isolation, but it does not automatically provide Nimbus' portable enterprise
product contract:

- historical reads at an application-level read timestamp
- retained logical document and index versions
- versioned table, schema, index, and read-policy identity
- historical authorization semantics
- stable historical pagination cursors
- PITR/export/import
- CDC snapshot plus log handoff
- typed retention-expired and unsupported-state errors
- deterministic cross-backend logical digest proofs
- adapter-specific honest exposure or fail-closed behavior
- tenant-scoped air-gapped history, CDC, PITR, and retention state

Audit whether the final code implements a minimal Nimbus-owned logical
semantics layer, or whether it accidentally became a duplicate physical storage
engine above other storage engines.

## Architecture Hypothesis To Test

The desired architecture is:

```text
adapter surface
  -> one engine-owned service path
  -> auth / historical policy / transaction session / OCC
  -> Nimbus logical storage semantics
  -> backend-owned physical transactions and layout
```

The desired division of responsibility is:

| Layer | Owns |
| --- | --- |
| Adapters | Protocol shape, compatibility caveats, typed unsupported errors, no private MVCC. |
| Engine | One mutation path, authorization, transaction sessions, dependency tracking, serving snapshots. |
| Nimbus logical MVCC | Commit sequence, read shape, versioned rows, retention, PITR, CDC, logical digests. |
| Storage backends | Physical layout, atomic transactions, backend-native indexes/locks/MVCC where useful. |
| Operator docs/diagnostics | Capability level, retention health, lag, format state, backend/adapter support state. |

Reject an architecture where:

- adapters have private historical-read, PITR, CDC, or retention implementations
- backend MVCC internals are mistaken for a retained product history API
- current-row fast paths are replaced without measured proof
- historical reads use current authorization by accident
- cursors omit tenant, read timestamp, read shape, or retained sequence identity
- retention is a single vague floor instead of inspectable watermarks
- CDC, PITR, and change streams have separate inconsistent cut definitions
- tenant history lives in a global table with tenant filters as the primary
  isolation mechanism

## Specific Improvements To Look For

Check whether the completed architecture clearly names and enforces a storage
semantic contract. If it does not, recommend a follow-up that introduces one.
Useful names may include `NimbusStorageSemantics`, `TenantTimeline`, or another
repo-idiomatic equivalent.

Check whether the final docs and diagnostics expose backend and adapter
capability levels instead of a vague supported/unsupported bit. The current
implementation may expose precise per-feature support states; that is useful
evidence. The review question is whether operators can also derive a clear
coarse capability profile without losing typed per-feature error detail.
Candidate aggregate profiles include:

- `LatestOnly`
- `HistoricalReads`
- `HistoricalReadsPitr`
- `HistoricalReadsPitrCdc`
- `EnterpriseComplete`

The exact names can differ. Do not replace a richer per-feature matrix with one
coarse enum. Instead, flag a gap only if backend/adapter diagnostics and docs
make an enterprise buyer infer capability from scattered booleans, vague
supported/unsupported text, or optimistic adapter claims.

Check whether historical reads resolve a first-class immutable read shape:

```text
ReadShape =
  tenant_id
  principal/auth context
  read timestamp or commit sequence
  table identity
  schema version
  index version
  read-policy version
  cursor identity
```

Check whether PITR and CDC share one cut model:

```text
snapshot cut = retained logical state at sequence N
log handoff = tenant event journal records after N
```

If PITR, CDC, DynamoDB Streams, MongoDB change streams, Cloud Functions triggers,
Firebase Listen replay, or native changefeeds use different cut semantics,
identify it as an architecture issue.

Check whether retention is multi-watermark and tenant-scoped. At minimum,
retention should account for:

- active historical reads
- transaction/session snapshots
- CDC consumers
- PITR/export points
- materialized serving snapshots
- replica freshness
- table/schema/index/read-policy dependencies
- backend format and migration safety

Check whether canonical digest proofs exist for:

- latest state
- selected historical state
- PITR snapshot
- CDC cut
- restored tenant
- cross-backend replay equivalence

## Tenant Isolation And Air-Gapping Checks

Tenant isolation should be the outer boundary, and MVCC should live inside it:

```text
tenant boundary -> tenant timeline -> tenant MVCC versions -> adapter view
```

Reject a design whose primary mental model is:

```text
global MVCC table -> tenant filter
```

Verify every tenant has isolated or tenant-namespaced:

- commit sequence namespace
- event journal
- document and index version storage
- table/schema/index/read-policy history
- retention watermarks
- CDC cursors
- PITR/export/import handles
- encryption key material or key derivation context
- adapter identity mapping

For each backend, verify the final architecture matches the intended isolation
shape:

| Backend | Expected isolation shape |
| --- | --- |
| SQLite/redb | Tenant-local store or file family. |
| Postgres | Tenant schema with fully qualified SQL, not mutable `search_path`. |
| MySQL | Tenant database with fully qualified SQL. |
| libSQL | Tenant namespace plus provider-owned per-tenant cache freshness proof. |
| DynamoDB adapter | Access key maps to exactly one tenant and cannot pivot to system tenants. |
| MongoDB adapter | Database/client context maps to one tenant without cross-tenant cursor escape. |
| Firebase/Convex/native | Route/project/deployment/tenant context resolves before storage access. |

No historical read, CDC stream, PITR bundle, diagnostic, cursor, adapter route,
or support-state endpoint should reveal another tenant's document IDs, table
names, counts, lag, cursor positions, or retained history.

## Adapter Honesty Checks

Audit every adapter surface:

- Convex: compatible semantics plus documented Nimbus extensions only.
- Firebase/Firestore: first-party SDK claims remain distinct from stock
  browser/admin gaps.
- Cloud Functions: trigger events derive from durable tenant events and SEQ CDC
  cuts, not a separate trigger-only history.
- DynamoDB: Streams, TTL, transactions, export/import-adjacent behavior, and
  divergences are explicit; no implied AWS PITR parity unless implemented.
- MongoDB: change streams are backed by real CDC semantics; PITR/historical
  reads are documented extensions or clear unsupported errors.
- Native HTTP/WebSocket: public routes are explicit. Historical reads, PITR, or
  changefeed routes must be typed and documented if exposed; absent public
  routes must remain honest `UnsupportedAdapter` states in diagnostics and docs.

## Verification Expectations

Do not treat the final verifier as sufficient until you inspect what it covers.
For each enterprise guarantee, identify the evidence:

- code path
- test or generated oracle
- proof file
- benchmark/report
- architecture or adapter doc
- verifier condition

If evidence is indirect, missing, too narrow, or only covers one backend while
the claim says all backends, call that out.

## Output Format

Return a structured audit:

```markdown
## Verdict
- Checkout/PR/source-ref state audited.
- Is the final architecture needed?
- Is it a semantic logical MVCC layer rather than duplicated backend MVCC?
- Is it enterprise-ready or still missing trust evidence?

## Evidence Matrix
- Enterprise guarantee -> code path -> test/oracle -> proof/doc -> verifier
  condition.

## Architecture Findings
- [Critical/Major/Minor] Finding with file/line evidence.

## Tenant Isolation Findings
- [Critical/Major/Minor] Finding with file/line evidence.

## Adapter Exposure Findings
- [Critical/Major/Minor] Finding with file/line evidence.

## Performance And Simplicity Findings
- [Critical/Major/Minor] Finding with file/line evidence.

## Recommended Follow-Ups
- Only include follow-ups that materially improve enterprise trust,
  simplicity, performance, or correctness.
```

The strongest possible outcome is not "add more machinery." The strongest
outcome is evidence that Nimbus has one tenant-scoped logical timeline, one
semantic read contract, one atomic write bundle, many backend-owned physical
layouts, honest adapter exposure, and no unnecessary second database engine.
