---
title: Tenant isolation
description: Create and administer tenants, choose where each tenant's data lives, and verify isolation on a running Nimbus server.
sidebar:
  label: Tenant isolation
  order: 8
---

This guide covers the day-to-day tenant tasks: creating, listing, and
deleting tenants, choosing where tenant data lives, keeping the network
surface private, and checking that isolation holds on your server. For the
isolation model itself — what Nimbus enforces between tenants and why — see
[the tenant isolation concept page](/concepts/tenant-isolation/).

## Before you start

You need a running server and its local admin token.

1. Start the server if you haven't:

   ```bash
   nimbus start --port 8080 --data-dir ./data
   ```

   See the [self-host quickstart](/get-started/self-host/) for installation.

2. Load the admin token. It is created on first boot and stored as a JSON
   file:

   ```bash
   # Linux
   export NIMBUS_TOKEN=$(jq -r .token ~/.local/share/nimbus/auth/token)

   # macOS
   export NIMBUS_TOKEN=$(jq -r .token "$HOME/Library/Application Support/nimbus/auth/token")
   ```

   On Windows the file is `%LOCALAPPDATA%\nimbus\auth\token.json`. You can
   also print it with `nimbus auth token`.

3. Send the token on every tenant-administration request, either as
   `Authorization: Bearer <token>` or as an `X-Nimbus-Admin-Token` header.
   Requests without it get a `401`.

## Create a tenant

```bash
curl -s -X POST http://localhost:8080/api/tenants \
  -H "Authorization: Bearer $NIMBUS_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"id": "acme"}'
```

A successful create returns `201` with the tenant ID:

```json
{"id": "acme"}
```

Tenant IDs must follow these rules:

| Constraint | Value |
| --- | --- |
| Characters | ASCII letters, digits, `_`, `-` |
| Length | 1–128 characters |
| Reserved | IDs starting with `_` belong to Nimbus system tenants |

Expect these errors:

- `400` — invalid ID, including any ID starting with `_` (for example
  `_nimbus`, the internal system tenant).
- `409` — the tenant already exists.

## List tenants

```bash
curl -s http://localhost:8080/api/tenants \
  -H "Authorization: Bearer $NIMBUS_TOKEN"
```

```json
{"tenants": ["acme", "demo"]}
```

The list contains only your tenants. Reserved system tenants are never
included.

## Delete a tenant

```bash
curl -s -X DELETE http://localhost:8080/api/tenants/acme \
  -H "Authorization: Bearer $NIMBUS_TOKEN"
```

A successful delete returns `204` with no body. Deletion first tears down
the tenant's runtime and services, then removes the tenant's storage
namespace — the embedded database file, or the per-tenant schema, database,
or namespace on an external provider.

Deletion is irreversible. There is no soft-delete or trash; take a backup
first if you might need the data again.

## Choose where tenant data lives

Every tenant gets its own storage namespace. The shape of that namespace
depends on the tenant provider selected at startup with `--tenant-provider`
(or the `NIMBUS_TENANT_PROVIDER` environment variable):

| Provider | Per-tenant namespace | Key flags |
| --- | --- | --- |
| `sqlite` (default) | One file per tenant: `<data-dir>/<tenant>.sqlite3` | `--data-dir` |
| `redb` | One file per tenant: `<data-dir>/<tenant>.redb` | `--data-dir` |
| `postgres` | One schema per tenant in your Postgres database | `--postgres-url`, `--postgres-tenant-schema-prefix` |
| `mysql` | One database per tenant on your MySQL server | `--mysql-url`, `--mysql-tenant-database-prefix` |
| `libsql-replica` | One namespace per tenant on your libSQL primary | `--libsql-url`, `--libsql-admin-url`, `--libsql-tenant-namespace-prefix`, `--libsql-replica-cache-dir` |

The external providers derive per-tenant names from the tenant ID with a
configurable prefix that defaults to `tenant_`. Keep that prefix (or your
override) reserved for Nimbus in the target database: don't create
unrelated schemas, databases, or namespaces under it, and don't point two
Nimbus servers at the same database with the same prefix unless they are
meant to share tenants.

Each flag has a matching environment variable (`NIMBUS_POSTGRES_URL`,
`NIMBUS_MYSQL_TENANT_DATABASE_PREFIX`, and so on). CLI flags override
environment values, which override the optional `--config` JSON file. See
[storage backends](/operators/storage-backends/) for provider topologies
and [configuration reference](/reference/configuration/) for the full flag
and environment list.

## Keep the network surface private

`nimbus start` always runs with production tenant isolation — there is no
flag to weaken it. The relaxed local-development mode exists only in
`nimbus dev`, the single-developer workflow that auto-creates a `demo`
tenant.

By default the server binds to `127.0.0.1` and refuses any `--host`
outside the loopback range. To listen on a non-loopback interface you must
opt in, and the admin token must have been rotated recently:

```bash
nimbus auth rotate-admin
nimbus start --host 0.0.0.0 --allow-network
```

