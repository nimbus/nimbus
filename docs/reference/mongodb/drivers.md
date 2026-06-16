---
title: MongoDB drivers
description: Client requirements for the Nimbus MongoDB endpoint and the compatibility status of official drivers and tools.
sidebar:
  order: 1
---

The Nimbus MongoDB endpoint speaks the standard MongoDB wire protocol. Any
client that meets the requirements below can connect; no Nimbus-specific
driver is needed.

## Endpoint requirements

| Requirement | Value |
| --- | --- |
| Wire protocol | `OP_MSG`; the server reports version `7.0.0` and wire versions 0–21 |
| Connection mode | Direct connection to a single host; Nimbus is a single endpoint, not a replica set — never pass a `replicaSet` option (`directConnection=true` is accepted but not required) |
| Authentication | SCRAM-SHA-256 only; other SASL mechanisms are rejected |
| Transport | Plain TCP; the listener binds loopback addresses only and refuses non-loopback binds |
| Database name | Selects the Nimbus tenant; must be ASCII letters, digits, `_`, or `-`, at most 128 characters |

Connection string shape:

```text
mongodb://<username>:<password>@<host>:<port>/<database>
```

## Official drivers

The same connection string works across languages. `directConnection=true`
is part of the standard connection-string format and honored by all current
official drivers — include it if you want the direct topology to be
explicit, but with a single host the drivers connect directly without it.

| Driver | Verification status |
| --- | --- |
| Node.js (`mongodb`) | Exercised against Nimbus by a bundled demo application |
| Python (`pymongo`) | Protocol-compatible; not exercised in the Nimbus repository |
| Go (`mongo-go-driver`) | Protocol-compatible; not exercised in the Nimbus repository |
| Rust (`mongodb`) | Protocol-compatible; not exercised in the Nimbus repository |
| Java / Kotlin | Protocol-compatible; not exercised in the Nimbus repository |
| C# / .NET | Protocol-compatible; not exercised in the Nimbus repository |

"Protocol-compatible" means the driver's requirements — `OP_MSG`,
SCRAM-SHA-256, direct connection — are all on the endpoint's supported
surface; it is not an executed end-to-end claim.

## Helper package

`@nimbus/mongodb` is a small optional package, provisioned into a Nimbus
project with `nimbus packages provision mongodb`. It exports one function:

| Export | Behavior |
| --- | --- |
| `mongoUri(options?)` | Builds a Nimbus connection string. Defaults: host `127.0.0.1`, port `27017`, database `default`. URL-encodes credentials, includes them only when both `username` and `password` are set, and always appends `directConnection=true`. |

## Shells and GUI tools

Shells and GUI tools are wire-protocol clients like any driver. The
endpoint serves the standard pre-authentication handshake command set —
`hello` / `isMaster`, `buildInfo`, `ping`, `whatsmyuri`, `getParameter`,
`serverStatus`, `connectionStatus`, `getCmdLineOpts`,
`getFreeMonitoringStatus`, and `getLog` — without credentials, which is the
sequence tools such as `mongosh` issue on connect. All data commands then
require SCRAM-SHA-256 authentication.

## Compatibility boundary

The binding constraint for higher-level libraries (ODMs, query builders) is
the operator surface, not the protocol. Nimbus accepts a defined subset of
filter operators, update operators, and aggregation stages, and rejects
everything else with an explicit error instead of approximating it. A
library that emits an unsupported operator — for example `$in` or `$regex`
in a filter — receives a `BadValue` error. See
[supported operations](/reference/mongodb/operations/) for the exact
surface, and [tenant isolation](/reference/mongodb/tenant-isolation/) for
how database names map to tenants.
