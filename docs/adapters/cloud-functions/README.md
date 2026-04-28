# Cloud Functions Adapter

```typescript
// functions/src/index.ts
import { onRequest } from "firebase-functions/v2/https";
import { onDocumentCreated } from "firebase-functions/v2/firestore";

export const hello = onRequest(async (req, res) => {
  res.json({ message: "Hello from Neovex Cloud Functions!" });
});

export const onMessageCreated = onDocumentCreated("messages/{messageId}", async (event) => {
  console.log("New message:", event.data?.data());
});
```

```bash
neovex codegen && neovex start
```

Your existing `firebase-functions/v2` handlers run on Neovex unchanged --
with at-least-once delivery, durable retry, and Firestore document triggers.
~5 minutes from install to a working trigger.

## Quick start

**New project:**

```bash
neovex init cloud-functions my-functions-app
```

```bash
cd my-functions-app
```

```bash
neovex dev
```

**Existing Firebase project:**

**1. Keep your existing Firebase functions code unchanged.**

**2. Generate artifacts and start:**

```bash
neovex codegen
neovex start
```

**3. Test it:**

```bash
curl http://localhost:8080/hello
```

**4. Deploy to production:**

```bash
neovex deploy --url http://production:8080 --token <deploy-token>
```

## Project Layout (Firebase style)

```
my-functions-app/
├── firebase.json
├── functions/
│   ├── package.json
│   ├── tsconfig.json
│   └── src/
│       └── index.ts
└── .neovex/
    └── firebase/
        ├── functions.json
        ├── bundle.mjs
        └── bundle.sha256
```

## Project Layout (standalone Functions Framework)

```
my-functions/
├── package.json
├── src/
│   └── index.ts
└── .neovex/
    └── firebase/
        ├── targets.json       # Maps functions to routes
        ├── functions.json
        ├── bundle.mjs
        └── bundle.sha256
```

## Example Code

```typescript
// functions/src/index.ts
import { onDocumentCreated, onDocumentDeleted } from "firebase-functions/v2/firestore";
import { onRequest, onCall } from "firebase-functions/v2/https";
import { initializeApp } from "firebase-admin/app";
import { getFirestore } from "firebase-admin/firestore";

initializeApp();
const db = getFirestore();

export const onMessageCreated = onDocumentCreated("messages/{messageId}", async (event) => {
  const data = event.data?.data();
  await db.collection("audit").doc().set({
    action: "message_created",
    messageId: event.params.messageId,
    timestamp: new Date().toISOString(),
  });
});

export const hello = onRequest(async (req, res) => {
  res.json({ message: "Hello from Neovex Cloud Functions!" });
});

export const greet = onCall(async (request) => {
  return { greeting: `Hello, ${request.data.name}!` };
});
```

### Standalone targets.json

```json
{
  "functions": [
    { "name": "hello", "type": "http", "path": "/hello" },
    { "name": "greet", "type": "callable" },
    { "name": "onMessageCreated", "type": "cloudEvent", "topic": "firestore.default" }
  ]
}
```

## Delivery Guarantees

- At-least-once delivery backed by durable invocation ledger
- Crash/restart replay for pending and due-retry invocations
- Chain depth limiting for recursive trigger chains
- No-op suppression (overwrites that don't change data skip update events)
- Service principal execution (not end-user principal)

## Configuration

- Firebase project auto-detected from `firebase.json` or `--app-dir`
- Standalone Functions Framework requires `.neovex/firebase/targets.json`
- Covered `firebase-admin` surface: `initializeApp()`, `getFirestore()`, `get()`, `set()`, `update()`, `delete()`

## Known Limitations

- Default database only
- `onInit()` not supported
- Broader `setGlobalOptions()` fields fail fast
- No App Check support

See the [Cloud Functions compatibility matrix](compatibility.md) for the full scope.

## Related Docs

- [Cloud Functions compatibility matrix](compatibility.md)
- [Cloud Functions migration guide](migration.md)
- [Cloud Functions artifact contract](artifact-contract.md)
- [Cloud Functions target binding contract](target-binding-contract.md)
