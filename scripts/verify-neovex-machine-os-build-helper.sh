#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temp_dir="$(mktemp -d)"
trap 'rm -rf "${temp_dir}"' EXIT

fake_bin="${temp_dir}/bin"
mkdir -p "${fake_bin}"

cat >"${fake_bin}/cargo" <<'EOF'
#!/bin/bash
set -euo pipefail
printf '%s\n' "$*" >>"${TMPDIR}/cargo.log"
if [[ "$*" == *"--release"* ]]; then
  mkdir -p "${PWD}/target/release"
  printf '#!/usr/bin/env bash\nexit 0\n' >"${PWD}/target/release/neovex"
  chmod 0755 "${PWD}/target/release/neovex"
else
  mkdir -p "${PWD}/target/debug"
  printf '#!/usr/bin/env bash\nexit 0\n' >"${PWD}/target/debug/neovex"
  chmod 0755 "${PWD}/target/debug/neovex"
fi
EOF

cat >"${fake_bin}/bash" <<'EOF'
#!/bin/bash
set -euo pipefail
if [[ "${1:-}" == *"images/neovex-machine-os/build.sh" ]]; then
  shift
  printf '%s\n' "$*" >>"${TMPDIR}/recipe.log"
  exit 0
fi
exec /bin/bash "$@"
EOF

chmod 0755 "${fake_bin}/cargo" "${fake_bin}/bash"

PATH="${fake_bin}:${PATH}" \
TMPDIR="${temp_dir}" \
NEOVEX_MACHINE_OS_BUILD_WRAPPER_TEST_UNAME=Linux \
bash "${repo_root}/scripts/build-neovex-machine-os.sh" \
  --cargo-profile release \
  --output-dir /tmp/neovex-machine-os-out \
  --custom-coreos-disk-images /tmp/custom-coreos-disk-images.sh

grep -F -- 'build --release -p neovex-bin' "${temp_dir}/cargo.log" >/dev/null
grep -F -- '--neovex-binary' "${temp_dir}/recipe.log" >/dev/null
grep -F -- '--output-dir /tmp/neovex-machine-os-out' "${temp_dir}/recipe.log" >/dev/null
grep -F -- '--custom-coreos-disk-images /tmp/custom-coreos-disk-images.sh' "${temp_dir}/recipe.log" >/dev/null
grep -F -- "${repo_root}/target/release/neovex" "${temp_dir}/recipe.log" >/dev/null

printf 'verified neovex machine-os build wrapper\n'
