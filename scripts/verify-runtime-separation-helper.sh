#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-runtime-separation-verify.XXXXXX")"
trap 'rm -rf "${tmp_dir}"' EXIT

bin_dir="${tmp_dir}/bin"
private_dir="${tmp_dir}/private"
shared_dir="${tmp_dir}/shared"
output_file="${tmp_dir}/output.txt"
collision_output_file="${tmp_dir}/collision-output.txt"
private_runtime_realpath="$(python3 - "${private_dir}/crun" <<'PY'
import os
import sys

print(os.path.realpath(sys.argv[1]))
PY
)"

mkdir -p "${bin_dir}" "${private_dir}" "${shared_dir}"

cat > "${bin_dir}/crun" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == "--version" ]]; then
  echo "crun version 1.22-system"
  exit 0
fi
echo "unexpected args for fake crun: $*" >&2
exit 64
EOF

cat > "${private_dir}/crun" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == "--version" ]]; then
  echo "crun version 1.22-nimbus"
  exit 0
fi
echo "unexpected args for fake private crun: $*" >&2
exit 64
EOF

cat > "${bin_dir}/podman" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == "--version" ]]; then
  echo "podman version 5.8.1"
  exit 0
fi
if [[ "${1:-}" == "info" && "${2:-}" == "--format" ]]; then
  echo "crun /usr/bin/crun"
  exit 0
fi
echo "unexpected args for fake podman: $*" >&2
exit 64
EOF

chmod +x "${bin_dir}/crun" "${private_dir}/crun" "${bin_dir}/podman"

bash "${repo_root}/scripts/verify-runtime-separation.sh" \
  --system-runtime "${bin_dir}/crun" \
  --private-runtime "${private_dir}/crun" \
  --podman "${bin_dir}/podman" \
  > "${output_file}"

grep -F "system.runtime.path          ${bin_dir}/crun" "${output_file}" >/dev/null
grep -F "system.runtime.version       crun version 1.22-system" "${output_file}" >/dev/null
grep -F "private.runtime.path         ${private_dir}/crun" "${output_file}" >/dev/null
grep -F "private.runtime.version      crun version 1.22-nimbus" "${output_file}" >/dev/null
grep -F "podman.path                  ${bin_dir}/podman" "${output_file}" >/dev/null
grep -F "podman.version               podman version 5.8.1" "${output_file}" >/dev/null
grep -F "podman.runtime               crun /usr/bin/crun" "${output_file}" >/dev/null
grep -F "podman.runtime.path          /usr/bin/crun" "${output_file}" >/dev/null
grep -F "runtime.separation           ok" "${output_file}" >/dev/null
grep -F "podman.runtime.separation    ok" "${output_file}" >/dev/null
grep -F "result                       separate" "${output_file}" >/dev/null

ln -s "${private_dir}/crun" "${shared_dir}/private-runtime-link"

cat > "${bin_dir}/podman-private" <<EOF
#!/usr/bin/env bash
set -euo pipefail
if [[ "\${1:-}" == "--version" ]]; then
  echo "podman version 5.8.1"
  exit 0
fi
if [[ "\${1:-}" == "info" && "\${2:-}" == "--format" ]]; then
  echo "crun ${shared_dir}/private-runtime-link"
  exit 0
fi
echo "unexpected args for fake collision podman: \$*" >&2
exit 64
EOF

chmod +x "${bin_dir}/podman-private"

if bash "${repo_root}/scripts/verify-runtime-separation.sh" \
  --system-runtime "${bin_dir}/crun" \
  --private-runtime "${private_dir}/crun" \
  --podman "${bin_dir}/podman-private" \
  > "${collision_output_file}"; then
  echo "expected Podman/private runtime collision to fail" >&2
  exit 1
fi

grep -F "podman.runtime.path          ${shared_dir}/private-runtime-link" "${collision_output_file}" >/dev/null
grep -F "podman.runtime.realpath      ${private_runtime_realpath}" "${collision_output_file}" >/dev/null
grep -F "podman.runtime.separation    failed (Podman points at the private nimbus runtime)" "${collision_output_file}" >/dev/null
grep -F "result                       not-separate (1 failing checks)" "${collision_output_file}" >/dev/null

echo "verified: runtime separation helper distinguishes separated and colliding runtimes"
