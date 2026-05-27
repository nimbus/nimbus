# Bun/JSC Gate 25: Linux Minicloud Verification

Date: 2026-05-23

Nimbus plan: `docs/plans/archive/bun-jsc-in-process-lockdown-plan.md`

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun proof head: `ce5aa2a389` (`Stabilize Bun embed cancellation proof on Linux`)

## Decision

Status: Linux/minicloud proof passed.

The reproducible Bun/JSC in-process lockdown gate now passes on the Debian 13
`minicloud` host as well as the macOS worktree. This proves the current
proof-only embed lane is not macOS-only, but it does not make Bun/JSC
selectable for tenant code.

## Host Setup

The minicloud proof deliberately avoided host-wide apt trust changes. LLVM was
installed under the `nimbus` user's home directory from a verified official
release artifact.

| Component | Evidence |
| --- | --- |
| Host | `Linux minicloud 6.12.88+deb13-amd64 #1 SMP PREEMPT_DYNAMIC Debian 6.12.88-1 (2026-05-15) x86_64 GNU/Linux` |
| Rust | `rustc 1.95.0 (59807616e 2026-04-14)` via user-local rustup stable |
| Bun Rust build toolchain | Bun build graph selected `nightly-2026-05-06-x86_64-unknown-linux-gnu` |
| Node | `v24.16.0` via user-local nvm LTS |
| npm | `11.13.0` |
| Bun CLI | `1.3.14` under `~/.bun/bin/bun` |
| LLVM | `21.1.8` under `~/.local/toolchains/LLVM-21.1.8-Linux-X64` |
| LLVM asset digest | `sha256:b3b7f2801d15d50736acea3c73982994d025b01c2f035b91ae3b49d1b575732b` |
| Scratch root | `~/.cache/nimbus-proof` |

## Fresh Debian 13 Bootstrap Notes

This section records what was needed to bootstrap the fresh Debian 13
`minicloud` host for this proof. It is evidence for the Bun/JSC proof lane, not
a general Nimbus installer contract.

Base packages came from Bun's current Debian development dependency list and
used only Debian's normal package repositories:

```sh
sudo apt update
sudo apt install curl wget lsb-release software-properties-common cmake git \
  golang libtool ninja-build pkg-config ruby-full xz-utils ca-certificates \
  unzip
```

Rust was installed with user-local `rustup`, not Debian's `rustc` or `cargo`,
so Bun's pinned nightly toolchain could be selected by `rust-toolchain.toml`:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"
rustup default stable
```

Node/npm were installed user-locally with `nvm`, and the Bun CLI was installed
under `~/.bun/bin` because Bun's build graph uses Bun for code generation:

```sh
export NVM_DIR="$HOME/.nvm"
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.3/install.sh | bash
. "$NVM_DIR/nvm.sh"
nvm install --lts
nvm use --lts
curl -fsSL https://bun.com/install | bash
```

The important Linux lesson is that Debian 13's default `clang 19.1.7` was not
accepted by Bun's native build. Bun currently requires LLVM 21.1.8. For this
proof we deliberately avoided `apt.llvm.org` and host-wide apt trust changes;
instead, LLVM was installed as a user-local toolchain from the official
`llvmorg-21.1.8` release asset and verified before extraction:

```sh
mkdir -p "$HOME/.local/toolchains" "$HOME/.cache/nimbus-proof/downloads"
cd "$HOME/.cache/nimbus-proof/downloads"
curl -LO \
  https://github.com/llvm/llvm-project/releases/download/llvmorg-21.1.8/LLVM-21.1.8-Linux-X64.tar.xz
printf '%s  %s\n' \
  'b3b7f2801d15d50736acea3c73982994d025b01c2f035b91ae3b49d1b575732b' \
  'LLVM-21.1.8-Linux-X64.tar.xz' | sha256sum -c -
