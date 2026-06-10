#!/usr/bin/env bash
# Post-install verification helper for nimbus.
#
# Checks that all required components are installed and accessible.
# Run standalone after install, or called automatically by install.sh.
#
# See docs/private/plans/install-script-plan.md for the full verification contract.

set -euo pipefail

failures=0
warnings=0

print_line() {
  printf '%-22s %s\n' "$1" "$2"
}

compact_value() {
  printf '%s' "$1" | tr '\n' ' ' | sed -e 's/[[:space:]]\+/ /g' -e 's/^ //' -e 's/ $//'
}

mark_failure() {
  failures=$((failures + 1))
}

mark_warning() {
  warnings=$((warnings + 1))
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
  else
    mark_warning
  fi
}

check_file() {
  local label="$1"
  local file_path="$2"
  local required="${3:-required}"

  if [[ -f "${file_path}" ]]; then
    if [[ -x "${file_path}" ]]; then
      print_line "${label}" "present path=${file_path} executable=yes"
    else
      print_line "${label}" "present path=${file_path} executable=no"
    fi
    return 0
  fi

  print_line "${label}" "missing path=${file_path}"
  if [[ "${required}" == "required" ]]; then
    mark_failure
  else
    mark_warning
  fi
}

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

  # Check common non-standard paths directly
  for candidate in /usr/local/lib64/${soname}* /usr/local/lib/${soname}* /usr/lib64/${soname}* /usr/lib/${soname}*; do
    if [[ -f "${candidate}" ]]; then
      print_line "${label}" "present path=${candidate} (not in ldconfig cache)"
      return 0
    fi
  done

  print_line "${label}" "missing"
  if [[ "${required}" == "required" ]]; then
    mark_failure
  else
    mark_warning
  fi
}

check_private_libkrun_stack() {
  local lib_root="/usr/libexec/nimbus/lib"
  local release_info="/usr/libexec/nimbus/NIMBUS_LIBKRUN_RELEASE.txt"
  local installed_version=""

  if [[ -f "${release_info}" ]]; then
    installed_version="$(awk -F= '$1 == "nimbus-libkrun" { print $2; exit }' "${release_info}" 2>/dev/null || true)"
    print_line "nimbus-libkrun" "present path=${lib_root} version=${installed_version:-unknown}"
  else
    print_line "nimbus-libkrun" "missing path=${release_info}"
    mark_failure
  fi

  if [[ -e "${lib_root}/libkrun.so.1" ]]; then
    print_line "libkrun.so" "present path=${lib_root}/libkrun.so.1"
  else
    print_line "libkrun.so" "missing path=${lib_root}/libkrun.so.1"
    mark_failure
  fi

  if [[ -e "${lib_root}/libkrunfw.so.5" ]]; then
    print_line "libkrunfw.so" "present path=${lib_root}/libkrunfw.so.5"
  else
    print_line "libkrunfw.so" "missing path=${lib_root}/libkrunfw.so.5"
    mark_failure
  fi

  if command -v nm >/dev/null 2>&1 && [[ -e "${lib_root}/libkrun.so.1" ]]; then
    local nm_output=""
    nm_output="$(nm -D "${lib_root}/libkrun.so.1" 2>/dev/null || true)"
    if [[ "${nm_output}" == *" krun_set_port_map_with_bind_address"* ]]; then
      print_line "libkrun.symbol" "present krun_set_port_map_with_bind_address"
    else
      print_line "libkrun.symbol" "missing krun_set_port_map_with_bind_address"
      mark_failure
    fi
  else
    print_line "libkrun.symbol" "skipped (nm or libkrun missing)"
    mark_warning
  fi

  local crun_path="/usr/libexec/nimbus/crun"
  if command -v readelf >/dev/null 2>&1 && [[ -x "${crun_path}" ]]; then
    local dynamic_entries=""
    dynamic_entries="$(readelf -d "${crun_path}" 2>/dev/null || true)"
    if [[ "${dynamic_entries}" == *'$ORIGIN/lib'* ]]; then
      print_line "nimbus-crun.runpath" 'present $ORIGIN/lib'
    else
      print_line "nimbus-crun.runpath" 'missing $ORIGIN/lib'
      mark_failure
    fi
  else
    print_line "nimbus-crun.runpath" "skipped (readelf or crun missing)"
    mark_warning
  fi
}

