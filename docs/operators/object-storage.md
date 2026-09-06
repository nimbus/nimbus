---
title: Object storage
description: Administer the Nimbus object byte plane — placement policy, the master key, garbage-collection and erasure health, byte-plane backup and restore, and destructive tenant removal.
sidebar:
  label: Object storage
  order: 8
---

Nimbus keeps uploaded objects in a byte plane that is separate from the
tenant metadata database. Documents, indexes, and scheduled jobs live in the
tenant database (see [storage backends](/operators/storage-backends/));
object bytes live under `<data-dir>/object-blobs/<tenant-id>/`. The
`nimbus object-storage` commands administer that byte plane: placement
policy, the encryption master key, garbage-collection and erasure health,
byte-plane backup and restore, and destructive per-tenant removal.

This plane is on by default because the S3 wire adapter (served by default
on `127.0.0.1:9000` when the port is free, switched off with `--no-s3`)
reads and writes object bytes through it. Any object uploaded through that S3
surface — including the Convex-compatible file-storage face the same adapter
serves — lands in this byte plane, so it needs administering whenever the S3
listener is left on.

**Maturity.** The byte plane runs by default with the local pack leg,
local-only placement, and a per-deployment master key that is created on
first use. The erasure-coded leg and cloud placement targets are opt-in
through environment variables, and the operator command surface documented
here is bounded — placement changes, backup, restore, erasure healing, and
tenant removal are offline maintenance that require the server to be stopped.

## How the byte plane is laid out

- **Local leg.** By default each tenant's objects are stored in a local
  pack store at `<data-dir>/object-blobs/<tenant-id>/`. Setting
  `NIMBUS_OBJECT_STORAGE_LOCAL_LEG=erasure` switches the deployment to a
  Reed–Solomon erasure-coded leg spread across the drives named in
  `NIMBUS_OBJECT_STORAGE_ERASURE_DRIVES`. The two legs are mutually
  exclusive: the maintenance commands follow whichever leg the environment
  selects and refuse to operate against the other.
- **Encryption.** Objects are encrypted at rest. Each tenant gets a data
  encryption key wrapped under the deployment's object-storage master key,
  and the wrapped key is stored in the tenant's blob-key sidecar at
  `<data-dir>/object-blobs/<tenant-id>/blob-key.nimbus-enc`. The master key
  file defaults to `<data-dir>/keys/object-storage.master.key` and can be
  relocated with `NIMBUS_OBJECT_STORAGE_MASTER_KEY_FILE`. It is created
  automatically the first time the byte plane is used.
- **Placement.** Every tenant has a placement policy that decides whether
  objects stay local or also reach a cloud object store. The default is
  local-only. Persist a per-tenant override with `set-placement`.

Content addresses in the byte plane are ciphertext hashes, so backup and
restore move ciphertext end to end and never write plaintext into a bundle.

## Bootstrap the master key

The master key is created automatically on first use. Bootstrap it ahead of
time when you want to place it at a specific path (for example the one named
by `NIMBUS_OBJECT_STORAGE_MASTER_KEY_FILE`) with locked-down permissions
before the server ever starts:

```bash
nimbus object-storage bootstrap-master-key \
  --path /secure/path/object-storage.master.key
```

The command writes 32 bytes of random key material and, on Unix, sets the
file to `0600` and its parent directory to `0700`. It refuses to overwrite
an existing file. Back this key up separately from the object data — without
it, every encrypted object and every byte-plane backup is unreadable.

## Set a tenant placement policy

`set-placement` persists the engine placement policy for one tenant. Stop the
server first. Run it against the deployment's data directory and pass
`--control-data-dir` when the deployment keeps its control plane elsewhere:

```bash
# Keep a tenant's objects on the local byte plane only.
nimbus object-storage set-placement \
  --data-dir ./data \
  --control-data-dir ./control \
  --tenant acme \
  --mode local
```

Non-local modes send objects to a cloud object store as well as, or instead
of, the local leg:

```bash
nimbus object-storage set-placement \
  --data-dir ./data \
  --tenant acme \
  --mode mirror \
  --target-provider s3 \
  --bucket acme-objects \
  --region us-east-1 \
  --credentials environment
```

