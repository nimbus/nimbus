#!/usr/bin/env bash
set -euo pipefail

cargo test -p nimbus-runtime node_compat_harness -- --nocapture
python3 scripts/runtime/node/watchpoints.py validate
python3 scripts/runtime/node/classifications.py sync --preserve-existing --check
python3 scripts/runtime/node/status.py --output-root target/node-compat/harness-hardening-status
