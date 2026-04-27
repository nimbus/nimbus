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

[Quick start](#quick-start) · [Adapters](#adapters) · [Install](#install) · [Docs](#documentation) · [Architecture](ARCHITECTURE.md)

</div>

---

> [!WARNING]
> Neovex is in **beta**. APIs, storage formats, and configuration flags are changing quickly and will break between releases. Do not run production workloads against it yet. If you're evaluating Neovex or building on it early, [open an issue](https://github.com/agentstation/neovex/issues) or [start a discussion](https://github.com/agentstation/neovex/discussions) -- we're working with a small group of design partners and want to hear what matters to you.

Neovex packages the three backend primitives -- storage, compute, and networking -- into a single binary you run on your own infrastructure. It gives you the developer experience of a managed BaaS -- document storage, server-side JavaScript, real-time subscriptions, durable scheduling -- without the cloud lock-in

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

It's built for teams and agents that need to run their full application stack on their own hardware: regulated industries, air-gapped environments, customers with data residency requirements, AI agent infrastructure, and any team whose SaaS bill has stopped making sense.

## Why Neovex

Most self-hosted backends are dev tools wearing a production costume. They run on a single machine, can't migrate without wiping the database, and ship with a "we strongly recommend the cloud version for real workloads" warning in the README. They exist to address lock-in fear, not to actually run your application.

Neovex is designed from day one to be the thing you actually deploy.

| Capability             | Convex OSS           | Firebase           | Vercel                | **Neovex**                  |
| ---------------------- | -------------------- | ------------------ | --------------------- | --------------------------- |
| Self-hostable          | Single-machine only  | No                 | No                    | **Yes**                     |
| Tenant isolation       | No                   | Managed only       | N/A                   | **Built into storage layer** |
| Storage backends       | Embedded only        | Managed only       | N/A                   | **SQLite, Postgres, MySQL, libSQL** |
| Compatibility adapters | N/A                  | N/A                | N/A                   | **Convex, Firebase, Cloud Functions, MongoDB** |
| Air-gapped deploy      | Not supported        | Not supported      | Not supported         | **Yes**                     |
| Telemetry phone-home   | On by default        | Always             | Always                | **None**                    |
| Pricing model          | Cloud-only paid tier | Per-read/per-write | Per-request, per-GB   | **License, not metered**    |
| Source available       | Yes (FSL)            | No                 | No                    | **Yes (Community License)** |

## Quick start

```bash
# macOS / Linux via Homebrew
brew install agentstation/tap/neovex
```

```bash
# Start the server
neovex start --port 8080 --data-dir ./data
```

That's it. Storage, compute, and networking running on `http://localhost:8080` -- document storage, a V8 runtime, HTTP and WebSocket APIs, and durable scheduling -- in a single process, writing to a single directory.

```bash
# Create a tenant
curl -s -X POST http://localhost:8080/api/tenants \
  -H "Content-Type: application/json" \
  -d '{"id": "demo"}'

# Insert a document
curl -s -X POST http://localhost:8080/api/tenants/demo/documents \
  -H "Content-Type: application/json" \
  -d '{"table": "messages", "fields": {"text": "hello world", "author": "you"}}'

# Query it back
curl -s -X POST http://localhost:8080/api/tenants/demo/query \
  -H "Content-Type: application/json" \
  -d '{"table": "messages", "filters": []}'
```

Live query subscriptions are available over WebSocket at `/ws`. See the [HTTP & WebSocket API reference](docs/reference/http-api.md) for the full route catalog.

## Adapters

Neovex speaks the protocols of platforms developers already use, so migration is a configuration change, not a rewrite.

| Adapter | Client Package | Protocol | Guide |
|---------|---------------|----------|-------|
| **Convex** | `convex` | Convex WebSocket + HTTP | [docs/adapters/convex.md](docs/adapters/convex.md) |
| **Firebase / Firestore** | `@neovex/firebase` | REST, gRPC-Web, WebSocket Listen | [docs/adapters/firebase.md](docs/adapters/firebase.md) |
| **Cloud Functions** | *(server-side)* | Firebase v2 + Functions Framework | [docs/adapters/cloud-functions.md](docs/adapters/cloud-functions.md) |
| **MongoDB** | `@neovex/mongodb` | MongoDB Wire Protocol | [docs/adapters/mongodb.md](docs/adapters/mongodb.md) |
| **Native HTTP/WS** | `neovex` | REST + WebSocket | [docs/adapters/native.md](docs/adapters/native.md) |

All adapters share the same engine -- every mutation flows through the same write path, the same storage transactions, and the same subscription fan-out. Choosing an adapter is a client-side decision, not a server-side fork.

> [!TIP]
> Running on one of these today and the bill, the lock-in, or the compliance gap has you looking for the door? [Open an issue](https://github.com/agentstation/neovex/issues) -- we want to hear about your migration scenario.

## What's in the box

**Storage** -- the persistence layer
- **Document storage** with optional per-table schemas, indexed queries, and cursor-based pagination
- **Pluggable backends** -- SQLite by default, with Postgres, MySQL, libSQL, and redb. See the [storage backends guide](docs/guides/storage-backends.md).
- **Tenant isolation** built into the storage layer, not bolted on

**Compute** -- the execution layer
- **JavaScript runtime** powered by V8 for server-side queries, mutations, actions, and HTTP routes
- **Durable scheduling** with `runAfter`, `runAt`, and recurring cron jobs that survive restarts

**Networking** -- the transport layer
- **Reactive subscriptions** over WebSocket -- clients see live updates without polling
- **Five compatibility adapters** so migration is a config change, not a rewrite
- **JWT / JWKS authentication** with support for any standards-compliant identity provider

**Delivery** -- a single Rust binary you can `scp` to a server and run. No Docker required, no Kubernetes required, no external database required.

## Who Neovex is for

- **Platform teams at regulated companies** who can't put production data in someone else's cloud -- healthcare, finance, defense, government, EU companies with data residency requirements.
- **Engineering teams whose SaaS bill stopped making sense** -- Firebase, Vercel, or Convex bills that grew faster than the business and now look like the most expensive line item in infrastructure.
- **Developers building for air-gapped environments** -- defense, industrial, or any deployment where "make an outbound HTTPS call to a vendor's API" is not an option.
- **Teams who want one binary instead of fifteen services** -- replace a Postgres + Redis + cron + WebSocket gateway + serverless function host stack with a single process that does all of it.
- **AI agent builders** who need persistent state, real-time coordination, and scheduled execution for autonomous agents -- without standing up five separate services or paying per-token for a managed backend.

## Install

### Homebrew (macOS and Linux)

```bash
brew install agentstation/tap/neovex
```

Homebrew automatically verifies the SHA256 checksum of the downloaded archive.
On Apple Silicon macOS, the cask owns the machine helper contract:
`slp/krunkit/krunkit` is the explicit VM dependency and `gvproxy` ships inside
the Neovex archive under `libexec/gvproxy`.

### Download binary

Download the latest release for your platform from [GitHub Releases](https://github.com/agentstation/neovex/releases/latest).

| Platform | Architecture | Archive |
|----------|-------------|---------|
| Linux | x86_64 | `neovex_linux_x86_64.tar.gz` |
| Linux | ARM64 | `neovex_linux_arm64.tar.gz` |
| macOS | Apple Silicon | `neovex_darwin_arm64.tar.gz` |
| Windows | x86_64 | `neovex_windows_x86_64.zip` |

On macOS, the darwin archive contains the bundled `libexec/gvproxy` helper. Preserve the bundled layout when installing from the tarball directly:

```bash
curl -LO https://github.com/agentstation/neovex/releases/latest/download/neovex_darwin_arm64.tar.gz
tar xzf neovex_darwin_arm64.tar.gz
sudo mkdir -p /opt/neovex/bin /opt/neovex/libexec
sudo install -m 0755 neovex /opt/neovex/bin/neovex
sudo install -m 0755 libexec/gvproxy /opt/neovex/libexec/gvproxy
sudo ln -sf /opt/neovex/bin/neovex /usr/local/bin/neovex
```

Do not move only the `neovex` binary on macOS. The machine contract expects the bundled helper layout beside the binary, or provided via `NEOVEX_MACHINE_HELPER_BINARY_DIR`.

To build from source, see [build from source](#build-from-source).

## Documentation

| Document | What's in it |
|----------|-------------|
| [Documentation index](docs/README.md) | Start here for deeper technical docs |
| [Architecture](ARCHITECTURE.md) | How Neovex is built and why |
| [Convex adapter](docs/adapters/convex.md) | Convex-compatible queries, mutations, React hooks |
| [Firebase adapter](docs/adapters/firebase.md) | Firestore REST/gRPC-Web/WebSocket Listen |
| [Cloud Functions adapter](docs/adapters/cloud-functions.md) | Firebase v2 triggers and HTTP handlers |
| [MongoDB adapter](docs/adapters/mongodb.md) | MongoDB wire protocol compatibility |
| [Native adapter](docs/adapters/native.md) | Direct REST and WebSocket API |
| [Storage backends](docs/guides/storage-backends.md) | Choosing and configuring SQLite, Postgres, MySQL, libSQL |
| [HTTP & WebSocket API](docs/reference/http-api.md) | Full route catalog |
| [CLI reference](docs/reference/cli.md) | Every flag and default |
| [Current capabilities](docs/reference/current-capabilities.md) | What works today |
| [Convex compatibility](docs/convex/compatibility.md) | Convex surface scope and limits |
| [Demos](demos/README.md) | Working example applications |

## Community

- **GitHub Issues** -- [bugs and concrete problems](https://github.com/agentstation/neovex/issues)
- **GitHub Discussions** -- [feature requests and longer-form conversation](https://github.com/agentstation/neovex/discussions)

## Contributing

Contributions are welcome. We particularly value bug reports, design feedback, compatibility test cases from people migrating off other platforms, and documentation improvements.

Before opening a PR, please read [**CONTRIBUTING.md**](CONTRIBUTING.md) for the workflow, the CLA, and the coding standards. For larger changes, open a discussion or issue first so we can align on the approach before you write code.

## Security

> [!IMPORTANT]
> If you've found a security vulnerability, please **do not** open a public GitHub issue. Instead, report it through [GitHub Security Advisories](https://github.com/agentstation/neovex/security/advisories/new) so we can triage it privately. See [SECURITY.md](SECURITY.md) for the full policy.

## Licensing

Neovex is **source-available** under the [Neovex Community License](LICENSE).

**Free use** -- you can use Neovex for free, including in production, if:

- you are an individual developer, **or**
- you are a nonprofit or educational institution, **or**
- your organization has not exceeded **both** $10M USD annual revenue **and** 500 monthly active users in the same calendar month

Free use includes self-hosting, internal modifications, and private forks. There are no restrictions on team size, project type, or deployment model within the free-use boundary.

The threshold is a dual gate -- both conditions must be true simultaneously. A $500M company with 100 monthly active users is free. A $2M startup with 50,000 monthly active users is free.

**Monthly active users include internal users.** An employee who uses a Neovex-backed internal tool counts toward the MAU threshold, not just external customers.

**Enterprise threshold** -- if your organization exceeds both gates, you receive a one-time 90-day trial. After that, continued use requires a commercial license. The trial does not reset.

**Competing offering restriction** -- regardless of your size, you may not offer Neovex itself, or a materially similar managed backend/database/platform service, as a hosted service to third parties without a commercial license. Using Neovex to power your own product is always allowed.

| Document | What's in it |
|----------|-------------|
| [LICENSE](LICENSE) | Full legal text |
| [LICENSING.md](LICENSING.md) | Plain-English summary with examples |
| [COMMERCIAL.md](COMMERCIAL.md) | Commercial license overview |
| [TRADEMARKS.md](TRADEMARKS.md) | Trademark usage policy |

## Build from source

Requires [Rust](https://rustup.rs/) stable toolchain.

```bash
git clone https://github.com/agentstation/neovex.git
cd neovex
cargo install --path crates/neovex-bin
```

## Verify releases

Every release includes SHA256 checksums and [build provenance attestations](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations/using-artifact-attestations-to-establish-provenance-for-builds) signed via [Sigstore](https://www.sigstore.dev/).

```bash
# Checksum verification
curl -LO https://github.com/agentstation/neovex/releases/latest/download/checksums-sha256.txt
sha256sum --check --ignore-missing checksums-sha256.txt

# Build provenance (requires GitHub CLI)
gh attestation verify neovex_darwin_arm64.tar.gz --owner agentstation
```

On macOS, use `shasum -a 256 --check` instead of `sha256sum`.

---

<div align="center">

Built by [agentstation](https://github.com/agentstation) and the Neovex contributors.

</div>
