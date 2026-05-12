# Native Transport Evolution Plan

Proposed follow-on plan for Nimbus-native client transport evolution after the
completed Firebase compatibility, multi-adapter hardening, and
runtime-capability boundary cleanup waves. This plan exists to keep the native
transport architecture clean and extensible pre-launch without conflating that
work with Firebase compatibility or duplicating the active WebSocket protocol
plan.

The intended scope is narrow and explicit:

- preserve the current JSON-over-HTTP plus JSON-over-WebSocket baseline as the
  shipping native contract until a better option is proven on Nimbus's actual
  workload,
- extract the internal seams needed for future codec negotiation and future
  transport alternatives,
- evaluate optional binary wire formats such as MessagePack with benchmark
  evidence,
- evaluate WebTransport only as a follow-on transport behind the same
  session semantics, not as a replacement forced across every adapter,
- and add cursor-based connection resumption so reconnecting clients can
  resume subscriptions from a known position instead of re-fetching from
  scratch.

This is a Nimbus-native plan. It does **not** own Firebase unary transport,
Firestore protobuf schemas, Firebase WebChannel compatibility, or Convex wire
compatibility work.

## Status

- **Plan status:** `proposed`
- **Primary owner:** unassigned until activation
- **Historical activation gate:** satisfied on `2026-04-26`:
  `docs/plans/archive/websocket-protocol-plan.md`,
  `docs/plans/archive/firebase-adapter-plan.md`, and
  `docs/plans/archive/firebase-cloud-functions-plan.md`, and
  `docs/plans/archive/runtime-capability-adapter-boundary-plan.md`, and
  `docs/plans/archive/multi-adapter-boundary-hardening-plan.md`, and
  `docs/plans/archive/server-runtime-canonicalization-plan.md` are `done`
- **Execution posture:** authoring and plan refinement may happen pre-launch;
  implementation remains deferred until this plan is explicitly activated

## Why This Exists

Pre-launch is the right time to decide whether Nimbus should keep its current
native JSON transport as the long-term default, add optional binary codecs, or
grow an alternative transport such as WebTransport. It is **not** the right
time to assume those changes are automatically better without repo-specific
evidence.

This plan captures the future architecture work so the repo can:

- stay DRY through shared session and codec seams,
- avoid transport-specific logic leaking into `nimbus-core` or
  `nimbus-engine`,
- and preserve compatibility boundaries for Convex and Firebase instead of
  forcing one public wire protocol everywhere.

## Current Assessed State

- The native WebSocket path is JSON-text only today. Inbound and outbound
  serialization happen directly in
  `crates/nimbus-server/src/ws/socket/transport.rs`.
- Native WebSocket frame shapes are currently defined in
  `crates/nimbus-server/src/protocol.rs` as JSON-tagged `ClientMessage` and
  `ServerMessage`.
- The browser SDK path in `packages/nimbus/src/browser.ts`,
  `packages/nimbus/src/browser-utils.ts`, and
  `packages/nimbus/src/http-client.ts` is also JSON-specific today.
- `docs/plans/archive/websocket-protocol-plan.md` already owns subprotocol
  negotiation, hello or client_hello handshake, structured error schema, and
  versioned protocol negotiation. This plan must consume that groundwork
  rather than re-own it.
- Firebase browser compatibility already has its own direction: generated
  Firestore protobuf types, modern unary browser transport, and a dedicated
  Firestore `Listen` transport. That work stays under
  `docs/plans/archive/firebase-adapter-plan.md`.
- Convex compatibility keeps its own observable wire contract. Native
  transport evolution must not force Convex or Firebase adapters to use the
  same public wire format.
- The completed WebSocket protocol plan landed a `NegotiatedWebSocketProtocol`
  enum (`ws/negotiation.rs`) and a `ServerMessage::to_text(protocol)` dispatch
  method (`protocol.rs`) that already branches on protocol version to produce
  different JSON shapes (v1 flat errors vs v2 structured envelopes). This is a
  nascent codec dispatch point that NTE2 should formalize rather than replace.
