# Codex Agent Prompt - Native Transport Evolution Plan Research And Refresh

Use this prompt when the repo is ready to refresh or activate
`docs/plans/native-transport-evolution-plan.md`.

Copy the full text below into a fresh Codex agent.

---

## Prompt

You are Codex working in the Nimbus repository:

`/Users/jack/src/github.com/nimbus/nimbus`

Nimbus is a Rust workspace plus npm monorepo that implements a
Convex-compatible backend server. Your task is to research and refine the
future native transport plan at:

`docs/plans/native-transport-evolution-plan.md`

This is a **Nimbus-native transport** planning task. It is not a Firebase
transport task, and it must not duplicate the ownership already held by:

- `docs/plans/archive/websocket-protocol-plan.md`
- `docs/plans/archive/firebase-adapter-plan.md`
- `docs/plans/archive/firebase-cloud-functions-plan.md`

## Required Startup

Read these files first, in order:

- `AGENTS.md`
- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `docs/plans/archive/websocket-protocol-plan.md`
- `docs/plans/native-transport-evolution-plan.md`
- `docs/plans/archive/firebase-adapter-plan.md`
- `docs/plans/archive/firebase-cloud-functions-plan.md`

Then run:

- `git status --short`

If the worktree is dirty, inspect the changed files before editing. Treat
existing changes as user or prior-agent work; do not revert them unless the
user explicitly asks.

## Goal

Produce or refine a plan for Nimbus-native transport evolution that:

- keeps `nimbus-core` and `nimbus-engine` transport-agnostic,
- keeps Convex and Firebase compatibility contracts intact,
- avoids duplicating the active WebSocket protocol plan,
- and makes future binary codec or WebTransport decisions based on evidence
  rather than assumption.

## Important Ownership Rules

1. `docs/plans/archive/websocket-protocol-plan.md` already owns:
   - WebSocket subprotocol negotiation
   - hello or client_hello handshake
   - structured error schema
   - versioned WebSocket protocol negotiation
2. `docs/plans/native-transport-evolution-plan.md` must only own:
   - Nimbus-native transport and codec evolution after that groundwork
   - transport-neutral session and codec seams
   - optional binary native codec evaluation
   - optional WebTransport evaluation
3. Do not move Firebase browser transport work into the native transport plan.
4. Do not move Convex wire-compatibility work into the native transport plan.

## Code And Docs To Inspect

Server transport and protocol:

- `crates/nimbus-server/src/protocol.rs`
- `crates/nimbus-server/src/ws/mod.rs`
- `crates/nimbus-server/src/ws/socket/transport.rs`
- `crates/nimbus-server/src/ws/socket/session.rs`
- `crates/nimbus-server/src/adapters/convex/subscriptions/socket/`

Browser SDK transport:

- `packages/nimbus/src/browser.ts`
- `packages/nimbus/src/browser-utils.ts`
- `packages/nimbus/src/http-client.ts`

Current Firebase transport references, only to preserve boundaries:

- `docs/plans/archive/firebase-adapter-plan.md`
- `crates/nimbus-server/src/adapters/firebase/grpc/`
- `packages/firebase/`

## Research Questions To Answer In The Plan

1. Where does native JSON serialization happen today for HTTP and WebSocket?
2. Which parts of session and subscription behavior are transport-specific, and
   which parts are reusable semantics?
3. What exactly must remain owned by the WebSocket protocol plan versus the
   native transport evolution plan?
4. What realistic performance or ergonomics problem would optional MessagePack
   solve for Nimbus-native clients?
5. What realistic performance or reliability problem would optional
   WebTransport solve for Nimbus-native clients?
6. What benchmark corpus and harness would be needed before changing any native
   wire-format default?
7. What activation gate should remain in place before this plan turns active?

## External Research

Use current 2025-2026 sources and compare:

- WebTransport browser support and caveats
- Rust WebTransport libraries and maturity
- MessagePack library choices for JS/browser clients
- Real-time product patterns for JSON WebSocket, binary WebSocket, and
  WebTransport adoption
- Evidence relevant to browser bidi transport constraints

Prefer official docs, maintained library docs, and primary project sources.

## Plan Requirements

When updating `docs/plans/native-transport-evolution-plan.md`:

- Keep it clearly marked as `proposed` unless the user explicitly asks to
  activate it.
- Do not re-own work already listed in `docs/plans/archive/websocket-protocol-plan.md`.
- Keep JSON as the default native baseline unless the plan has benchmark-backed
  evidence to recommend changing that default.
- Treat MessagePack and WebTransport as optional future capabilities, not as
  assumed immediate replacements.
- Keep the plan concise and concrete.
- Use the repo's normal plan conventions for status, activation gate, current
  state, and roadmap items.

## Explicit Non-Goals

- Do not propose protobuf for the native Nimbus protocol.
- Do not propose replacing tonic for Firebase transport.
- Do not propose forcing one public wire protocol across Nimbus, Convex, and
  Firebase.
- Do not propose stock Firebase WebChannel work in this plan.

## Final Output

When you finish:

- summarize what changed in the proposed plan,
- state whether the plan should remain deferred or be promoted,
- list the recommended execution order relative to
  `docs/plans/archive/websocket-protocol-plan.md`,
  `archive/firebase-adapter-plan.md`, and
  `archive/firebase-cloud-functions-plan.md`,
- and keep the answer concise.
