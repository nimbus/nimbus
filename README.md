# neovex

[![CI](https://github.com/agentstation/neovex/actions/workflows/ci.yml/badge.svg)](https://github.com/agentstation/neovex/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/agentstation/neovex/graph/badge.svg)](https://codecov.io/gh/agentstation/neovex)
[![Release](https://img.shields.io/github/v/release/agentstation/neovex)](https://github.com/agentstation/neovex/releases/latest)
[![Homebrew](https://img.shields.io/badge/homebrew-agentstation%2Ftap%2Fneovex-orange)](https://github.com/agentstation/homebrew-tap)

Self-hosted JavaScript backend runtime powered by V8.

Neovex combines tenant-isolated embedded storage, native HTTP and WebSocket
APIs, scheduled work, and an optional Convex compatibility surface in a single
Rust binary.

## What It Does

- tenant-isolated document storage with optional per-table schemas
- document CRUD, explicit queries, and paginated queries over HTTP
- live query subscriptions over WebSocket
- durable scheduled mutations and recurring cron jobs
- an optional V8 runtime and in-repo Convex compatibility layer

## Install

### Homebrew (macOS and Linux)

```bash
brew install agentstation/tap/neovex
```

Homebrew automatically verifies the SHA256 checksum of the downloaded archive.

### Download binary

Download the latest release for your platform from [GitHub Releases](https://github.com/agentstation/neovex/releases/latest).

| Platform | Architecture | Archive |
|----------|-------------|---------|
| Linux | x86_64 | `neovex_linux_x86_64.tar.gz` |
| Linux | ARM64 | `neovex_linux_arm64.tar.gz` |
| macOS | Intel | `neovex_darwin_x86_64.tar.gz` |
| macOS | Apple Silicon | `neovex_darwin_arm64.tar.gz` |
| Windows | x86_64 | `neovex_windows_x86_64.zip` |

```bash
# Example: download and install on macOS Apple Silicon
curl -LO https://github.com/agentstation/neovex/releases/latest/download/neovex_darwin_arm64.tar.gz
tar xzf neovex_darwin_arm64.tar.gz
sudo mv neovex /usr/local/bin/
```

### Build from source

Requires [Rust](https://rustup.rs/) stable toolchain.

```bash
git clone https://github.com/agentstation/neovex.git
cd neovex
cargo install --path crates/neovex-bin
```

## Verify

Every release includes SHA256 checksums and [build provenance attestations](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations/using-artifact-attestations-to-establish-provenance-for-builds) signed via [Sigstore](https://www.sigstore.dev/). These provide cryptographic proof that each binary was built by our GitHub Actions CI from this repository's source code.

### Checksum verification

Each release includes a `checksums-sha256.txt` file:

```bash
# Download the binary and checksums
curl -LO https://github.com/agentstation/neovex/releases/latest/download/neovex_darwin_arm64.tar.gz
curl -LO https://github.com/agentstation/neovex/releases/latest/download/checksums-sha256.txt

# Verify
sha256sum --check --ignore-missing checksums-sha256.txt
```

On macOS, use `shasum -a 256 --check` instead of `sha256sum`.

### Build provenance attestation

Verify that a binary was built by GitHub Actions from this repository:

```bash
gh attestation verify neovex_darwin_arm64.tar.gz --owner agentstation
```

This checks the Sigstore-signed attestation against the [GitHub attestation ledger](https://github.com/agentstation/neovex/attestations). It confirms the exact workflow, commit, and runner that produced the artifact. Requires the [GitHub CLI](https://cli.github.com/).

## Licensing

- source-available under the [Neovex Community License](LICENSE)
- plain-English summary in [LICENSING.md](LICENSING.md)
- commercial terms overview in [COMMERCIAL.md](COMMERCIAL.md)
- contributor policy in [CONTRIBUTING.md](CONTRIBUTING.md)
- optional runtime license loading via `--license-file`, `NEOVEX_LICENSE_FILE`, or `./.neovex/license.json`
- current in-product license status exposed at `GET /debug/license/status`

## Docs

- [Documentation index](docs/README.md)
- [Architecture](ARCHITECTURE.md)
- [Current capabilities](docs/reference/current-capabilities.md)
- [HTTP and WebSocket API](docs/reference/http-api.md)
- [CLI reference](docs/reference/cli.md)
- [Convex compatibility](docs/convex/compatibility.md)
- [Demos](demos/README.md)
- [Plans](docs/plans/README.md)
