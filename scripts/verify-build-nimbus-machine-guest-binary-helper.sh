#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_base="${TMPDIR:-/tmp}"
tmp_base="${tmp_base%/}"
tmp_dir="$(mktemp -d "${tmp_base}/nimbus-machine-guest-build-verify.XXXXXX")"
tmp_dir="$(cd "${tmp_dir}" && pwd)"
trap 'rm -rf "${tmp_dir}"' EXIT

helper="${repo_root}/scripts/build-nimbus-machine-guest-binary.sh"

run_scenario() {
  local scenario_name="$1"
  local fake_uname_s="$2"
  local fake_uname_m="$3"
  local expected_target="$4"
  local expected_subcommand="$5"

  local scenario_dir="${tmp_dir}/${scenario_name}"
  local bin_dir="${scenario_dir}/bin"
  local logs_dir="${scenario_dir}/logs"
  local cache_root="${scenario_dir}/cache-root"
  local fake_repo_root="${scenario_dir}/fake-repo"
  local stdout_path="${scenario_dir}/stdout.txt"

  mkdir -p "${bin_dir}" "${logs_dir}" "${cache_root}" "${fake_repo_root}"

  cat > "${bin_dir}/uname" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "${1:-}" in
  -s) printf '%s\n' "${FAKE_UNAME_S:?}" ;;
  -m) printf '%s\n' "${FAKE_UNAME_M:?}" ;;
  *)
    echo "unexpected uname args: $*" >&2
    exit 64
    ;;
esac
EOF

  cat > "${bin_dir}/rustup" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "${FAKE_RUSTUP_LOG:?}"
if [[ "${1:-}" != "target" || "${2:-}" != "add" ]]; then
  echo "unexpected rustup args: $*" >&2
  exit 64
fi
EOF

  cat > "${bin_dir}/zig" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == "version" ]]; then
  echo "0.16.0"
  exit 0
fi
echo "fake zig $*" >> "${FAKE_ZIG_LOG:?}"
exit 0
EOF

  cat > "${bin_dir}/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

subcommand="${1:-}"
printf '%s\n' "$*" >> "${FAKE_CARGO_LOG:?}"

case "${subcommand}" in
  install)
    root=""
    shift
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --root)
          root="${2:?missing install root}"
          shift 2
          ;;
        *)
          shift
          ;;
      esac
    done
    mkdir -p "${root}/bin"
    cat > "${root}/bin/cargo-zigbuild" <<'INNER'
#!/usr/bin/env bash
exit 0
INNER
    chmod +x "${root}/bin/cargo-zigbuild"
    ;;
  build|zigbuild)
    target=""
    profile="debug"
    shift
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --target)
          target="${2:?missing target}"
          shift 2
          ;;
        --release)
          profile="release"
          shift
          ;;
        *)
          shift
          ;;
      esac
    done

    if [[ -z "${target}" ]]; then
      echo "missing target for fake cargo ${subcommand}" >&2
      exit 64
    fi

    if [[ "${subcommand}" == "zigbuild" ]]; then
      cat > "${FAKE_ZIGBUILD_ENV_LOG:?}" <<ENV
CARGO_ZIGBUILD_CACHE_DIR=${CARGO_ZIGBUILD_CACHE_DIR:-}
ZIG_LOCAL_CACHE_DIR=${ZIG_LOCAL_CACHE_DIR:-}
ZIG_GLOBAL_CACHE_DIR=${ZIG_GLOBAL_CACHE_DIR:-}
LIBZ_SYS_STATIC=${LIBZ_SYS_STATIC:-}
AR_${target//-/_}=${AR_aarch64_unknown_linux_gnu:-${AR_x86_64_unknown_linux_gnu:-}}
RANLIB_${target//-/_}=${RANLIB_aarch64_unknown_linux_gnu:-${RANLIB_x86_64_unknown_linux_gnu:-}}
ENV
    fi

    mkdir -p "${FAKE_REPO_ROOT:?}/target/${target}/${profile}"
    cat > "${FAKE_REPO_ROOT}/target/${target}/${profile}/nimbus" <<'INNER'
#!/usr/bin/env bash
exit 0
INNER
    chmod +x "${FAKE_REPO_ROOT}/target/${target}/${profile}/nimbus"
    ;;
  *)
    echo "unexpected cargo subcommand: ${subcommand}" >&2
    exit 64
    ;;
esac
EOF

  chmod +x "${bin_dir}/uname" "${bin_dir}/rustup" "${bin_dir}/zig" "${bin_dir}/cargo"

  FAKE_CARGO_LOG="${logs_dir}/cargo.log" \
  FAKE_RUSTUP_LOG="${logs_dir}/rustup.log" \
  FAKE_ZIG_LOG="${logs_dir}/zig.log" \
  FAKE_ZIGBUILD_ENV_LOG="${logs_dir}/zigbuild-env.log" \
  FAKE_REPO_ROOT="${fake_repo_root}" \
  FAKE_UNAME_S="${fake_uname_s}" \
  FAKE_UNAME_M="${fake_uname_m}" \
  NIMBUS_MACHINE_GUEST_BUILD_REPO_ROOT="${fake_repo_root}" \
  PATH="${bin_dir}:${PATH}" \
  TMPDIR="${scenario_dir}/tmp" \
  bash "${helper}" --cache-root "${cache_root}" > "${stdout_path}"

  grep -F "${fake_repo_root}/target/${expected_target}/release/nimbus" "${stdout_path}" >/dev/null
  grep -F "target add ${expected_target}" "${logs_dir}/rustup.log" >/dev/null
  grep -F "${expected_subcommand} --target ${expected_target} -p nimbus-bin --release" "${logs_dir}/cargo.log" >/dev/null

  if [[ "${expected_subcommand}" == "zigbuild" ]]; then
    grep -F "install --root ${cache_root}/cargo-zigbuild cargo-zigbuild" "${logs_dir}/cargo.log" >/dev/null
    grep -F "CARGO_ZIGBUILD_CACHE_DIR=${cache_root}/cargo-zigbuild-cache" "${logs_dir}/zigbuild-env.log" >/dev/null
    grep -F "ZIG_LOCAL_CACHE_DIR=${cache_root}/zig-local-cache" "${logs_dir}/zigbuild-env.log" >/dev/null
    grep -F "ZIG_GLOBAL_CACHE_DIR=${cache_root}/zig-global-cache" "${logs_dir}/zigbuild-env.log" >/dev/null
    grep -F "LIBZ_SYS_STATIC=1" "${logs_dir}/zigbuild-env.log" >/dev/null
  else
    if grep -q "install --root" "${logs_dir}/cargo.log"; then
      echo "native linux scenario should not install cargo-zigbuild" >&2
      exit 70
    fi
  fi
}

run_scenario "darwin-arm64" "Darwin" "arm64" "aarch64-unknown-linux-gnu" "zigbuild"
grep -F "AR_aarch64_unknown_linux_gnu=/usr/bin/ar" "${tmp_dir}/darwin-arm64/logs/zigbuild-env.log" >/dev/null
grep -F "RANLIB_aarch64_unknown_linux_gnu=/usr/bin/ranlib" "${tmp_dir}/darwin-arm64/logs/zigbuild-env.log" >/dev/null

run_scenario "linux-x86_64" "Linux" "x86_64" "x86_64-unknown-linux-gnu" "build"

echo "verified: machine guest binary helper selects the expected macOS zigbuild and native Linux build contracts"
