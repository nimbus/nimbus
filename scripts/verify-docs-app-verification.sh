#!/usr/bin/env bash
# Aggregate source contract for documentation and application verification.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEFAULT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
ROOT="${NIMBUS_AVR_ROOT:-${DEFAULT_ROOT}}"
APP_CONTRACT="${ROOT}/scripts/examples-verify-contract-test.sh"
NETWORK_PLAN_REL="docs/private/plans/nimbus-network-control-plane-"'plan.md'

pass_count=0
fail_count=0
baseline_mode=0

has() {
  local path="$1" pattern="$2"
  [ -f "${ROOT}/${path}" ] && grep -Eq -- "${pattern}" "${ROOT}/${path}"
}

lacks() {
  local path="$1" pattern="$2"
  [ -f "${ROOT}/${path}" ] && ! grep -Eq -- "${pattern}" "${ROOT}/${path}"
}

no_executable_plan_readers() {
  local matches
  matches="$(grep -RIl --include='*.sh' --include='*.mjs' --include='*.js' --include='*.py' --include='Makefile' "${NETWORK_PLAN_REL}" "${ROOT}/scripts" "${ROOT}/crates" "${ROOT}/tests" "${ROOT}/.github" "${ROOT}/Makefile" 2>/dev/null || true)"
  [ -z "${matches}" ]
}

network_contract_valid() {
  node - "${ROOT}/scripts/nimbus-network-control-plane/verification-contract.json" <<'NODE'
const fs = require("fs");
const input = process.argv[2];
try {
  const contract = JSON.parse(fs.readFileSync(input, "utf8"));
  process.exit(
    contract.schemaVersion === 1 &&
      contract.status === "complete" &&
      contract.archivedPlan ===
        "docs/private/plans/archive/nimbus-network-control-plane-plan.md" &&
      /^[0-9a-f]{40}$/u.test(contract.completionCheckpoint ?? "") &&
      /^[0-9a-f]{40}$/u.test(contract.itemCheckpoints?.["NNC6.5"] ?? "")
      ? 0
      : 1,
  );
} catch {
  process.exit(1);
}
NODE
}

public_private_fence_clean() {
  node - "${ROOT}" <<'NODE'
const fs = require("fs");
const path = require("path");

const root = path.resolve(process.argv[2]);
const privateRoot = path.join(root, "docs", "private");
const publicRoots = [
  "get-started",
  "developers",
  "agents",
  "operators",
  "concepts",
  "reference",
].map((name) => path.join(root, "docs", name));

function markdownFiles(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const candidate = path.join(directory, entry.name);
    if (entry.isDirectory()) return markdownFiles(candidate);
    return entry.isFile() && entry.name.endsWith(".md") ? [candidate] : [];
  });
}

function resolvesIntoPrivate(file, rawTarget) {
  const targetWithoutSuffix = rawTarget.split(/[?#]/u, 1)[0];
  if (!targetWithoutSuffix || /^[a-z][a-z0-9+.-]*:/iu.test(targetWithoutSuffix)) {
    return false;
  }

  let target;
  try {
    target = decodeURIComponent(targetWithoutSuffix).replaceAll("\\", "/");
  } catch {
    target = targetWithoutSuffix.replaceAll("\\", "/");
  }
  if (target.startsWith("//")) return false;
  if (/^\/?docs\/private(?:\/|$)/u.test(target)) return true;

  const resolved = target.startsWith("/")
    ? path.resolve(root, target.slice(1))
    : path.resolve(path.dirname(file), target);
  return resolved === privateRoot || resolved.startsWith(`${privateRoot}${path.sep}`);
}

const inlineLink = /!?\[[^\]]*\]\(\s*(?:<([^>\n]+)>|([^\s)]+))/gu;
const referenceLink = /^\s*\[[^\]]+\]:\s*(?:<([^>\n]+)>|(\S+))/gmu;
for (const file of publicRoots.flatMap(markdownFiles)) {
  const markdown = fs.readFileSync(file, "utf8");
  for (const pattern of [inlineLink, referenceLink]) {
    pattern.lastIndex = 0;
    for (const match of markdown.matchAll(pattern)) {
      const target = match[1] ?? match[2];
      if (resolvesIntoPrivate(file, target)) {
        console.error(`${path.relative(root, file)} links into private docs: ${target}`);
        process.exit(1);
      }
    }
  }
}
NODE
}

