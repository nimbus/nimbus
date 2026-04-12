#!/usr/bin/env bash
set -euo pipefail

failures=0

print_line() {
  printf '%-22s %s\n' "$1" "$2"
}

compact_value() {
  printf '%s' "$1" | tr '\n' ' ' | sed -e 's/[[:space:]]\+/ /g' -e 's/^ //' -e 's/ $//'
}

mark_failure() {
  failures=$((failures + 1))
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
check_command "runtime.private_crun" "/usr/libexec/neovex/crun" optional
check_command "runtime.podman" "podman" optional
check_any_command "runtime.init" "catatonit" "tini" "dumb-init"

if command -v dpkg-query >/dev/null 2>&1; then
  print_line "host.packages" "dpkg-query"
  check_package_dpkg "conmon"
  check_package_dpkg "buildah"
  check_package_dpkg "libkrun" optional
  check_package_dpkg "libkrunfw" optional
  check_package_dpkg "uidmap" optional
  check_package_dpkg "passt" optional
  check_package_dpkg "fuse-overlayfs" optional
elif command -v rpm >/dev/null 2>&1; then
  print_line "host.packages" "rpm"
  check_package_rpm "conmon"
  check_package_rpm "buildah"
  check_package_rpm "libkrun" optional
  check_package_rpm "libkrunfw" optional
  check_package_rpm "shadow-utils" optional
  check_package_rpm "passt" optional
  check_package_rpm "fuse-overlayfs" optional
else
  print_line "host.packages" "unavailable (dpkg-query/rpm not found)"
fi

# Check for libkrun/libkrunfw shared libraries regardless of how they were
# installed (distro package or source build).  The package checks above only
# cover the distro path; these library checks cover source-built installs at
# /usr/local/lib64 or any other ldconfig-visible path.
check_shared_lib() {
  local label="$1"
  local soname="$2"
  local required="${3:-required}"
  local path=""

  path="$(ldconfig -p 2>/dev/null | grep -m1 "${soname}" | sed 's/.*=> //' || true)"
  if [[ -n "${path}" ]]; then
    print_line "${label}" "present path=${path}"
    return 0
  fi

  # Also check common non-standard paths directly
  for candidate in /usr/local/lib64/${soname}* /usr/local/lib/${soname}* /usr/lib64/${soname}* /usr/lib/${soname}*; do
    if [[ -f "${candidate}" ]]; then
      print_line "${label}" "present path=${candidate} (not in ldconfig cache — run sudo ldconfig)"
      return 0
    fi
  done

  print_line "${label}" "missing"
  if [[ "${required}" == "required" ]]; then
    mark_failure
  fi
}

check_shared_lib "lib.libkrun" "libkrun.so"
check_shared_lib "lib.libkrunfw" "libkrunfw.so"

# Check that pkg-config can find libkrun (needed for building patched crun)
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
    print_line "pkgconfig.libkrun" "missing (build-neovex-crun.sh will fail)"
    mark_failure
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
