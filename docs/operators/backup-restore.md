---
title: Backup and restore
description: Back up and restore a self-hosted Nimbus server — SQLite data directories, external database backends, encryption keys, and restore verification.
sidebar:
  label: "Backup & restore"
  order: 6
---

For the embedded providers, `nimbus backup create` and
`nimbus backup restore` are the first-class path: one point-in-time
archive per tenant, written to a single file and verified by fingerprint
on restore. The manual procedures on this page remain the path for
encrypted data directories and for the external database backends, which
use their own native tooling. A restore returns the server to the moment
the backup was taken.

## Know what you are backing up

With the default embedded SQLite backend, everything the server persists
lives in the data directory (`./data` by default, or whatever you passed to
`--data-dir` / `NIMBUS_DATA_DIR`):

- `<tenant>.sqlite3` — one SQLite database per tenant. Documents, schemas,
  indexes, scheduled jobs, and deployed app state for that tenant are all
  inside this single file.
- `<tenant>.sqlite3-wal` and `<tenant>.sqlite3-shm` — SQLite WAL sidecars.
  Nimbus runs SQLite in WAL mode (`journal_mode=WAL`,
  `synchronous=FULL`), so recent committed writes can live in the `-wal`
  file rather than the main database file.
- `nimbus-control.db` — the embedded control-plane database (redb format,
  not SQLite). It holds monthly-active-user usage tracking used for
  license accounting. By default it sits in the same data directory; if you
  set `--control-data-dir` / `NIMBUS_CONTROL_DATA_DIR`, it lives there
  instead.
- `<file>.nimbus-enc` — encryption manifest sidecars, present only when
  at-rest encryption is enabled. Each one stores the wrapped data
  encryption key for the database file it sits next to. **A database file
  restored without its `.nimbus-enc` sidecar is unrecoverable**, even if
  you still have the master key.

The local admin token file (`~/.local/share/nimbus/auth/token` on Linux,
`~/Library/Application Support/nimbus/auth/token` on macOS,
`%LOCALAPPDATA%\nimbus\auth\token.json` on Windows) does **not** need to be
backed up. The server creates a new token on first boot if the file is
missing, and you can rotate it at any time with `nimbus token rotate` or
`nimbus auth rotate-admin`. After restoring onto a fresh machine, re-read
the token file before making admin API calls.

## nimbus backup (embedded providers)

With the server stopped, back up every tenant in the data directory to
one file, and restore it into a fresh directory:

```bash
nimbus backup create --data-dir ./data --out backups/nimbus-backup.json

nimbus backup restore --in backups/nimbus-backup.json --data-dir ./restored
```

- `--provider` selects the embedded backend (`sqlite`, the default, or
  `redb`).
- Each tenant is captured as a point-in-time restore archive at its
  latest committed sequence; the file records one archive per tenant.
- `create` refuses to overwrite an existing output file.
- `restore` requires every restored tenant to have an empty journal —
  restore into a fresh data directory. The storage layer verifies the
  restored state's fingerprint against the archive and fails closed on
  any mismatch.
- Encrypted data directories are not yet covered by `nimbus backup`; use
  the cold-backup procedure below and keep every `.nimbus-enc` sidecar
  with its database file.
- External backends (`postgres`, `mysql`, `libsql-replica`) use their
  native backup tooling — see below.

## Back up the SQLite backend

### Cold backup (recommended)

Stop the server, copy the whole data directory, start the server again:

```bash
# Stop the server (Ctrl+C in the foreground, or your service manager)
systemctl stop nimbus

# Copy everything: tenant databases, WAL sidecars, control-plane db,
# and any .nimbus-enc manifests
tar -czf nimbus-backup-$(date +%Y%m%d).tar.gz -C /path/to data

systemctl start nimbus
```

Copying the directory wholesale is the safest procedure: it captures the
`-wal` and `-shm` sidecars together with each main database file, the
control-plane database, and every encryption manifest in one consistent
set.

### Hot backup of an unencrypted tenant database

For an unencrypted deployment you can snapshot a tenant database while the
server is running, using SQLite's online backup. This produces a single
consistent file with the WAL contents folded in:

```bash
sqlite3 data/demo.sqlite3 ".backup 'backups/demo.sqlite3'"
```

Two caveats:

- Repeat this for every `<tenant>.sqlite3` file. Each tenant is an
  independent database, so the per-tenant snapshots are not mutually
  consistent at a single instant — only within each tenant.
- `nimbus-control.db` is a redb file, not SQLite, so `sqlite3 .backup`
  cannot copy it. Copy it with the server stopped.

### Encrypted tenant databases