Treat the admin token as the keys to every tenant: anyone holding it can
read and delete any tenant's data. Rotate it with `nimbus auth
rotate-admin` whenever it may have leaked. See
[hardening](/operators/hardening/) before exposing the server beyond
localhost.

## Set per-tenant runtime limits

Function execution is budgeted per tenant so one tenant's load cannot
starve the others. Tune the budgets at startup:

| Flag | Limits |
| --- | --- |
| `--runtime-max-active-per-tenant` | Concurrent top-level invocations actively running for one tenant |
| `--runtime-max-in-flight-per-tenant` | Active plus parked invocations for one tenant |
| `--runtime-max-queued-per-tenant` | Invocations one tenant may have waiting in queue |
| `--runtime-heap-mb` | V8 heap limit per runtime isolate, in megabytes |
| `--runtime-timeout-secs` | Wall-clock limit per invocation, in seconds |

When a tenant exceeds its queue budget the request is rejected with `429`
rather than degrading other tenants.

## Review policy files before applying changes

Workloads that need explicit runtime, network-egress, image, secret,
volume, or quota authority are described in an operator policy file
(YAML). The `nimbus policy` commands review such a file offline — they
read the file you pass and never contact the server:

```bash
nimbus policy validate --file nimbus.policy.yaml
nimbus policy explain --file nimbus.policy.yaml
nimbus policy prove --file nimbus.policy.yaml
nimbus policy diff --from before.policy.yaml --to after.policy.yaml
```

- `validate` compiles the policy and reports errors.
- `explain` shows the resulting decision IDs and grant traces.
- `prove` flags risky grants as advisories: broad egress, write-capable
  endpoint bypass, secret exposure, and cross-tenant-looking regressions.
- `diff` classifies what changed between two policy versions.

All four take `-f json` for machine-readable output.

If `prove` reports an advisory you have reviewed and accept, record it in
the policy file rather than ignoring the output. Each accepted risk names
the exact advisory, who approved it, and why:

```yaml
accepted_risks:
  - advisory_id: "<advisory id from nimbus policy prove>"
    approved_by: "ops@example.com"
    reason: "Webhook delivery requires egress to the partner API."
```

Accepted risks silence only the matching advisory — new or unrelated
advisories still surface on the next `prove` run.

## Verify isolation on your server

A short manual check after setup changes:

1. Create two tenants and write a document into the first:

   ```bash
   curl -s -X POST http://localhost:8080/api/tenants \
     -H "Authorization: Bearer $NIMBUS_TOKEN" \
     -H "Content-Type: application/json" -d '{"id": "tenant-a"}'

   curl -s -X POST http://localhost:8080/api/tenants \
     -H "Authorization: Bearer $NIMBUS_TOKEN" \
     -H "Content-Type: application/json" -d '{"id": "tenant-b"}'

   curl -s -X POST http://localhost:8080/api/tenants/tenant-a/documents \
     -H "Authorization: Bearer $NIMBUS_TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"table": "secrets", "fields": {"value": "tenant-a only"}}'
   ```

2. Confirm the second tenant cannot see it — the same table name in
   `tenant-b` is empty:

   ```bash
   curl -s -X POST http://localhost:8080/api/tenants/tenant-b/query \
     -H "Authorization: Bearer $NIMBUS_TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"table": "secrets", "filters": []}'
   ```

   Expected: `{"data": []}`.

3. Confirm reserved tenants are refused — creating `_anything` returns
   `400`, and unauthenticated requests return `401`:

   ```bash
   curl -s -o /dev/null -w "%{http_code}\n" -X POST http://localhost:8080/api/tenants \
     -H "Authorization: Bearer $NIMBUS_TOKEN" \
     -H "Content-Type: application/json" -d '{"id": "_evil"}'

   curl -s -o /dev/null -w "%{http_code}\n" http://localhost:8080/api/tenants
   ```

4. Check storage-level health for a tenant. The consistency endpoint
   verifies the tenant's documents, indexes, and commit log agree:

   ```bash
   curl -s http://localhost:8080/debug/tenants/tenant-a/consistency \
     -H "Authorization: Bearer $NIMBUS_TOKEN"
   ```

   A companion endpoint, `/debug/tenants/<tenant>/engine/metrics`, reports
   per-tenant engine diagnostics.

If step 2 ever returns tenant-a's data, or step 3 returns anything other
than `400`/`401`, stop and investigate before putting more data on the
server — those checks should never fail on a healthy deployment.

## Related pages

- [Tenant isolation concepts](/concepts/tenant-isolation/) — the model:
  storage scoping, runtime isolation, and auth boundaries.
- [Storage backends](/operators/storage-backends/) — provider topologies
  in depth.
- [Hardening](/operators/hardening/) — network exposure, TLS, and origin
  policy.
- [MongoDB tenant isolation](/reference/mongodb/tenant-isolation/) — how
  MongoDB database names map onto tenants.
