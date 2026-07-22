---
title: Scale out by partitioning tenants
description: Grow past one Nimbus machine by partitioning tenants across independent instances — decision signals, the lever ladder to climb first, moving a tenant's data, and cutting client routing over.
sidebar:
  label: Scale out
  order: 14
---

A Nimbus deployment is one process, and Nimbus instances do not coordinate:
there is no clustering, no request forwarding, and no support for two
instances serving the same tenant. The one supported way to grow past a
single machine is to partition — run independent instances and place
different tenants on each. Because every tenant is a self-contained
namespace, moving one is a data move, and the client routing is yours to
operate.

This page is the procedure for that move. Climb the in-process lever ladder
first; partition only when a single machine is genuinely out of headroom.

## Decide when to scale out

Scale out when one machine is saturated and the lever ladder below is
exhausted — not before. The per-tenant engine diagnostics tell you which
tenant is consuming the headroom. Read them for each tenant:

```bash
curl -s http://localhost:8080/debug/tenants/demo/engine/metrics \
  -H "Authorization: Bearer $NIMBUS_TOKEN" | jq '.diagnostics'
```

Watch these signals over time (see [Inspect the server](/operators/observability/)
for the full field list):

- **`mutation_admission`** — the per-tenant write admission gate. Sustained
  backpressure or a persistently deep queue means that tenant is saturating
  its write path.
- **`mutation_journal`** — mutation pipeline progress. A widening
  `assignment_lag`, `apply_lag`, `publication_lag`, or `visibility_lag`
  localizes the backlog to durable append, storage application, contiguous
  publication, or reader visibility respectively. A durable head that stalls
  while admission keeps accepting writes means the journal worker is not
  draining.
- **`libsql_replica_freshness`** — replica staleness, when the tenant runs
  on a libSQL replica. Growing lag means reads are drifting from the primary.

Also check the process-wide runtime counters at `/debug/runtime/metrics`:
a sustained `queued_invocations` backlog and a rising ratio of
`rejected_invocations` to `started_invocations` (tenants over budget are
rejected with `429`) are machine-level pressure signals.

A tenant that stays pressured while the machine is at its ceiling is a
partition candidate.

## Climb the lever ladder first

Moving a tenant is disruptive. Exhaust the in-process levers before you
partition. Each rung ships today.

1. **Scale the machine up.** With the embedded backends (SQLite, redb) the
   server and its data share one host, and scaling is vertical: give the
   machine more disk, memory, and CPU. This is the cheapest move and needs
   no data migration.
2. **Move to an external database.** Point Nimbus at Postgres or MySQL with
   `--tenant-provider`. Durability and capacity become the database's job —
   its own replication, pooling, and backup — while Nimbus stays the single
   coordinator for mutations. See
   [Storage backends](/operators/storage-backends/).
3. **Add read-locality with libSQL.** With `--tenant-provider libsql-replica`,
   writes go to a remote libSQL primary while reads are served from a
   per-tenant local replica cache on the Nimbus host. Replica freshness is
   measured in the per-tenant diagnostics above.

Only when a single instance can no longer carry the load do you partition.

## Stand up a second instance

Start another independent Nimbus process on its own machine. It shares
nothing with the first — give it its own data directory, control directory,
and bind address:

```bash
nimbus start \
  --data-dir /var/lib/nimbus/data \
  --control-data-dir /var/lib/nimbus/control \
  --port 8080
```

The two instances do not know about each other, and that is expected. Each
serves its own set of tenants.

A non-loopback bind requires `--allow-network`, and Nimbus refuses one whose
admin token has never been rotated — the auto-minted first-boot token. (An
already-rotated token binds with only an advisory warning.) Rotate the token
before first use:

```bash
nimbus auth rotate-admin
```

Run each instance as a supervised service rather than a bare foreground
process — see [Run Nimbus as a service](/operators/node-lifecycle/).

## Move a tenant's data

Moving a tenant copies its namespace from the source instance to the target.
Quiesce the tenant on the source first — stop the source server, or cut its
clients over before the final export — so no writes land after the snapshot
you carry. The archive captures the tenant's latest committed state at the
moment it is taken.

### Embedded backends (SQLite, redb)

Each embedded tenant is a standalone database file, so back up exactly the
one you are moving. With the source server stopped, stage a directory
containing only that tenant's files, then archive it:

```bash
# Stage just the tenant being moved. For SQLite include the WAL sidecars;
# for redb the single .redb file. Include the .nimbus-enc manifest if the
# deployment is encrypted.
mkdir -p /tmp/move-demo
cp /var/lib/nimbus/data/demo.sqlite3     /tmp/move-demo/
cp /var/lib/nimbus/data/demo.sqlite3-wal /tmp/move-demo/ 2>/dev/null || true
cp /var/lib/nimbus/data/demo.sqlite3-shm /tmp/move-demo/ 2>/dev/null || true

# Produce a single-tenant archive from the staged directory.
nimbus backup create --data-dir /tmp/move-demo --out demo-move.json
```

`nimbus backup create` writes one point-in-time archive per tenant it finds
in the data directory; a directory holding one tenant yields a one-tenant
archive. Copy `demo-move.json` to the target host and restore it into the
target's data directory while the target server is stopped and the tenant is
not already present there:

```bash
nimbus backup restore --in demo-move.json --data-dir /var/lib/nimbus/data
```

Restore creates the tenant, imports its state, and verifies the restored
fingerprint against the archive, failing closed on any mismatch. It requires
the restored tenant to have an empty journal, so restore into a target that
does not yet own that tenant. Pass `--provider redb` on both commands for a
redb deployment. See [Backup and restore](/operators/backup-restore/) for the
archive format and encrypted-directory caveats.

### External backends (Postgres, MySQL)

There is no built-in cross-instance mover for external backends. Treat the
move as a database migration you script with native tooling, then point the
target instance at the destination database.

Each tenant is one schema (Postgres) or one database (MySQL), so native
per-namespace export captures it. For a Postgres tenant schema:

```bash
# Export the tenant's schema from the source database.
pg_dump --schema=tenant_demo --format=custom \
  --file=tenant_demo.dump "$SOURCE_POSTGRES_URL"

# Load it into the database the target instance is configured against.
pg_restore --dbname="$TARGET_POSTGRES_URL" tenant_demo.dump
```

For MySQL, use `mysqldump tenant_demo` against the source and load the dump
into the target's database. The schema/database name follows your
`--postgres-tenant-schema-prefix` / `--mysql-tenant-database-prefix` setting.
Reconciling the target instance's metadata so it recognizes the imported
tenant is part of this manual move — Nimbus does not do it for you.

## Route clients to the owning instance

Nimbus does not forward requests between instances, so each tenant must be
reached at the instance that now owns it. You own the mapping from tenant to
instance address.

Clients address their tenant on the request — the native and Convex HTTP
surfaces carry it as a URL path segment (`/api/tenants/{tenant_id}/...`,
`/convex/{tenant_id}/...`), and the native WebSocket surface (`/ws`) reads an
`X-Tenant-Id` header or a `tenant_id` query parameter. Route on that
identity, or give each tenant a distinct deployment URL, using whatever you
already run: DNS, a reverse proxy keyed on the tenant, or per-tenant URLs in
client configuration. Cut a tenant's clients over to the target only after
its data is in place there.

## Verify the move, then decommission the source

Confirm the target is healthy and serving the tenant before you remove
anything from the source.

1. The target answers liveness: `curl -s http://<target>:8080/health` returns
   `{ "ok": true }`.
2. The moved tenant's engine is healthy — `mutation_journal` durable head is
   advancing and `mutation_admission` is not backed up:

   ```bash
   curl -s http://<target>:8080/debug/tenants/demo/engine/metrics \
     -H "Authorization: Bearer $NIMBUS_TOKEN" | jq '.diagnostics'
   ```

3. The tenant's state is internally consistent:

   ```bash
   curl -s http://<target>:8080/debug/tenants/demo/consistency \
     -H "Authorization: Bearer $NIMBUS_TOKEN" | jq '.ok, .mismatches'
   ```

   Expect `true` and an empty array.
4. Spot-check documents in the moved tenant through your normal application
   path, and confirm client traffic now reaches the target (its access log
   and metrics show the requests).

Only once the target is confirmed, decommission the copy on the source so no
two instances serve the same tenant. With the source server stopped, remove
the tenant's namespace from the source: delete its `<tenant>.sqlite3` (and
`-wal`, `-shm`, `.nimbus-enc` sidecars) or `<tenant>.redb` file for an
embedded backend, or drop the tenant's schema/database from the source's
external backend.

## Ceiling

Partitioning is the horizontal-growth move Nimbus supports today; keep
exactly one instance per tenant, and size on the assumption that one binary,
one machine, and one storage backend carry each partition — see
[Scaling](/concepts/scaling/).
