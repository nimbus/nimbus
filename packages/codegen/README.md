# @nimbus/codegen

Code generation for Nimbus applications. Generates TypeScript types and runtime
artifacts from your schema and function definitions.

`@nimbus/codegen` is a private, internal package. It is **embedded in the
`nimbus` binary** and run from there — it is not published to npm, not installed
into your app, and not invoked directly.

## Usage

Run codegen through the Nimbus CLI:

```bash
nimbus codegen --app .
```

`nimbus dev` also runs a codegen pass before starting the local server. The
entire default Convex authoring surface — schema, server, http, and
`auth.config.{ts,js}` — is generated in-binary (the binary's embedded V8 tooling
runtime), so no external Node.js toolchain or `node_modules/@nimbus/codegen` is
required. Cloud Functions is the one out-of-contract surface and runs codegen on
an external Node.js runner, but that runner still executes the
binary-materialized embedded codegen bundle rather than an app-installed
`@nimbus/codegen` package; see
[the CLI and codegen architecture page](../../docs/concepts/architecture/cli-codegen.md)
for the runner contract.

## Documentation

See the [main Nimbus documentation](../../README.md) for complete usage
instructions.

## License

See [LICENSE](../../LICENSE) in the repository root.
