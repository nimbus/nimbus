# Node Compatibility Surface Matrix

Status: draft-active

This matrix is the checked-in source of truth for the currently supported
Node-facing surface in `crates/neovex-runtime`.

It complements, but does not replace, the generated Node LTS artifact set in
[`node-lts-compat/`](node-lts-compat/node-lts-compat-summary.md):

- [`node-lts-compat-summary.md`](node-lts-compat/node-lts-compat-summary.md)
- [`node-lts-compat-matrix.csv`](node-lts-compat/node-lts-compat-matrix.csv)
- the supporting `node20` / `node22` / Deno inventory CSVs in the same folder

Use the generated baseline for broad built-in coverage truth. Use this
document for the narrower, fixture-backed Neovex runtime contract.

The important rule is simple:

- only claim behavior that has a named fixture
- treat everything else as unsupported until a fixture proves otherwise

## Current Baseline

Neovex currently has one runtime backend (`V8DenoCore`) and two initial
compatibility targets:

- `WebStandardIsolate`
- `Node22`

It also has two runtime profiles:

- `Application`
- `Tooling`

At this stage, the verified Node22 surface is still deliberately narrow. It
now includes the first capability-scoped local runtime services plus the first
scoped CommonJS/package-resolution bridge, but it is still well short of
general Node parity and should be read as an explicit contract, not an
implication of future support.

Neovex's named compatibility baseline is `Node22`. Current upstream Convex and
Firebase / Cloud Functions stacks still support Node 20, and some codegen
bundles continue to emit `node20` targets for portability, but Neovex does not
yet claim a separate verified `Node20` runtime contract.

## Public Support-State Vocabulary

Neovex uses these support-state labels in its public Node-facing contract:

- `Supported`
- `SupportedToolingOnly`
- `Partial`
- `StubOnly`
- `NotSupported`
- `NeedsVerification`

Current public contract:

- `Node22` is the primary compatibility target.
- `Node20` is a measured compatibility lane, not a separate runtime contract.
- Neovex does **not** currently claim full Node built-in compatibility for any
  runtime profile.
- Any built-in that is `SupportedToolingOnly`, `Partial`, `StubOnly`,
  `NotSupported`, or `NeedsVerification` prevents a blanket "full Node built-in
  compatibility" claim for that target/profile pair.

## Verified Surface

