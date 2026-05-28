# NDB1 Proof — Cargo wiring

Adds `zbus_systemd` + direct `zbus` as workspace deps, wires the
`systemd-dbus` / `systemd-dbus-test-bus` / `systemd-dbus-integration-tests`
features onto `nimbus-node` (optional deps, default OFF), and proves the
deps resolve to the latest versions, are feature-gated, and add no new
`cargo deny` violations.

## Dependency declarations

Root `Cargo.toml` `[workspace.dependencies]`:

```toml
zbus = { version = "5.15", default-features = false, features = ["tokio"] }
zbus_systemd = { version = "=0.26000.0", default-features = false, features = ["systemd1", "zbus-async-tokio"] }
```

`crates/nimbus-node/Cargo.toml`:

```toml
[dependencies]
zbus = { workspace = true, optional = true }
zbus_systemd = { workspace = true, optional = true }

[features]
systemd-dbus = ["dep:zbus", "dep:zbus_systemd"]
systemd-dbus-test-bus = ["systemd-dbus"]
systemd-dbus-integration-tests = ["systemd-dbus"]
```

No `default = [...]` lists these — the crate compiles exactly as before
until NDB7 flips `systemd-dbus` into `default`.

## Latest versions (new deps only)

`zbus_systemd 0.26000.0` is the newest published stable (2026-03-28 per
crates.io); `zbus` resolves to the latest 5.x. `deno_core` and every
other workspace dep are untouched.

Resolved in `Cargo.lock`:

```text
zbus          5.15.0
zbus_systemd  0.26000.0
zvariant      5.11.0
```

## Reachability + feature gating

`cargo tree -p nimbus-node --features systemd-dbus -e normal` (excerpt):

```text
├── zbus v5.15.0
│   ├── zbus_macros v5.15.0 (proc-macro)
│   │   ├── zbus_names v4.3.2
│   │   │   └── zvariant v5.11.0
│   ...
```

`cargo tree -p nimbus-node -e normal` (no feature) contains **0** `zbus`
lines — the binding deps are truly optional and off by default.

## Deny: zero new violations

`[graph] all-features = true` means cargo-deny evaluates the
`systemd-dbus` subtree even though it is off by default. Result of
`cargo deny check bans licenses`:

- **licenses: ok** — `zbus`, `zbus_systemd`, `zvariant`, `zbus_names`,
  `zvariant_utils` and their deps are all MIT/Apache, already allowlisted.
- **bans: the zbus subtree adds zero new duplicates.** The only
  duplicate errors are `bindgen`, `libloading`, `which` — and these fire
  **identically with the zbus subtree removed** (verified by stashing the
  NDB1 edits and re-running on the pristine base).

The base-branch `bans FAILED` is a pre-existing, unrelated issue: the
deno lane bumped `deno_core` to `0.401.0` while `deny.toml` still
skip-trees `deno_core@0.400.0` (cargo-deny prints `unmatched-skip-root`,
which un-suppresses those three deno-subtree duplicates). NDB does not
own that fix and does not touch `deno_core` or its skip-tree entry. NDB1's
evidence is the **with-vs-without-zbus delta = zero**, not an absolute
green. `deny.toml` gains only an informational comment explaining the
`zbus_systemd` `0.26000.x` version scheme.

## Verifier

`bash scripts/verify-node-dbus-binding.sh` condition 4 flips to PASS
(`zbus_systemd/zbus deps plus systemd-dbus test and integration
features`). Verifier state after NDB1: `4 passed, 6 failed`.
