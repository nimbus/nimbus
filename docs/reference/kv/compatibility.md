---
title: KV (Redis) compatibility
description: The exact RESP command surface of the Nimbus KV listener — commands, protocol versions, storage modes, and limits.
sidebar:
  order: 1
---

Started with the explicit `nimbus kv` command; loopback-only RESP listener on
`127.0.0.1:6380`; authentication is mandatory.

The Nimbus KV listener speaks the Redis serialization protocol (RESP) over TCP.
It implements the command surface listed on this page and nothing else: an
unrecognized command is rejected with an `ERR unknown command '<name>'` error,
and an unsupported subcommand or argument shape is rejected with an explicit
error. Nothing is silently ignored.

This is a distinct surface from the Cloudflare Workers KV data plane. The
Cloudflare KV binding is an HTTP-style namespace store documented on the
[Cloudflare compatibility reference](/reference/cloudflare/compatibility/); this
page covers only the RESP wire listener started by `nimbus kv`.

## Maturity

The RESP surface is an early, bounded compatibility surface. It covers string
and key operations only — there are no hash, list, set, sorted-set, stream, or
pub/sub commands. It runs only as the explicit `nimbus kv` command, is not
started by `nimbus dev` or `nimbus start`, and refuses any non-loopback bind
address.

## Protocol

| Property | Value |
| --- | --- |
| Wire protocol | RESP2 and RESP3, negotiated with `HELLO` |
| Protocol before `HELLO` | RESP2 |
| Server identity (`HELLO` reply) | RESP3: `server` `nimbus-kv`, `version`, `proto`, `mode` `standalone`, `role` `master`. RESP2: `server` `nimbus-kv` and `proto` only |
| Max buffered request | 1,048,576 bytes (1 MiB); a request that grows past this closes the connection |
| Network binding | Loopback only; a non-loopback bind is refused at startup |

## Authentication

Authentication is required. Every data, connection, and introspection command
is rejected with `NOAUTH Authentication required` until the connection
authenticates. A connection authenticates with `AUTH`, or with `HELLO` carrying
an `AUTH` clause.

| Command | Forms | Notes |
| --- | --- | --- |
| `AUTH` | `AUTH password`, `AUTH username password` | A wrong pair returns `WRONGPASS`. The one-argument form matches the single configured credential by password. |
| `HELLO` | `HELLO 2`, `HELLO 3`, either with `AUTH username password` | Selects the protocol version for the connection; an unsupported version returns `NOPROTO`. Without a prior `AUTH` and without an `AUTH` clause, `HELLO` returns `NOAUTH`. |
| `QUIT` | `QUIT` | Closes the connection with `OK` when authenticated; `NOAUTH` otherwise. |

A credential is bound to exactly one tenant. The connection operates entirely
within that tenant's keyspace.

## Data commands

Each command below requires an authenticated connection.

| Command | Forms | Behavior |
| --- | --- | --- |
| `GET` | `GET key` | Returns the value, or a null reply when the key is absent or expired. |
| `SET` | `SET key value`, `SET key value EX seconds`, `SET key value PX milliseconds` | Stores the value. `EX`/`PX` set a relative expiry in seconds/milliseconds. Other options are rejected with a syntax error. |
| `DEL` | `DEL key [key ...]` | Deletes one or more keys; returns the count actually removed. |
| `INCR` | `INCR key` | Increments the integer value by one; a missing key starts at zero. A non-integer value or an overflow is an error. |
| `EXPIRE` | `EXPIRE key seconds` | Sets a relative expiry. Returns `1` when applied, `0` when the key is absent. |
| `TTL` | `TTL key` | Remaining time to live in seconds. `-1` when the key has no expiry; `-2` when the key is absent. |
| `FLUSHALL` | `FLUSHALL`, `FLUSHALL SYNC`, `FLUSHALL ASYNC` | Clears the authenticated tenant's keyspace. `SYNC`/`ASYNC` are accepted and behave identically. |

## Connection and introspection commands

| Command | Forms | Behavior |
| --- | --- | --- |
| `PING` | `PING`, `PING message` | Returns `PONG`, or echoes the message. |
| `ECHO` | `ECHO message` | Returns the message. |
| `SELECT` | `SELECT 0`, `SELECT <bound-tenant-id>` | Accepts logical database `0` or the connection's bound tenant id. Any other value is rejected; `SELECT` cannot change tenant. |
| `CLIENT` | `CLIENT SETINFO ...` | `SETINFO` returns `OK`. Other `CLIENT` subcommands are rejected. |
| `COMMAND` | `COMMAND` | Returns an empty array. |
| `FUNCTION` | `FUNCTION FLUSH [SYNC\|ASYNC]` | `FLUSH` returns `OK`. Other `FUNCTION` subcommands are rejected. |
| `NIMBUS.READY` | `NIMBUS.READY` | Returns `READY`. |
| `NIMBUS.METRICS` | `NIMBUS.METRICS` | Returns a text metrics dump for the listener. |

## Storage modes

The listener stores each tenant's keyspace through the Nimbus storage layer.
`nimbus kv` selects one of three modes:

| Mode | Flag | Behavior |
| --- | --- | --- |
| Durable | default | A redb file per tenant, fronted by a read-through cache. |
| Durable without cache | `--no-cache` | Durable redb storage with the read-through cache disabled. |
| In-memory | `--no-disk` | State kept in memory only; nothing is written to disk. |

`--maxmemory` sets an approximate byte budget for the read-through cache;
`--no-disk` and `--no-cache` cannot be combined.

## Tenancy

The listener serves one tenant per credential. Keys, expiries, and `FLUSHALL`
are scoped to the authenticated tenant, and `SELECT` cannot cross that
boundary. Run separate `nimbus kv` credentials, or separate listeners, for
separate tenants.
