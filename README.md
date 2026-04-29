<div align="center">

# Neovex

**BaaS in a binary. For apps and agents.**

Storage, compute, and networking -- with real-time and scheduling -- in a single Rust binary.

[![CI](https://github.com/agentstation/neovex/actions/workflows/ci.yml/badge.svg)](https://github.com/agentstation/neovex/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/agentstation/neovex/graph/badge.svg)](https://codecov.io/gh/agentstation/neovex)
[![Release](https://img.shields.io/github/v/release/agentstation/neovex)](https://github.com/agentstation/neovex/releases/latest)
[![Homebrew](https://img.shields.io/badge/homebrew-agentstation%2Ftap%2Fneovex-orange)](https://github.com/agentstation/homebrew-tap)
[![Status](https://img.shields.io/badge/status-beta-yellow)]()
[![License](https://img.shields.io/badge/license-Neovex%20Community-blue)](LICENSE)

[Quick start](#quick-start) · [Why Neovex](#why-neovex) · [Adapters](#adapters) · [Install](#install) · [Docs](docs/README.md) · [Architecture](ARCHITECTURE.md)

</div>

---

> [!WARNING]
> **Beta.** APIs may break between releases. Not for production yet. [Feedback welcome.](https://github.com/agentstation/neovex/discussions)

```
                                            ┌───────────────┐
                                            │ Apps & Agents │
                                            └───────┬───────┘
                                                    │
                                                    ▼
                   ┌─ Machine (local dev · cloud vm · bare metal) ─────────────────────┐
                   │                                │                                  │
                   │                                ▼                                  │
                   │   ┌─ neovex (single Rust binary) ─────────────────────────────┐   │
                   │   │                            │                              │   │
                   │   │                            ▼                              │   │
                   │   │  ┌─ Adapters ──────────────────────────────────────────┐  │   │
                   │   │  │ Convex  ·  Firebase  ·  Cloud Functions  ·  MongoDB │  │   │
                   │   │  └───────┬─────────────────┬──────────────────┬────────┘  │   │
                   │   │          │                 │                  │           │   │
                   │   │          ▼                 ▼                  ▼           │   │
  (optional)       │   │  ┌─ Storage ────┐  ┌─ Compute ────┐  ┌─ Networking ────┐  │   │
┌─ DB Conn ───┐    │   │  │ • SQLite     │  │ • V8 Runtime │  │ • HTTP / WS     │  │   │
│ • Postgres  │◀─────────▶│ • libSQL     │  │ • Scheduling │  │ • Realtime Sync │  │   │
│ • MySQL     │    │   │  │ • redb       │  │ • Crons      │  │ • Auth          │  │   │
└─────────────┘    │   │  └──────────────┘  └───────┬──────┘  └─────────────────┘  │   │
                   │   └────────────────────────────┼──────────────────────────────┘   │
                   │                                │                                  │
                   │                                │                                  │
                   │                                ▼                                  │
                   │      ┌─ krun sandbox (compose.yml · programmatic) ────────┐       │
                   │      │                         │                          │       │
                   │      │         ┌───────────────┼─────────────────┐        │       │
                   │      │         ▼               ▼                 ▼        │       │
                   │      │ ┌─ MicroVM #1 ─┐ ┌─ MicroVM #2 ─┐ ┌─ MicroVM #3 ─┐ │       │ 
                   │      │ │    Agent     │ │   Service    │ │    Agent     │ │       │
                   │      │ │  OCI Image   │ │  OCI Image   │ │  OCI Image   │ │       │
                   │      │ └──────────────┘ └──────────────┘ └──────────────┘ │       │
                   │      └────────────────────────────────────────────────────┘       │
                   └───────────────────────────────────────────────────────────────────┘
```

## Quick start

If you're authoring Convex or Cloud Functions code locally, install Node.js 22
with `npm` first. `neovex dev` still runs codegen through external `node` by
default and can auto-run `npm install` when declared packages are missing
locally or when the recorded package/lockfile fingerprint has changed. The
external authoring path now verifies `node --version` against the `22.x`
baseline before it runs codegen. The embedded codegen path is still an
experimental pilot, and Neovex does not yet
claim a separate Node 20 compatibility target even though current upstream
Convex and Firebase tooling still support it. Firebase / Cloud Functions
authoring still uses the external Node.js runner; the embedded pilot does not
yet support that package layout.

If you're using `neovex start` with MongoDB, the Firebase client adapter, or
the native HTTP/WebSocket API, Node.js is not required.

**1. Install Neovex:**

```bash
brew install agentstation/tap/neovex
```

See [Install](#install) for other platforms or building from source.

**2. Scaffold a Convex app:**

```bash
neovex init convex my-app
cd my-app
```

`neovex init convex` scaffolds backend files only: `convex/schema.ts`,
`convex/messages.ts`, `package.json`, `tsconfig.json`, and `.gitignore`.

**3. Start the dev server:**

```bash
neovex dev
```

> [!TIP]
> `neovex dev` auto-runs `npm install` when declared packages are missing
> locally or when the recorded package/lockfile fingerprint has changed,
> creates a `demo` tenant, and starts the server on `localhost:3210`.

### Server-side functions

```typescript
// convex/messages.ts
import { query, mutation } from "./_generated/server";
import { v } from "convex/values";

export const list = query({
  args: {},
  handler: async (ctx) => await ctx.db.query("messages").take(50),
});

export const send = mutation({
  args: { author: v.string(), body: v.string() },
  handler: async (ctx, { author, body }) =>
    await ctx.db.insert("messages", { author, body }),
});
```

```tsx
// In your React app — data updates in real time
const messages = useQuery(api.messages.list);
```

Write TypeScript functions, run `neovex dev`, and your frontend gets reactive
queries and mutations — no REST endpoints, no GraphQL, no polling. Everything
runs locally in a single process. See the [full tutorial](docs/adapters/convex/).

### Or use it with curl

Start the server:

```bash
neovex start --port 8080 --data-dir ./data
```

Create a tenant:

```bash
curl -s -X POST http://localhost:8080/api/tenants \
  -H "Content-Type: application/json" \
  -d '{"id": "demo"}'
```

Insert a document:

```bash
curl -s -X POST http://localhost:8080/api/tenants/demo/documents \
  -H "Content-Type: application/json" \
  -d '{"table": "messages", "fields": {"text": "hello world", "author": "you"}}'
```

Query it back:

```bash
curl -s -X POST http://localhost:8080/api/tenants/demo/query \
  -H "Content-Type: application/json" \
  -d '{"table": "messages", "filters": []}'
```

`neovex start` runs the same engine without codegen — connect with
[stock MongoDB drivers](docs/adapters/mongodb/),
[Firebase SDKs](docs/adapters/firebase/), or any HTTP client.
See the [getting started guide](docs/getting-started.md) to pick your adapter.

## Why Neovex

Most self-hosted backends are dev tools wearing a production costume. They run
on a single machine, can't migrate without wiping the database, and ship with a
"we strongly recommend the cloud version" warning. Neovex is designed from day
one to be the thing you actually deploy — on your own hardware, air-gapped if
needed, with no telemetry and no metered pricing. Built for regulated
industries, air-gapped environments, teams replacing expensive BaaS bills, and
AI agent infrastructure.

## Adapters

Build with server-side TypeScript functions, or connect existing drivers and
SDKs. Every adapter shares the same engine — same storage, same mutations, same
real-time subscriptions.

| Adapter | What you get | Guide |
|---------|-------------|-------|
| **Convex** | Server-side TypeScript functions, reactive queries, React hooks | [docs/adapters/convex/](docs/adapters/convex/) |
| **MongoDB** | Stock MongoDB drivers in any language — no codegen, no schema | [docs/adapters/mongodb/](docs/adapters/mongodb/) |
| **Firebase / Firestore** | Firestore-compatible SDK, real-time listeners | [docs/adapters/firebase/](docs/adapters/firebase/) |
| **Cloud Functions** | Firebase v2 triggers and Functions Framework handlers | [docs/adapters/cloud-functions/](docs/adapters/cloud-functions/) |
| **Native HTTP/WS** | Direct REST + WebSocket API — just curl | [docs/adapters/native/](docs/adapters/native/) |

> [!TIP]
> Running on one of these today and the bill, the lock-in, or the compliance gap has you looking for the door? [Open an issue](https://github.com/agentstation/neovex/issues) -- we want to hear about your migration scenario.

## What's in the box

**Storage** — Document storage with optional schemas, indexed queries, cursor-based pagination. Pluggable backends: SQLite (default), Postgres, MySQL, libSQL, redb. Tenant isolation built into the storage layer. See the [storage backends guide](docs/operating/storage-backends.md).

**Compute** — V8 JavaScript runtime for server-side queries, mutations, actions, and HTTP routes. Durable scheduling with `runAfter`, `runAt`, and cron jobs that survive restarts.

**Networking** — Reactive WebSocket subscriptions, five compatibility adapters, JWT/JWKS authentication with any standards-compliant identity provider.

**Delivery** — A single Rust binary you can `scp` to a server and run. No Docker required, no Kubernetes required, no external database required.

## Install

### Homebrew (macOS and Linux)

```bash
brew install agentstation/tap/neovex
```

For Convex or Cloud Functions authoring, also install Node.js 22 with `npm`.

### Download binary

Download the latest release from [GitHub Releases](https://github.com/agentstation/neovex/releases/latest).

| Platform | Architecture | Archive |
|----------|-------------|---------|
| Linux | x86_64 | `neovex_linux_x86_64.tar.gz` |
| Linux | ARM64 | `neovex_linux_arm64.tar.gz` |
| macOS | Apple Silicon | `neovex_darwin_arm64.tar.gz` |
| Windows | x86_64 | `neovex_windows_x86_64.zip` |

### Build from source

```bash
git clone https://github.com/agentstation/neovex.git
cd neovex
cargo install --path crates/neovex-bin
```

This installs the Rust binary only. For Convex or Cloud Functions authoring,
also install Node.js 22 with `npm`. Runtime-only `neovex start` workflows do
not need the Node toolchain.

## Community

- **[Issues](https://github.com/agentstation/neovex/issues)** — bugs and concrete problems
- **[Discussions](https://github.com/agentstation/neovex/discussions)** — feature requests and longer-form conversation
- **[Contributing](CONTRIBUTING.md)** — workflow, CLA, and coding standards

## Security

If you've found a security vulnerability, report it through [GitHub Security Advisories](https://github.com/agentstation/neovex/security/advisories/new). See [SECURITY.md](SECURITY.md) for the full policy.

## Licensing

Neovex is **source-available** under the [Neovex Community License](LICENSE). Free for individuals, nonprofits, education, and organizations under a [$10M revenue + 500 MAU dual gate](LICENSING.md). No telemetry, no metered pricing. See [LICENSING.md](LICENSING.md) for the full plain-English summary.

---

<div align="center">

Built by [agentstation](https://github.com/agentstation) and the Neovex contributors.

</div>
