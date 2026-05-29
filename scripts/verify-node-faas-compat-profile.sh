#!/usr/bin/env bash
set -euo pipefail

python3 scripts/runtime/node/faas_profile.py validate
python3 scripts/runtime/node/faas_profile.py self-test
