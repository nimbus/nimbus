<div align="center">

# Nimbus

**BaaS in a binary. For apps and agents.**

Storage, compute, and networking -- with real-time and scheduling -- in a single Rust binary.

[![CI](https://github.com/nimbus/nimbus/actions/workflows/ci.yml/badge.svg)](https://github.com/nimbus/nimbus/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/nimbus/nimbus/graph/badge.svg)](https://codecov.io/gh/nimbus/nimbus)
[![Release](https://img.shields.io/github/v/release/nimbus/nimbus)](https://github.com/nimbus/nimbus/releases/latest)
[![Homebrew](https://img.shields.io/badge/homebrew-nimbus%2Ftap%2Fnimbus-orange)](https://github.com/nimbus/homebrew-tap)
[![Status](https://img.shields.io/badge/status-beta-yellow)]()
[![License](https://img.shields.io/badge/license-Nimbus%20Community-blue)](LICENSE)

[Quick start](#quick-start) · [Why Nimbus](#why-nimbus) · [Adapters](#adapters) · [Install](#install) · [Docs](docs/README.md) · [Architecture](ARCHITECTURE.md)

</div>

---

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
                   │      │ ┌─ Sandbox #1 ─┐ ┌─ Sandbox #2 ─┐ ┌─ Sandbox #3 ─┐ │       │
                   │      │ │    Agent     │ │   Service    │ │    Agent     │ │       │
                   │      │ │  OCI Image   │ │  OCI Image   │ │  OCI Image   │ │       │
                   │      │ └──────────────┘ └──────────────┘ └──────────────┘ │       │
                   │      └────────────────────────────────────────────────────┘       │
                   └───────────────────────────────────────────────────────────────────┘
```

Nimbus uses four resource nouns consistently. A **service** is a named app or
agent dependency such as `db`, `search`, `browser`, or `api-lb`; it may be
sandbox-backed, built in, or external. A **sandbox** is an isolated execution
resource addressed by id/handle, not by name. A future **session** is a scoped
interaction with either a named service or a sandbox id. A runtime **isolate**
executes app code, but is not an SDK sandbox resource. If Nimbus later exposes
isolate execution as a user-created sandbox, the reserved profile spelling is
`profile: "isolate"`.

## Quick start

If you're authoring Convex or Cloud Functions code locally, install Node.js 22
with `npm` first. `nimbus dev` still runs codegen through external `node` by
default and can auto-run `npm install` when declared packages are missing
locally or when the recorded package/lockfile fingerprint has changed. The
external authoring path verifies `node --version` against the `22.x` baseline
before it runs codegen. Convex-compatible runtime execution can still target
Node20, Node22, or Node24 through `convex.json` for `"use node"` actions.
Firebase / Cloud Functions authoring still uses the external Node.js runner;
the embedded pilot does not yet support that package layout.

If you're using `nimbus start` with MongoDB, the Firebase client adapter, or
the native HTTP/WebSocket API, Node.js is not required.

## Node compatibility contract

Nimbus's default Node-facing compatibility target is `Node22`.

- `Node22` is the default built-in module contract we verify and evolve.
- `Node20` and `Node24` are supported Convex Node action targets selected by
  `convex.json`; Node22 remains the default until a deliberate Node24-default
  migration.
- Nimbus does **not** currently claim full Node built-in compatibility for any
  runtime profile.

Convex-compatible projects may configure Node actions like this:

```json
{
  "node": {
    "nodeVersion": "22",
    "externalPackages": ["sharp"]
  }
}
```

Only action modules may opt into Node APIs. Put `"use node";` at the top of an
action-only file, and import builtins as either `fs` or `node:fs`. If codegen
reports a Node builtin in a default-runtime file, run
`nimbus dev --once --debug-node-apis` or
`nimbus codegen --app . --debug-node-apis` for file-level diagnostics.
Node action npm package imports must currently be externalized with
`node.externalPackages` or `["*"]`; codegen validates the local `node_modules`
install, stages package roots under `.nimbus/convex/node_modules/`, and emits a
package evidence report. Full Convex cloud-style dependency installation is not
claimed yet.

Public support states follow the generated compatibility baseline:

- `Supported`
- `SupportedToolingOnly`
- `Partial`
- `StubOnly`
- `NotSupported`
- `NeedsVerification`

Use these documents together:

- [Generated Node LTS baseline](docs/private/staging/architecture/runtime/node-lts-compat/node-lts-compat-summary.md)
- [Detailed runtime surface matrix](docs/private/staging/architecture/runtime/node-compat-surface-matrix.md)

Current high-level posture:

- `Application + WebStandardIsolate` is the non-Node target.
- `Application + Node22` is a partial Node22 compatibility target with
  documented exclusions and `NeedsVerification` areas.
- `Tooling + Node22` is also partial today; some host-sensitive surfaces may
  eventually become `SupportedToolingOnly`, but they do not justify a blanket
  "full Node compatibility" claim.

**1. Install Nimbus:**

```bash
brew install nimbus/tap/nimbus
```

See [Install](#install) for other platforms or building from source.

**2. Scaffold a Convex app:**

```bash
nimbus init convex my-app
cd my-app
```

`nimbus init convex` scaffolds backend files only: `convex/schema.ts`,
`convex/messages.ts`, `package.json`, `tsconfig.json`, and `.gitignore`.

**3. Start the dev server:**

```bash
nimbus dev
```

> [!TIP]
> `nimbus dev` auto-runs `npm install` when declared packages are missing
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

Write TypeScript functions, run `nimbus dev`, and your frontend gets reactive
queries and mutations — no REST endpoints, no GraphQL, no polling. Everything
runs locally in a single process. See the [full tutorial](docs/private/staging/adapters/convex/).

### Or use it with curl

Start the server:

```bash
nimbus start --port 8080 --data-dir ./data
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

`nimbus start` runs the same engine without codegen — connect with
[stock MongoDB drivers](docs/private/staging/adapters/mongodb/),
[Firebase SDKs](docs/private/staging/adapters/firebase/), or any HTTP client.
See the [getting started guide](docs/get-started/) to pick your adapter.

## Why Nimbus

Most self-hosted backends are dev tools wearing a production costume. They run
on a single machine, can't migrate without wiping the database, and ship with a
"we strongly recommend the cloud version" warning. Nimbus is designed from day
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
| **Convex** | Server-side TypeScript functions, reactive queries, React hooks | [docs/private/staging/adapters/convex/](docs/private/staging/adapters/convex/) |
| **MongoDB** | Stock MongoDB drivers in any language — no codegen, no schema | [docs/private/staging/adapters/mongodb/](docs/private/staging/adapters/mongodb/) |
| **Firebase / Firestore** | Firestore-compatible SDK, real-time listeners | [docs/private/staging/adapters/firebase/](docs/private/staging/adapters/firebase/) |
| **Cloud Functions** | Firebase v2 triggers and Functions Framework handlers | [docs/private/staging/adapters/cloud-functions/](docs/private/staging/adapters/cloud-functions/) |
| **Native HTTP/WS** | Direct REST + WebSocket API — just curl | [docs/private/staging/adapters/native/](docs/private/staging/adapters/native/) |

> [!TIP]
> Running on one of these today and the bill, the lock-in, or the compliance gap has you looking for the door? [Open an issue](https://github.com/nimbus/nimbus/issues) -- we want to hear about your migration scenario.

## What's in the box

**Storage** — Document storage with optional schemas, indexed queries, cursor-based pagination. Pluggable backends: SQLite (default), Postgres, MySQL, libSQL, redb. Tenant isolation built into the storage layer. See the [storage backends guide](docs/private/staging/operating/storage-backends.md).

**Compute** — V8 JavaScript runtime for server-side queries, mutations, actions, and HTTP routes, with Node compatibility lanes and an optional fail-closed Bun/JSC lane behind a verified adapter artifact. Durable scheduling with `runAfter`, `runAt`, and cron jobs that survive restarts.

**Networking** — Reactive WebSocket subscriptions, five compatibility adapters, JWT/JWKS authentication with any standards-compliant identity provider.

**Delivery** — A single Rust binary you can `scp` to a server and run. Release
builds also publish a normal foreground `ghcr.io/nimbus/nimbus:<version>` OCI
image for orchestrators. No Docker, Kubernetes, or external database is
required for the binary path.

## Install

### Homebrew (macOS and Linux)

```bash
brew install nimbus/tap/nimbus
```

For Convex or Cloud Functions authoring, also install Node.js 22 with `npm`.
Convex Node action execution can be configured separately in `convex.json`.

### Download binary

Download the latest release from [GitHub Releases](https://github.com/nimbus/nimbus/releases/latest).

| Platform | Architecture | Archive |
|----------|-------------|---------|
| Linux | x86_64 | `nimbus_linux_x86_64.tar.gz` |
| Linux | ARM64 | `nimbus_linux_arm64.tar.gz` |
| macOS | Apple Silicon | `nimbus_darwin_arm64.tar.gz` |
| Windows | x86_64 | `nimbus_windows_x86_64.zip` |

Linux hardware-isolated service execution also needs the paired private
runtime stack: `nimbus-libkrun` from
[`nimbus/nimbus-libkrun`](https://github.com/nimbus/nimbus-libkrun) and
`nimbus-crun` from
[`nimbus/nimbus-crun`](https://github.com/nimbus/nimbus-crun). The install
script installs those release artifacts under `/usr/libexec/nimbus` and does
not depend on distro `libkrun` for Nimbus service execution.

The in-process Bun/JSC runtime is optional and separate from the default
Deno/V8/Node lanes. Linux direct installs can opt in with `install.sh
--with-bun-jsc`, which installs the verified `nimbus-bun-jsc-adapter` artifact
beside the main binary. Without that verified adapter, `/debug/runtime/metrics`
reports the Bun/JSC lane as `not_linked` and Bun-selected functions fail
closed.

### Container image

Tagged releases also publish `ghcr.io/nimbus/nimbus:<version>` with
linux/amd64 and linux/arm64 manifests. The image runs `nimbus` directly in the
foreground and records digest, license, attestation, SBOM, vulnerability-scan,
and smoke evidence in the GitHub Release.

```bash
docker pull ghcr.io/nimbus/nimbus:vX.Y.Z
```

See [`docs/private/staging/operating/container-image.md`](docs/private/staging/operating/container-image.md)
for digest pinning, the one-time admin-token rotation required before public
binds, and Compose/Podman/Kubernetes examples.

### Build from source

```bash
git clone https://github.com/nimbus/nimbus.git
cd nimbus
cargo install --path crates/nimbus-bin
```

This installs the Rust binary only. For Convex or Cloud Functions authoring,
also install Node.js 22 with `npm`. Runtime-only `nimbus start` workflows do
not need the Node toolchain after artifacts have been generated.

### Desktop console (optional)

Nimbus ships a native desktop shell — a signed, notarized Electron app that
wraps the operator console UI served at `/ui/`. The shell is independent of
the CLI release cadence; you can run the CLI headless and connect to `/ui/`
in any browser, or use the desktop for an OS-integrated window with tray and
auto-updates.

```bash
# macOS (Homebrew Cask)
brew install --cask nimbus/tap/nimbus-desktop
```

Linux installers (`.deb`, `.rpm`, `.AppImage`) and direct downloads live on
the [nimbus-desktop releases page](https://github.com/nimbus/desktop/releases).
Full install + troubleshooting reference:
[`nimbus/desktop`](https://github.com/nimbus/desktop#install).

The shell does not bundle `nimbus`; install the CLI above first. On launch
it discovers a running instance via `server.json` or spawns one on demand.

For how the CLI and desktop shell update themselves, what the staleness
indicators in the UI mean, and how to disable update checks on air-gapped
hosts, see [`docs/private/staging/operating/updates.md`](docs/private/staging/operating/updates.md).

## Community

- **[Issues](https://github.com/nimbus/nimbus/issues)** — bugs and concrete problems
- **[Discussions](https://github.com/nimbus/nimbus/discussions)** — feature requests and longer-form conversation
- **[Contributing](CONTRIBUTING.md)** — workflow, CLA, and coding standards

## Security

If you've found a security vulnerability, report it through [GitHub Security Advisories](https://github.com/nimbus/nimbus/security/advisories/new). See [SECURITY.md](SECURITY.md) for the full policy.

## Licensing

Nimbus is **source-available** under the [Nimbus Community License](LICENSE). Free for individuals, nonprofits, education, and organizations under a [$10M revenue + 500 MAU dual gate](LICENSING.md). No telemetry, no metered pricing. See [LICENSING.md](LICENSING.md) for the full plain-English summary.

---

<div align="center">

Built by [nimbus](https://github.com/nimbus) and the Nimbus contributors.

</div>