When at-rest encryption is enabled, tenant databases are SQLCipher
databases. A stock `sqlite3` binary cannot open them, so the online
`.backup` path is unavailable — use the cold-backup procedure and make
sure every `.nimbus-enc` sidecar is included next to its database file.

For a plaintext recovery export of a single encrypted database, the CLI
provides:

```bash
nimbus encryption export \
  --source data/demo.sqlite3 \
  --target recovery/demo-plaintext.sqlite3 \
  --provider sqlite \
  --tenant-id demo
```

Treat plaintext exports as sensitive material — they undo the at-rest
protection. See the [encryption guide](/operators/encryption/) for the
full key-management story.

## Back up external backends

With `--tenant-provider postgres`, `mysql`, or `libsql-replica`, document
data lives in the external system and you should use that system's native
backup tools. What must be captured together:

- **Postgres** — Nimbus stores everything in one database: a
  `nimbus_provider` metadata schema plus one `tenant_<id>` schema per
  tenant (names configurable via `--postgres-metadata-schema` and
  `--postgres-tenant-schema-prefix`). Dump the whole database in a single
  `pg_dump` run so the metadata schema and tenant schemas stay consistent
  with each other:

  ```bash
  pg_dump --format=custom --file=nimbus.dump "$NIMBUS_POSTGRES_URL"
  ```

- **MySQL** — Nimbus uses a `nimbus_provider` metadata database plus one
  `tenant_<id>` database per tenant (configurable via
  `--mysql-metadata-database` and `--mysql-tenant-database-prefix`).
  Dump the metadata database and all tenant databases in one
  `mysqldump --databases` invocation with a consistency option
  appropriate to your engine.

- **libSQL** — data lives on the libSQL server in a `nimbus_provider`
  metadata namespace plus `tenant_<id>` namespaces. Back up the libSQL
  server with its own tooling. The local replica cache directory
  (`--libsql-replica-cache-dir`, containing `cache-<generation>.sqlite3`
  files per tenant) is a rebuildable cache and does not need backup.

**The local control-plane file still matters.** Even with an external
backend, the server keeps `nimbus-control.db` in the control data
directory on the server host. Include it in your backups alongside the
external database dump, plus its `.nimbus-enc` manifest if encryption is
enabled.

## Back up encryption key material

If your deployment runs with at-rest encryption
(`--encryption-key-provider`), the data backup is unreadable without the
key material. Back up keys separately from data — a backup that bundles
both in one place defeats the encryption:

- **`master-key-file`** — back up the 32-byte master key file
  (`--encryption-master-key-file` / `NIMBUS_ENCRYPTION_MASTER_KEY_FILE`).
  Without it, every encrypted database in the backup is permanently
  unreadable.
- **`key-dir`** — back up the entire key directory
  (`--encryption-key-dir`). It contains one `.key` file per subject.
- **`aws-kms`** — there is no local key file to copy. Ensure the KMS key
  referenced by `--encryption-aws-kms-key-id` is protected against
  deletion; the `.nimbus-enc` manifests in your data backup are wrapped by
  that KMS key.

In all three cases the `.nimbus-enc` manifest sidecars must travel with
the database files they protect. Key material alone cannot reconstruct a
lost manifest.

## Restore

Restore is the inverse of backup:

1. Stop the server if it is running.
2. Restore the data directory wholesale — main database files, `-wal` and
   `-shm` sidecars, `nimbus-control.db`, and all `.nimbus-enc` manifests —
   to the path the server will use. For external backends, restore the
   dump with the native tool (`pg_restore`, `mysql`, libSQL tooling) and
   restore the local control data directory.
3. Restore encryption key material to the same paths the server
   configuration points at (`--encryption-master-key-file`,
   `--encryption-key-dir`, or the same AWS KMS key).
4. Start the server with the same persistence flags it ran with before:

   ```bash
   nimbus start --data-dir ./data
   ```

### Verify the restore

Work through these checks in order:

1. The process stays up and logs `nimbus listening on <address>`.
2. The health endpoint answers:

   ```bash
   curl -s http://127.0.0.1:8080/health
   # {"ok":true}
   ```

3. Tenants are visible through the admin API (re-read the token file
   first — see [the self-host quickstart](/get-started/self-host/) for
   token file locations):

   ```bash
   curl -s http://127.0.0.1:8080/api/tenants \
     -H "Authorization: Bearer $NIMBUS_TOKEN"
   ```

4. Spot-check that documents in a restored tenant are readable through
   your normal application path.
5. For encrypted deployments, confirm encryption coverage:

   ```bash
   nimbus encryption status
   ```

If the server fails to start or requests fail after a restore, the most
common causes are a missing `.nimbus-enc` sidecar or mismatched key
material — see [troubleshooting](/operators/troubleshooting/) for the
exact error messages.
