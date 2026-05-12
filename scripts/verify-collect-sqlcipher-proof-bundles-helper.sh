#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_base="${TMPDIR:-/tmp}"
tmp_base="${tmp_base%/}"
tmp_dir="$(mktemp -d "${tmp_base}/nimbus-sqlcipher-proof-helper.XXXXXX")"
tmp_dir="$(cd "${tmp_dir}" && pwd)"

cleanup() {
  local status="$1"
  if [[ "${status}" -eq 0 ]]; then
    rm -rf "${tmp_dir}"
  else
    echo "debug: preserved helper tmp dir at ${tmp_dir}" >&2
  fi
}
trap 'cleanup "$?"' EXIT

bin_dir="${tmp_dir}/bin"
output_dir="${tmp_dir}/output"
mkdir -p "${bin_dir}" "${output_dir}"

cat > "${bin_dir}/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == "api" ]]; then
  cat <<'JSON'
{
  "artifacts": [
    { "name": "sqlcipher-proof-x86_64-unknown-linux-gnu" },
    { "name": "sqlcipher-package-proof-amd64" },
    { "name": "unrelated-artifact" }
  ]
}
JSON
  exit 0
fi

if [[ "${1:-}" == "run" && "${2:-}" == "download" ]]; then
  run_id="${3:-}"
  artifact_name=""
  destination=""
  shift 3
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --repo)
        shift 2
        ;;
      --name)
        artifact_name="${2:?missing artifact name}"
        shift 2
        ;;
      --dir)
        destination="${2:?missing destination}"
        shift 2
        ;;
      *)
        echo "unexpected gh run download arg: $1" >&2
        exit 64
        ;;
    esac
  done

  mkdir -p "${destination}"
  printf 'run_id=%s\nartifact=%s\n' "${run_id}" "${artifact_name}" > "${destination}/downloaded.txt"
  exit 0
fi

echo "unexpected gh invocation: $*" >&2
exit 64
EOF
chmod +x "${bin_dir}/gh"

PATH="${bin_dir}:${PATH}" \
  bash "${repo_root}/scripts/collect-sqlcipher-proof-bundles.sh" \
    --run-id 12345678901 \
    --repo nimbus/nimbus \
    --output-dir "${output_dir}"

summary_file="${output_dir}/summary.txt"
artifact_names_file="${output_dir}/artifact-names.txt"
artifacts_json="${output_dir}/artifacts.json"

test -f "${summary_file}"
test -f "${artifact_names_file}"
test -f "${artifacts_json}"

grep -Fq 'repo=nimbus/nimbus' "${summary_file}"
grep -Fq 'run_id=12345678901' "${summary_file}"
grep -Fq 'artifact_count=2' "${summary_file}"
grep -Fq 'sqlcipher-proof-x86_64-unknown-linux-gnu' "${artifact_names_file}"
grep -Fq 'sqlcipher-package-proof-amd64' "${artifact_names_file}"
! grep -Fq 'unrelated-artifact' "${artifact_names_file}"

test -f "${output_dir}/sqlcipher-proof-x86_64-unknown-linux-gnu/downloaded.txt"
test -f "${output_dir}/sqlcipher-package-proof-amd64/downloaded.txt"
grep -Fq 'artifact=sqlcipher-proof-x86_64-unknown-linux-gnu' \
  "${output_dir}/sqlcipher-proof-x86_64-unknown-linux-gnu/downloaded.txt"
grep -Fq 'artifact=sqlcipher-package-proof-amd64' \
  "${output_dir}/sqlcipher-package-proof-amd64/downloaded.txt"

echo "verified: sqlcipher proof bundle collector discovers and downloads matching artifacts"
