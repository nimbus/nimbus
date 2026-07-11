# Nimbus agent chat

A durable chat agent built with the native `@nimbus/nimbus` SDK. Every turn —
user message and assistant reply — is a document in the `messages` table, so
the conversation survives a server restart with no separate session store.
The agent recognizes three tool triggers in plain text: `remember: <fact>`
persists a fact to a per-conversation `agentMemory` table, `what do you
remember` recalls a live count of stored facts, and `remind me in <N>ms:
<text>` schedules a follow-up turn via `ctx.scheduler.runAfter` that lands in
the conversation on its own, with no client-side polling or timer.

This is the sovereignty story made concrete: the "agent" here is not a call
out to a hosted model API — it is application code running inside your own
Nimbus deployment, reading and writing through the same database, scheduler,
and trust boundary as any other mutation. There is no external inference
service in this example; the point is to demonstrate the durable-state and
scheduling primitives an agent needs, all owned by you, not fabricate a
chatbot. See the [`agent-chat` spec](../../specs/agent-chat.md) for exactly
which primitives that covers and why (SDK-real surfaces only — query,
mutation, `ctx.scheduler.runAfter`, and the database; no cron, no sandbox
execution, no egress).

The app implements the full shared [`agent-chat` spec](../../specs/agent-chat.md).

## Spec subset

| Flow anchor | Supported | Observable behavior |
| --- | --- | --- |
| `agent-chat.converse` | yes | A plain message gets a persisted assistant reply with no tool tag. |
| `agent-chat.remember` | yes | `remember: <fact>` persists a new `agentMemory` row with that exact text. |
| `agent-chat.recall` | yes | `what do you remember` replies with a live, dynamically computed fact count. |
| `agent-chat.schedule` | yes | `remind me in <N>ms: <text>` schedules a follow-up turn that lands in the conversation after the delay, with no client action. |

### A scheduler constraint this app works within, not around

`ctx.scheduler.runAfter` can only target a mutation whose handler compiles to
a single, statically-analyzable database operation that returns that
operation's own result directly — the scheduler replays a stored plan, not a
V8 closure. `deliverReminder` (the scheduled target) is written to fit that
shape: it takes the already-formatted reminder text and delivery-time
timestamp as plain arguments and does exactly one `ctx.db.insert`, returning
its result. All of the actual tool-detection logic — recognizing `remind me
in ...`, formatting the reminder text, and computing the delivery timestamp —
lives in `send`, the unrestricted runtime-handler mutation that calls the
scheduler. This is not a workaround or a fabricated API; it is how a
schedulable mutation is written against the real constraint, the same way
`examples/convex/http/convex/messages.ts`'s `sendInternal` is written as a
single pass-through insert for the same reason.

## Running

```bash
nimbus dev
nimbus deploy [TARGET]
```

`TARGET` is a URL or a configured target name; omit it to target your local
server. For the standalone Vite dev server, run `npm run nimbus:example:agent-chat`.

Tenant creation in browser code (`src/main.tsx`'s unauthenticated
`POST /api/tenants`) is a local-development convenience. Provision tenants
separately before deploying beyond your own environment.

**This is a single-user local demo with no auth.** `list`, `listMemory`, and
`send` (`nimbus/agent.ts`) are public functions that take a plain
`conversationId` string and read or write whatever conversation that id
names, with no identity or ownership check — the UI hardcodes one shared id
(`CONVERSATION_ID` in `src/App.tsx`), so every browser tab is the same
conversation, by design. Identity and ownership checks are required before
deploying this beyond your own machine; this example does not add them. See
[authenticate users](../../../docs/developers/auth.md) for how to wire a
provider and check `ctx.auth` in your own functions.

## Smoke verification

With Nimbus running at `http://localhost:8080`:

```bash
NIMBUS_ADMIN_TOKEN="$(nimbus auth token)" npm run smoke -w nimbus-agent-chat
```

Set `NIMBUS_NATIVE_URL` to exercise another Nimbus URL. The smoke uses a
fresh, timestamp-suffixed conversation id per run and prints one `PASS` line
per flow anchor, including a real `NimbusClient.onUpdate` push proving the
scheduled reminder lands server-side with no polling. A server that does not
require local admin authentication can omit `NIMBUS_ADMIN_TOKEN`.
