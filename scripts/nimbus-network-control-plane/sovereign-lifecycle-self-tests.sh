#!/bin/bash
set -euo pipefail

exec python3 scripts/nimbus-network-control-plane/sovereign-lifecycle-self-tests.py "$@"
