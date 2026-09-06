#!/usr/bin/env bash
set -euo pipefail

failures=0
allow_pending_private_runtime=false
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LINUX_DISTRIBUTION_CONTRACT_ENV="${NIMBUS_LINUX_DISTRIBUTION_CONTRACT_ENV:-${SCRIPT_DIR}/../packaging/linux-distribution-contract.env}"
DEFAULT_NIMBUS_CRUN_VERSION="v1.29.1-nimbus.2"
DEFAULT_NIMBUS_CRUN_UPSTREAM_VERSION="1.29.1"
DEFAULT_NIMBUS_LIBKRUN_VERSION="v1.19.4-nimbus.3"
DEFAULT_NIMBUS_LIBKRUN_UPSTREAM_VERSION="1.19.4"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --allow-pending-private-runtime)
      allow_pending_private_runtime=true
      shift
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 64
      ;;
  esac
done

if [[ -r "${LINUX_DISTRIBUTION_CONTRACT_ENV}" ]]; then
  # shellcheck disable=SC1090
  source "${LINUX_DISTRIBUTION_CONTRACT_ENV}"
fi

EXPECTED_NIMBUS_CRUN_VERSION="${EXPECTED_NIMBUS_CRUN_VERSION:-${NIMBUS_CRUN_VERSION:-${DEFAULT_NIMBUS_CRUN_VERSION}}}"
EXPECTED_NIMBUS_CRUN_UPSTREAM_VERSION="${EXPECTED_NIMBUS_CRUN_UPSTREAM_VERSION:-${NIMBUS_CRUN_UPSTREAM_VERSION:-${DEFAULT_NIMBUS_CRUN_UPSTREAM_VERSION}}}"
EXPECTED_NIMBUS_LIBKRUN_VERSION="${EXPECTED_NIMBUS_LIBKRUN_VERSION:-${NIMBUS_LIBKRUN_VERSION:-${DEFAULT_NIMBUS_LIBKRUN_VERSION}}}"
EXPECTED_NIMBUS_LIBKRUN_UPSTREAM_VERSION="${EXPECTED_NIMBUS_LIBKRUN_UPSTREAM_VERSION:-${NIMBUS_LIBKRUN_UPSTREAM_VERSION:-${DEFAULT_NIMBUS_LIBKRUN_UPSTREAM_VERSION}}}"
EXPECTED_LIBKRUN_SONAME="${EXPECTED_LIBKRUN_SONAME:-libkrun.so.1}"
EXPECTED_LIBKRUNFW_SONAME="${EXPECTED_LIBKRUNFW_SONAME:-libkrunfw.so.5}"
EXPECTED_LIBKRUN_ABI_SYMBOL="${EXPECTED_LIBKRUN_ABI_SYMBOL:-krun_set_port_map_with_bind_address}"
EXPECTED_CRUN_RUNPATH="${EXPECTED_CRUN_RUNPATH:-\$ORIGIN/lib}"

print_line() {
  printf '%-22s %s\n' "$1" "$2"
}

compact_value() {
  printf '%s' "$1" | tr '\n' ' ' | sed -e 's/[[:space:]]\+/ /g' -e 's/^ //' -e 's/ $//'
}

mark_failure() {
  failures=$((failures + 1))
}

mark_private_runtime_failure() {
  if [[ "${allow_pending_private_runtime}" != "true" ]]; then
    mark_failure
  fi
}

tuple_action() {
  printf 'action=install validated tuple with scripts/install.sh --crun-version %s --libkrun-version %s' \
    "${EXPECTED_NIMBUS_CRUN_VERSION}" \
    "${EXPECTED_NIMBUS_LIBKRUN_VERSION}"
}

command_version_line() {
  local command_name="$1"
  local version_line=""

  version_line="$("$command_name" --version 2>/dev/null | head -n1 || true)"
  printf '%s' "${version_line}"
}