- There is a reader/writer asymmetry in the current transport layer:
  `spawn_socket_writer` uses the negotiated protocol via `to_text(protocol)`,
  but `spawn_socket_reader` hardcodes `serde_json::from_str::<ClientMessage>`
  and is not protocol-aware. This asymmetry is fine while both v1 and v2 use
  JSON text, but NTE2 must make the reader protocol-aware before binary frames
  are possible.
- The native WebSocket protocol has no connection-resumption or cursor-based
  subscription resume mechanism today. Reconnection means re-subscribing from
  scratch. This is a bigger user-experience gap than wire-format efficiency for
  most workloads.

## Design Rules

1. `nimbus-core` remains zero-I/O and wire-format agnostic.
2. `nimbus-engine` remains transport-agnostic; transport framing and codec
   selection stay in server and SDK layers.
3. JSON remains the default native wire format until a benchmark-backed plan
   item explicitly changes that default.
4. Future binary codec support is optional and negotiated; it is not assumed to
   replace JSON globally.
5. Future WebTransport support is additive behind shared session semantics; it
   is not a reason to fork Nimbus semantics or bypass the active WebSocket
   protocol plan.
6. Firebase and Convex compatibility contracts stay intact even if native
   Nimbus transport evolves.

## Out Of Scope

- Firebase WebChannel compatibility
- Firebase unary gRPC-Web or protobuf transport work
- Firestore `Listen` browser transport work
- Convex wire-protocol changes
- Replacing tonic or the Firebase transport stack
- Immediate activation of WebTransport without repo-specific evidence

## Proposed Phases

### NTE1 Evidence And Decision Baseline

Refresh the current native transport evidence before any execution work:

- inventory exactly where JSON serialization and WebSocket framing are coupled
  in server and SDK code,
- collect current browser and Rust ecosystem state for WebTransport and codec
  libraries,
- and define the benchmark harness and representative Nimbus-native payloads
  needed to compare JSON versus optional binary codecs, including
  JSON-with-`permessage-deflate` WebSocket compression as a baseline so the
  evidence can distinguish whether the simpler change (enabling compression)
  closes most of the gap before committing to a binary codec.

Completion gate:

- The plan records the concrete serialization and session coupling points, the
  candidate codec and transport libraries, and the benchmark methodology needed
  before any activation decision.

### NTE2 Shared Native Session And Codec Seams

Extract the internal server and SDK seams required to avoid codec and transport
logic being scattered:

- formalize the existing `NegotiatedWebSocketProtocol` + `to_text(protocol)`
  dispatch into a general codec dispatch point that returns `Vec<u8>` (or a
  `Bytes`-like type) instead of `String`, so binary codecs can use the same
  path,
- make `spawn_socket_reader` protocol-aware so inbound deserialization is
  symmetric with the outbound path,
- extract transport-neutral session semantics where feasible,
- and keep explicit boundaries between message semantics, codec, and socket
  transport.

Completion gate:

- Native session semantics no longer assume JSON at every call site, the
  reader and writer both dispatch through the negotiated protocol, and the
  WebSocket protocol plan remains the owner of negotiation and error-shape
  behavior.

### NTE3 Optional Binary Native Codec

Evaluate and, if justified by evidence, add an optional negotiated binary codec
for the native Nimbus protocol.

Likely first candidate:

- MessagePack because the repo already uses `rmp-serde` at rest and the data
  model is already `serde`-driven.

Completion gate:

- A benchmark-backed decision exists for whether Nimbus should ship optional
  MessagePack support, and if yes, the rollout is explicitly negotiated and
  remains JSON-default.

### NTE4 WebTransport Evaluation

Evaluate WebTransport as an optional future native transport, only after the
earlier phases land and the supporting ecosystem is mature enough.

Known risk: WebTransport requires HTTP/3, which requires TLS. For local
development (`localhost`), this means generating and trusting self-signed
certificates or using a tool like `mkcert`. The local-dev ergonomic cost must
be weighed during evaluation — if the certificate ceremony is too painful for
the developer-machine flow, WebTransport may only be viable for deployed
environments.

Completion gate:

- The plan records whether WebTransport should remain deferred, ship as an
  experiment, or be promoted toward wider use, based on browser support, Rust
  implementation maturity, local-development ergonomics (including TLS
  certificate management), and Nimbus workload evidence.

### NTE5 Connection Resumption

Add cursor-based subscription resumption so clients can reconnect after a
transient disconnection and resume from a known position without re-fetching
all subscription state from scratch.

This is arguably more user-visible than binary codecs or WebTransport:
reconnection storms after a brief network interruption are the most common
real-time UX degradation, and re-subscribing from scratch multiplies both
server load and perceived latency.

Scope:

- server-side: subscription snapshots carry a resume cursor (commit sequence
  or opaque token) that the client can present on reconnect,
- client-side: the SDK stores the last-seen cursor per subscription and sends
  it in the `subscribe` message on reconnect,
- server responds with a delta from the cursor position when possible, or a
  full snapshot with a flag indicating the cursor was too stale.

Completion gate:

- A reconnecting client with a valid cursor receives only the changes since
  its last-seen position, and the server gracefully degrades to a full
  snapshot when the cursor is expired.

## Initial Roadmap

| Item | Status | Depends on | Goal |
|------|--------|------------|------|
| NTE1.1 Evidence refresh and codebase inventory | `pending` | activation gate | Record current JSON framing points, existing `NegotiatedWebSocketProtocol` seam, reader/writer asymmetry, session coupling, and the exact reuse boundary with `docs/plans/archive/websocket-protocol-plan.md` |
| NTE1.2 External transport and codec research refresh | `pending` | NTE1.1 | Refresh 2026 ecosystem evidence for WebTransport, MessagePack libraries, browser support, and Rust implementation maturity |
| NTE1.3 Benchmark harness and payload definition | `pending` | NTE1.1 | Define the Nimbus-native message corpus and benchmark method; must include JSON-with-`permessage-deflate` as a baseline alongside raw JSON and binary candidates |
| NTE2.1 Server codec dispatch seam | `pending` | NTE1.3 | Formalize `to_text(protocol)` into a bytes-level dispatch; make `spawn_socket_reader` protocol-aware to match the writer |
| NTE2.2 SDK codec and transport seam | `pending` | NTE2.1 | Keep browser and HTTP clients transport-aware but message-semantics-shared |
| NTE3.1 MessagePack spike | `pending` | NTE2.2 | Prototype optional native MessagePack negotiation behind the existing native protocol semantics |
| NTE3.2 Native codec default decision | `pending` | NTE3.1 | Decide whether JSON stays default, MessagePack stays optional, or JSON-with-compression closes the gap and NTE3 ships as optional-only |
| NTE4.1 WebTransport spike | `pending` | NTE2.2 | Prototype native WebTransport behind the shared session semantics without replacing WebSocket; evaluate TLS certificate ergonomics for local development |
| NTE4.2 Native transport posture decision | `pending` | NTE4.1, NTE3.2 | Decide whether WebTransport stays deferred, experimental, or promotable after the pre-launch evidence review |
| NTE5.1 Resume cursor design | `pending` | NTE2.1 | Design the cursor token format, staleness policy, and delta-vs-full-snapshot protocol extension |
| NTE5.2 Server-side resume support | `pending` | NTE5.1 | Server stores resume cursors per subscription and serves deltas or full snapshots on reconnect |
| NTE5.3 SDK resume integration | `pending` | NTE5.2 | SDK persists last-seen cursor and sends it on reconnect; handles stale-cursor fallback gracefully |

## Planned Execution Order

When the activation gate is met, the intended order is:

1. Finish the active WebSocket protocol hardening work first.
2. Close the Firebase adapter and Firebase Cloud Functions compatibility work.
3. Activate this plan and start with evidence plus benchmark work, not with a
   forced transport rewrite.
4. Land NTE2 (codec and session seams) — this is prerequisite infrastructure
   for both binary codecs and connection resumption.
5. NTE5 (connection resumption) may proceed in parallel with or before NTE3
   (binary codec) since it depends only on NTE2.1 and has higher user-visible
   impact than wire-format optimization.
6. Only after the evidence refresh should the repo decide whether optional
   MessagePack or optional WebTransport should move from plan to code.