- `--mode` is one of `local`, `mirror`, `tier`, or `cloud-primary`. Every
  mode except `local` requires `--bucket`.
- `--target-provider` is `s3` (default), `gcs`, `azure`, `local`, or
  `memory`. `--region` and `--endpoint` are optional overrides.
- `--credentials` is `environment` (default), `anonymous`, or `secret-ref`.
  With `secret-ref`, pass `--secret-ref <id>`.
- `--require-ack` (mirror mode) makes a write wait for the remote leg before
  it succeeds.

A server-wide default policy can also be set through the
`NIMBUS_OBJECT_STORAGE_*` environment variables below; `set-placement`
records a per-tenant override that takes precedence over that default.

## Inspect garbage-collection status

`gc-status` reports the live-blob count for a tenant's local pack leg. It
opens the pack store read-only, so it is safe to run alongside a running
server:

```bash
nimbus object-storage gc-status --data-dir ./data --tenant acme
```

This command targets the pack leg only. On a deployment configured for the
erasure leg it exits with an error directing you to `erasure-status`.

## Inspect erasure health

On an erasure-coded deployment
(`NIMBUS_OBJECT_STORAGE_LOCAL_LEG=erasure`), `erasure-status` reports the
tenant's blob count and per-drive byte accounting without taking the
drives' write locks, so it too coexists with a running server:

```bash
nimbus object-storage erasure-status --tenant acme
nimbus object-storage erasure-status --tenant acme --json
```

For each drive it prints the live, reclaimable, and quarantined byte totals
and the pack count. Both `erasure-status` and `erasure-heal` require the
erasure environment variables (drives, shard counts) to be set; against a
pack-leg deployment they exit with an error.

## Heal an erasure leg

`erasure-heal` rebuilds degraded stripes from parity. It is offline
maintenance: it takes the drives' write locks and fails closed with a
"stop the server" message while a server is running.

```bash
# Stop the server first.
nimbus object-storage erasure-heal --tenant acme
```

Bound a single run with `--max-bytes` to cap how much erasure payload it
repairs, and add `--json` for a machine-readable report:

```bash
nimbus object-storage erasure-heal --tenant acme --max-bytes 1073741824 --json
```

The report records blobs examined, stripes repaired, shards rewritten,
degraded and beyond-repair counts, and whether the byte budget was
exhausted. Exit codes let automation react:

| Exit code | Meaning |
| --- | --- |
| 0 | Heal completed: nothing deferred, no beyond-repair blobs |
| 1 | Operational error |
| 3 | One or more blobs are beyond repair |
| 4 | Byte budget exhausted before all repairs ran — re-run or raise `--max-bytes` |

A code of 4 means degraded blobs remain; re-run the command (optionally with
a larger `--max-bytes`) until it exits 0.

## Back up and restore a tenant's objects

The byte plane is backed up separately from the tenant metadata database.
`nimbus backup` (see [backup and restore](/operators/backup-restore/))
captures the tenant database; `object-storage backup-object-store` captures
one tenant's objects into a single ciphertext bundle. Capture both to have a
complete tenant backup.

These maintenance verbs currently support embedded SQLite or redb tenant
metadata only (`--provider sqlite|redb`). They construct an embedded engine so
the metadata archive and byte-plane bundle share one local deployment
boundary. They cannot back up or restore tenant metadata in PostgreSQL, MySQL,
or libSQL; use the external backend's native backup procedure and manage its
object-byte recovery separately.

Both verbs are offline maintenance — they take exclusive ownership of the
byte-plane leg and fail closed with a "stop the server first" message while a
server is running.

The bundle is ciphertext, so it is worthless without the key material.
Backup and restore carry that key as an **escrow file**: the exact bytes of
the tenant's blob-key sidecar (`blob-key.nimbus-enc`). Copy that sidecar out
before backing up:

```bash
cp ./data/object-blobs/acme/blob-key.nimbus-enc ./acme-escrow.bin

# Stop the server, then export the tenant's objects.
nimbus object-storage backup-object-store \
  --data-dir ./data \
  --control-data-dir ./control \
  --tenant acme \
  --out backups/acme-objects.nobb \
  --key-escrow-id acme \
  --key-escrow-file ./acme-escrow.bin
```

