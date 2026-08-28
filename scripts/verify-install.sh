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

resolve_nimbus_release_document() {
  local document_name="$1"
  local install_prefix="${NIMBUS_PREFIX:-/usr/local}"
  local nimbus_path=""
  local real_path=""
  local real_dir=""
  local derived_prefix=""
  local candidate=""

  if [[ -s "${install_prefix}/share/doc/nimbus/${document_name}" ]]; then
    printf '%s\n' "${install_prefix}/share/doc/nimbus/${document_name}"
    return 0
  fi

  # A binary in the requested prefix owns its payload. Falling through to a
  # different PATH channel would turn a partial direct install into a false pass.
  [[ -x "${install_prefix}/bin/nimbus" ]] && return 1

  nimbus_path="$(command -v nimbus 2>/dev/null || true)"
  if [[ -n "${nimbus_path}" ]]; then
    real_path="$(readlink "${nimbus_path}" 2>/dev/null || echo "${nimbus_path}")"
    if [[ "${real_path#/}" == "${real_path}" ]]; then
      real_path="$(cd "$(dirname "${nimbus_path}")" && cd "$(dirname "${real_path}")" && pwd)/$(basename "${real_path}")"
    fi
    real_dir="$(dirname "${real_path}")"
    derived_prefix="$(dirname "${real_dir}")"
    for candidate in "${real_dir}/${document_name}" "${derived_prefix}/share/doc/nimbus/${document_name}"; do
      if [[ -s "${candidate}" ]]; then
        printf '%s\n' "${candidate}"
        return 0
      fi
    done
  fi

  return 1
}

check_nimbus_release_documents() {
  local document_name=""
  local document_path=""
  for document_name in LICENSE README.md; do
    if document_path="$(resolve_nimbus_release_document "${document_name}")"; then
      print_line "nimbus.${document_name}" "present path=${document_path}"
    else
      print_line "nimbus.${document_name}" "missing"
      mark_failure
    fi
  done
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
    # shellcheck disable=SC2016 # $ORIGIN is a literal ELF loader token.
    if [[ "${dynamic_entries}" == *'$ORIGIN/lib'* ]]; then
      # shellcheck disable=SC2016 # Keep the literal loader token in output.
      print_line "nimbus-crun.runpath" 'present $ORIGIN/lib'
    else
      # shellcheck disable=SC2016 # Keep the literal loader token in output.
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
    # shellcheck disable=SC1091 # The guarded system file supplies distro data.
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

  check_nimbus_release_documents

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
  check_nimbus_release_documents

  # krunkit — optional macOS-dev dependency for the `nimbus machine` flow,
  # installed via the libkrun/krun Homebrew tap. The server runs without it.
  check_command "krunkit" "krunkit" recommended

  # Optional Bun/JSC in-process runtime adapter
  check_macos_bun_jsc_adapter

  # gvproxy is bundled + pinned in the macOS release archive and resolved
  # bundled-first (mirroring resolve_macos_bundled_helper in install.sh and the
  # Rust runtime resolver): the install prefix's libexec, then the Caskroom
  # libexec beside the resolved nimbus binary, then the Homebrew prefix / PATH.
  if gvproxy_path="$(resolve_macos_bundled_helper_path "gvproxy")"; then
    print_line "gvproxy" "present path=${gvproxy_path}"
  else
    print_line "gvproxy" "missing (expected bundled in the release archive; or 'brew install libkrun/krun/krunkit')"
    mark_warning
  fi

  # vfkit is the opt-in macOS VMM backend (NIMBUS_MACHINE_PROVIDER=vfkit), also
  # bundled + pinned in the release archive. The default backend stays krunkit,
  # so a missing vfkit is informational rather than a warning.
  if vfkit_path="$(resolve_macos_bundled_helper_path "vfkit")"; then
    print_line "vfkit" "present path=${vfkit_path} (opt-in: NIMBUS_MACHINE_PROVIDER=vfkit)"
  else
    print_line "vfkit" "absent (opt-in backend; bundled in the release archive when shipped)"
  fi
}

# Resolve a bundled-first macOS machine helper, mirroring install.sh's
# resolve_macos_bundled_helper and the Rust runtime resolver. Prints the first
# match to stdout and returns 0, or returns 1 when no candidate is found.
resolve_macos_bundled_helper_path() {
  local helper_name="$1"
  local install_prefix="${NIMBUS_PREFIX:-/usr/local}"

  if [[ -x "${install_prefix}/libexec/${helper_name}" ]]; then
    printf '%s\n' "${install_prefix}/libexec/${helper_name}"
    return 0
  fi

  local nimbus_path=""
  nimbus_path="$(command -v nimbus 2>/dev/null || true)"
  if [[ -n "${nimbus_path}" ]]; then
    local real_path=""
    real_path="$(readlink "${nimbus_path}" 2>/dev/null || echo "${nimbus_path}")"
    if [[ "${real_path#/}" == "${real_path}" ]]; then
      real_path="$(cd "$(dirname "${nimbus_path}")" && cd "$(dirname "${real_path}")" && pwd)/$(basename "${real_path}")"
    fi
    local real_dir=""
    real_dir="$(dirname "${real_path}")"
    if [[ -x "${real_dir}/libexec/${helper_name}" ]]; then
      printf '%s\n' "${real_dir}/libexec/${helper_name}"
      return 0
    fi
  fi

  local brew_prefix=""
  brew_prefix="$(brew --prefix 2>/dev/null || echo "/opt/homebrew")"
  local candidate=""
  for candidate in "${brew_prefix}/bin/${helper_name}" "/usr/local/bin/${helper_name}"; do
    if [[ -x "${candidate}" ]]; then
      printf '%s\n' "${candidate}"
      return 0
    fi
  done

  local path_helper=""
  if path_helper="$(command -v "${helper_name}" 2>/dev/null)"; then
    printf '%s\n' "${path_helper}"
    return 0
  fi

  return 1
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
