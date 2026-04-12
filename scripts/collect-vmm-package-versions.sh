#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: collect-vmm-package-versions.sh

Collect package-manager and command-level version evidence for the Linux VMM
dependency set. This helper is best-effort: it reports what is installed and
available on the current host without failing when packages are missing.

examples:
  bash scripts/collect-vmm-package-versions.sh
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

print_line() {
  printf '%-28s %s\n' "$1" "$2"
}

compact_value() {
  printf '%s' "$1" | tr '\n' ' ' | sed -e 's/[[:space:]]\+/ /g' -e 's/^ //' -e 's/ $//'
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
}

check_any_command() {
  local label="$1"
  shift

  local candidate=""
  for candidate in "$@"; do
    if command -v "${candidate}" >/dev/null 2>&1; then
      check_command "${label}" "${candidate}"
      return 0
    fi
  done

  print_line "${label}" "missing"
}

check_package_dpkg() {
  local package_name="$1"
  local version=""

  version="$(dpkg-query -W -f='${Version}' "${package_name}" 2>/dev/null || true)"
  if [[ -n "${version}" ]]; then
    print_line "package.${package_name}" "installed version=${version}"
  else
    print_line "package.${package_name}" "missing"
  fi
}

check_package_rpm() {
  local package_name="$1"
  local version=""

  version="$(rpm -q --qf '%{VERSION}-%{RELEASE}\n' "${package_name}" 2>/dev/null || true)"
  if [[ -n "${version}" && "${version}" != *"not installed"* ]]; then
    print_line "package.${package_name}" "installed version=${version}"
  else
    print_line "package.${package_name}" "missing"
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

if command -v dpkg-query >/dev/null 2>&1; then
  print_line "host.packages" "dpkg-query"
  check_package_dpkg "conmon"
  check_package_dpkg "buildah"
  check_package_dpkg "libkrun"
  check_package_dpkg "libkrunfw"
  check_package_dpkg "catatonit"
  check_package_dpkg "tini"
  check_package_dpkg "dumb-init"
  check_package_dpkg "crun"
  check_package_dpkg "podman"
elif command -v rpm >/dev/null 2>&1; then
  print_line "host.packages" "rpm"
  check_package_rpm "conmon"
  check_package_rpm "buildah"
  check_package_rpm "libkrun"
  check_package_rpm "libkrunfw"
  check_package_rpm "catatonit"
  check_package_rpm "tini"
  check_package_rpm "dumb-init"
  check_package_rpm "crun"
  check_package_rpm "podman"
else
  print_line "host.packages" "unavailable (dpkg-query/rpm not found)"
fi

check_command "tool.conmon" "conmon"
check_command "tool.buildah" "buildah"
check_command "tool.crun" "crun"
check_command "tool.private_crun" "/usr/libexec/neovex/crun"
check_command "tool.podman" "podman"
check_any_command "tool.init" "catatonit" "tini" "dumb-init"

if command -v podman >/dev/null 2>&1; then
  podman_runtime="$(podman info --format '{{.Host.OCIRuntime.Name}} {{.Host.OCIRuntime.Path}}' 2>/dev/null || true)"
  if [[ -n "${podman_runtime}" ]]; then
    print_line "podman.runtime" "$(compact_value "${podman_runtime}")"
  else
    print_line "podman.runtime" "unavailable"
  fi
fi
