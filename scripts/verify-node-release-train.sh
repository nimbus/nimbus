#!/usr/bin/env bash
set -euo pipefail

python3 scripts/runtime/node/release_train.py check
python3 scripts/runtime/node/release_train.py self-test
