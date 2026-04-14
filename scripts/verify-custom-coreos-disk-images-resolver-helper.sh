#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temp_dir="$(mktemp -d)"
trap 'rm -rf "${temp_dir}"' EXIT

fake_bin="${temp_dir}/bin"
checkout_dir="${temp_dir}/checkout"
mkdir -p "${fake_bin}"

cat >"${fake_bin}/git" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"${TMPDIR}/git.log"
if [[ "${1:-}" == "clone" ]]; then
  target="${@: -1}"
  mkdir -p "${target}/.git"
  cat >"${target}/custom-coreos-disk-images.sh" <<'SCRIPT'
#!/usr/bin/env bash
exit 0
SCRIPT
  chmod 0755 "${target}/custom-coreos-disk-images.sh"
fi
exit 0
EOF
chmod 0755 "${fake_bin}/git"

PATH="${fake_bin}:${PATH}" \
TMPDIR="${temp_dir}" \
bash "${repo_root}/scripts/resolve-custom-coreos-disk-images.sh" \
  --checkout-dir "${checkout_dir}" >"${temp_dir}/resolved-path.txt"

resolved_path="$(cat "${temp_dir}/resolved-path.txt")"
test -x "${resolved_path}"
grep -F -- "clone https://github.com/coreos/custom-coreos-disk-images.git ${checkout_dir}" "${temp_dir}/git.log" >/dev/null
grep -F -- "fetch --depth 1 origin e017ddda3b20b09627f90f68ef1b708016d10864" "${temp_dir}/git.log" >/dev/null
grep -F -- "checkout --detach e017ddda3b20b09627f90f68ef1b708016d10864" "${temp_dir}/git.log" >/dev/null

printf 'verified custom-coreos-disk-images resolver\n'