tar -xJf LLVM-21.1.8-Linux-X64.tar.xz -C "$HOME/.local/toolchains"
export PATH="$HOME/.local/toolchains/LLVM-21.1.8-Linux-X64/bin:$HOME/.bun/bin:$PATH"
```

The proof also needed a home-backed scratch root. On this host, `/tmp` was a
small tmpfs-backed volume and was a poor native-build scratch location:

```sh
export PROOF_ROOT="$HOME/.cache/nimbus-proof"
mkdir -p "$PROOF_ROOT/tmp" \
  "$PROOF_ROOT/bun-embed-native" \
  "$PROOF_ROOT/bun-cache" \
  "$PROOF_ROOT/bun-rust-only" \
  "$PROOF_ROOT/bun-cargo-target"
export TMPDIR="$PROOF_ROOT/tmp"
```

## Command

```sh
cd ~/src/github.com/nimbus/nimbus
. "$HOME/.cargo/env"
export NVM_DIR="$HOME/.nvm"
. "$NVM_DIR/nvm.sh"
nvm use --lts >/dev/null
export PROOF_ROOT="$HOME/.cache/nimbus-proof"
mkdir -p "$PROOF_ROOT/tmp" \
  "$PROOF_ROOT/bun-embed-native" \
  "$PROOF_ROOT/bun-cache" \
  "$PROOF_ROOT/bun-rust-only" \
  "$PROOF_ROOT/bun-cargo-target"
export TMPDIR="$PROOF_ROOT/tmp"
export PATH="$HOME/.local/toolchains/LLVM-21.1.8-Linux-X64/bin:$HOME/.bun/bin:$PATH"
NIMBUS_BUN_REPO=~/src/github.com/oven-sh/bun \
NIMBUS_BUN_BUILD_DIR="$PROOF_ROOT/bun-embed-native" \
NIMBUS_BUN_CACHE_DIR="$PROOF_ROOT/bun-cache" \
NIMBUS_BUN_RUST_ONLY_BUILD_DIR="$PROOF_ROOT/bun-rust-only" \
NIMBUS_BUN_CARGO_TARGET_DIR="$PROOF_ROOT/bun-cargo-target" \
bash scripts/verify-bun-jsc-in-process-lockdown.sh
```

## Result

Passed all ten script steps:

| Step | Result |
| --- | --- |
| 1. Nimbus format | passed |
| 2. Nimbus UI build prerequisites | passed; `make build-ui` had nothing to rebuild after the clean setup |
| 3. Runtime/backend policy tests | 9 passed |
| 4. Registry/runtime metadata rejection tests | 10 passed |
| 5. Runtime diagnostics tests | 2 passed |
| 6. Ignored Bun source proof lane | 1 passed |
| 7. Nimbus whitespace diff check | passed |
| 8. Bun Rust format | passed |
| 9. Bun native embed probe | passed; emitted `[build] check-bun-embed-probe done` |
| 10. Bun whitespace diff check | passed |

## Findings

The Linux lane found real proof-harness portability issues:

- clean Linux checkouts need the embedded UI build before `nimbus-server` tests
- Debian 13's default `clang 19.1.7` is not accepted by Bun's current native
  build; the proof used user-local LLVM 21.1.8 instead
- small tmpfs-backed `/tmp` volumes are poor native-build scratch roots
- JSC termination calls from background proof threads need Bun/WebKit stack
  bounds initialized on that thread
- the proof must clear the VM termination-request bit after owner-thread
  termination-exception priming
- a 10 ms cancellation delay was too timing-sensitive for debug Linux builds;
  the proof now uses a less aggressive cancellation delay while still checking
  that the generated spin handler entered before accepting recovery

## Product Implication

This pass reduces platform risk for the future Bun/JSC backend, but it does
not change the go/no-go decision:

- Bun/JSC remains proof-only and not selectable.
- No Nimbus Bun fork is required yet.
- A future in-process backend still needs explicit Bun embedder APIs for
  construction profiles, native permission denial/mediation, resolver policy,
  worker propagation, dynamic-code policy, lifecycle, memory, cancellation, and
  pool teardown.
