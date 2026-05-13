# Embedded Node Builtin Sources

These files are JavaScript sources that the Rust module loader embeds with
`include_str!` for Nimbus-owned Node compatibility shims. Keep behavioral
changes in the source file that owns the affected builtin surface, and keep the
Rust loader focused on resolution, permissions, and cache orchestration.

The `module_*` files are concatenated in order by `embedded_builtins.rs` to form
the `node:nimbus/module` shim:

1. `module_prelude.js`
2. `module_fs_helpers.js`
3. `module_fs_modules.js`
4. `module_wiring.js`
