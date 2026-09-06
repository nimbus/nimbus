# RRC4 Storage and Recovery

Status: `RRC4_STORAGE_RECOVERY_PASS`

Date: 2026-09-06

## Candidate under test

The terminal provider replay used Nimbus
`68855f172fc8e5c2fdc333e85b6dae351491d532`, Deno
`95413e012ee9f73e7f652e1e7b1ad9e351b9a8df`, and upstream baseline
`b57a2d680891de852d5576e65ccaea787b005431`. The rebuilt debug binary reports
Nimbus 0.1.46 and has SHA-256
`e087377289de29df70dae9d1253b74b7ab2f8b0ecafe7cf77e15d98c89c53c72`.

The final candidate, Nimbus
`7d0ca18a709d1b78e087bc4a69c8a96bee6f32b9`, changes only the server router
preparation task boundary after that replay. Its complete 770-test server
suite, native restart smoke, nine-application lane, and direct S3 application
path pass. No storage implementation or provider dependency changed.

The earlier integrated candidate supplied the live encryption, backup,
restore, redb, and object-recovery defect-discovery evidence below. The exact
candidate repeats those contracts through the terminal storage and CLI test
sets. It also repeats the live SQLite restart and direct S3 protocol paths.

No accepted RRC4 product defect remains open.

## Fail-before ledger

| ID | Severity | Fail-before evidence | Terminal verdict |
|---|---|---|---|
| RRC4-001 | P1 | `nimbus backup create` tried to read an encrypted SQLite database as plaintext and reported `file is not a database`. This looked like corruption and ignored the documented cold-copy recovery contract. | Fixed in `965bfd379`. Backup now resolves encryption configuration and rejects before opening the database. Its error identifies the encrypted-at-rest contract and directs the operator to cold-copy recovery. A unit test proves that no output file is created. |
| RRC4-002 | P2 | A successful plaintext backup emitted `trigger candidate worker failed` because `Engine::quiesce` stopped the committer executor before it joined the trigger-candidate worker. | Fixed in `965bfd379`. Quiesce permanently closes and joins the worker before executor shutdown. A regression also proves that a late start cannot recreate the worker. The real backup replay completed with no warning. |
| RRC4-003 | P1 | redb backup with separate data and control roots opened a new control database under the data root. Export of `_nimbus` failed because the real durable incarnation was not there. | Fixed in `464e30596`. Backup create and restore accept `--control-data-dir`, use the selected control root, and quiesce on error. A regression covers `_nimbus` and an application tenant with separate roots. |
| RRC4-004 | P1 | Object-storage administration ignored a separate control root. This could misplace placement metadata, fail backup incarnation lookup, create restored incarnations in the wrong database, or remove byte-plane data while leaving the actual tenant metadata. | Fixed in `652fb93b6`. Placement, backup, restore, and tenant removal all accept and use `--control-data-dir`. Parse, split-root placement, and ciphertext backup/restore regressions pass. |

An early restore attempt used `--input`. The command requires `--in`. That was
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

Fixture versions:

- PostgreSQL: `16`.
- MySQL: `8.4`.
- libSQL server: `0.24.33`.

The exact candidate passed 84 PostgreSQL tests, 54 MySQL tests, and 59 libSQL
tests. The fixture manager removed its containers and volumes. A user process
already used the default PostgreSQL port. The lane used an isolated port and
did not alter that process.

An initial aggregate run failed after compilation while the host was under
storage pressure. The isolated libSQL replay passed 59 of 59 tests. A complete
cached rerun then passed all 197 tests. The failure did not reproduce, and no
accepted product defect came from it.

The exact candidate passed the complete focused storage selection:

```text
NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 \
cargo nextest run --profile ci-pr --no-tests fail \
  -E 'package(nimbus-storage) or package(nimbus-blob) or package(nimbus-object-storage)'
```

Nextest ran 850 tests with four declared skips. The selection covers SQLite
physical durability, encryption, redb, backup data structures, object storage,
garbage collection, erasure coding, retention, and restart recovery. It did not
require external cloud credentials or live erasure hardware.

The exact candidate also passed 40 focused `nimbus-cli` tests. They cover
plaintext and redb backup/restore, encrypted-backup rejection, redb DEK
rotation, object backup/restore, master-key validation, separate control roots,
and CLI option validation.

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

Plaintext redb passed the same native workload. The RRC4-003 repair let the
offline backup export two tenants and 182176 bytes. Restore imported both
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

The exact candidate passes the SQLite, PostgreSQL, MySQL, libSQL, and redb
storage contracts. It passes encryption, backup/restore, object storage,
consistency, physical durability, restart, and process-fence checks. The live
replays cover SQLite restart and direct S3 behavior. The 850-test storage set
and 40-test CLI set cover the deeper failure, recovery, and administration
contracts. Regressions cover all four confirmed fixes.

Candidate binding: Nimbus `7d0ca18a709d1b78e087bc4a69c8a96bee6f32b9`, Deno
`95413e012ee9f73e7f652e1e7b1ad9e351b9a8df`, and upstream baseline
`b57a2d680891de852d5576e65ccaea787b005431`.