check_bun_jsc_adapter() {
  local manifest_path="/usr/libexec/nimbus/runtime/bun-jsc/current/nimbus-bun-jsc-adapter.json"
  local adapter_dir="/usr/libexec/nimbus/runtime/bun-jsc/current"
  local adapter_version=""

  if [[ ! -f "${manifest_path}" ]]; then
    print_line "nimbus-bun-jsc" "absent optional"
    return 0
  fi

  adapter_version="$(sed -n 's/^[[:space:]]*"adapter_version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "${manifest_path}" | head -n 1)"
  print_line "nimbus-bun-jsc" "present path=${manifest_path} version=${adapter_version:-unknown}"

  if [[ -x "${adapter_dir}/libnimbus_bun_jsc_embedder.so" ]]; then
    print_line "bun-jsc.library" "present path=${adapter_dir}/libnimbus_bun_jsc_embedder.so"
  else
    print_line "bun-jsc.library" "missing path=${adapter_dir}/libnimbus_bun_jsc_embedder.so"
    mark_failure
  fi
}

check_macos_bun_jsc_adapter() {
  local brew_prefix=""
  local manifest_path=""
  local adapter_dir=""
  local adapter_version=""

  brew_prefix="$(brew --prefix 2>/dev/null || echo "/opt/homebrew")"
  manifest_path="${brew_prefix}/opt/nimbus/libexec/runtime/bun-jsc/current/nimbus-bun-jsc-adapter.json"
  adapter_dir="$(dirname "${manifest_path}")"
  if [[ ! -f "${manifest_path}" ]]; then
    print_line "nimbus-bun-jsc" "absent optional"
    return 0
  fi

  adapter_version="$(sed -n 's/^[[:space:]]*"adapter_version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "${manifest_path}" | head -n 1)"
  print_line "nimbus-bun-jsc" "present path=${manifest_path} version=${adapter_version:-unknown}"

  if [[ -x "${adapter_dir}/libnimbus_bun_jsc_embedder.dylib" ]]; then
    print_line "bun-jsc.library" "present path=${adapter_dir}/libnimbus_bun_jsc_embedder.dylib"
  else
    print_line "bun-jsc.library" "missing path=${adapter_dir}/libnimbus_bun_jsc_embedder.dylib"
    mark_failure
  fi
}

# --- Platform detection -----------------------------------------------------

os_name="$(uname -s)"
arch_name="$(uname -m)"

print_line "host.os" "${os_name}"
print_line "host.arch" "${arch_name}"

# --- Linux checks -----------------------------------------------------------

verify_linux() {
  if [[ -r /etc/os-release ]]; then
    distro_name="$(. /etc/os-release && printf '%s %s' "${NAME:-unknown}" "${VERSION_ID:-unknown}")"
    print_line "host.distro" "${distro_name}"
  else
    print_line "host.distro" "unavailable"
  fi

  # nimbus binary
  local install_prefix="${NIMBUS_PREFIX:-/usr/local}"
  local nimbus_path="${install_prefix}/bin/nimbus"
  if [[ -x "${nimbus_path}" ]]; then
    local nimbus_version=""
    nimbus_version="$("${nimbus_path}" --version 2>/dev/null | head -n1 || true)"
    if [[ -n "${nimbus_version}" ]]; then
      print_line "nimbus" "present path=${nimbus_path} version=${nimbus_version}"
    else
      print_line "nimbus" "present path=${nimbus_path}"
    fi
  else
    check_command "nimbus" "nimbus" required
  fi

  # nimbus-crun at /usr/libexec/nimbus/crun
  local crun_path="/usr/libexec/nimbus/crun"
  if [[ -x "${crun_path}" ]]; then
    local crun_version=""
    crun_version="$("${crun_path}" --version 2>/dev/null || true)"
    if echo "${crun_version}" | grep -q '+LIBKRUN'; then
      print_line "nimbus-crun" "present path=${crun_path} version=$(compact_value "${crun_version}")"
    else
      print_line "nimbus-crun" "present path=${crun_path} (missing +LIBKRUN flag)"
      mark_failure
    fi
  else
    print_line "nimbus-crun" "missing path=${crun_path}"
    mark_failure
  fi

  # /dev/kvm
  if [[ -c /dev/kvm ]]; then
    print_line "kvm.device" "present path=/dev/kvm"
    # Check access
    if [[ -r /dev/kvm && -w /dev/kvm ]]; then
      print_line "kvm.access" "ok"
    else
      print_line "kvm.access" "denied (add user to kvm group)"
      mark_warning
    fi
  else
    print_line "kvm.device" "missing"
    mark_warning
  fi

  # Required runtime dependencies
  check_command "conmon" "conmon" required
  check_command "buildah" "buildah" required

  # Recommended dependencies
  check_command "catatonit" "catatonit" recommended
  check_command "passt" "passt" recommended
  check_command "newuidmap" "newuidmap" recommended
  check_command "fuse-overlayfs" "fuse-overlayfs" recommended

  # Nimbus-private libkrun stack
  check_private_libkrun_stack

  # Optional Bun/JSC in-process runtime adapter
  check_bun_jsc_adapter

  # containers config
  if [[ -d /etc/containers || -d /usr/share/containers ]]; then
    print_line "containers.config" "present"
  else
    print_line "containers.config" "missing"
    mark_warning
  fi
}

