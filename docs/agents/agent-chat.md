---
title: Build a durable chat agent
description: Run a chat agent whose memory, tools, and scheduled follow-ups all live inside your own Nimbus deployment — built on the native SDK's document store and scheduler.
sidebar:
  label: Agent chat
  order: 6
---

This tutorial runs a durable chat agent built on the native `@nimbus/nimbus`
SDK and shows what makes it durable: its conversation, its memory, and its
scheduled follow-ups all live in your own Nimbus deployment, behind the same
trust boundary as any other write. Nothing here calls out to a hosted model —
the "agent" is application code you run and own.

The example lives at
[`examples/nimbus/agent-chat`](https://github.com/nimbus/nimbus/tree/main/examples/nimbus/agent-chat).
Run it in place from a checkout of the repository.

## What you will build

Every turn — your message and the agent's reply — is a document in a `messages`
table, so the conversation survives a server restart with no separate session
store. The agent recognizes three tool triggers in plain text:

- `remember: <fact>` persists a fact to a per-conversation `agentMemory` table.
- `what do you remember` replies with a live count of the stored facts.
- `remind me in <N>ms: <text>` schedules a follow-up turn that lands in the
  conversation on its own after the delay, with no client-side polling or timer.

## Run it

Start the local development server:

```bash
nimbus dev
```

Then deploy the app:

```bash
nimbus deploy [TARGET]
```

`TARGET` is a URL or a configured target name; omit it to target your local
server. To interact in the browser, run the standalone dev server:

```bash
npm run nimbus:example:agent-chat
```

## Watch each capability

Send a plain message. The conversation gains your `user` turn and a persisted
`assistant` reply — an ordinary reactive read, no hidden state.

Send `remember: the launch is on Friday`. The agent writes the fact to its
`agentMemory` table and replies to confirm. Then send `what do you remember`:
the reply reports a fact count computed live from the memory store, not a
hardcoded number.

Send `remind me in 5000ms: stand up`. You get an immediate acknowledgement, and
about five seconds later a further `assistant` turn containing the reminder text
appears in the conversation with no further action from you. That follow-up was
delivered by the server's own scheduler, not the browser.

## Why the reminder lands on its own

The reminder uses `ctx.scheduler.runAfter`, which can only target a mutation
that compiles to a single database operation returning its own result — the
scheduler replays a stored plan, not a live function closure. So the scheduled
target, `deliverReminder`, does exactly one `ctx.db.insert` of the
already-formatted reminder turn. All the tool detection — recognizing the
`remind me in ...` trigger, formatting the text, and computing the delivery time
— happens up front in `send`, the unrestricted mutation that calls the
scheduler. This is how a schedulable mutation is written against Nimbus's real
constraint, not a workaround.

## The sovereignty point

There is no external inference service in this example. The agent's state, its
tool effects, and its scheduled follow-ups run inside your Nimbus deployment,
reading and writing through the same document store, scheduler, and trust
boundary as any other mutation. It uses only surfaces that exist in the public
SDK today — `query`, `mutation`, `ctx.db`, and `ctx.scheduler.runAfter` — and no
sandbox execution or egress. Swapping the plain-text triggers for real model
calls is an application change; the durable-state and scheduling primitives an
agent needs are already yours.

The browser code creates its tenant with an unauthenticated `POST /api/tenants`,
which is a local-development convenience. Provision tenants separately before
deploying beyond your own environment.

**This is a single-user local demo with no auth.** The `list`, `listMemory`,
and `send` functions are public and take a plain `conversationId` string with
no identity or ownership check — the UI hardcodes a single shared
conversation id, so every browser tab talks to the same conversation, by
design. Add identity and ownership checks (see
[authenticate users](/developers/auth/)) before deploying this beyond your
own machine; this example does not demonstrate that.
