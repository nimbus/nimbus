---
title: Run Cloud Functions workloads on Nimbus
description: Run firebase-functions/v2 and Functions Framework handlers on Nimbus with durable Firestore triggers and at-least-once delivery.
sidebar:
  label: Overview
  order: 1
---

Run your existing `firebase-functions/v2` handlers and
`@google-cloud/functions-framework` targets on Nimbus without source
rewrites. Firestore document triggers get at-least-once delivery with
durable retry, and HTTP/callable handlers serve directly from the Nimbus
server.

```typescript
// functions/src/index.ts
import { onRequest } from "firebase-functions/v2/https";
import { onDocumentCreated } from "firebase-functions/v2/firestore";

export const hello = onRequest(async (req, res) => {
  res.json({ message: "Hello from Nimbus Cloud Functions!" });
});

export const onMessageCreated = onDocumentCreated(
  "messages/{messageId}",
  async (event) => {
    console.log("New message:", event.data?.data());
  },
);
```

## Before you start

- Install Nimbus. See the [developer quickstart](/get-started/quickstart/)
  for install options.
- Install Node.js 22 or newer with `npm`. Cloud Functions code generation
  runs through the external Node toolchain.
- The `firebase-functions` and `firebase-admin` packages are your own
  dependencies, installed from the npm registry (or a preinstalled
  `node_modules`).

## Start a new project

```bash
nimbus init cloud-functions my-functions-app
cd my-functions-app
nimbus dev
```

`nimbus init cloud-functions` scaffolds a conventional Firebase layout. It
creates `firebase.json` and a `functions/` package. The package contains
`package.json`, `tsconfig.json`, and `src/index.ts`. The source file defines an
`onRequest` handler and an `onDocumentCreated` trigger. `nimbus dev` finds the
app root, runs codegen, and serves the local deployment. It runs codegen again
when you edit the source.

## Bring an existing Firebase project

Keep your functions source unchanged and run `nimbus dev` from anywhere in
the project:

```bash
# in your existing Firebase project
nimbus dev
```

`nimbus dev` walks up to the `firebase.json` root and detects the Cloud
Functions app. It runs codegen and serves the local deployment. It runs
codegen again when you edit the source.

For an explicit or CI-shaped run, generate artifacts from the project root.
Then start the server with an explicit `--app-dir`. The `nimbus start`
command does not discover the source tree:

```bash
nimbus codegen
nimbus start --app-dir .
```

Test an HTTP handler. Firebase `onRequest` exports serve at
`/<exportName>` on the main server port (default `8080`):

```bash
curl http://localhost:8080/hello
```

## What you get

Firestore document triggers run with:

- **at-least-once delivery** backed by a durable invocation ledger
- **crash/restart replay** for pending and due-retry invocations
- **bounded retry** for retryable failures
- **service-principal execution** (not the calling end-user principal)
- **chain-depth limiting** so recursive write-back triggers stop at a
  configured depth instead of looping forever
- **no-op suppression**: overwrites that change nothing do not emit
  update events

Write handlers to be idempotent: redelivery is possible by design.

## Generated artifacts

`nimbus codegen` writes Cloud Functions outputs under `.nimbus/firebase/`:

```text
.nimbus/firebase/
  artifact.json
  targets.json
  bundle.mjs
  bundle.sha256
```

Standalone Functions Framework packages must author
`.nimbus/firebase/targets.json`. The `functions.http(...)` and
`functions.cloudEvent(...)` calls name targets without source binding
metadata. See [Migrate Cloud Functions](/developers/cloud-functions/migrate/)
for the file format.

## Deploy

```bash
nimbus deploy <server-url> --token <deploy-token>
```

`TARGET` (the positional server URL or a configured target name) and `--token`
can also come from the `NIMBUS_TARGET_URL` and `NIMBUS_DEPLOY_TOKEN`
environment variables. Omit `TARGET` to deploy to the running local server.

## Where next

- [Example bundle](/developers/cloud-functions/examples/): a runnable v2
  bundle with a Firestore trigger and an HTTP handler.
- [Migrate Cloud Functions](/developers/cloud-functions/migrate/): the
  step-by-step path for Firebase v2 and Functions Framework codebases.
- [Cloud Functions compatibility](/reference/cloud-functions/compatibility/)
  defines the precise support matrix, option boundaries, and non-goals.
