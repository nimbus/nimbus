<div align="center">

# Neovex

**The production-grade self-hosted application runtime.**

Compute, storage, real-time, and scheduling — in a single Rust binary.

[![CI](https://github.com/agentstation/neovex/actions/workflows/ci.yml/badge.svg)](https://github.com/agentstation/neovex/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/agentstation/neovex)](https://github.com/agentstation/neovex/releases/latest)
[![License](https://img.shields.io/badge/license-Neovex%20Community-blue)](LICENSE)

[Quick start](#quick-start) · [Why Neovex](#why-neovex) · [Docs](docs/README.md) · [Architecture](ARCHITECTURE.md)

</div>

---

Neovex is a single-binary application backend you can run on your own infrastructure. It gives you the developer experience of a managed backend-as-a-service — reactive queries, document storage, scheduled jobs, a JavaScript runtime — without the SaaS lock-in, the per-request billing, or the *"this is a dev tool, don't run it in production"* disclaimer that comes with most self-hosted alternatives.

It's built for teams that need to run their full application stack on their own hardware: regulated industries, air-gapped environments, customers with data residency requirements, and any team whose SaaS bill has stopped making sense.

> [!NOTE]
> Neovex is **pre-1.0** and under active development. The Convex compatibility layer is the most mature surface area; everything else is moving fast. If you're evaluating Neovex, [open an issue](https://github.com/agentstation/neovex/issues) or [start a discussion](https://github.com/agentstation/neovex/discussions) — we're working with a small group of design partners and want to make sure we're solving the right problems.

## Why Neovex

Most self-hosted backends are dev tools wearing a production costume. They run on a single machine, can't migrate without wiping the database, and ship with a "we strongly recommend the cloud version for real workloads" warning in the README. They exist to address lock-in fear, not to actually run your application.

Neovex is designed from day one to be the thing you actually deploy.

| Capability             | Convex OSS           | Firebase           | Vercel                | **Neovex**                  |
| ---------------------- | -------------------- | ------------------ | --------------------- | --------------------------- |
| Self-hostable          | Single-machine only  | No                 | No                    | **Yes**                     |
| Tenant isolation       | No                   | Managed only       | N/A                   | **Built into storage layer** |
| Backend migrations     | "Wipe and restart"   | Managed only       | N/A                   | **Yes**                     |
| Air-gapped deploy      | Not supported        | Not supported      | Not supported         | **Yes**                     |
| Telemetry phone-home   | On by default        | Always             | Always                | **None**                    |
| Pricing model          | Cloud-only paid tier | Per-read/per-write | Per-request, per-GB   | **License, not metered**    |
| Source available       | Yes (FSL)            | No                 | No                    | **Yes (Community License)** |

If you've ever opened the Convex OSS README, read *"we strongly recommend you switch to the cloud service"*, and thought *that's not actually self-hosting* — Neovex is for you.

## Quick start

### Install

```bash
# macOS / Linux via Homebrew
brew install agentstation/tap/neovex
```

```bash
# Or download a binary directly (example: macOS Apple Silicon)
curl -LO https://github.com/agentstation/neovex/releases/latest/download/neovex_darwin_arm64.tar.gz
tar xzf neovex_darwin_arm64.tar.gz && sudo mv neovex /usr/local/bin/
```

| Platform | Architecture | Archive |
|----------|-------------|---------|
| Linux | x86_64 | `neovex_linux_x86_64.tar.gz` |
| Linux | ARM64 | `neovex_linux_arm64.tar.gz` |
| macOS | Apple Silicon | `neovex_darwin_arm64.tar.gz` |
| Windows | x86_64 | `neovex_windows_x86_64.zip` |

To build from source, see [build from source](#build-from-source).

### Run

```bash
neovex --port 8080 --data-dir ./data
```

That's it. You now have a backend running on `http://localhost:8080` with document storage, HTTP and WebSocket APIs, scheduled jobs, and a V8 runtime — in a single process, with a single binary, writing to a single directory.

### Talk to it

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

## What's in the box

- **Document storage** with optional per-table schemas, single-field indexed queries, and cursor-based pagination
- **Reactive subscriptions** over WebSocket — clients see live updates without polling
- **JavaScript runtime** powered by V8 for server-side queries, mutations, actions, and HTTP routes
- **Durable scheduling** with `runAfter`, `runAt`, and recurring cron jobs that survive restarts
- **Tenant isolation** built into the storage layer, not bolted on
- **JWT / JWKS authentication** with support for any standards-compliant identity provider
- **A single Rust binary** you can `scp` to a server and run — no Docker required, no Kubernetes required, no external database required

## Compatibility adapters

Neovex speaks the protocols of platforms developers already use, so migration is a configuration change, not a rewrite.

| Platform                                          | Status                      | Coverage                                                                                  |
| ------------------------------------------------- | --------------------------- | ----------------------------------------------------------------------------------------- |
| [Convex](docs/convex/compatibility.md)            | 🚧 Under active development | Queries, mutations, actions, scheduling, WebSocket subscriptions, JWT auth, HTTP routes   |

The Convex adapter is where active development is focused today. Additional compatibility surfaces — including Firebase/Firestore, Next.js hosting, and Parse Server — may be explored in the future.

> [!TIP]
> Running on one of these today and the bill, the lock-in, or the compliance gap has you looking for the door? [Open an issue](https://github.com/agentstation/neovex/issues) — we want to hear about your migration scenario.

## How it works

Neovex is a single Rust process that embeds:

- A storage engine ([redb](https://github.com/cberner/redb)) for tenant-isolated document and metadata storage
- An HTTP and WebSocket server for client communication
- A V8 isolate pool for executing user JavaScript with a host bridge for storage, scheduling, and network access
- A durable scheduler for cron jobs and delayed mutations
- A reactive query engine that tracks subscriptions and pushes invalidations to connected clients

Everything runs in one process, shares one transaction model, and writes to one local data directory. There are no sidecars, no required external services, and no daemon zoo to operate. The architecture is designed around tenant-level isolation, with each tenant getting its own embedded database within the data directory.

For the full design — why redb, how the reactive engine works, the V8 host bridge model, and the trade-offs we made — see [**ARCHITECTURE.md**](ARCHITECTURE.md).

## Who Neovex is for

- **Platform teams at regulated companies** who can't put production data in someone else's cloud — healthcare, finance, defense, government, EU companies with data residency requirements.
- **Engineering teams whose SaaS bill stopped making sense** — Firebase, Vercel, or Convex bills that grew faster than the business and now look like the most expensive line item in infrastructure.
- **Developers building for air-gapped environments** — defense, industrial, or any deployment where "make an outbound HTTPS call to a vendor's API" is not an option.
- **Teams who want one binary instead of fifteen services** — replace a Postgres + Redis + cron + WebSocket gateway + serverless function host stack with a single process that does all of it.

If you're a hobbyist on a side project that happily runs on Vercel's free tier, Neovex isn't for you yet — and that's fine. Use the right tool for the job.

## Documentation

| Document                                                              | What's in it                                  |
| --------------------------------------------------------------------- | --------------------------------------------- |
| [Documentation index](docs/README.md)                                 | Start here for deeper technical docs          |
| [Architecture](ARCHITECTURE.md)                                       | How Neovex is built and why                   |
| [HTTP & WebSocket API](docs/reference/http-api.md)                    | Full route catalog                            |
| [CLI reference](docs/reference/cli.md)                                | Every flag and default                        |
| [Current capabilities](docs/reference/current-capabilities.md)        | What works today                              |
| [Convex compatibility](docs/convex/compatibility.md)                  | Convex surface scope and limits               |
| [Demos](demos/README.md)                                              | Working example applications                  |
| [Plans index](docs/plans/README.md)                                   | Active and deferred execution plans           |

## Community

- **GitHub Issues** — [bugs and concrete problems](https://github.com/agentstation/neovex/issues)
- **GitHub Discussions** — [feature requests and longer-form conversation](https://github.com/agentstation/neovex/discussions)

<!-- Future community channels:
- **Discord** — [discord.gg/your-invite](https://discord.gg/your-invite)
- **Twitter / X** — [@neovexdev](https://twitter.com/neovexdev)
- **Blog** — [neovex.dev/blog](https://neovex.dev/blog)
-->

## Licensing

Neovex is **source-available** under the [Neovex Community License](LICENSE).

**Free use** — you can use Neovex for free, including in production, if:

- you are an individual developer, **or**
- you are a nonprofit or educational institution, **or**
- your organization has not exceeded **both** $10M USD annual revenue **and** 500 monthly active users in the same calendar month

Free use includes self-hosting, internal modifications, and private forks. There are no restrictions on team size, project type, or deployment model within the free-use boundary.

The threshold is a dual gate — both conditions must be true simultaneously. A $500M company with 100 monthly active users is free. A $2M startup with 50,000 monthly active users is free.

**Monthly active users include internal users.** An employee who uses a Neovex-backed internal tool counts toward the MAU threshold, not just external customers.

**Enterprise threshold** — if your organization exceeds both gates, you receive a one-time 90-day trial. After that, continued use requires a commercial license. The trial does not reset.

**Competing offering restriction** — regardless of your size, you may not offer Neovex itself, or a materially similar managed backend/database/platform service, as a hosted service to third parties without a commercial license. Using Neovex to power your own product is always allowed.

| Document                            | What's in it                                   |
| ----------------------------------- | ---------------------------------------------- |
| [LICENSE](LICENSE)                   | Full legal text                                |
| [LICENSING.md](LICENSING.md)        | Plain-English summary with examples            |
| [COMMERCIAL.md](COMMERCIAL.md)      | Commercial license overview                    |
| [TRADEMARKS.md](TRADEMARKS.md)      | Trademark usage policy                         |

## Contributing

Contributions are welcome. We particularly value bug reports, design feedback, compatibility test cases from people migrating off other platforms, and documentation improvements.

Before opening a PR, please read [**CONTRIBUTING.md**](CONTRIBUTING.md) for the workflow, the CLA, and the coding standards. For larger changes, open a discussion or issue first so we can align on the approach before you write code.

## Security

> [!IMPORTANT]
> If you've found a security vulnerability, please **do not** open a public GitHub issue. Instead, report it through [GitHub Security Advisories](https://github.com/agentstation/neovex/security/advisories/new) so we can triage it privately. See [SECURITY.md](SECURITY.md) for the full policy.

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
