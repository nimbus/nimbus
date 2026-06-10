---
title: Developers
description: Build apps on Nimbus — functions, schema, scheduling, adapters, and the SDK.
sidebar:
  order: 1
---

Guides for app authors building on Nimbus: tutorials that teach the platform
and how-to guides for getting specific things done. Start with the
[developer quickstart](/get-started/quickstart/) if you haven't run Nimbus
yet, then [build your first app](/developers/first-app/).

## Platform guides

- [Build your first app](/developers/first-app/) — a complete tutorial:
  schema, functions, and live queries.
- [Authenticate users](/developers/auth/) — wire an identity provider into
  your functions.
- [Node.js runtime](/developers/runtimes/nodejs/) — `"use node"` actions,
  packages, and bundling.
- [SDK resource model](/developers/sdk/resource-model/) — services,
  sandboxes, and sessions with the `nimbus` JS SDK.

## Adapter guides

Each protocol surface has a front door and a migration guide:

- [Convex](/developers/convex/) — the Convex function model, schema, and
  clients; [migrate a Convex app](/developers/convex/migrate/).
- [Firestore](/developers/firebase/) — Firestore SDKs against Nimbus;
  [migrate a Firebase app](/developers/firebase/migrate/).
- [Cloud Functions](/developers/cloud-functions/) — Cloud Functions
  workloads; [migrate](/developers/cloud-functions/migrate/).
- [MongoDB](/developers/mongodb/) — connect official MongoDB drivers;
  [driver recipes](/developers/mongodb/examples/).
- [DynamoDB](/developers/dynamodb/) — point AWS SDK clients at Nimbus.
- [Native API](/developers/native/) — plain HTTP and WebSocket from any
  language, no SDK required.

The full per-surface compatibility matrices live in the
[Reference](/reference/) section.