check_command() {
  local label="$1"
  local command_name="$2"
  local required="${3:-required}"
  local path=""
  local version_line=""

  if path="$(command -v "$command_name" 2>/dev/null)"; then
    version_line="$(command_version_line "$command_name")"
    if [[ -n "${version_line}" ]]; then
      print_line "${label}" "present path=${path} version=${version_line}"
    else
      print_line "${label}" "present path=${path}"
    fi
    return 0
  fi

  print_line "${label}" "missing"
  if [[ "${required}" == "required" ]]; then
    mark_failure
  fi
}

check_package_dpkg() {
  local package_name="$1"
  local required="${2:-required}"
  local version=""

  version="$(dpkg-query -W -f='${Version}' "${package_name}" 2>/dev/null || true)"
  if [[ -n "${version}" ]]; then
    print_line "package.${package_name}" "installed version=${version}"
    return 0
  fi

  print_line "package.${package_name}" "missing"
  if [[ "${required}" == "required" ]]; then
    mark_failure
  fi
}

check_package_rpm() {
  local package_name="$1"
  local required="${2:-required}"
  local version=""

  version="$(rpm -q --qf '%{VERSION}-%{RELEASE}\n' "${package_name}" 2>/dev/null || true)"
  if [[ -n "${version}" && "${version}" != *"not installed"* ]]; then
    print_line "package.${package_name}" "installed version=${version}"
    return 0
  fi

  print_line "package.${package_name}" "missing"
  if [[ "${required}" == "required" ]]; then
    mark_failure
  fi
}

check_any_command() {
  local label="$1"
  shift

  local candidate=""
  for candidate in "$@"; do
    if command -v "${candidate}" >/dev/null 2>&1; then
      check_command "${label}" "${candidate}" optional
      return 0
    fi
  done

  print_line "${label}" "missing"
  mark_failure
}

