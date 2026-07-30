#!/usr/bin/env bash
# Host-independent mutation suite for the NNC4.7 privileged proof adapter.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${REPO_ROOT}"
exec /usr/bin/env python3 scripts/nimbus-network-control-plane/sovereignty-tripwire-self-tests.py