# --- macOS checks -----------------------------------------------------------

verify_macos() {
  local macos_version=""
  macos_version="$(sw_vers -productVersion 2>/dev/null || echo "unknown")"
  print_line "host.macos" "${macos_version}"

  # Check macOS version >= 14
  local macos_major=""
  macos_major="$(echo "${macos_version}" | cut -d. -f1)"
  if [[ "${macos_major}" -lt 14 ]]; then
    print_line "host.macos.version" "unsupported (requires macOS 14+)"
    mark_failure
  else
    print_line "host.macos.version" "supported"
  fi

  # Check architecture is arm64
  if [[ "${arch_name}" != "arm64" ]]; then
    print_line "host.arch.check" "unsupported (requires Apple Silicon)"
    mark_failure
  else
    print_line "host.arch.check" "supported"
  fi

  # nimbus binary
  check_command "nimbus" "nimbus" required

  # krunkit
  check_command "krunkit" "krunkit" required

  # Optional Bun/JSC in-process runtime adapter
  check_macos_bun_jsc_adapter

  # gvproxy — find it relative to the installed nimbus binary in Caskroom
  local nimbus_path=""
  nimbus_path="$(command -v nimbus 2>/dev/null || true)"
  if [[ -n "${nimbus_path}" ]]; then
    # Resolve symlink to get Caskroom path
    local real_path=""
    real_path="$(readlink "${nimbus_path}" 2>/dev/null || echo "${nimbus_path}")"

    # If it's a relative symlink, resolve from the symlink's directory
    if [[ "${real_path}" != /* ]]; then
      real_path="$(cd "$(dirname "${nimbus_path}")" && cd "$(dirname "${real_path}")" && pwd)/$(basename "${real_path}")"
    fi

    # Check if it looks like a Caskroom path
    if [[ "${real_path}" == *Caskroom* ]]; then
      local caskroom_version_dir=""
      caskroom_version_dir="$(dirname "${real_path}")"
      local gvproxy_path="${caskroom_version_dir}/libexec/gvproxy"

      if [[ -x "${gvproxy_path}" ]]; then
        print_line "gvproxy" "present path=${gvproxy_path}"
      else
        print_line "gvproxy" "missing path=${gvproxy_path}"
        mark_failure
      fi
    else
      # Not a Caskroom install — check common locations
      local brew_prefix=""
      brew_prefix="$(brew --prefix 2>/dev/null || echo "/opt/homebrew")"
      local gvproxy_candidates=(
        "${brew_prefix}/bin/gvproxy"
        "/usr/local/bin/gvproxy"
      )
      local found_gvproxy=""
      for candidate in "${gvproxy_candidates[@]}"; do
        if [[ -x "${candidate}" ]]; then
          print_line "gvproxy" "present path=${candidate}"
          found_gvproxy="1"
          break
        fi
      done
      if [[ -z "${found_gvproxy}" ]]; then
        print_line "gvproxy" "missing (not found in standard locations)"
        mark_failure
      fi
    fi
  else
    print_line "gvproxy" "skipped (nimbus not found)"
  fi
}

# --- Main -------------------------------------------------------------------

main() {
  case "${os_name}" in
    Linux)
      verify_linux
      ;;
    Darwin)
      verify_macos
      ;;
    *)
      print_line "host.support" "unsupported (${os_name})"
      mark_failure
      ;;
  esac

  echo ""
  if [[ "${failures}" -eq 0 && "${warnings}" -eq 0 ]]; then
    print_line "result" "supported (0 failures)"
    exit 0
  elif [[ "${failures}" -eq 0 ]]; then
    print_line "result" "supported (0 failures, ${warnings} warnings)"
    exit 0
  else
    print_line "result" "unsupported (${failures} failures, ${warnings} warnings)"
    exit 1
  fi
}

main "$@"
