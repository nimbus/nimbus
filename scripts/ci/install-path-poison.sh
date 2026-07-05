#!/usr/bin/env bash

set -euo pipefail

if [[ "$#" -eq 0 ]]; then
  echo "usage: install-path-poison.sh <tool> [<tool>...]" >&2
  exit 2
fi
if [[ -z "${GITHUB_PATH:-}" ]]; then
  echo "GITHUB_PATH must be set so PATH poison applies to later workflow steps" >&2
  exit 2
fi

shim_dir="${RUNNER_TEMP:-/tmp}/nimbus-path-poison"
rm -rf "${shim_dir}"
mkdir -p "${shim_dir}"

for tool in "$@"; do
  case "${tool}" in
    */*|"")
      printf 'invalid tool name for PATH poison: %s\n' "${tool}" >&2
      exit 2
      ;;
  esac
  cat > "${shim_dir}/${tool}" <<'EOF'
#!/usr/bin/env bash
tool="$(basename "$0")"
echo "::error::PATH poison blocked unexpected ${tool} invocation in archive-consumer lane" >&2
exit 127
EOF
  chmod +x "${shim_dir}/${tool}"
done

printf '%s\n' "${shim_dir}" >> "${GITHUB_PATH}"
printf 'installed PATH poison shims in %s for: %s\n' "${shim_dir}" "$*"