evaluate_condition() {
  local id="$1"
  case "${id}" in
    AVRC01)
      no_executable_plan_readers
      ;;
    AVRC02)
      network_contract_valid &&
        has scripts/verify-nimbus-network-control-plane.sh 'verification-contract\.json'
      ;;
    AVRC03)
      has docs/private/plans/archive/nimbus-network-control-plane-plan.md "Status: \`complete; NNC0-NNC9 done\`" &&
        lacks docs/private/architecture/network/control-plane.md 'NNC9 closeout is active'
      ;;
    AVRC04)
      [ ! -e "${ROOT}/${NETWORK_PLAN_REL}" ] &&
        [ -f "${ROOT}/docs/private/plans/archive/nimbus-network-control-plane-plan.md" ] &&
        has docs/private/plans/README.md 'archive/nimbus-network-control-plane-plan\.md'
      ;;
    AVRC05)
      [ -f "${ROOT}/docs/concepts/architecture/network-control-plane.md" ]
      ;;
    AVRC06)
      [ "$(find "${ROOT}/docs/concepts/architecture" -maxdepth 1 -type f -name '*.md' | wc -l | tr -d ' ')" -eq 14 ] &&
        has docs/concepts/architecture/index.md 'thirteen pages'
      ;;
    AVRC07)
      has docs/source-map.md 'concepts/architecture/network-control-plane\.md' &&
        has docs/source-map.md 'crates/nimbus-network'
      ;;
    AVRC08)
      has docs/concepts/architecture/network-control-plane.md 'transport-free' &&
        has docs/concepts/architecture/network-control-plane.md 'cluster transport' &&
        has docs/concepts/architecture/network-control-plane.md 'concrete effects'
      ;;
    AVRC09)
      has docs/concepts/architecture/network-control-plane.md 'desired' &&
        has docs/concepts/architecture/network-control-plane.md 'durable' &&
        has docs/concepts/architecture/network-control-plane.md 'observed'
      ;;
    AVRC10)
      has docs/concepts/how-nimbus-works.md '/concepts/architecture/network-control-plane/' &&
        has docs/concepts/architecture/sandbox-machines.md '/concepts/architecture/network-control-plane/' &&
        has docs/concepts/architecture/server-transport.md '/concepts/architecture/network-control-plane/' &&
        public_private_fence_clean
      ;;
    AVRC11|AVRC12|AVRC13|AVRC14|AVRC15|AVRC16|AVRC17|AVRC18|AVRC19|AVRC20|AVRC21|AVRC22|AVRC23|AVRC24)
      bash "${APP_CONTRACT}" --condition "${id}" >/dev/null
      ;;
    *)
      return 2
      ;;
  esac
}

owner_for() {
  case "$1" in
    AVRC01|AVRC02|AVRC03|AVRC04) printf 'AVR1\n' ;;
    AVRC05|AVRC06|AVRC07|AVRC08|AVRC09|AVRC10) printf 'AVR2\n' ;;
    AVRC11|AVRC12) printf 'AVR3\n' ;;
    AVRC13|AVRC14|AVRC15) printf 'AVR4\n' ;;
    AVRC16|AVRC17) printf 'AVR5\n' ;;
    AVRC18) printf 'AVR6\n' ;;
    AVRC19|AVRC20) printf 'AVR7\n' ;;
    AVRC21|AVRC22) printf 'AVR8\n' ;;
    AVRC23) printf 'AVR9\n' ;;
    AVRC24) printf 'AVR10\n' ;;
    *) return 1 ;;
  esac
}

phase_for() {
  case "$1" in
    AVRC01|AVRC02|AVRC03|AVRC04|AVRC05|AVRC06|AVRC07|AVRC08|AVRC09|AVRC10) printf '1\n' ;;
    AVRC11|AVRC12|AVRC13|AVRC14|AVRC15|AVRC16|AVRC17|AVRC18|AVRC19|AVRC20) printf '2\n' ;;
    AVRC21|AVRC22|AVRC23|AVRC24) printf '3\n' ;;
    *) return 1 ;;
  esac
}

