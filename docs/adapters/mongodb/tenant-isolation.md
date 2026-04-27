# Tenant Isolation

The MongoDB adapter maps **database names to Neovex tenants**. Each tenant
is fully isolated at the storage level -- separate data, separate indexes,
no cross-tenant visibility.

```
┌──────────────────────────────────────────────────────────┐
│  MongoDB Client A                                        │
│  client.db("tenant_a").collection("users").insertOne()   │
└──────────────────┬───────────────────────────────────────┘
                   │
                   ▼
┌──────────────────────────────────────────────────────────┐
│  Neovex Server                                           │
│                                                          │
│  ┌────────────────────┐    ┌────────────────────┐        │
│  │ Tenant: tenant_a   │    │ Tenant: tenant_b   │        │
│  │ ┌────────────────┐ │    │ ┌────────────────┐ │        │
│  │ │ users          │ │    │ │ users          │ │        │
│  │ │ orders         │ │    │ │ orders         │ │        │
│  │ └────────────────┘ │    │ └────────────────┘ │        │
│  └────────────────────┘    └────────────────────┘        │
│        isolated                  isolated                │
└──────────────────────────────────────────────────────────┘
```

## Mapping Rules

| MongoDB database name | Neovex tenant ID | Notes |
|---|---|---|
| `"myapp"` | `myapp` | Direct mapping -- database name becomes tenant ID |
| `"customer_123"` | `customer_123` | Any valid name works |
| `"admin"` | `default` | MongoDB built-in databases redirect to the `default` tenant |
| `"local"` | `default` | Same redirect |
| `"config"` | `default` | Same redirect |
| *(missing)* | `default` | When `$db` is absent from the wire command |

Tenants are auto-created on first access (`ensure_tenant`). No upfront
provisioning is needed for development. In production, pre-provision tenants
via the admin API or CLI.

## Storage Backend Isolation

A `users` collection in `tenant_a` and a `users` collection in `tenant_b`
are completely separate. There is no operation that can read or write across
tenant boundaries.

How isolation is enforced depends on which storage backend is active
(file-per-tenant for SQLite, schema-per-tenant for Postgres, etc.). See
[Storage Backends: Tenant Isolation](../../operating/storage-backends.md#tenant-isolation)
for the full breakdown.

## Single-Tenant Apps

Most apps use a single database name. The `uri()` helper defaults to
`"default"`, so the simplest setup routes everything to one tenant:

```typescript
import { MongoClient } from "mongodb";
import { uri } from "@neovex/mongodb";

const client = new MongoClient(uri()); // database: "default"
await client.connect();
const db = client.db("default");       // tenant: "default"
```

## Multi-Tenant Apps

Use a different database name per tenant. The adapter creates each tenant
on first access:

```typescript
import { MongoClient } from "mongodb";
import { uri } from "@neovex/mongodb";

async function clientForTenant(tenantId: string) {
  const client = new MongoClient(uri({ database: tenantId }));
  await client.connect();
  return client;
}

const aliceClient = await clientForTenant("alice");
const bobClient   = await clientForTenant("bob");

const aliceDb = aliceClient.db("alice");
const bobDb   = bobClient.db("bob");
// alice and bob cannot see each other's data
```
