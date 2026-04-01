# CLI Reference

Neovex runs as a single server process:

```bash
neovex [flags]
```

For local development from source:

```bash
cargo run -p neovex-bin -- [flags]
```

## Core Flags

| Flag | Default | Meaning |
| --- | --- | --- |
| `--port` | `8080` | port to listen on |
| `--data-dir` | `./data` | data directory for tenant databases |
| `--convex-app-dir` | unset | optional app directory containing a generated `.neovex/convex/functions.json` manifest |
| `--license-file` | unset | optional explicit path to a Neovex license file |

If `--license-file` is not provided, Neovex next checks
`NEOVEX_LICENSE_FILE`, then `./.neovex/license.json`, and otherwise falls back
to the built-in community license.

## Runtime Flags

| Flag | Default | Meaning |
| --- | --- | --- |
| `--runtime-heap-mb` | `128` | V8 heap limit per isolate in megabytes |
| `--runtime-initial-heap-mb` | `8` | initial V8 heap size per isolate in megabytes |
| `--runtime-timeout-secs` | `30` | maximum wall-clock runtime invocation time |
| `--runtime-max-isolates` | available hardware parallelism | maximum concurrent top-level runtime isolates |
| `--runtime-max-nested-calls` | `64` | maximum nested `ctx.run*` invocations per request tree |

## Startup Behavior

On startup, the server:

- initializes tracing
- loads the service with the configured data directory
- loads tenants with scheduled work
- starts the scheduler loop
- optionally loads the Convex registry from `--convex-app-dir`
- loads license state from the explicit path, environment, default path, or
  built-in community defaults

Related references:

- [HTTP and WebSocket API](http-api.md)
- [Convex compatibility](../convex/compatibility.md)