- `backup-object-store` refuses to overwrite an existing `--out` file.
- The escrow must match the tenant's live sidecar, and the key must actually
  decrypt the stored objects. A wrong or regenerated key fails the backup
  rather than producing a bundle that can never be restored.

Restore reinstalls the escrowed key and the objects into a data directory:

```bash
nimbus object-storage restore-object-store \
  --in backups/acme-objects.nobb \
  --data-dir ./data \
  --control-data-dir ./control \
  --tenant acme \
  --key-escrow-id acme \
  --key-escrow-file ./acme-escrow.bin
```

- The escrow must be the one recorded in the bundle, and it must unwrap
  under this deployment's master key for this tenant. Restore validates all
  of that before it writes anything, so a mismatched key, tenant, or
  data-directory layout fails without leaving partial state behind.
- Restore refuses to overwrite a different, live key sidecar. Restoring the
  same bundle twice is idempotent.

Restore recreates the tenant if it is absent, so pairing it with a tenant
database restore reconstructs a tenant end to end.

## Remove a tenant

`tenant rm` deletes a tenant's metadata and its local objects. It is
destructive and offline: it takes exclusive ownership of the byte-plane leg
(failing closed while a server runs) and requires `--yes`.

```bash
# Stop the server first.
nimbus object-storage tenant rm \
  --data-dir ./data \
  --control-data-dir ./control \
  --tenant acme \
  --yes
```

The command removes the tenant from the control plane and unlinks its local
byte-plane trees (the pack root, or every erasure drive tree). Objects that a
placement policy wrote to an external cloud store are not deleted from that
store. A partially completed removal is safe to re-run.

For `set-placement`, `backup-object-store`, `restore-object-store`, and
`tenant rm`, omit `--control-data-dir` when the control plane uses the default
location inside `--data-dir`.

## Environment configuration

The default placement policy, the local leg, and the master key path come
from the environment:

| Variable | Purpose |
| --- | --- |
| `NIMBUS_OBJECT_STORAGE_MODE` | Default placement: `local`, `mirror`, `tier`, or `cloud-primary` |
| `NIMBUS_OBJECT_STORAGE_PROVIDER` | Cloud target for non-local modes: `s3`, `gcs`, `azure`, `local`, `memory` |
| `NIMBUS_OBJECT_STORAGE_BUCKET` | Target bucket/container (required for non-local modes) |
| `NIMBUS_OBJECT_STORAGE_REGION` | Target region |
| `NIMBUS_OBJECT_STORAGE_ENDPOINT` | Custom endpoint URL |
| `NIMBUS_OBJECT_STORAGE_PREFIX` | Key prefix below the bucket |
| `NIMBUS_OBJECT_STORAGE_CREDENTIALS` | `environment`, `anonymous`, or `secret-ref` |
| `NIMBUS_OBJECT_STORAGE_SECRET_REF` | Secret id when credentials are `secret-ref` |
| `NIMBUS_OBJECT_STORAGE_REQUIRE_ACK` | Mirror mode waits for the remote leg |
| `NIMBUS_OBJECT_STORAGE_MASTER_KEY_FILE` | Master key path override |
| `NIMBUS_OBJECT_STORAGE_LOCAL_LEG` | `pack` (default) or `erasure` |
| `NIMBUS_OBJECT_STORAGE_ERASURE_DRIVES` | Comma-separated absolute drive paths (required for `erasure`) |
| `NIMBUS_OBJECT_STORAGE_ERASURE_DATA` | Data shards (default 4) |
| `NIMBUS_OBJECT_STORAGE_ERASURE_PARITY` | Parity shards (default 2) |
| `NIMBUS_OBJECT_STORAGE_ERASURE_STRIPE` | Stripe width in bytes (default 1048576) |

The maintenance commands read the same environment, so export the erasure
variables in the same shell before running `erasure-status`, `erasure-heal`,
`backup-object-store`, `restore-object-store`, or `tenant rm` on an
erasure-coded deployment.
