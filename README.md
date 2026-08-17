<div align="center">

# Nimbus

**The single-binary backend for apps and AI agents.**
**Drop-in compatible with Convex, Firestore, MongoDB, and DynamoDB.**

*BaaS in a binary.*

[![CI](https://github.com/nimbus/nimbus/actions/workflows/ci.yml/badge.svg)](https://github.com/nimbus/nimbus/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/nimbus/nimbus)](https://github.com/nimbus/nimbus/releases/latest)
[![Homebrew](https://img.shields.io/badge/homebrew-nimbus%2Ftap%2Fnimbus-orange)](https://github.com/nimbus/homebrew-tap)
[![License](https://img.shields.io/badge/license-Nimbus%20Community-blue)](LICENSE)

[Docs](https://nimbusdocs.com) · [Quickstart](https://nimbusdocs.com/get-started/quickstart/) · [Discussions](https://github.com/nimbus/nimbus/discussions)

</div>

---

Nimbus is a backend you can `scp` to a server and run: document storage,
server-side TypeScript functions, real-time subscriptions, durable scheduling,
and authentication in one Rust binary — no Docker, no Kubernetes, no external
database required. It speaks the wire protocols your code already uses, so
existing clients connect without an SDK swap. Built for teams replacing a BaaS
bill, regulated and air-gapped environments, and AI agent infrastructure —
whether the developer is human or LLM.

- **Five protocol front doors, one engine** — Convex functions, Firestore
  SDKs, Cloud Functions triggers, stock MongoDB drivers, and the AWS DynamoDB
  SDK all hit the same storage, mutations, and subscriptions.
- **Storage** — documents with optional schemas, indexed queries, and
  tenant isolation; SQLite by default, with Postgres, MySQL, libSQL, and redb
  backends.
- **Compute** — a V8 runtime for queries, mutations, actions, and HTTP
  routes, plus crons and durable `runAfter`/`runAt` scheduling that survive
  restarts.
- **Networking** — a transport-free control plane for attachments, endpoints,
  durable port and segment leases, provider capability evidence, and recovery.
  Concrete sockets and packet effects stay with their owning providers.
- **No strings** — source-available, no telemetry, no metered pricing.

**For documentation and examples, visit [nimbusdocs.com](https://nimbusdocs.com).**

> [!WARNING]
> **Beta.** APIs may break between releases. Not for production yet. [Feedback welcome.](https://github.com/nimbus/nimbus/discussions)

```
                                            ┌───────────────┐
                                            │ Apps & Agents │
                                            └───────┬───────┘
                                                    │
                                                    ▼
                   ┌─ Machine (local dev · cloud vm · bare metal) ─────────────────────┐
                   │                                │                                  │
                   │                                ▼                                  │
                   │   ┌─ nimbus (single Rust binary) ─────────────────────────────┐   │
                   │   │                            │                              │   │
                   │   │                            ▼                              │   │
                   │   │  ┌─ Adapters ──────────────────────────────────────────┐  │   │
                   │   │  │ Convex · Firestore · Cloud Fns · MongoDB · DynamoDB │  │   │
                   │   │  └───────┬─────────────────┬──────────────────┬────────┘  │   │
                   │   │          │                 │                  │           │   │
                   │   │          ▼                 ▼                  ▼           │   │
  (optional)       │   │  ┌─ Storage ────┐  ┌─ Compute ────┐  ┌─ Networking ────┐  │   │
┌─ DB Conn ───┐    │   │  │ • SQLite     │  │ • V8 Runtime │  │ • Endpoints     │  │   │
│ • Postgres  │◀─────────▶│ • libSQL     │  │ • Scheduling │  │ • Attachments   │  │   │
│ • MySQL     │    │   │  │ • redb       │  │ • Crons      │  │ • Leases        │  │   │
└─────────────┘    │   │  └──────────────┘  └───────┬──────┘  └─────────────────┘  │   │
                   │   └────────────────────────────┼──────────────────────────────┘   │
                   │                                │                                  │
                   │                                │                                  │
                   │                                ▼                                  │
                   │      ┌─ krun sandbox (compose.yml · programmatic) ────────┐       │
                   │      │                         │                          │       │
                   │      │         ┌───────────────┼─────────────────┐        │       │
                   │      │         ▼               ▼                 ▼        │       │
                   │      │ ┌─ Sandbox #1 ─┐ ┌─ Sandbox #2 ─┐ ┌─ Sandbox #3 ─┐ │       │
                   │      │ │    Agent     │ │   Service    │ │    Agent     │ │       │
                   │      │ │  OCI Image   │ │  OCI Image   │ │  OCI Image   │ │       │
                   │      │ └──────────────┘ └──────────────┘ └──────────────┘ │       │
                   │      └────────────────────────────────────────────────────┘       │
                   └───────────────────────────────────────────────────────────────────┘
```

## Two clients, one binary

Server-side TypeScript with reactive queries, and a stock MongoDB driver —
both against the same running `nimbus` process:

```typescript
// convex/messages.ts — Convex-compatible functions, served by Nimbus
import { query, mutation } from "./_generated/server";
import { v } from "convex/values";

export const send = mutation({
  args: { author: v.string(), body: v.string() },
  handler: async (ctx, { author, body }) =>
    await ctx.db.insert("messages", { author, body }),
});
```

```javascript
// Official MongoDB driver against the same engine's wire-protocol endpoint
import { MongoClient } from "mongodb";

const client = new MongoClient(
  "mongodb://app-user:app-secret@127.0.0.1:27017/myapp?directConnection=true",
);
await client.connect();
const docs = await client.db("myapp").collection("messages").find().toArray();
```

## Quick start

```bash
brew install nimbus/tap/nimbus   # see Install below for other platforms
nimbus init convex my-app
cd my-app
nimbus dev
```

`nimbus dev` generates types, creates a `demo` tenant, and serves on
`localhost:3210` — write a function, and your frontend gets reactive queries
with no REST endpoints and no polling. Convex and Cloud Functions authoring
need Node.js 22 with `npm`; MongoDB, Firestore clients, and the native HTTP
API need no Node at all. Full walkthrough:
[nimbusdocs.com/get-started/quickstart](https://nimbusdocs.com/get-started/quickstart/).

## Protocol compatibility

Every adapter shares the same engine — same storage, same mutation path, same
real-time subscriptions. Current status:

| Protocol | Status | Docs |
|----------|--------|------|
| **Convex** (functions, reactive queries, React hooks) | Available | [guide](https://nimbusdocs.com/developers/convex/) · [compatibility](https://nimbusdocs.com/reference/convex/compatibility/) |
| **Firestore** (SDKs, real-time listeners) | Available — on by default; opt out with `--no-firestore` | [guide](https://nimbusdocs.com/developers/firebase/) · [compatibility](https://nimbusdocs.com/reference/firebase/compatibility/) |
| **Cloud Functions** (v2 triggers, Functions Framework) | Available | [guide](https://nimbusdocs.com/developers/cloud-functions/) · [compatibility](https://nimbusdocs.com/reference/cloud-functions/compatibility/) |
| **MongoDB** (stock drivers, wire protocol) | Available — on by default on `127.0.0.1:27017`; opt out with `--no-mongodb` | [guide](https://nimbusdocs.com/developers/mongodb/) · [operations](https://nimbusdocs.com/reference/mongodb/operations/) |
| **DynamoDB** (AWS SDKs, dedicated listener) | Available — on by default on `127.0.0.1:8000`; opt out with `--no-dynamodb` | [guide](https://nimbusdocs.com/developers/dynamodb/) · [coverage](https://nimbusdocs.com/reference/dynamodb/feature-coverage/) |
| **Native HTTP/WebSocket** (just curl) | Available — always on | [guide](https://nimbusdocs.com/developers/native/) · [API](https://nimbusdocs.com/reference/native/http-api/) |

See [current capabilities](https://nimbusdocs.com/reference/current-capabilities/)
for the full status matrix, including what is not built yet.

## Built for agents

- The full docs corpus ships as machine-readable artifacts:
  [llms.txt](https://nimbusdocs.com/llms.txt) ·
  [llms-full.txt](https://nimbusdocs.com/llms-full.txt) ·
  [llms-small.txt](https://nimbusdocs.com/llms-small.txt).
- [AGENTS.md](AGENTS.md) gives coding agents repo-specific routing and
  guardrails; adapter usage rules live at
  [Convex usage rules](https://nimbusdocs.com/reference/convex/usage-rules/).
- An MCP server is on the roadmap.

## Install

**Homebrew (macOS and Linux):**

```bash
brew install nimbus/tap/nimbus
```

**Binary download:** grab the archive for your platform from
[GitHub Releases](https://github.com/nimbus/nimbus/releases/latest).

**Container image:**

```bash
docker pull ghcr.io/nimbus/nimbus:<version>
```

**Build from source:**

```bash
git clone https://github.com/nimbus/nimbus.git
cd nimbus
cargo install --path crates/nimbus-bin
```

**Desktop console (optional):** a signed Electron shell for the operator UI —
`brew install --cask nimbus/tap/nimbus-desktop`, or see
[nimbus/desktop](https://github.com/nimbus/desktop#install).

Platform details — the Linux hardware-isolation stack, container digest
pinning, admin-token rotation, and update behavior — live in the
[self-host guide](https://nimbusdocs.com/get-started/self-host/) and the
[Operators docs](https://nimbusdocs.com/operators/).

## Community

- **[Issues](https://github.com/nimbus/nimbus/issues)** — bugs and concrete problems
- **[Discussions](https://github.com/nimbus/nimbus/discussions)** — feature requests and longer-form conversation
- **[Contributing](CONTRIBUTING.md)** — workflow, CLA, and coding standards; [ARCHITECTURE.md](ARCHITECTURE.md) maps the codebase

## Security

If you've found a security vulnerability, report it through [GitHub Security Advisories](https://github.com/nimbus/nimbus/security/advisories/new). See [SECURITY.md](SECURITY.md) for the full policy.

## Licensing

Nimbus is **source-available** under the [Nimbus Community License](LICENSE). Free for individuals, nonprofits, education, and organizations under a [$10M revenue + 500 MAU dual gate](LICENSING.md). No telemetry, no metered pricing. See [LICENSING.md](LICENSING.md) for the full plain-English summary.

---

<div align="center">

Built by [nimbus](https://github.com/nimbus) and the Nimbus contributors.

</div>
