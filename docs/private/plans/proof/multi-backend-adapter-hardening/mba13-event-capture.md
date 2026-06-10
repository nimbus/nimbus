# MBA13 Hybrid Event-Capture Proof

status: done
storage_atomicity: document_write_index_effects_commit_log_single_transaction

## Current Code Evidence

- Engine-owned committed mutation observers are in
  `crates/nimbus-engine/src/service/committed_mutations.rs`.
- The mutation path records storage writes and commit metadata before
  observers/subscriptions see the event.
- Convex subscription forwarding converts `SubscriptionUpdate` into Convex
  WebSocket `ServerMessage` values in
  `crates/nimbus-server/src/adapters/convex/subscriptions/socket/forwarding.rs`.
- Firebase listen translates `SubscriptionResultSnapshot` into Firestore
  `ListenResponse` frames in
  `crates/nimbus-server/src/adapters/firebase/grpc/listen_stream.rs`.
- Cloud Functions event payload construction remains adapter/server-owned;
  storage does not construct CloudEvent wire records.

## Greppable Invariant

Storage modules may persist generic documents, indexes, trigger cursors,
trigger invocation records, durable journals, and commit logs. They must not
emit Convex WebSocket frames, Firestore `ListenResponse` messages, MongoDB wire
messages, or Cloud Functions CloudEvent payloads.

Adapters may translate generic subscription/commit snapshots into protocol
events. They must not own the transaction boundary that makes document writes,
index effects, and commit-log append atomic.
