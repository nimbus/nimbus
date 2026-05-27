# MBA12 Read-Consistency Routing Proof

status: done

not_applicable: redb, SQLite, Postgres, MySQL, Cloud Functions, MongoDB

## Current Code Evidence

- libSQL replica has an explicit freshness and catch-up surface in
  `crates/nimbus-storage/src/libsql/freshness.rs` and provider poll workers
  behind `RuntimeHooks`.
- Engine/provider hints perform authoritative catch-up before relying on
  notifications in `crates/nimbus-engine/src/service/provider_hints.rs`.
- Embedded replica and consistency routes are exercised by
  `crates/nimbus-server/src/tests/core_http/documents_and_commits/consistency.rs`.
- Firebase listen resume uses retained snapshots only when sequence/read-time
  proof matches in
  `crates/nimbus-server/src/adapters/firebase/grpc/listen_stream.rs`.

## Routing Decision

Backends without a separate read surface are marked `not_applicable` and use
the authoritative path. libSQL replica is the current real eventual-read family:
writes go to the remote primary, and local cache reads are valid only after the
provider-owned refresh/freshness path proves they are sufficiently caught up.

No adapter gets a synthetic eventual-consistency layer.
