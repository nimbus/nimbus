# Getting Started

Install Neovex, then pick how you want to build.

## Install

```bash
brew install agentstation/tap/neovex
```

See [Install](../README.md#install) for other platforms or building from source.

## Server-side functions

```bash
mkdir my-app && cd my-app
neovex dev
```

`neovex dev` scaffolds a starter project with a schema and example functions
when no `convex/` directory exists. It creates a `demo` tenant automatically
and starts the server on `localhost:3210`. Run `npm install` when prompted,
then `neovex dev` again — your functions are live.

From there, edit the TypeScript files in `convex/` and `neovex dev` watches
for changes, re-runs codegen, and activates updated functions with reactive
subscriptions.

This is the recommended path for new projects. Your frontend connects with
`useQuery` and `useMutation` — data updates in real time without REST
endpoints, GraphQL, or polling.

**[Full tutorial →](adapters/convex/)**

## Existing drivers and SDKs

Run `neovex start` and connect with drivers you already know. No codegen, no
schema files, no special project layout.

```bash
neovex start --port 8080
```

| Adapter | What it gives you | Time to first query |
|---------|-------------------|---------------------|
| [**MongoDB**](adapters/mongodb/) | Stock MongoDB drivers in any language | ~2 min |
| [**Firebase**](adapters/firebase/) | Firestore-compatible SDK with real-time listeners | ~3 min |
| [**Cloud Functions**](adapters/cloud-functions/) | Firebase v2 triggers and Functions Framework handlers | ~5 min |
| [**Native HTTP/WS**](adapters/native/) | Direct REST + WebSocket — just curl | ~1 min |

**Not sure?** Start with [MongoDB](adapters/mongodb/). It uses drivers you
already have, works in every language, and requires nothing beyond
`neovex start`.

**Just want to kick the tires?** The [README quick start](../README.md#quick-start)
has a curl walkthrough you can copy-paste.

## Next steps

- [Current capabilities](current-capabilities.md) -- what works today
- [Storage backends](operating/storage-backends.md) -- switch to Postgres, MySQL, or other backends
- [CLI reference](operating/cli.md) -- all server flags and commands
- [Demos](../demos/README.md) -- working example applications
