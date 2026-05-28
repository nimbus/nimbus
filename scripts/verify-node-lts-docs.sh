#!/usr/bin/env bash

set -euo pipefail

python3 scripts/runtime/node/publish_docs.py --check
python3 scripts/runtime/node/docs_guard.py
