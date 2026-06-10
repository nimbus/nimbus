# NLRT1 Deno Fork Provenance

Date: 2026-05-28
Authoring agent: Codex
Status: done

## Scope

Add a scripted provenance gate for the Deno-family runtime dependency shape.
The verifier must inspect `Cargo.toml`, `Cargo.lock`, and
`cargo tree -p nimbus-runtime`, then prove that patch-sensitive crates resolve
to the expected Nimbus fork revisions while Deno-family crates that remain on
crates.io are explicitly allowlisted with reasons.

## Files Changed

- `scripts/verify-deno-fork-provenance.sh`
- `docs/architecture/runtime/deno-fork-provenance-allowlist.tsv`
- `docs/plans/node-lts-runtime-trust-plan.md`
- `docs/plans/proof/node-lts-runtime-trust/README.md`
- `docs/plans/proof/node-lts-runtime-trust/nlrt1-deno-fork-provenance.md`

## Decisions

- Added a focused verifier now rather than waiting for the final NLRT11 verifier.
  NLRT11 can call this verifier as one of its completion-gate checks.
- Treated Deno-family runtime closure as the actual `nimbus-runtime` tree, not
  just direct workspace dependencies. The scanner covers crates named
  `deno_*`, `denort_helper`, `napi_sym`, `node_*`, `serde_v8`, `sys_traits`,
  `urlpattern`, and `v8`.
- Required `Cargo.toml` patch entries for the root patch-sensitive crates and
  required `Cargo.lock` sources to match exact fork tags and SHAs.
- Added a TSV allowlist instead of embedding exception reasons only in shell
  code. New crates.io Deno-family crates now fail until they are either forked
  or deliberately allowlisted with a reason.

## Expected Fork Revisions

- `nimbus/deno`: `v2.8.0-nimbus.5`
  at `37b6333a1f703db523efe8a703d36f2152ad087a`.
- `nimbus/rusty_v8`: `v149.0.0-nimbus.1`
  at `9b77553883f1117ab3df62709b8673b803ed721b`.

## Cargo Tree Evidence

Command:

```text
cargo tree -p nimbus-runtime --prefix none --charset ascii
```

The verifier extracts the Deno-family subset from that runtime tree. On this
baseline it classified 55 unique runtime crates:

- 40 forked crates resolved to the expected `nimbus/deno` or `nimbus/rusty_v8`
  source.
- 15 crates resolved to crates.io and are allowlisted with reasons.

Representative forked crates:

```text
deno_core v0.401.0 -> git+https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a1f703db523efe8a703d36f2152ad087a
deno_node v0.186.0 -> git+https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a1f703db523efe8a703d36f2152ad087a
node_resolver v0.86.0 -> git+https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a1f703db523efe8a703d36f2152ad087a
serde_v8 v0.310.0 -> git+https://github.com/nimbus/deno?tag=v2.8.0-nimbus.5#37b6333a1f703db523efe8a703d36f2152ad087a
v8 v149.0.0 -> git+https://github.com/nimbus/rusty_v8?tag=v149.0.0-nimbus.1#9b77553883f1117ab3df62709b8673b803ed721b
```

Allowlisted crates.io exceptions:

```text
deno_ast
deno_error
deno_error_macro
deno_graph
deno_media_type
deno_native_certs
deno_path_util
deno_semver
deno_terminal
deno_tunnel
deno_unsync
deno_whoami
sys_traits
sys_traits_macros
urlpattern
```

Each exception has a reason in
`docs/architecture/runtime/deno-fork-provenance-allowlist.tsv`.

## Verification

```text
bash scripts/verify-deno-fork-provenance.sh
Summary: 5 passed, 0 failed
Runtime Deno-family classification: 40 forked, 15 allowlisted
```

```text
npm run docs:validate-refs:strict
docs reference validation: pass (218 working-tree Markdown files)
```

## Remaining Risks

- NLRT11 still needs to wire this focused verifier into
  `scripts/verify-node-lts-runtime-trust.sh`.
- If a future Deno or V8 fork bump changes tags or SHAs, this verifier should
  fail until the expected provenance is intentionally updated with proof.
