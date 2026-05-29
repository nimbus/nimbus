# Host-Heavy Node Diagnostic Canaries

These fixtures prove that production in-process Node application profiles do
not silently grant host-heavy behavior. The canaries pass only when the runtime
returns an actionable denial or service/microVM boundary diagnostic for the
selected surface.

Run through the registry:

```bash
make node-compat-canaries PRESET=application
```

Focused lanes:

```bash
cargo test -p nimbus-runtime application_node22_host_heavy_diagnostic_canary_batch -- --nocapture --test-threads=1 --ignored
cargo test -p nimbus-runtime application_node24_host_heavy_diagnostic_canary_batch -- --nocapture --test-threads=1 --ignored
cargo test -p nimbus-runtime application_node26_host_heavy_diagnostic_canary_batch -- --nocapture --test-threads=1 --ignored
```