check_private_libkrun_stack() {
  local lib_root="/usr/libexec/nimbus/lib"
  local release_info="/usr/libexec/nimbus/NIMBUS_LIBKRUN_RELEASE.txt"
  local crun_path="/usr/libexec/nimbus/crun"
  local installed_version=""
  local crun_version=""

  print_line "nimbus.expected_tuple" "nimbus-crun=${EXPECTED_NIMBUS_CRUN_VERSION} upstream-crun=${EXPECTED_NIMBUS_CRUN_UPSTREAM_VERSION} nimbus-libkrun=${EXPECTED_NIMBUS_LIBKRUN_VERSION} upstream-libkrun=${EXPECTED_NIMBUS_LIBKRUN_UPSTREAM_VERSION}"

  if [[ -f "${release_info}" ]]; then
    installed_version="$(awk -F= '$1 == "nimbus-libkrun" { print $2; exit }' "${release_info}" 2>/dev/null || true)"
    if [[ "${installed_version}" == "${EXPECTED_NIMBUS_LIBKRUN_VERSION}" ]]; then
      print_line "nimbus.libkrun" "present path=${lib_root} version=${installed_version} expected=${EXPECTED_NIMBUS_LIBKRUN_VERSION}"
    else
      print_line "nimbus.libkrun" "mismatch path=${release_info} actual=${installed_version:-unknown} expected=${EXPECTED_NIMBUS_LIBKRUN_VERSION} $(tuple_action)"
      mark_private_runtime_failure
    fi
  else
    print_line "nimbus.libkrun" "missing path=${release_info} expected=${EXPECTED_NIMBUS_LIBKRUN_VERSION} $(tuple_action)"
    mark_private_runtime_failure
  fi

  if [[ -e "${lib_root}/${EXPECTED_LIBKRUN_SONAME}" ]]; then
    print_line "nimbus.libkrun.so" "present path=${lib_root}/${EXPECTED_LIBKRUN_SONAME} expected_soname=${EXPECTED_LIBKRUN_SONAME}"
  else
    print_line "nimbus.libkrun.so" "missing path=${lib_root}/${EXPECTED_LIBKRUN_SONAME} expected_soname=${EXPECTED_LIBKRUN_SONAME} $(tuple_action)"
    mark_private_runtime_failure
  fi

  if [[ -e "${lib_root}/${EXPECTED_LIBKRUNFW_SONAME}" ]]; then
    print_line "nimbus.libkrunfw.so" "present path=${lib_root}/${EXPECTED_LIBKRUNFW_SONAME} expected_soname=${EXPECTED_LIBKRUNFW_SONAME}"
  else
    print_line "nimbus.libkrunfw.so" "missing path=${lib_root}/${EXPECTED_LIBKRUNFW_SONAME} expected_soname=${EXPECTED_LIBKRUNFW_SONAME} $(tuple_action)"
    mark_private_runtime_failure
  fi

  if command -v nm >/dev/null 2>&1 && [[ -e "${lib_root}/${EXPECTED_LIBKRUN_SONAME}" ]]; then
    local nm_output=""
    nm_output="$(nm -D "${lib_root}/${EXPECTED_LIBKRUN_SONAME}" 2>/dev/null || true)"
    if [[ "${nm_output}" == *" ${EXPECTED_LIBKRUN_ABI_SYMBOL}"* ]]; then
      print_line "nimbus.libkrun.symbol" "present expected_symbol=${EXPECTED_LIBKRUN_ABI_SYMBOL}"
    else
      print_line "nimbus.libkrun.symbol" "missing expected_symbol=${EXPECTED_LIBKRUN_ABI_SYMBOL} $(tuple_action)"
      mark_private_runtime_failure
    fi
  else
    print_line "nimbus.libkrun.symbol" "missing expected_symbol=${EXPECTED_LIBKRUN_ABI_SYMBOL} (nm or libkrun unavailable) $(tuple_action)"
    mark_private_runtime_failure
  fi

  if [[ -x "${crun_path}" ]]; then
    crun_version="$("${crun_path}" --version 2>/dev/null || true)"
    if echo "${crun_version}" | grep -q '+LIBKRUN'; then
      print_line "nimbus.crun.version" "present expected=${EXPECTED_NIMBUS_CRUN_VERSION} upstream=${EXPECTED_NIMBUS_CRUN_UPSTREAM_VERSION} actual=$(compact_value "${crun_version}")"
    else
      print_line "nimbus.crun.version" "missing +LIBKRUN path=${crun_path} expected=${EXPECTED_NIMBUS_CRUN_VERSION} $(tuple_action)"
      mark_private_runtime_failure
    fi
  else
    print_line "nimbus.crun.version" "missing path=${crun_path} expected=${EXPECTED_NIMBUS_CRUN_VERSION} $(tuple_action)"
    mark_private_runtime_failure
  fi

  if command -v readelf >/dev/null 2>&1 && [[ -x "${crun_path}" ]]; then
    if readelf -d "${crun_path}" 2>/dev/null | grep -q "\$ORIGIN/lib"; then
      print_line "nimbus.crun.runpath" "present expected=${EXPECTED_CRUN_RUNPATH}"
    else
      print_line "nimbus.crun.runpath" "missing expected=${EXPECTED_CRUN_RUNPATH} $(tuple_action)"
      mark_private_runtime_failure
    fi
  else
    print_line "nimbus.crun.runpath" "missing expected=${EXPECTED_CRUN_RUNPATH} (readelf or crun unavailable) $(tuple_action)"
    mark_private_runtime_failure
  fi
}

os_name="$(uname -s)"
arch_name="$(uname -m)"
kernel_name="$(uname -r)"

print_line "host.os" "${os_name}"
print_line "host.arch" "${arch_name}"
print_line "host.kernel" "${kernel_name}"

if [[ -r /etc/os-release ]]; then
  distro_name="$(. /etc/os-release && printf '%s %s' "${NAME:-unknown}" "${VERSION_ID:-unknown}")"
  print_line "host.distro" "${distro_name}"
else
  print_line "host.distro" "unavailable"
fi

if [[ "${os_name}" != "Linux" ]]; then
  print_line "host.support" "unsupported (Linux host required for krun/conmon validation)"
  mark_failure
else
  print_line "host.support" "linux"
fi

if [[ -e /dev/kvm ]]; then
  if [[ "$(id -u)" == "0" ]] || id -Gn | tr ' ' '\n' | grep -qx 'kvm'; then
    print_line "host.kvm" "present path=/dev/kvm access=ok"
  else
    print_line "host.kvm" "present path=/dev/kvm access=current-user-not-in-kvm-group"
    mark_failure
  fi
