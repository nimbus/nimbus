# RRC4 Storage and Recovery

Status: `RRC4_STORAGE_RECOVERY_PROVISIONAL_PASS`

Date: 2026-08-27

## Candidate under test

The storage campaign used the provisional integrated debug binary at
`/tmp/nimbus-ws-test.0rXOFY/worktree/target/debug/nimbus`. It contains the
local Deno WebSocket-egress commits through path dependencies and all RRC4
repairs. These tests are valid defect-discovery evidence, but they are not
exact-candidate evidence. The fixed release matrix stays `UNVERIFIED` until
RRC1 has a reachable immutable Deno reference and a clean candidate repeats
the affected live lanes.

No accepted RRC4 product defect remains open.

## Fail-before ledger

| ID | Severity | Fail-before evidence | Terminal verdict |
|---|---|---|---|
| RRC4-001 | P1 | `nimbus backup create` tried to read an encrypted SQLite database as plaintext and reported `file is not a database`. This looked like corruption and ignored the documented cold-copy recovery contract. | Fixed in `965bfd379`. Backup now resolves encryption configuration and rejects before opening the database. Its error identifies the encrypted-at-rest contract and directs the operator to cold-copy recovery. A unit test proves that no output file is created. |
| RRC4-002 | P2 | A successful plaintext backup emitted `trigger candidate worker failed` because `Engine::quiesce` stopped the committer executor before it joined the trigger-candidate worker. | Fixed in `965bfd379`. Quiesce permanently closes and joins the worker before executor shutdown. A regression also proves that a late start cannot recreate the worker. The real backup replay completed with no warning. |
| RRC4-003 | P1 | redb backup with separate data and control roots opened a new control database under the data root. Export of `_nimbus` failed because the real durable incarnation was not there. | Fixed in `464e30596`. Backup create and restore accept `--control-data-dir`, use the selected control root, and quiesce on error. A regression covers `_nimbus` and an application tenant with separate roots. |
| RRC4-004 | P1 | Object-storage administration ignored a separate control root. This could misplace placement metadata, fail backup incarnation lookup, create restored incarnations in the wrong database, or remove byte-plane data while leaving the actual tenant metadata. | Fixed in `652fb93b6`. Placement, backup, restore, and tenant removal all accept and use `--control-data-dir`. Parse, split-root placement, and ciphertext backup/restore regressions pass. |

An early restore attempt used `--input`; the command requires `--in`. That was
a test-driver error and not a Nimbus defect.

## Provider and durability matrix

The external-provider lane passed with real local fixtures:

```text
NIMBUS_PROVIDER_FIXTURE_POSTGRES_PORT=25432 \
NIMBUS_PROVIDER_FIXTURE_MYSQL_PORT=23306 \
NIMBUS_PROVIDER_FIXTURE_LIBSQL_PORT=28080 \
NIMBUS_PROVIDER_FIXTURE_LIBSQL_ADMIN_PORT=28081 \
make test-external-providers
```

The fixtures used PostgreSQL 16, MySQL 8.4, and libSQL server 0.24.33. All
provider tests passed, and the fixture containers and volumes were removed.
The default PostgreSQL port was already in use by a user process, so the lane
used an isolated port and did not alter that process.

The complete focused storage selection passed:

```text
NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 \
cargo nextest run --profile ci-pr --no-tests fail \
  -E 'package(nimbus-storage) or package(nimbus-blob) or package(nimbus-object-storage)'
```

Nextest metadata selected 90 suites and 854 tests. The focused SQLite physical
durability test also passed. The storage selection includes the erasure-code
contract tests. This run did not require external cloud credentials or live
erasure hardware.

## SQLite encryption and recovery

An encrypted SQLite server used separate data and control roots. The native
driver passed health, fail-closed auth, tenant lifecycle, schema, indexes,
CRUD, pagination, updates, deletes, WebSocket push, scheduler execution, and a
forced full-consistency report. Graceful shutdown returned status 0.

The cold-copy recovery copied the complete encrypted data and control roots
without copying key material into either root. A new server opened the copy
with the original key, retained the durable head, passed restart diagnostics,
and deleted the disposable tenant.

KEK rotation rewrapped three data manifests and one control manifest. SQLite
DEK rotation created its recovery backup and completed. A start with the
retired key failed closed with the provider-identity mismatch. A start with
the new key passed restart durability and diagnostics.

The CLI encrypted-backup rejection returned status 1 and created no output.
The supported cold-copy path passed.

## Plaintext SQLite backup and restore

The native workload passed against plaintext SQLite with separate data and
control roots. Offline backup exported two tenants. Restore into fresh roots
imported both tenants. The restored server retained the updated survivor,
passed diagnostics, and deleted the disposable tenant. The repaired backup
completed without the earlier trigger-worker warning.

The backup and trigger-focused regression selection passed 24 tests. Focused
Clippy for `nimbus-engine` and `nimbus-cli` passed.

## redb encryption, backup, and restore

Encrypted redb passed the complete native workload with separate data and
control roots. DEK rotation re-encrypted 900 pages. The same roots then passed
restart durability, diagnostics, tenant deletion, and graceful shutdown.

Plaintext redb passed the same native workload. After RRC4-003 was repaired,
offline backup exported two tenants and 182176 bytes. Restore imported both
tenants. The restored server retained the survivor and passed diagnostics and
tenant deletion.

All 17 backup tests and focused `nimbus-cli` Clippy passed.

## Object-plane recovery

The live object recovery used the official AWS S3 client against separate data
and control roots. The plan-owned driver is
`rrc3-s3-client/rrc4-object-recovery.mjs`.

The driver wrote two objects, read their exact bytes, and verified their stable
list order. Online garbage-collection status reported two live blobs. An
offline backup attempt while the server held the control-root fence failed
with `Busy` and created no output.

After graceful shutdown, placement update, object-store backup, garbage-
collection inspection, tenant removal, and restore all used the explicit
control root. Backup exported two chunks in a 10426-byte bundle. Tenant removal
removed the byte root. Restore imported two chunks and 177 logical bytes.
Garbage-collection status again reported two live blobs.

The restarted server returned both original objects through the official S3
client. The client deleted them and confirmed an empty list. The server then
shut down with status 0.

All 13 object-storage CLI tests and focused `nimbus-cli` Clippy passed.

## Process fence and documentation

The two `process_fence` unit tests passed. The encrypted embedded-engine
integration test also proved that a second process cannot open the same root.
This covers aliasing, duplicate-root domains, exclusive ownership, release,
and encrypted redb.

The public backup, scale-out, CLI, source-map, and object-storage pages now
describe the split-control-root commands and the offline placement contract.
These gates passed:

```text
bash scripts/check-docs.sh
npm --prefix website run build
bash scripts/verify-nimbus-docs-site.sh
```

The docs verifier reported 17 of 17 checks green. The site build retained its
existing non-fatal markdown-option and missing `docs` entry warnings.

## Terminal RRC4 verdict

SQLite, PostgreSQL, MySQL, libSQL, redb, encryption, backup/restore, object
storage, consistency, physical durability, restart, and process fencing have
direct provisional evidence. All four confirmed defects are fixed and have
regressions. Exact-candidate replay remains blocked only by RRC1's unreachable
Deno commit. The release matrix therefore remains red by design.
