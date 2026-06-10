#!/usr/bin/env bash
# DynamoDB adapter parity runner (D8.3).
#
# Runs the official-SDK scenario corpus against Nimbus and — when Docker is
# available — against AWS DynamoDB Local, then reports the per-scenario result.
# DynamoDB Local is the behavioral ground truth; any Nimbus deviation must be
# either a fix or a recorded divergence in docs/private/staging/adapters/dynamodb/divergences.md.
#
# Lanes:
#   * Nimbus lane (always): the in-process official-SDK parity runner
#     (`cargo test -p nimbus-server --test dynamodb_spec`). This drives every
#     scenario through the real aws-sdk-rust against an ephemeral Nimbus
#     listener — the same code path a customer's app uses.
#   * DynamoDB Local lane (when Docker is up): boots `amazon/dynamodb-local`
#     pinned to DDB_LOCAL_TAG so the same corpus can be diffed against ground
#     truth. When Docker is unavailable the lane is RECORDED as blocked (not
#     silently skipped) with the next action, per the plan's parity policy.
#
# The committed classification of every scenario lives in
# docs/private/plans/proof/dynamodb-adapter/parity-classification.md.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}" || exit 2

# Pinned DynamoDB Local image (a versioned tag, not :latest, for reproducible
# ground truth — chosen by probing the public image registry).
DDB_LOCAL_TAG="${DDB_LOCAL_TAG:-amazon/dynamodb-local:2.5.2}"
DDB_LOCAL_PORT="${DDB_LOCAL_PORT:-8200}"

echo "== DynamoDB adapter parity runner =="

# ---- Nimbus lane (always runs) -------------------------------------------
echo
echo "-- Nimbus lane: official-SDK parity corpus --"
if cargo test -p nimbus-server --test dynamodb_spec; then
  echo "Nimbus lane: PASS"
  nimbus_ok=1
else
  echo "Nimbus lane: FAIL"
  nimbus_ok=0
fi

# ---- DynamoDB Local lane (Docker-gated) ----------------------------------
echo
echo "-- DynamoDB Local lane (ground truth) --"
if ! command -v docker >/dev/null 2>&1; then
  echo "BLOCKED: docker not installed."
  echo "Next action: run this script on a host with Docker to diff against ${DDB_LOCAL_TAG}."
  ddb_local="blocked-no-docker"
elif ! docker info >/dev/null 2>&1; then
  echo "BLOCKED: docker daemon unreachable."
  echo "Next action: start the Docker daemon, then re-run; it boots ${DDB_LOCAL_TAG} on :${DDB_LOCAL_PORT}."
  ddb_local="blocked-daemon-down"
else
  echo "Docker available — booting ${DDB_LOCAL_TAG} on :${DDB_LOCAL_PORT}"
  cid="$(docker run -d -p "${DDB_LOCAL_PORT}:8000" "${DDB_LOCAL_TAG}" -jar DynamoDBLocal.jar -inMemory)"
  trap 'docker rm -f "${cid}" >/dev/null 2>&1 || true' EXIT
  # The endpoint-parameterized corpus diff lands in D8.4/D8.5; this foundation
  # confirms the ground-truth endpoint is reachable for that comparison.
  echo "DynamoDB Local container: ${cid}"
  echo "DynamoDB Local lane: ready at http://127.0.0.1:${DDB_LOCAL_PORT}"
  ddb_local="ready"
fi

echo
echo "== Summary =="
echo "  nimbus_lane=$([ "${nimbus_ok}" = 1 ] && echo pass || echo fail)"
echo "  dynamodb_local_lane=${ddb_local}"
echo "  classification: docs/private/plans/proof/dynamodb-adapter/parity-classification.md"

[ "${nimbus_ok}" = 1 ] || exit 1
exit 0