write_green_fixture() {
  local id="$1" root="$2"
  mkdir -p "${root}/scripts/nimbus-network-control-plane" \
    "${root}/crates" "${root}/tests" "${root}/.github" \
    "${root}/docs/private/plans/archive" "${root}/docs/private/architecture/network" \
    "${root}/docs/concepts/architecture" "${root}/docs/get-started" \
    "${root}/docs/developers" "${root}/docs/agents" "${root}/docs/operators" \
    "${root}/docs/concepts" "${root}/docs/reference"
  : >"${root}/Makefile"
  case "${id}" in
    AVRC01)
      printf '%s\n' '# stable verifier input' >"${root}/scripts/safe.sh"
      ;;
    AVRC02)
      printf '%s\n' '{"schemaVersion":1,"status":"complete","archivedPlan":"docs/private/plans/archive/nimbus-network-control-plane-plan.md","completionCheckpoint":"1111111111111111111111111111111111111111","itemCheckpoints":{"NNC6.5":"2222222222222222222222222222222222222222"}}' >"${root}/scripts/nimbus-network-control-plane/verification-contract.json"
      printf '%s\n' 'scripts/nimbus-network-control-plane/verification-contract.json' >"${root}/scripts/verify-nimbus-network-control-plane.sh"
      ;;
    AVRC03)
      printf '%s\n' "Status: \`complete; NNC0-NNC9 done\`" >"${root}/docs/private/plans/archive/nimbus-network-control-plane-plan.md"
      printf '%s\n' 'Status: landed architecture; implementation merged.' >"${root}/docs/private/architecture/network/control-plane.md"
      ;;
    AVRC04)
      printf '%s\n' '# archived network plan' >"${root}/docs/private/plans/archive/nimbus-network-control-plane-plan.md"
      printf '%s\n' 'archive/nimbus-network-control-plane-plan.md' >"${root}/docs/private/plans/README.md"
      ;;
    AVRC05)
      printf '%s\n' '# Network control plane' >"${root}/docs/concepts/architecture/network-control-plane.md"
      ;;
    AVRC06)
      printf '%s\n' 'thirteen pages' >"${root}/docs/concepts/architecture/index.md"
      for number in 1 2 3 4 5 6 7 8 9 10 11 12 13; do
        printf '# page\n' >"${root}/docs/concepts/architecture/page-${number}.md"
      done
      ;;
    AVRC07)
      printf '%s\n' '| concepts/architecture/network-control-plane.md | crates/nimbus-network |' >"${root}/docs/source-map.md"
      ;;
    AVRC08)
      printf '%s\n' 'transport-free control plane; cluster transport is separate; concrete effects stay with providers' >"${root}/docs/concepts/architecture/network-control-plane.md"
      ;;
    AVRC09)
      printf '%s\n' 'desired generation, durable lease, observed status' >"${root}/docs/concepts/architecture/network-control-plane.md"
      ;;
    AVRC10)
      printf '%s\n' '[network](/concepts/architecture/network-control-plane/)' >"${root}/docs/concepts/how-nimbus-works.md"
      printf '%s\n' '[network](/concepts/architecture/network-control-plane/)' >"${root}/docs/concepts/architecture/sandbox-machines.md"
      printf '%s\n' '[network](/concepts/architecture/network-control-plane/)' >"${root}/docs/concepts/architecture/server-transport.md"
      ;;
  esac
}

mutate_fixture() {
  local id="$1" root="$2"
  case "${id}" in
    AVRC01) printf '%s\n' "${NETWORK_PLAN_REL}" >"${root}/scripts/safe.sh" ;;
    AVRC02) rm "${root}/scripts/nimbus-network-control-plane/verification-contract.json" ;;
    AVRC03) printf '%s\n' 'Status: landed architecture; NNC9 closeout is active.' >"${root}/docs/private/architecture/network/control-plane.md" ;;
    AVRC04) printf '%s\n' '# plan returned to active tree' >"${root}/${NETWORK_PLAN_REL}" ;;
    AVRC05) rm "${root}/docs/concepts/architecture/network-control-plane.md" ;;
    AVRC06) rm "${root}/docs/concepts/architecture/page-13.md" ;;
    AVRC07) printf '%s\n' '# no source-map row' >"${root}/docs/source-map.md" ;;
    AVRC08) printf '%s\n' 'network owns all transport and effects' >"${root}/docs/concepts/architecture/network-control-plane.md" ;;
    AVRC09) printf '%s\n' 'network state' >"${root}/docs/concepts/architecture/network-control-plane.md" ;;
    AVRC10) printf '%s\n' '[private](../private/secret.md)' >>"${root}/docs/concepts/how-nimbus-works.md" ;;
  esac
}

