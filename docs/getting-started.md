# Getting Started

Install Nimbus, then pick how you want to build.

If you're authoring Convex or Cloud Functions code locally, install Node.js
with `npm` first. If you're connecting MongoDB, Firebase client, or native
HTTP/WebSocket clients to `nimbus start`, Node.js is not required.

## Install

```bash
brew install nimbus/tap/nimbus
```

See [Install](../README.md#install) for other platforms or building from source.

## Server-side functions

**1. Scaffold a Convex app:**

```bash
nimbus init convex my-app
cd my-app
```

`nimbus init convex` scaffolds backend files only: a schema, example query and
mutation, `package.json`, `tsconfig.json`, and `.gitignore`. Add your frontend
separately or point an existing frontend at the local deployment URL.

**2. Start the dev server:**

```bash
nimbus dev
```

`nimbus dev` auto-runs `npm install` when declared packages are missing
locally, creates a `demo` tenant, and starts the server on `localhost:3210`.

From there, edit the TypeScript files in `convex/` and `nimbus dev` watches
for changes, re-runs codegen, and activates updated functions with reactive
subscriptions.

This is the recommended path for new projects. Your frontend connects with
`useQuery` and `useMutation` — data updates in real time without REST
endpoints, GraphQL, or polling.

**[Full tutorial →](adapters/convex/)**

## Existing drivers and SDKs

Run `nimbus start` and connect with drivers you already know. No codegen, no
schema files, no special project layout.

```bash
nimbus start --port 8080
```

| Adapter | What it gives you | Time to first query |
|---------|-------------------|---------------------|
| [**MongoDB**](adapters/mongodb/) | Stock MongoDB drivers in any language | ~2 min |
| [**Firebase**](adapters/firebase/) | Firestore-compatible SDK with real-time listeners | ~3 min |
| [**Cloud Functions**](adapters/cloud-functions/) | Firebase v2 triggers and Functions Framework handlers | ~5 min |
| [**Native HTTP/WS**](adapters/native/) | Direct REST + WebSocket — just curl | ~1 min |

For MongoDB, Firebase client, and native HTTP/WS, `nimbus start` is enough and
Node.js is not required. Cloud Functions authoring still requires Node.js
because `nimbus codegen` runs through the Node toolchain.

**Not sure?** Start with [MongoDB](adapters/mongodb/). It uses drivers you
already have, works in every language, and requires nothing beyond
`nimbus start`.

**Just want to kick the tires?** The [README quick start](../README.md#quick-start)
has a curl walkthrough you can copy-paste.

## Next steps

- [Current capabilities](current-capabilities.md) -- what works today
- [Storage backends](operating/storage-backends.md) -- switch to Postgres, MySQL, or other backends
- [CLI reference](operating/cli.md) -- all server flags and commands
- [Demos](../demos/README.md) -- working example applications
