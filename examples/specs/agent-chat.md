# Spec: `agent-chat`

A durable chat agent: every message is a mutation against Nimbus's own
document store, every reply is produced by a plain function handler running
tool-call-style branching, and a reminder tool proves the agent can act later
by itself through `ctx.scheduler`. Nothing here is a hosted "AI" call — the
point is that agent state, tool effects, and scheduled follow-ups all live
inside your own trust boundary, backed by nothing but the public SDK surface
(`query` / `mutation` / `ctx.db` / `ctx.scheduler`).

This is a Nimbus-native (`@nimbus/nimbus`) spec, not a cross-adapter one like
[`tasks`](tasks.md): it exercises `nimbus/`-authored function definitions
directly, with no Convex compatibility layer involved.

## Schema

Two collections.

`messages` — one row per chat turn, user or assistant:

| Field | Type | Notes |
| --- | --- | --- |
| `conversationId` | string | Groups turns into one conversation. |
| `role` | string | `"user"` or `"assistant"`. |
| `text` | string | The turn's text. |
| `tool` | string (optional) | Which tool produced an assistant reply (`"remember"`, `"recall"`, `"remind"`), if any. |
| `createdAt` | number | Creation time in epoch milliseconds. |

`agentMemory` — one row per fact the agent has been told to remember:

| Field | Type | Notes |
| --- | --- | --- |
| `conversationId` | string | Which conversation the fact belongs to. |
| `text` | string | The remembered fact, verbatim. |
| `createdAt` | number | Creation time in epoch milliseconds. |

## Flows

Each flow has a stable **anchor** (`agent-chat.converse`, …). The anchor is
the contract key: the smoke script references the flow it exercises by anchor
name. Anchors are append-only — never renamed or reused.

| Anchor | Flow | Observable assertion (what the smoke asserts) |
| --- | --- | --- |
| `agent-chat.converse` | Send a plain message with no tool trigger. | The conversation gains a `role: "user"` turn with the sent text and a `role: "assistant"` reply turn with `tool` unset. |
| `agent-chat.remember` | Send `remember: <fact>`. | The agent's memory tool inserts the fact; a memory read for the conversation grows by one entry containing that exact text, and the assistant reply's `tool` is `"remember"`. |
| `agent-chat.recall` | Send `what do you remember` after at least one remembered fact. | The assistant reply's `tool` is `"recall"` and its text reports a fact count greater than zero, computed live from the memory store (not hardcoded). |
| `agent-chat.schedule` | Send `remind me in <N>ms: <text>`. | The agent's scheduling tool calls `ctx.scheduler.runAfter`; the immediate assistant reply has `tool === "remind"`; after waiting past the delay, a further `role: "assistant"` turn containing the reminder text has landed in the conversation with no further client action — proof the follow-up was delivered by the server's own scheduler, not the client. |

A smoke script must assert the observable outcome in the third column, not
just that the flow's code ran.

## Supported subset by adapter

| Adapter | Supported | Notes |
| --- | --- | --- |
| Native (`@nimbus/nimbus`) | yes, all four anchors | Full spec: `query` / `mutation` for `agent-chat.converse` / `.remember` / `.recall`, `ctx.scheduler.runAfter` plus an `internalMutation` delivery handler for `agent-chat.schedule`. |

This spec is scoped to surfaces that exist today in the public SDK: durable
function definitions, the document store, and the scheduler. It deliberately
does not use cron (no public SDK/CLI surface), the sandbox/`nimbus run exec`
CLI (stubbed), or an egress policy knob (none exposed) — see the EX0.3 scoping
note in `docs/private/plans/examples-and-target-resolution-plan.md` for why
those are out of scope rather than omitted by oversight.