else
  print_line "host.kvm" "missing"
  mark_failure
fi

check_command "tool.patch" "patch"
check_command "tool.make" "make"
check_command "tool.autoreconf" "autoreconf"
check_command "tool.autoconf" "autoconf"
check_command "tool.automake" "automake"
check_command "tool.pkg-config" "pkg-config"
check_any_command "tool.cc" "cc" "gcc" "clang"

check_command "runtime.conmon" "conmon"
check_command "runtime.buildah" "buildah"
check_command "runtime.system_crun" "crun"
if [[ "$(id -u)" -eq 0 ]]; then
  print_line "runtime.privilege" "root"
elif command -v sudo >/dev/null 2>&1 && sudo -n true >/dev/null 2>&1; then
  print_line "runtime.privilege" "noninteractive sudo available"
else
  print_line "runtime.privilege" "missing noninteractive sudo"
  mark_failure
fi
if [[ "${allow_pending_private_runtime}" == "true" ]]; then
  check_command "runtime.private_crun" "/usr/libexec/nimbus/crun" optional
  print_line "runtime.private_gate" "pending install permitted for preflight"
else
  check_command "runtime.private_crun" "/usr/libexec/nimbus/crun"
fi
check_command "runtime.podman" "podman" optional
check_any_command "runtime.init" "catatonit" "tini" "dumb-init"

if command -v dpkg-query >/dev/null 2>&1; then
  print_line "host.packages" "dpkg-query"
  check_package_dpkg "nimbus" optional
  check_package_dpkg "nimbus-crun" optional
  check_package_dpkg "nimbus-libkrun" optional
  check_package_dpkg "conmon"
  check_package_dpkg "buildah"
  check_package_dpkg "uidmap" optional
  check_package_dpkg "passt" optional
  check_package_dpkg "fuse-overlayfs" optional
elif command -v rpm >/dev/null 2>&1; then
  print_line "host.packages" "rpm"
  check_package_rpm "nimbus" optional
  check_package_rpm "nimbus-crun" optional
  check_package_rpm "nimbus-libkrun" optional
  check_package_rpm "conmon"
  check_package_rpm "buildah"
  check_package_rpm "shadow-utils" optional
  check_package_rpm "passt" optional
  check_package_rpm "fuse-overlayfs" optional
else
  print_line "host.packages" "unavailable (dpkg-query/rpm not found)"
fi

check_private_libkrun_stack

# Report source-build pkg-config visibility as an optional diagnostic. Nimbus
# service execution uses the installed private stack above, not distro libkrun.
if pkg-config --exists libkrun 2>/dev/null; then
  libkrun_pc_path="$(pkg-config --variable=libdir libkrun 2>/dev/null || true)"
  print_line "pkgconfig.libkrun" "present libdir=${libkrun_pc_path}"
else
  # Try known non-standard locations before failing
  for candidate in /usr/local/lib64/pkgconfig /usr/local/lib/pkgconfig; do
    if [[ -f "${candidate}/libkrun.pc" ]]; then
      print_line "pkgconfig.libkrun" "present path=${candidate}/libkrun.pc (not on default PKG_CONFIG_PATH)"
      break
    fi
  done
  if ! [[ -f /usr/local/lib64/pkgconfig/libkrun.pc || -f /usr/local/lib/pkgconfig/libkrun.pc ]]; then
    print_line "pkgconfig.libkrun" "missing (source-build diagnostics only)"
  fi
fi

if command -v podman >/dev/null 2>&1; then
  podman_runtime="$(podman info --format '{{.Host.OCIRuntime.Name}} {{.Host.OCIRuntime.Path}}' 2>/dev/null || true)"
  if [[ -n "${podman_runtime}" ]]; then
    print_line "podman.runtime" "$(compact_value "${podman_runtime}")"
  else
    print_line "podman.runtime" "unavailable"
  fi
fi

if [[ "${failures}" -eq 0 ]]; then
  print_line "result" "supported"
  exit 0
fi

print_line "result" "unsupported (${failures} failing checks)"
exit 1
