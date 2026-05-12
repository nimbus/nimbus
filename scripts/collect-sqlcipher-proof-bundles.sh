#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: collect-sqlcipher-proof-bundles.sh --run-id <id> [options]

Download the uploaded SQLCipher proof artifacts from a GitHub Actions workflow
run and write a small local summary bundle that can be cited from the archived
encryption-at-rest plan closeout record.

options:
  --run-id <id>                  GitHub Actions workflow run id
  --repo <owner/name>            GitHub repository (default: agentstation/neovex)
  --output-dir <path>            Output directory (default: mktemp under TMPDIR)
  --artifact-prefix <prefix>     Artifact prefix to collect
                                 (default: sqlcipher-proof- and
                                 sqlcipher-package-proof-)
  -h, --help                     Show this help

examples:
  bash scripts/collect-sqlcipher-proof-bundles.sh \
    --run-id 12345678901

  bash scripts/collect-sqlcipher-proof-bundles.sh \
    --run-id 12345678901 \
    --artifact-prefix sqlcipher-proof- \
    --output-dir /tmp/neovex-sqlcipher-release-proof
EOF
}

require_command() {
  local command_name="$1"
  if ! command -v "${command_name}" >/dev/null 2>&1; then
    echo "required command not found: ${command_name}" >&2
    exit 127
  fi
}

run_id=""
repo="agentstation/neovex"
output_dir=""
artifact_prefixes=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --run-id)
      run_id="${2:?missing run id}"
      shift 2
      ;;
    --repo)
      repo="${2:?missing repo}"
      shift 2
      ;;
    --output-dir)
      output_dir="${2:?missing output dir}"
      shift 2
      ;;
    --artifact-prefix)
      artifact_prefixes+=("${2:?missing artifact prefix}")
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 64
      ;;
  esac
done

if [[ -z "${run_id}" ]]; then
  echo "--run-id is required" >&2
  usage >&2
  exit 64
fi

if [[ ${#artifact_prefixes[@]} -eq 0 ]]; then
  artifact_prefixes=("sqlcipher-proof-" "sqlcipher-package-proof-")
fi

require_command gh
require_command python3

if [[ -z "${output_dir}" ]]; then
  output_dir="$(mktemp -d "${TMPDIR:-/tmp}/neovex-sqlcipher-proof-bundles.XXXXXX")"
else
  mkdir -p "${output_dir}"
fi
output_dir="$(cd "${output_dir}" && pwd)"

summary_file="${output_dir}/summary.txt"
artifacts_json="${output_dir}/artifacts.json"
artifacts_list="${output_dir}/artifact-names.txt"

{
  echo "repo=${repo}"
  echo "run_id=${run_id}"
  printf 'artifact_prefixes='
  printf '%s ' "${artifact_prefixes[@]}"
  printf '\n'
  echo "output_dir=${output_dir}"
} > "${summary_file}"

gh api \
  -H "Accept: application/vnd.github+json" \
  "/repos/${repo}/actions/runs/${run_id}/artifacts?per_page=100" \
  > "${artifacts_json}"

python3 - "${artifacts_json}" "${artifacts_list}" "${artifact_prefixes[@]}" <<'PY'
import json
import sys
from pathlib import Path

artifacts_path = Path(sys.argv[1])
output_path = Path(sys.argv[2])
prefixes = sys.argv[3:]

payload = json.loads(artifacts_path.read_text())
names = []
for artifact in payload.get("artifacts", []):
    name = artifact.get("name")
    if name and any(name.startswith(prefix) for prefix in prefixes):
        names.append(name)

if not names:
    raise SystemExit("no matching SQLCipher proof artifacts found on the selected run")

output_path.write_text("".join(f"{name}\n" for name in names))
PY

artifact_count=0
while IFS= read -r artifact_name; do
  [[ -n "${artifact_name}" ]] || continue
  artifact_count=$((artifact_count + 1))
  artifact_dir="${output_dir}/${artifact_name}"
  mkdir -p "${artifact_dir}"
  gh run download "${run_id}" \
    --repo "${repo}" \
    --name "${artifact_name}" \
    --dir "${artifact_dir}"
done < "${artifacts_list}"

{
  echo "artifact_count=${artifact_count}"
  echo "artifacts:"
  sed 's/^/  - /' "${artifacts_list}"
} >> "${summary_file}"

echo "downloaded ${artifact_count} SQLCipher proof artifact(s) into ${output_dir}"