| Surface | `Application + WebStandardIsolate` | `Application + Node22` | `Tooling + Node22` | Evidence |
| --- | --- | --- | --- | --- |
| `globalThis.global` | unsupported | supported | supported by same bootstrap contract | `runtime::tests::basic_invocation::web_standard_target_does_not_expose_node_globals`, `runtime::tests::basic_invocation::node22_target_exposes_minimal_node_globals` |
| `globalThis.Deno` | unsupported | unsupported | unsupported by the same Node22 bootstrap contract | `runtime::tests::basic_invocation::runtime_removes_deno_global_from_bundle_execution`, `runtime::tests::basic_invocation::node22_target_hides_deno_bootstrap_globals` |
| `globalThis.__bootstrap` / `globalThis.bootstrap` | unsupported | unsupported | unsupported by the same Node22 bootstrap contract | `runtime::tests::basic_invocation::runtime_removes_deno_global_from_bundle_execution`, `runtime::tests::basic_invocation::node22_target_hides_deno_bootstrap_globals` |
| `process.version` | unsupported | supported | supported by same bootstrap contract | `runtime::tests::basic_invocation::web_standard_target_does_not_expose_node_globals`, `runtime::tests::basic_invocation::node22_target_exposes_minimal_node_globals` |
| `process.versions.node` | unsupported | supported | supported by same bootstrap contract | `runtime::tests::basic_invocation::web_standard_target_does_not_expose_node_globals`, `runtime::tests::basic_invocation::node22_target_exposes_minimal_node_globals` |
| `process.cwd()` | unsupported | supported, scoped to the generated bundle root | supported, scoped to the app root | `runtime::tests::basic_invocation::application_node22_reads_local_files_and_denies_env_and_escape_writes`, `runtime::tests::basic_invocation::tooling_node22_allows_allowlisted_env_and_tmp_writes` |
| `process.env` | unsupported | explicit deny-by-default capability error | allowlist-only (`PATH`, `HOME`, `PWD`, `TMPDIR`, `TEMP`, `TMP`, `NODE_ENV`, `npm_config_cache`, `npm_config_user_agent`, `npm_execpath`, `ESBUILD_BINARY_PATH`, `ESBUILD_MAX_BUFFER`, `NODE_V8_COVERAGE`) | `runtime::tests::basic_invocation::application_node22_reads_local_files_and_denies_env_and_escape_writes`, `runtime::tests::basic_invocation::tooling_node22_allows_allowlisted_env_and_tmp_writes` |
| `node:fs/promises.readFile` | unsupported | supported inside the generated bundle root only | supported inside scoped runtime roots (`app_root`, `generated_root`, `.neovex/tmp`, `.neovex/cache`) | `runtime::tests::basic_invocation::application_node22_reads_local_files_and_denies_env_and_escape_writes`, `runtime::tests::basic_invocation::tooling_node22_allows_allowlisted_env_and_tmp_writes` |
| `node:fs/promises.writeFile` | unsupported | denied by the scoped Deno-family permission contract when the path escapes approved roots | supported only inside pre-existing directories under approved write roots (`generated_root`, `.neovex/tmp`, `.neovex/cache`) | `runtime::tests::basic_invocation::application_node22_reads_local_files_and_denies_env_and_escape_writes`, `runtime::tests::basic_invocation::tooling_node22_allows_allowlisted_env_and_tmp_writes`, `runtime::tests::basic_invocation::tooling_node22_write_file_requires_preexisting_parent_directory` |
| `node:path` builtin import and core path helpers | unsupported | supported | supported by the same Node22 bootstrap contract | `runtime::tests::basic_invocation::node22_target_supports_node_path_builtin_imports` |
| `node:module.createRequire()` / `Module._load()` | unsupported | supported for staged local CommonJS and JSON targets inside approved runtime roots | supported for the same staged local targets inside approved runtime roots | `runtime::tests::basic_invocation::application_node22_loads_commonjs_package_entries_via_esm_import`, `runtime::tests::basic_invocation::application_node22_loads_implicit_commonjs_packages_with_nested_require` |
| Local `node_modules` package resolution with `package.json` `main` / `exports` / `"type"` / import conditions | unsupported | supported inside the generated bundle root | supported inside approved resolution roots | `runtime::tests::basic_invocation::application_node22_resolves_local_esm_packages_from_scoped_node_modules`, `runtime::tests::basic_invocation::application_node22_resolves_package_exports_from_scoped_node_modules`, `runtime::tests::basic_invocation::application_node22_loads_commonjs_package_entries_via_esm_import`, `runtime::tests::basic_invocation::application_node22_loads_implicit_commonjs_packages_with_nested_require` |
| Local CommonJS package entrypoints (`.cjs` and implicit `.js`) via ESM import | unsupported | supported inside the generated bundle root | supported inside approved runtime roots | `runtime::tests::basic_invocation::application_node22_loads_commonjs_package_entries_via_esm_import`, `runtime::tests::basic_invocation::application_node22_loads_implicit_commonjs_packages_with_nested_require` |
| Nested local CommonJS `require(...)` and `require("./data.json")` inside staged packages | unsupported | supported inside the generated bundle root | supported inside approved runtime roots | `runtime::tests::basic_invocation::application_node22_loads_implicit_commonjs_packages_with_nested_require` |
| `node:child_process.spawnSync()` subprocess execution | unsupported | unsupported | supported for exact pre-existing staged binary paths inside approved tooling roots; subprocess env inherits only the explicit JS-visible runtime env | `runtime::tests::basic_invocation::tooling_node22_executes_esbuild_style_staged_binary` |
| `esbuild`-style staged dependency profile (`require("buffer").Buffer`, `node:crypto`, `node:os`, `node:tty`, staged sync subprocess) | unsupported | unsupported | supported for staged local package binaries inside approved tooling roots | `runtime::tests::basic_invocation::tooling_node22_executes_esbuild_style_staged_binary` |

## Explicitly Unsupported Right Now

These surfaces are not yet part of the runtime contract:

- ambient/global `require`
- `require(...)` of ESM targets
- most `node:` builtin usage beyond the verified `node:fs/promises`, `node:module`, `node:path`, and tooling-profile `node:child_process` / `buffer` / `crypto` / `os` / `tty` surfaces
- `node:worker_threads`
- Node-API addon loading
- Node inspector and broader worker-thread APIs

Until a fixture lands, treat them as unsupported even if a transitive runtime
dependency appears to expose pieces of them upstream.

## Notes

- The runtime only reads or writes pre-existing local artifacts inside the
  approved roots above. It never fetches packages from the network or
  materializes `node_modules` at invocation time; CLI-owned staging remains the
  only place where acquisition side effects are allowed.
- The verified CommonJS bridge is intentionally scoped: it uses
  `node:module.createRequire()` plus local staged artifacts inside approved
  runtime roots. It is not a claim of general Node builtin parity.
- The checked-in `esbuild` package is still not a Node-API case in this repo.
  The verified tooling-profile path is a staged JavaScript package plus a
  staged platform binary, not a claim that Neovex now supports general native
  addon loading. Current explicit evidence:
  `runtime::tests::basic_invocation::tooling_node22_executes_esbuild_style_staged_binary`.
- The tooling-profile subprocess contract is intentionally narrower than
  general Node host access: only exact pre-existing staged binaries under
  approved tooling roots are runnable, and the published `agentstation/deno`
  `v2.7.14-locker.4` family keeps subprocess env inheritance aligned with the
  explicit JS-visible runtime env instead of re-merging hidden host env.
- `RuntimeProfile::Tooling` is restricted to `Node22` in code today.
- `RuntimeProfile` is a compatibility/capability axis, not a scheduling axis.
  `RuntimeExecutionModel` remains separate.
