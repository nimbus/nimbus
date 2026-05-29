#!/usr/bin/env bash
set -euo pipefail

python3 scripts/runtime/node/latest_suite_tags.py validate
python3 scripts/runtime/node/latest_suite_tags.py self-test

if [[ "${NIMBUS_ENFORCE_CURRENT_NODE_CORPORA:-0}" == "1" ]]; then
  python3 scripts/runtime/node/latest_suite_tags.py enforce-current-corpora
fi