run_condition() {
  local id="$1"
  if evaluate_condition "${id}"; then
    printf 'PASS %s (%s)\n' "${id}" "$(owner_for "${id}")"
    pass_count=$((pass_count + 1))
  else
    printf 'FAIL %s (%s)\n' "${id}" "$(owner_for "${id}")"
    fail_count=$((fail_count + 1))
  fi
}

self_test_docs_condition() {
  local id="$1" tmp old_root old_contract
  tmp="$(mktemp -d -t nimbus-avr-docs.XXXXXX)"
  old_root="${ROOT}"
  old_contract="${APP_CONTRACT}"
  ROOT="${tmp}"
  APP_CONTRACT="${tmp}/scripts/examples-verify-contract-test.sh"
  write_green_fixture "${id}" "${tmp}"
  if ! evaluate_condition "${id}"; then
    printf 'self-test %s: green fixture did not pass\n' "${id}" >&2
    ROOT="${old_root}"
    APP_CONTRACT="${old_contract}"
    rm -rf "${tmp}"
    return 1
  fi
  mutate_fixture "${id}" "${tmp}"
  if evaluate_condition "${id}"; then
    printf 'self-test %s: mutation did not fail closed\n' "${id}" >&2
    ROOT="${old_root}"
    APP_CONTRACT="${old_contract}"
    rm -rf "${tmp}"
    return 1
  fi
  ROOT="${old_root}"
  APP_CONTRACT="${old_contract}"
  rm -rf "${tmp}"
  return 0
}

run_self_test() {
  local id passed
  passed=0
  for id in AVRC01 AVRC02 AVRC03 AVRC04 AVRC05 AVRC06 AVRC07 AVRC08 AVRC09 AVRC10; do
    self_test_docs_condition "${id}"
    printf 'PASS self-test %s\n' "${id}"
    passed=$((passed + 1))
  done
  for id in AVRC11 AVRC12 AVRC13 AVRC14 AVRC15 AVRC16 AVRC17 AVRC18 AVRC19 AVRC20 AVRC21 AVRC22 AVRC23 AVRC24; do
    bash "${APP_CONTRACT}" --self-test-condition "${id}" >/dev/null
    printf 'PASS self-test %s\n' "${id}"
    passed=$((passed + 1))
  done
  printf 'Mutation summary: %d/24\n' "${passed}"
  [ "${passed}" -eq 24 ]
}

usage() {
  printf 'usage: %s --baseline | --self-test | --task AVR1..AVR10 | --through-phase 1..3\n' "$0" >&2
}

mode="${1:-}"
selector="${2:-}"
case "${mode}" in
  --baseline)
    [ "$#" -eq 1 ] || { usage; exit 2; }
    baseline_mode=1
    ;;
  --self-test)
    [ "$#" -eq 1 ] || { usage; exit 2; }
    run_self_test
    exit 0
    ;;
  --task)
    [ "$#" -eq 2 ] || { usage; exit 2; }
    ;;
  --through-phase)
    [ "$#" -eq 2 ] || { usage; exit 2; }
    case "${selector}" in 1|2|3) ;; *) usage; exit 2 ;; esac
    ;;
  *)
    usage
    exit 2
    ;;
esac

selected=0
for id in AVRC01 AVRC02 AVRC03 AVRC04 AVRC05 AVRC06 AVRC07 AVRC08 AVRC09 AVRC10 AVRC11 AVRC12 AVRC13 AVRC14 AVRC15 AVRC16 AVRC17 AVRC18 AVRC19 AVRC20 AVRC21 AVRC22 AVRC23 AVRC24; do
  include=0
  case "${mode}" in
    --baseline) include=1 ;;
    --task) [ "$(owner_for "${id}")" = "${selector}" ] && include=1 ;;
    --through-phase) [ "$(phase_for "${id}")" -le "${selector}" ] && include=1 ;;
  esac
  if [ "${include}" -eq 1 ]; then
    run_condition "${id}"
    selected=$((selected + 1))
  fi
done

if [ "${selected}" -eq 0 ]; then
  printf 'no conditions selected for %s %s\n' "${mode}" "${selector}" >&2
  exit 2
fi

printf 'Summary: %d passed, %d failed\n' "${pass_count}" "${fail_count}"
if [ "${baseline_mode}" -eq 1 ]; then
  exit 0
fi
[ "${fail_count}" -eq 0 ]
