# DynamoDB Adapter — Hardening Plan

Close the verified correctness, data-integrity, and security gaps found in a
post-merge audit of `crates/nimbus-dynamodb` (shipped via PR #4) so the adapter
is trustworthy for production authentication, streamed/CDC consumers, and
multi-tenant enterprise workloads — not just local-dev parity.

## Status

- **Plan status:** `in_progress` (promoted 2026-05-31 on the
  `dynamodb-adapter-hardening` worktree branch; verifier
  `scripts/verify-dynamodb-adapter-hardening.sh` scaffolded RED at 2 passed,
  9 failed).
- **Active item:** H6 (query/scan robustness on heterogeneous items). H1–H5
  are `done`; H6..H7 are `pending`.
- **Auth decision (D-Auth):** resolved — **strict-by-default** with `LookupOnly`
  surviving only as a loopback-only `insecure_dev_auth` escape hatch (set by the
  `/goal` directive 2026-05-31).
- **Source:** every item below was **verified against the merged code at the
  cited `file:line`** — agent discovery was re-confirmed by direct reading. One
  agent claim (modularity of `query.rs`/`stream.rs`) was **refuted** and is
  excluded (both files are under the 1,500-line soft threshold).
- **Predecessor:** `docs/plans/archive/dynamodb-adapter-plan.md` (the completed
  build; D0.0a..D9.7). This plan is the hardening follow-on, not a rebuild.

## Why this matters (enterprise trust)

The adapter is functionally complete and SDK-proven, but the audit found that
its **write paths are not crash-/concurrency-safe**, **batch/transact writes are
invisible to Streams**, and **strict SigV4 does not actually bind the request
body**. Each of these silently violates a guarantee an enterprise DynamoDB
customer assumes (durable atomic writes, complete change capture, request
integrity). They do not block the merge that already happened, but they must be
closed before the adapter is the system of record for real tenants.

## Verified findings → severity

| ID | Finding | Verified at | Severity |
| --- | --- | --- | --- |
| F1 | Strict SigV4 never verifies `x-amz-content-sha256` == `sha256(body)` → request body is cryptographically unbound | `auth/sigv4/canonical.rs:41-48`, `verify.rs:70-78` | **Critical (security)** |
| F2 | Single-item + catalog writes use non-atomic `delete`+`insert` (crash window loses the row); the atomic `WriteSetMode::Overwrite` primitive *does* exist and is used by transact | `commands/item.rs:64-71,263-272,318-336`, `control_plane.rs:204-215`, `transact.rs:277-280` | **High (data integrity)** |
| F3 | `BatchWriteItem` and `TransactWriteItems` emit **no** stream records (0 `capture_event` calls); same writes via Put/Update/Delete do | `commands/batch.rs`, `transact.rs` (no `capture_event`) | **High (correctness/CDC)** |
| F4 | `DeleteTable` leaks sidecar state (`_ddb_stream_<t>`, `_ddb_streamseq_<t>`, `_ddb_ttl`, `_ddb_tags`); recreate-after-delete inherits a **stale stream sequence high-water mark** | `commands/control_plane.rs` `delete_table` | **High (correctness)** |
| F5 | Auth defaults to `LookupOnly` (no signature/secret check) and `DynamoDbConfig` has no ergonomic strict/secret builder — cross-cutting: the MongoDB sibling is also permissive (see D-Auth), and the adapter *has* a working strict path it just defaults off | `tenant.rs:30-35` (default), `config.rs:47-63` | **High (security), cross-cutting** |
| F6a | No reserved-name guard: an access key can be bound to the global key-store tenant `_nimbus_ddb_system`, exposing every stored credential | `tenant.rs` (`bind`/`with_access_key`), `key_management.rs:26` | **Medium (security)** |
| F6b | `list_access_keys` returns access-key **secrets cleartext over the management API** (redaction gap). At rest the secrets are ordinary documents already covered by the platform `LocalEncryptionConfig` envelope encryption when enabled — so the real gap is the API listing + requiring platform encryption in prod, **not** a missing per-secret scheme | `key_management.rs:189-222` (listing), `persistence_config.rs:24-80` (platform encryption) | **Medium (security)** |
| F7 | Index `Query`/`Scan` aborts the whole request (`ValidationException`) when any item's indexed key attribute is non-scalar/absent, instead of skipping it (DynamoDB's sparse-index semantics) | `commands/query.rs:89,420-421` (`?` propagates) vs `:428-429` (swallowed) | **Medium (correctness)** |
| F8 | Stream sequence counter is a non-atomic read-modify-write across three separate engine calls → duplicate/lost sequence numbers under concurrency | `commands/stream.rs` `capture_event` (`next_sequence_value` → insert → `set_sequence_value`) | **Medium (correctness)** |
| F9 | Conditional single-item writes have a check-then-write TOCTOU (read existing, evaluate condition, then non-transactional write) | `commands/item.rs:48-71,171-186` | **Medium (correctness)** |
| F10 | `parse_iso8601_basic` does not range-validate month/day/hour/min/sec | `auth/sigv4/verify.rs:158-179` | **Low (security)** |
| F11 | `constant_time_eq` early-returns on length mismatch (length is attacker-controlled; `parse.rs` does no length check) | `auth/sigv4/verify.rs:186-188`, `parse.rs:70` | **Low (security)** |
| F12 | `validate_attribute_names` is exported but never called on any write path (only `validate_item` runs) | `commands/item.rs:42,324` | **Low** |
| F13 | No explicit request-body size cap aligned to DynamoDB limits; the body is JSON-parsed before authentication (axum's 2 MB default is the only bound) | `adapters/dynamodb/listener.rs:58`, `dispatch.rs:112-119` | **Low (security)** |
| F14 | The "parity" runner has no executable ground-truth (DynamoDB Local / ExtendDB) lane; the divergences doc's "classifies any unrecorded difference" claim is not backed by code | `crates/nimbus-server/tests/dynamodb_spec/main.rs`, `docs/adapters/dynamodb/divergences.md` | **Test rigor** |
| F15 | The benchmark asserts only `status < 500` (passes on a 4xx) and runs in no CI lane; the soak test proves liveness, not correctness (no read-back) | `benches/operations.rs:~194`, `tests/soak.rs` | **Test rigor** |
| F16 | `DDB-DIV-002` divergence entry says its regression test is "planned" though the tests already exist; `DDB-DIV-005`'s "no atomic upsert" justification is factually wrong | `docs/adapters/dynamodb/divergences.md` | **Doc accuracy** |
| F17 | No replay protection beyond the ±15-minute window (inherent to SigV4); TLS-termination requirement is undocumented | `auth/sigv4/verify.rs:114-142` | **Doc / inherent** |

## Decisions (resolve before/at promotion)

- **D-Auth — set the default in conformance with the cross-cutting auth
  posture, not adapter-locally.** Today's `LookupOnly` (skip verification)
  mirrors DynamoDB Local exactly (accept-any-signature; a developer's dummy
  creds work unchanged) and is *no weaker than the MongoDB sibling*, whose
  `dispatch` (`commands/mod.rs:30-67`) runs `insert`/`find`/`update`/`delete`
  without ever checking `conn.authenticated`. So strict-by-default **raises** the
  bar rather than matching an existing strict practice — and the adapter already
  has the strongest auth *capability* of any Nimbus adapter (a real SigV4
  strict-verification path); it just ships it off by default. Target: make
  `AuthMode::Strict` the production default with ergonomic `DynamoDbConfig`
  builders for signed keys, and keep `LookupOnly` only as an explicit,
  loopback-only, loudly-logged `insecure_dev_auth` escape hatch that preserves
  DynamoDB-Local drop-in parity. **Decision needed:** confirm the trade-off
  (DynamoDB-Local drop-in parity vs. strict-by-default) and set the default in
  conformance with `docs/architecture/server/auth-runtime-trust.md`, ideally
  hardening both adapters together rather than the DynamoDB lane unilaterally.
  (Resolves F5. The F1 body-binding fix is an unambiguous bug and lands
  regardless of this decision.)
- **D-Secret — reuse the platform encryption-at-rest; do not invent a scheme.**
  The access-key store is ordinary documents in a tenant DB
  (`key_management.rs:81-89`, system tenant `_nimbus_ddb_system`), so it already
  inherits `nimbus-engine`'s `LocalEncryptionConfig` envelope encryption —
  `MasterKeyFile` (HKDF-derived) / `KeyDirectory` / **`AwsKms`** wrapping
  per-subject DEKs via the shared manifest-backed wrapped-DEK contract
  (`persistence_config.rs:24-80`, `nimbus-storage::AwsKmsKeyProvider`) — exactly
  like all other persisted data, when encryption is enabled. SigV4 needs the
  secret *recoverable* (it derives the date-scoped signing key), so it must be
  encrypted, not hashed — which the envelope scheme already provides. There is
  therefore **no bespoke sealing key and no per-field cipher to build.** The
  only adapter-specific work is: redact `list_access_keys` so secrets never
  leave over the management API (`lookup` keeps returning the secret internally
  for verification), and require encryption-at-rest (or an external DB with its
  own at-rest encryption) for any production deployment that uses the persisted
  key store. (Resolves F6b.)
- **D-Atomic — one engine transaction per logical write.** Route every
  single-item write, the catalog rewrite, the stream-event + sequence write, and
  the TTL-sweep delete through a single `AtomicWriteBatch` (the primitive
  `transact_write_items` already uses), so data + index + commit-log + stream
  capture commit together. (Resolves F2, F3, F8, F9, and the latent
  `AlreadyExists`→`ResourceInUseException` mis-map.)
- **D-Parity — captured ground-truth corpus.** Since Docker is not always
  present, capture a checked-in DynamoDB-Local golden-response corpus for the
  scenario set and diff Nimbus against it in CI, with the live Docker lane as an
  optional upgrade. (Resolves F14.)

## Roadmap

Statuses: `pending`, `in_progress`, `done`, `blocked`. One item at a time,
verifier-gated, regression test per fix. Ordered by severity then dependency.

| ID | Item | Status | Covers | Completion criteria | Required evidence |
| --- | --- | --- | --- | --- | --- |
| H1 | **SigV4 body-hash binding + auth robustness + strict default** | `done` | F1, F5, F10, F11, F13, F17 | **First (unambiguous bug):** `verify_signature` rejects when `x-amz-content-sha256 != sha256(body)`; `parse_iso8601_basic` range-validates; `constant_time_eq` handles unequal lengths safely; an explicit DynamoDB-aligned body limit is set on the listener; the TLS-termination requirement is documented. **Then (per the D-Auth cross-cutting decision):** `DynamoDbConfig` gains ergonomic `with_auth_mode` + signed-key binders; `AuthMode::Strict` becomes the production default with `LookupOnly` surviving only as a loopback-only `insecure_dev_auth` escape hatch | Tests: a request whose body is swapped under a fixed `x-amz-content-sha256` is rejected `InvalidSignatureException`; a correctly-signed body verifies (already green); out-of-range timestamps rejected; oversize body rejected pre-auth; a lookup-mode config on a non-loopback bind is refused. Parity test through the real SDK still green. |
| H2 | **Atomic single-item + catalog writes** | `done` | F2, F9 | PutItem / UpdateItem / overwrite `store_item` / `UpdateTable` catalog rewrite use a one-element `AtomicWriteBatch` with `WriteSetMode::Overwrite`; conditional writes use `WritePrecondition` (no read-then-write TOCTOU); correct DDB-DIV-005's justification | Tests: a simulated crash/error between the (former) delete and insert can no longer lose the item (assert the row survives an injected mid-write failure or that only the atomic path is used); two concurrent conditional PutItems serialize correctly; existing item/transact tests stay green |
| H3 | **Unified atomic stream capture (incl. batch/transact)** | `done` | F3, F8 | Stream-event emission + sequence allocation fold into the **same** `AtomicWriteBatch` as the source mutation for single-item, BatchWriteItem, **and** TransactWriteItems; the sequence high-water mark advances atomically (monotonic, no duplicates/loss under concurrency) | Tests: a BatchWriteItem and a TransactWriteItems on a stream-enabled table deliver the expected INSERT/MODIFY/REMOVE records via GetRecords (new parity + unit tests); concurrent writers produce strictly increasing, gap-checked sequence numbers |
| H4 | **Table-lifecycle sidecar reclamation** | `done` | F4 | `DeleteTable` drops `_ddb_stream_<t>`, `_ddb_streamseq_<t>`, the `_ddb_ttl` doc, and `_ddb_tags` entries; recreate-after-delete starts a fresh stream sequence at 0 | Tests: create→write→DeleteTable→recreate same name→stream sequence restarts and no orphaned events/tags/TTL config remain |
| H5 | **Credential-store hardening** | `done` | F6a, F6b | `bind`/`with_access_key`/`bind_signed`/`put_access_key` reject a reserved-prefix tenant (`_nimbus_*`) and `resolve_binding` refuses a stored key whose tenant is reserved; `list_access_keys` returns a redacted (secret-free) view; **at-rest protection is delegated to the platform `LocalEncryptionConfig` — no bespoke cipher** — and production use of the persisted key store is documented as requiring encryption-at-rest (or an external DB with its own at-rest encryption) | Tests: binding a key to `_nimbus_ddb_system` is rejected; a request authenticated against the reserved tenant cannot read `_ddb_access_keys`; `list_access_keys` never emits a secret; with `LocalEncryptionConfig` enabled, the access-key store's on-disk bytes contain no plaintext secret (proving it rides the platform envelope encryption rather than a per-secret scheme) |
| H6 | **Query/Scan robustness on heterogeneous items** | `pending` | F7 | An item whose indexed key attribute is absent or non-scalar is **skipped** (sparse-index semantics), not erroring the whole request; `sortable_eq` and `sort_cmp` behave consistently | Tests: a GSI Query over a table containing items with the indexed attribute missing / set to `M`/`L`/`BOOL`/`NULL` returns only the matching scalar items and does not raise `ValidationException` |
| H7 | **Evidence rigor + doc accuracy** | `pending` | F12, F14, F15, F16 | A captured DynamoDB-Local golden corpus is diffed against Nimbus in CI (Docker lane optional); the bench asserts the expected status and is wired to a CI lane enforcing the 2×-p99 thresholds; the soak test adds read-back correctness invariants; `validate_attribute_names` is wired into the write path or removed; DDB-DIV-002/005 doc text corrected; the "classification" language matches executable reality | Evidence: a committed golden-corpus diff report; a CI bench-regression lane; soak read-back assertions; the corrected divergence entries |

## Completion gate

The plan is complete when every H-item is `done` and:

1. Each fix carries a named regression test that fails against today's code and
   passes after the fix.
2. `cargo test -p nimbus-dynamodb` and
   `cargo test -p nimbus-server --test dynamodb_spec` are green, including new
   atomic-write, batch/transact-stream, sidecar-cleanup, sparse-index, and
   body-binding tests.
3. `docs/adapters/dynamodb/divergences.md` is factually correct (no claim that an
   available primitive is unavailable); the feature-coverage and
   enterprise-readiness docs reflect the hardened behavior.
4. A ground-truth (DynamoDB-Local golden) comparison runs in CI.
5. `cargo fmt --all --check`, `make clippy`, `make deny`,
   `make verify-third-party-attribution`, `npm run docs:validate-refs:strict`,
   and `git diff --check` all pass.
6. When promoted to `/goal` execution, a `scripts/verify-dynamodb-adapter-hardening.sh`
   verifier encodes conditions 1–5 and prints `N passed, 0 failed`.

## Execution log

| Date | Item | Status | Notes | Commands / evidence |
| --- | --- | --- | --- | --- |
| 2026-05-31 | — | `pending` | Plan authored from the post-merge audit. Every finding verified at the cited `file:line` against the merged adapter; the modularity claim was refuted and excluded. Awaiting promotion. | Audit + line-level re-verification of F1–F17. |
| 2026-05-31 | D-Secret / D-Auth | `pending` (revised) | **Aligned the encryption + auth decisions with existing platform practice** after confirming the repo already has unified envelope-encryption-at-rest. Dropped the bespoke "sealing key" from D-Secret/H5 — the access-key store inherits `nimbus-engine::LocalEncryptionConfig` (master-key-file / key-dir / AWS-KMS wrapped-DEK via `nimbus-storage::AwsKmsKeyProvider`) like all persisted data; F6b re-scoped to the `list_access_keys` redaction + a prod encryption-at-rest requirement. Reframed D-Auth/H1: the body-binding fix (F1) is an unambiguous bug and lands first; strict-by-default (F5) is a *cross-cutting* posture decision (the MongoDB sibling is also permissive — `commands/mod.rs:30-67` runs data commands without an `authenticated` check), to be set per `docs/architecture/server/auth-runtime-trust.md` with the DynamoDB-Local-parity trade-off flagged for a decision. | Read `nimbus-engine/src/persistence_config.rs`, `nimbus-storage/src/lib.rs` (`AwsKmsKeyProvider`, `encrypted_redb`), `nimbus-mongodb/src/lib.rs` (`AuthConfig`) + `commands/mod.rs` dispatch. |
| 2026-05-31 | H5 | `done` | **Credential-store hardening.** Added `tenant::is_reserved_tenant` (`_nimbus` prefix); `AccessKeyRegistry::binding` now refuses a key (mis-)bound to a reserved tenant, `put_access_key` rejects a reserved tenant at write, and `dispatch::resolve_binding` refuses a stored key whose tenant is reserved — three layers so no request can pivot to the global `_ddb_access_keys` catalog (F6a). `list_access_keys` now returns a secret-free `RedactedAccessKey { tenant, region }` view — the secret is never read back over a listing surface (F6b). Documented (module docs + enterprise-readiness) that at-rest protection rides the platform `LocalEncryptionConfig` envelope encryption (no bespoke cipher; enable in prod). | `cargo test -p nimbus-dynamodb` (253 pass: 241 unit + 7 + 1 + 4); `dynamodb_spec` 27/27; fmt + clippy clean; verifier HC7 green. New tests: `put_access_key_rejects_a_reserved_tenant`, `list_access_keys_redacts_secrets`, `binding_refuses_a_reserved_tenant`, `is_reserved_tenant_flags_the_internal_prefix`. |
| 2026-05-31 | H4 | `done` | **DeleteTable reclaims every sidecar.** `delete_table` now calls `stream::reclaim_for_table` (drops the `_ddb_stream_<t>` event store **and** the `_ddb_streamseq_<t>` high-water counter), `ttl::reclaim_for_table` (drops the `_ddb_ttl` config doc), and `tag::reclaim_for_table` (drops the `_ddb_tags` entry) — each owned by its module, after the data-item reclaim and before the catalog delete (F4). A table recreated under the same name now starts a fresh stream at sequence 0 with no orphaned events/TTL/tags. | `cargo test -p nimbus-dynamodb` (249 pass: 237 unit + 7 + 1 + 4); `dynamodb_spec` 27/27; fmt + clippy clean; verifier HC6 green. New test: `delete_table_reclaims_sidecars_so_recreate_starts_fresh` (create→write→seed TTL/tags→DeleteTable→assert all four sidecars gone + counter 0→recreate→write→sequence restarts). |
| 2026-05-31 | H3 | `done` | **Unified atomic stream capture + atomic sequence allocation.** `capture_event` now writes the event document and the advanced high-water counter in a **single** `AtomicWriteBatch`; the event uses `WriteSetMode::Create` keyed by its sequence number, so a concurrent writer that claimed the number first fails the commit (`AlreadyExists`) and we re-read + retry — strictly monotonic, gap-free, no-duplicate sequences under concurrency, replacing the former non-atomic read/insert/set across three engine calls (F8). BatchWriteItem now emits a stream record per write (reads the prior image for correct INSERT vs MODIFY vs REMOVE), and TransactWriteItems **folds** its events + per-table counter advances into the same atomic session commit batch (a lost sequence race surfaces as `TransactionConflictException`) — both previously emitted zero records (F3). Centralized `stream::stream_event_write` / `sequence_counter_write` / `stream_enabled` builders. | `cargo test -p nimbus-dynamodb` (248 pass: 236 unit + 7 + 1 + 4); `cargo test -p nimbus-server --test dynamodb_spec` (27/27, incl. official streams SDK); fmt + clippy clean; verifier HC5 green (also fixed verifier `-E`/soundness defects: HC5/HC6/HC7/HC8/HC9 alternations were BRE-literal `|`, and HC8 falsely matched the pre-existing absent-attr "sparse" handling — retargeted to the non-scalar marker). New tests: `batch_write_emits_stream_records`, `transact_write_emits_stream_records`, `capture_event_allocates_monotonic_sequences`. |
| 2026-05-31 | H2 | `done` | **Atomic single-item + catalog writes landed.** PutItem / UpdateItem / `store_item` / the UpdateTable catalog rewrite now run as a one-element `AtomicWriteBatch` with `WriteSetMode::Overwrite` via `begin_mutation_execution_unit().execute_atomic_write_batch()` — a single storage transaction replacing the former non-atomic `delete`+`insert` crash window (F2). New `item::atomic_overwrite` / `atomic_delete` helpers; the catalog rewrite reuses `atomic_overwrite`. Conditional writes pin the snapshot's existence with `WritePrecondition::exists(..)`, closing the check-then-write TOCTOU (F9, existence-level OCC matching the transactional path); a lost precondition race maps to `ConditionalCheckFailedException` (not ResourceInUse/NotFound). `DDB-DIV-005` corrected — the "no atomic upsert" justification was factually wrong; it now documents the atomic Overwrite + precondition and drops the follow-up. | `cargo test -p nimbus-dynamodb` (245 pass: 233 unit + 7 + 1 + 4); `cargo test -p nimbus-server --test dynamodb_spec` (27/27); fmt + clippy clean; verifier HC4 green. New tests: `atomic_overwrite_enforces_existence_precondition`, `store_item_overwrites_atomically`. |
| 2026-05-31 | H1 | `done` | **SigV4 body-binding + auth robustness + strict-by-default landed.** `verify_signature` now re-derives `sha256(body)` and rejects a mismatch against `x-amz-content-sha256` (and refuses `UNSIGNED-PAYLOAD`) — closing F1; `parse_iso8601_basic` range-validates each field (F10); `constant_time_eq` is length-safe (F11); the listener caps bodies at 16 MiB and returns 413 pre-auth (F13); the TLS-termination + transport-security posture is documented in `enterprise-readiness.md` (F17). `AuthMode::Strict` is now the **default** (F5); `DynamoDbConfig` gained `with_signed_access_key` / `with_auth_mode` / `insecure_dev_auth`; the lookup escape hatch is loopback-only (refused on non-loopback binds via `guard_lookup_is_loopback_only`, enforced in `construction.rs`). The parity suite was converted to **strict** (binds the SDK secret) so all 27 official-SDK scenarios now verify real signatures end to end. | `cargo test -p nimbus-dynamodb` (243 pass: 231 unit + 7 failure-injection + 1 soak + 4 isolation); `cargo test -p nimbus-server --test dynamodb_spec` (27/27 strict); listener unit tests 7/7 (incl. oversize-413 + loopback-guard); `cargo fmt --all --check` clean; `cargo clippy -p nimbus-dynamodb -p nimbus-server --all-targets` clean; verifier HC2+HC3 green. New tests: `tampered_body_is_rejected`, `strict_is_the_default_mode`, `lookup_mode_is_flagged_insecure`, `auth_mode_is_strict_by_default`, `insecure_dev_auth_opts_into_lookup`, `with_signed_access_key_binds_a_verifiable_key`, `with_auth_mode_sets_mode_explicitly`, `oversize_body_is_rejected_before_auth`, `guard_allows_loopback_lookup_and_strict_anywhere`, `guard_refuses_lookup_on_a_routable_address`. |
