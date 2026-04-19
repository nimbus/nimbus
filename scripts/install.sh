#!/bin/sh
# shellcheck shell=sh
# Neovex install script — portable bootstrapper for all supported platforms.
#
# Usage:
#   curl -fsSL https://neovex.dev/install.sh | sh
#   curl -fsSL https://neovex.dev/install.sh | sh -s -- --version v0.1.14
#   curl -fsSL https://neovex.dev/install.sh | sh -s -- --dry-run
#   curl -fsSL https://neovex.dev/install.sh | sh -s -- --uninstall
#
# See docs/plans/install-script-plan.md for the full contract.

set -eu

# --- Globals ----------------------------------------------------------------

NEOVEX_VERSION=""
NEOVEX_CRUN_VERSION=""
NEOVEX_PREFIX="/usr/local"
DRY_RUN=""
SKIP_DEPS=""
UNINSTALL=""
YES=""
REQUIRE_ATTESTATIONS="${NEOVEX_REQUIRE_ATTESTATIONS:-}"
PLATFORM=""
ARCH=""
DISTRO_ID=""
DISTRO_VERSION=""

# GitHub API endpoints
NEOVEX_RELEASES_API="https://api.github.com/repos/agentstation/neovex/releases"
NEOVEX_CRUN_RELEASES_API="https://api.github.com/repos/agentstation/neovex-crun/releases"

# Release asset base URLs
NEOVEX_RELEASES_DOWNLOAD="https://github.com/agentstation/neovex/releases/download"
NEOVEX_CRUN_RELEASES_DOWNLOAD="https://github.com/agentstation/neovex-crun/releases/download"

# --- Output helpers ---------------------------------------------------------

say() {
  printf '%s\n' "$*"
}

say_info() {
  printf '[info] %s\n' "$*"
}

say_warn() {
  printf '[warn] %s\n' "$*" >&2
}

err() {
  printf '[error] %s\n' "$*" >&2
  exit 1
}

# --- Dependency checks ------------------------------------------------------

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    err "need '$1' (command not found)"
  fi
}

check_cmd() {
  command -v "$1" >/dev/null 2>&1
}

# --- Download helper --------------------------------------------------------

download() {
  url="$1"
  if check_cmd curl; then
    if [ -n "${GITHUB_TOKEN:-}" ] && [ "${url#https://api.github.com/}" != "$url" ]; then
      curl -fsSL \
        -H "Authorization: Bearer $GITHUB_TOKEN" \
        -H "X-GitHub-Api-Version: 2022-11-28" \
        "$url"
    else
      curl -fsSL "$url"
    fi
  elif check_cmd wget; then
    if [ -n "${GITHUB_TOKEN:-}" ] && [ "${url#https://api.github.com/}" != "$url" ]; then
      wget \
        --header="Authorization: Bearer $GITHUB_TOKEN" \
        --header="X-GitHub-Api-Version: 2022-11-28" \
        -qO- "$url"
    else
      wget -qO- "$url"
    fi
  else
    err "need curl or wget to download files"
  fi
}

download_to_file() {
  url="$1"
  dest="$2"
  if check_cmd curl; then
    if [ -n "${GITHUB_TOKEN:-}" ] && [ "${url#https://api.github.com/}" != "$url" ]; then
      curl -fsSL \
        -H "Authorization: Bearer $GITHUB_TOKEN" \
        -H "X-GitHub-Api-Version: 2022-11-28" \
        -o "$dest" "$url"
    else
      curl -fsSL -o "$dest" "$url"
    fi
  elif check_cmd wget; then
    if [ -n "${GITHUB_TOKEN:-}" ] && [ "${url#https://api.github.com/}" != "$url" ]; then
      wget \
        --header="Authorization: Bearer $GITHUB_TOKEN" \
        --header="X-GitHub-Api-Version: 2022-11-28" \
        -qO "$dest" "$url"
    else
      wget -qO "$dest" "$url"
    fi
  else
    err "need curl or wget to download files"
  fi
}

sha256_file() {
  file_path="$1"

  if check_cmd sha256sum; then
    sha256sum "$file_path" | awk '{print $1}'
  elif check_cmd shasum; then
    shasum -a 256 "$file_path" | awk '{print $1}'
  else
    err "need sha256sum or shasum for checksum verification"
  fi
}

expected_sha256_from_manifest() {
  manifest_path="$1"
  subject_name="$2"

  awk -v name="$subject_name" '
    NF >= 2 {
      file = $NF
      sub(/^\*/, "", file)
      if (file == name) {
        print $1
        found = 1
        exit
      }
    }
    END {
      if (!found) {
        exit 1
      }
    }
  ' "$manifest_path" 2>/dev/null || true
}

verify_file_checksum() {
  file_path="$1"
  manifest_path="$2"
  subject_name="$3"

  expected_sha256="$(expected_sha256_from_manifest "$manifest_path" "$subject_name")"
  if [ -z "$expected_sha256" ]; then
    err "checksum entry for $subject_name not found in $(basename "$manifest_path")"
  fi

  actual_sha256="$(sha256_file "$file_path")"
  if [ "$actual_sha256" != "$expected_sha256" ]; then
    err "checksum verification failed for $subject_name"
  fi
}

file_matches_manifest_checksum() {
  file_path="$1"
  manifest_path="$2"
  subject_name="$3"

  expected_sha256="$(expected_sha256_from_manifest "$manifest_path" "$subject_name")"
  if [ -z "$expected_sha256" ]; then
    err "checksum entry for $subject_name not found in $(basename "$manifest_path")"
  fi

  actual_sha256="$(sha256_file "$file_path")"
  [ "$actual_sha256" = "$expected_sha256" ]
}

verify_github_attestation() {
  subject_path="$1"
  repo_name="$2"
  source_ref="$3"
  signer_workflow="$4"
  subject_label="$5"

  if ! check_cmd gh; then
    if [ -n "$REQUIRE_ATTESTATIONS" ]; then
      err "gh CLI is required for GitHub attestation verification of $subject_label"
    fi
    say_warn "gh CLI not found — skipping GitHub attestation verification for $subject_label"
    return 0
  fi

  say_info "Verifying GitHub attestation for $subject_label..."
  if gh attestation verify \
    "$subject_path" \
    --repo "$repo_name" \
    --source-ref "$source_ref" \
    --signer-workflow "$signer_workflow" \
    >/dev/null 2>&1; then
    say_info "GitHub attestation verified for $subject_label"
    return 0
  fi

  if [ -n "$REQUIRE_ATTESTATIONS" ]; then
    err "GitHub attestation verification failed for $subject_label"
  fi
  say_warn "GitHub attestation verification failed for $subject_label — continuing without enterprise-trust enforcement"
}

# --- Platform detection -----------------------------------------------------

detect_platform() {
  PLATFORM="$(uname -s)"
  ARCH="$(uname -m)"

  case "$PLATFORM" in
    Linux)
      PLATFORM="linux"
      ;;
    Darwin)
      PLATFORM="darwin"
      ;;
    *)
      err "unsupported platform: $PLATFORM"
      ;;
  esac

  case "$ARCH" in
    x86_64|amd64)
      ARCH="x86_64"
      ;;
    aarch64|arm64)
      ARCH="arm64"
      ;;
    *)
      err "unsupported architecture: $ARCH"
      ;;
  esac
}

check_platform_support() {
  if [ "$PLATFORM" = "darwin" ] && [ "$ARCH" = "x86_64" ]; then
    err "Apple Silicon (M1+) required — Intel Macs are not supported"
  fi
}

detect_distro() {
  if [ "$PLATFORM" != "linux" ]; then
    return 0
  fi

  if [ -r /etc/os-release ]; then
    # shellcheck source=/dev/null
    . /etc/os-release
    DISTRO_ID="${ID:-unknown}"
    DISTRO_VERSION="${VERSION_ID:-unknown}"
  else
    DISTRO_ID="unknown"
    DISTRO_VERSION="unknown"
  fi
}

get_package_manager() {
  case "$DISTRO_ID" in
    debian|ubuntu)
      echo "apt"
      ;;
    fedora|rhel|centos|rocky|almalinux)
      echo "dnf"
      ;;
    amzn)
      echo "dnf"
      ;;
    *)
      echo "unknown"
      ;;
  esac
}

# --- macOS helpers ----------------------------------------------------------

check_macos_version() {
  if [ "$PLATFORM" != "darwin" ]; then
    return 0
  fi

  macos_version="$(sw_vers -productVersion 2>/dev/null || echo "0.0")"
  macos_major="$(echo "$macos_version" | cut -d. -f1)"

  if [ "$macos_major" -lt 14 ]; then
    err "macOS 14 (Sonoma) or later required — found macOS $macos_version"
  fi
}

check_homebrew() {
  if ! check_cmd brew; then
    err "Homebrew is required on macOS — install from https://brew.sh"
  fi
}

# --- Linux helpers ----------------------------------------------------------

check_kvm_access() {
  if [ "$PLATFORM" != "linux" ]; then
    return 0
  fi

  if [ ! -c /dev/kvm ]; then
    say_warn "/dev/kvm not found — KVM is required for microVM isolation"
    say_warn "If running in a VM, enable nested virtualization"
    return 0
  fi

  if [ ! -r /dev/kvm ] || [ ! -w /dev/kvm ]; then
    kvm_group=""
    if check_cmd stat; then
      kvm_group="$(stat -c '%G' /dev/kvm 2>/dev/null || echo "kvm")"
    else
      kvm_group="kvm"
    fi
    say_warn "/dev/kvm exists but is not accessible"
    say_warn "Add your user to the '$kvm_group' group: sudo usermod -aG $kvm_group \$USER"
    say_warn "Then log out and back in"
  fi
}

# --- Version resolution -----------------------------------------------------

resolve_neovex_version() {
  if [ -n "$NEOVEX_VERSION" ]; then
    return 0
  fi

  say_info "Resolving latest neovex version..."

  response="$(download "${NEOVEX_RELEASES_API}/latest" 2>/dev/null || true)"

  if [ -z "$response" ]; then
    err "failed to fetch latest neovex release — try --version <tag> or set GITHUB_TOKEN"
  fi

  # Simple JSON parsing for tag_name — avoids jq dependency
  NEOVEX_VERSION="$(echo "$response" | tr ',' '\n' | grep '"tag_name"' | head -1 | sed 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')"

  if [ -z "$NEOVEX_VERSION" ]; then
    # Check for rate limiting
    if echo "$response" | grep -qi "rate limit"; then
      err "GitHub API rate limit reached — try --version <tag> or set GITHUB_TOKEN"
    fi
    err "failed to parse latest neovex version from GitHub API"
  fi

  say_info "Latest neovex version: $NEOVEX_VERSION"
}

resolve_crun_version() {
  if [ -n "$NEOVEX_CRUN_VERSION" ]; then
    return 0
  fi

  if [ "$PLATFORM" != "linux" ]; then
    return 0
  fi

  say_info "Resolving latest neovex-crun version..."

  response="$(download "${NEOVEX_CRUN_RELEASES_API}/latest" 2>/dev/null || true)"

  if [ -z "$response" ]; then
    err "failed to fetch latest neovex-crun release — try --crun-version <tag> or set GITHUB_TOKEN"
  fi

  NEOVEX_CRUN_VERSION="$(echo "$response" | tr ',' '\n' | grep '"tag_name"' | head -1 | sed 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')"

  if [ -z "$NEOVEX_CRUN_VERSION" ]; then
    if echo "$response" | grep -qi "rate limit"; then
      err "GitHub API rate limit reached — try --crun-version <tag> or set GITHUB_TOKEN"
    fi
    err "failed to parse latest neovex-crun version from GitHub API"
  fi

  say_info "Latest neovex-crun version: $NEOVEX_CRUN_VERSION"
}

# --- Asset naming -----------------------------------------------------------

get_neovex_asset_name() {
  case "$PLATFORM" in
    linux)
      case "$ARCH" in
        x86_64) echo "neovex_linux_x86_64.tar.gz" ;;
        arm64) echo "neovex_linux_arm64.tar.gz" ;;
      esac
      ;;
    darwin)
      echo "neovex_darwin_arm64.tar.gz"
      ;;
  esac
}

get_crun_asset_name() {
  case "$ARCH" in
    x86_64) echo "neovex-crun-linux-amd64" ;;
    arm64) echo "neovex-crun-linux-arm64" ;;
  esac
}

# --- Sudo handling ----------------------------------------------------------

maybe_sudo() {
  if [ "$(id -u)" -eq 0 ]; then
    "$@"
  elif check_cmd sudo; then
    sudo "$@"
  else
    err "need sudo to install to system paths"
  fi
}

# --- Interactive detection --------------------------------------------------

is_interactive() {
  # When piped (curl | sh), stdin is the pipe, not the terminal
  [ -t 0 ]
}

confirm() {
  prompt="$1"
  if [ -n "$YES" ] || ! is_interactive; then
    return 0
  fi

  printf '%s [y/N] ' "$prompt"
  read -r answer
  case "$answer" in
    [Yy]|[Yy][Ee][Ss])
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

# --- Idempotent checks ------------------------------------------------------

get_installed_neovex_version() {
  if check_cmd neovex; then
    neovex --version 2>/dev/null | head -1 | sed 's/neovex /v/'
  fi
}

get_installed_crun_version() {
  crun_path="/usr/libexec/neovex/crun"
  if [ -x "$crun_path" ]; then
    "$crun_path" --version 2>/dev/null | head -1 | sed 's/crun version /v/' | sed 's/ .*//'
  fi
}

# --- Print install plan -----------------------------------------------------

print_install_plan() {
  say ""
  say "=== Neovex Install Plan ==="
  say ""
  say "Platform:      $PLATFORM ($ARCH)"

  if [ "$PLATFORM" = "linux" ]; then
    say "Distribution:  $DISTRO_ID $DISTRO_VERSION"
    say "Package mgr:   $(get_package_manager)"
  elif [ "$PLATFORM" = "darwin" ]; then
    say "macOS version: $(sw_vers -productVersion 2>/dev/null || echo "unknown")"
  fi

  say ""
  say "Versions:"
  if [ "$PLATFORM" = "darwin" ]; then
    if [ -n "$NEOVEX_VERSION" ]; then
      say "  neovex:      current Homebrew cask (ignoring requested pin ${NEOVEX_VERSION})"
    else
      say "  neovex:      current Homebrew cask"
    fi
  else
    say "  neovex:      ${NEOVEX_VERSION:-latest}"
  fi
  if [ "$PLATFORM" = "linux" ]; then
    say "  neovex-crun: ${NEOVEX_CRUN_VERSION:-latest}"
  fi

  say ""
  say "Install paths:"
  if [ "$PLATFORM" = "linux" ]; then
    say "  neovex:      ${NEOVEX_PREFIX}/bin/neovex"
    say "  neovex-crun: /usr/libexec/neovex/crun"
  elif [ "$PLATFORM" = "darwin" ]; then
    say "  neovex:      \$(brew --prefix)/bin/neovex (via Homebrew cask)"
    say "  gvproxy:     \$(brew --prefix)/Caskroom/neovex/<version>/libexec/gvproxy"
    say "  krunkit:     \$(brew --prefix)/bin/krunkit (via Homebrew formula dependency)"
  fi

  if [ "$PLATFORM" = "linux" ] && [ -z "$SKIP_DEPS" ]; then
    say ""
    say "System dependencies to install:"
    pkg_mgr="$(get_package_manager)"
    case "$pkg_mgr" in
      apt)
        say "  apt-get install: conmon buildah catatonit passt uidmap fuse-overlayfs"
        if [ "$DISTRO_ID" = "debian" ] || [ "$DISTRO_ID" = "ubuntu" ]; then
          say "  libkrun/libkrunfw: manual build required (not in repos)"
        fi
        ;;
      dnf)
        say "  dnf install: conmon buildah catatonit passt shadow-utils fuse-overlayfs libkrun libkrunfw"
        ;;
      *)
        say "  (unknown package manager — manual installation required)"
        ;;
    esac
  fi

  if [ "$PLATFORM" = "linux" ]; then
    say ""
    say "Supply-chain verification:"
    say "  checksum:     enforced"
    if check_cmd gh; then
      say "  attestation:  GitHub provenance verification enabled for neovex"
    elif [ -n "$REQUIRE_ATTESTATIONS" ]; then
      say "  attestation:  required, but gh CLI is missing"
    else
      say "  attestation:  best-effort (install gh or set NEOVEX_REQUIRE_ATTESTATIONS=1 to fail closed)"
    fi
  fi

  say ""
}

warn_ignored_args_for_platform() {
  if [ "$PLATFORM" != "darwin" ]; then
    return 0
  fi

  if [ -n "$NEOVEX_VERSION" ]; then
    say_warn "--version is currently ignored on macOS — Homebrew installs the published neovex cask version"
  fi
  if [ -n "$NEOVEX_CRUN_VERSION" ]; then
    say_warn "--crun-version is ignored on macOS"
  fi
  if [ "$NEOVEX_PREFIX" != "/usr/local" ]; then
    say_warn "--prefix is ignored on macOS — Homebrew manages the install prefix"
  fi
}

# --- Linux installation -----------------------------------------------------

install_deps_debian() {
  if [ -n "$SKIP_DEPS" ]; then
    say_info "Skipping system dependency installation (--skip-deps)"
    return 0
  fi

  if [ -n "$DRY_RUN" ]; then
    say_info "[dry-run] Would install: conmon buildah catatonit passt uidmap fuse-overlayfs"
    return 0
  fi

  say_info "Installing system dependencies via apt..."
  maybe_sudo apt-get update -qq
  maybe_sudo apt-get install -y conmon buildah catatonit passt uidmap fuse-overlayfs

  say ""
  say_warn "libkrun and libkrunfw are not yet available as Debian/Ubuntu packages."
  say ""
  say "To build from source:"
  say "  git clone https://github.com/containers/libkrunfw && cd libkrunfw"
  say "  git checkout v5.3.0 && make && sudo make install"
  say ""
  say "  git clone https://github.com/containers/libkrun && cd libkrun"
  say "  git checkout v1.17.4 && make && sudo make install"
  say ""
  say "  echo \"/usr/local/lib64\" | sudo tee /etc/ld.so.conf.d/libkrun.conf"
  say "  sudo ldconfig"
  say ""
  say "On Fedora, these are available from repos: sudo dnf install libkrun libkrunfw"
  say ""
}

install_deps_fedora() {
  if [ -n "$SKIP_DEPS" ]; then
    say_info "Skipping system dependency installation (--skip-deps)"
    return 0
  fi

  if [ -n "$DRY_RUN" ]; then
    say_info "[dry-run] Would install: conmon buildah catatonit passt shadow-utils fuse-overlayfs libkrun libkrunfw"
    return 0
  fi

  say_info "Installing system dependencies via dnf..."
  maybe_sudo dnf install -y conmon buildah catatonit passt shadow-utils fuse-overlayfs libkrun libkrunfw
}

install_system_deps() {
  pkg_mgr="$(get_package_manager)"
  case "$pkg_mgr" in
    apt)
      install_deps_debian
      ;;
    dnf)
      install_deps_fedora
      ;;
    *)
      say_warn "Unknown package manager — skipping system dependency installation"
      say_warn "Please install manually: conmon buildah catatonit passt libkrun libkrunfw"
      ;;
  esac
}

download_and_install_neovex_linux() {
  if [ -n "$DRY_RUN" ]; then
    say_info "[dry-run] Would download and install neovex $NEOVEX_VERSION to ${NEOVEX_PREFIX}/bin/neovex"
    return 0
  fi

  asset_name="$(get_neovex_asset_name)"
  download_url="${NEOVEX_RELEASES_DOWNLOAD}/${NEOVEX_VERSION}/${asset_name}"
  checksums_url="${NEOVEX_RELEASES_DOWNLOAD}/${NEOVEX_VERSION}/checksums-sha256.txt"

  tmpdir="$(mktemp -d)"
  trap 'rm -rf "$tmpdir"' EXIT

  say_info "Downloading checksums for neovex ${NEOVEX_VERSION}..."
  download_to_file "$checksums_url" "$tmpdir/checksums-sha256.txt"

  # Check if same version already installed
  installed_version="$(get_installed_neovex_version)"
  if [ "$installed_version" = "$NEOVEX_VERSION" ]; then
    say_info "neovex $NEOVEX_VERSION is already installed — skipping"
    return 0
  elif [ -n "$installed_version" ]; then
    say_info "Upgrading neovex from $installed_version to $NEOVEX_VERSION"
  fi

  say_info "Downloading neovex ${NEOVEX_VERSION}..."
  download_to_file "$download_url" "$tmpdir/$asset_name"

  say_info "Verifying checksum..."
  verify_file_checksum "$tmpdir/$asset_name" "$tmpdir/checksums-sha256.txt" "$asset_name"
  verify_github_attestation \
    "$tmpdir/$asset_name" \
    "agentstation/neovex" \
    "refs/tags/$NEOVEX_VERSION" \
    "agentstation/neovex/.github/workflows/release.yml" \
    "$asset_name"

  say_info "Extracting and installing..."
  tar -xzf "$tmpdir/$asset_name" -C "$tmpdir"

  maybe_sudo install -d "${NEOVEX_PREFIX}/bin"
  maybe_sudo install -m 0755 "$tmpdir/neovex" "${NEOVEX_PREFIX}/bin/neovex"

  say_info "Installed neovex to ${NEOVEX_PREFIX}/bin/neovex"
}

download_and_install_crun() {
  if [ -n "$DRY_RUN" ]; then
    say_info "[dry-run] Would download and install neovex-crun $NEOVEX_CRUN_VERSION to /usr/libexec/neovex/crun"
    return 0
  fi

  asset_name="$(get_crun_asset_name)"
  download_url="${NEOVEX_CRUN_RELEASES_DOWNLOAD}/${NEOVEX_CRUN_VERSION}/${asset_name}"
  checksums_url="${NEOVEX_CRUN_RELEASES_DOWNLOAD}/${NEOVEX_CRUN_VERSION}/checksums.txt"

  tmpdir="$(mktemp -d)"
  trap 'rm -rf "$tmpdir"' EXIT

  say_info "Downloading checksums for neovex-crun ${NEOVEX_CRUN_VERSION}..."
  download_to_file "$checksums_url" "$tmpdir/checksums.txt"

  # Check if the installed binary already matches the target release.
  crun_path="/usr/libexec/neovex/crun"
  if [ -x "$crun_path" ]; then
    crun_version="$("$crun_path" --version 2>/dev/null | head -1 || true)"
    if echo "$crun_version" | grep -q '+LIBKRUN' && file_matches_manifest_checksum "$crun_path" "$tmpdir/checksums.txt" "$asset_name"; then
      say_info "neovex-crun $NEOVEX_CRUN_VERSION is already installed — skipping"
      return 0
    fi
  fi

  say_info "Downloading neovex-crun ${NEOVEX_CRUN_VERSION}..."
  download_to_file "$download_url" "$tmpdir/$asset_name"

  say_info "Verifying checksum..."
  verify_file_checksum "$tmpdir/$asset_name" "$tmpdir/checksums.txt" "$asset_name"
  verify_github_attestation \
    "$tmpdir/$asset_name" \
    "agentstation/neovex-crun" \
    "refs/tags/$NEOVEX_CRUN_VERSION" \
    "agentstation/neovex-crun/.github/workflows/build.yml" \
    "$asset_name"

  say_info "Installing neovex-crun..."
  maybe_sudo install -d /usr/libexec/neovex
  maybe_sudo install -m 0755 "$tmpdir/$asset_name" /usr/libexec/neovex/crun

  say_info "Installed neovex-crun to /usr/libexec/neovex/crun"
}

install_linux() {
  check_kvm_access
  install_system_deps
  resolve_neovex_version
  resolve_crun_version
  download_and_install_neovex_linux
  download_and_install_crun
  verify_installation
  print_getting_started_linux
}

print_getting_started_linux() {
  say ""
  say "=== Getting Started ==="
  say ""
  say "Neovex is installed! To start the server:"
  say ""
  say "  neovex serve"
  say ""
  say "For more information:"
  say "  neovex --help"
  say "  https://neovex.dev/docs"
  say ""
}

# --- macOS installation -----------------------------------------------------

install_or_upgrade_homebrew_cask() {
  if [ -n "$DRY_RUN" ]; then
    say_info "[dry-run] Would install or upgrade agentstation/tap/neovex via Homebrew"
    return 0
  fi

  say_info "Tapping agentstation/tap..."
  brew tap agentstation/tap 2>/dev/null || true
  brew tap slp/krunkit 2>/dev/null || true

  if brew list --cask neovex >/dev/null 2>&1; then
    say_info "Upgrading neovex cask..."
    brew upgrade --cask neovex
  else
    say_info "Installing neovex cask..."
    brew install --cask agentstation/tap/neovex
  fi
}

install_macos() {
  check_macos_version
  check_homebrew
  install_or_upgrade_homebrew_cask
  verify_installation
  print_getting_started_macos
}

print_getting_started_macos() {
  say ""
  say "=== Getting Started ==="
  say ""
  say "Neovex is installed! To initialize and start the machine VM:"
  say ""
  say "  neovex machine init"
  say "  neovex serve"
  say ""
  say "For more information:"
  say "  neovex --help"
  say "  https://neovex.dev/docs"
  say ""
}

# --- Uninstall --------------------------------------------------------------

uninstall_linux() {
  say_info "Uninstalling neovex from Linux..."

  if [ -n "$DRY_RUN" ]; then
    say_info "[dry-run] Would remove ${NEOVEX_PREFIX}/bin/neovex"
    say_info "[dry-run] Would remove /usr/libexec/neovex/crun"
    return 0
  fi

  if [ -f "${NEOVEX_PREFIX}/bin/neovex" ]; then
    maybe_sudo rm -f "${NEOVEX_PREFIX}/bin/neovex"
    say_info "Removed ${NEOVEX_PREFIX}/bin/neovex"
  fi

  if [ -f "/usr/libexec/neovex/crun" ]; then
    maybe_sudo rm -f "/usr/libexec/neovex/crun"
    say_info "Removed /usr/libexec/neovex/crun"
  fi

  if [ -d "/usr/libexec/neovex" ]; then
    maybe_sudo rmdir "/usr/libexec/neovex" 2>/dev/null || true
  fi

  say_info "Neovex uninstalled"
  say ""
  say "System dependencies (conmon, buildah, etc.) were not removed."
  say "Remove them manually if no longer needed."
}

uninstall_macos() {
  say_info "Uninstalling neovex from macOS..."

  if [ -n "$DRY_RUN" ]; then
    say_info "[dry-run] Would run: brew uninstall --cask neovex"
    return 0
  fi

  if brew list --cask neovex >/dev/null 2>&1; then
    brew uninstall --cask neovex
    say_info "Uninstalled neovex cask"
  else
    say_info "neovex cask is not installed"
  fi

  say ""
  say "Note: krunkit (installed as a dependency) was not removed."
  say "Run 'brew autoremove' or 'brew uninstall krunkit' if no longer needed."
}

# --- Verification -----------------------------------------------------------

verify_installation() {
  if [ -n "$DRY_RUN" ]; then
    say_info "[dry-run] Would verify installation"
    return 0
  fi

  script_dir="$(cd "$(dirname "$0")" 2>/dev/null && pwd || true)"
  if [ -f "${script_dir}/verify-install.sh" ] && check_cmd bash; then
    say_info "Running installation verification..."
    if NEOVEX_PREFIX="$NEOVEX_PREFIX" bash "${script_dir}/verify-install.sh"; then
      say_info "Verification passed"
    else
      say_warn "Verification reported issues — see output above"
    fi
    return 0
  fi

  say_info "Running inline installation verification..."
  if verify_installation_inline; then
    say_info "Inline verification passed"
  else
    say_warn "Inline verification reported issues — see output above"
  fi
}

inline_failures=0
inline_warnings=0

inline_print_line() {
  printf '%-22s %s\n' "$1" "$2"
}

inline_mark_failure() {
  inline_failures=$((inline_failures + 1))
}

inline_mark_warning() {
  inline_warnings=$((inline_warnings + 1))
}

inline_check_command() {
  label="$1"
  command_name="$2"
  required="${3:-required}"

  if command_path="$(command -v "$command_name" 2>/dev/null)"; then
    inline_print_line "$label" "present path=$command_path"
    return 0
  fi

  inline_print_line "$label" "missing"
  if [ "$required" = "required" ]; then
    inline_mark_failure
  else
    inline_mark_warning
  fi
}

inline_check_linux_shared_lib() {
  label="$1"
  soname="$2"
  required="${3:-required}"
  found_path=""

  if check_cmd ldconfig; then
    found_path="$(ldconfig -p 2>/dev/null | awk -v name="$soname" '$0 ~ name { print $NF; exit }' || true)"
  fi

  if [ -z "$found_path" ]; then
    for candidate in /usr/local/lib64/${soname}* /usr/local/lib/${soname}* /usr/lib64/${soname}* /usr/lib/${soname}*; do
      if [ -f "$candidate" ]; then
        found_path="$candidate"
        break
      fi
    done
  fi

  if [ -n "$found_path" ]; then
    inline_print_line "$label" "present path=$found_path"
    return 0
  fi

  inline_print_line "$label" "missing"
  if [ "$required" = "required" ]; then
    inline_mark_failure
  else
    inline_mark_warning
  fi
}

verify_linux_inline() {
  if [ -x "${NEOVEX_PREFIX}/bin/neovex" ]; then
    inline_print_line "neovex" "present path=${NEOVEX_PREFIX}/bin/neovex"
  elif command -v neovex >/dev/null 2>&1; then
    inline_print_line "neovex" "present path=$(command -v neovex)"
  else
    inline_print_line "neovex" "missing"
    inline_mark_failure
  fi

  crun_path="/usr/libexec/neovex/crun"
  if [ -x "$crun_path" ]; then
    crun_version="$("$crun_path" --version 2>/dev/null | head -1 || true)"
    if echo "$crun_version" | grep -q '+LIBKRUN'; then
      inline_print_line "neovex-crun" "present path=$crun_path version=$crun_version"
    else
      inline_print_line "neovex-crun" "present path=$crun_path (missing +LIBKRUN flag)"
      inline_mark_failure
    fi
  else
    inline_print_line "neovex-crun" "missing path=$crun_path"
    inline_mark_failure
  fi

  inline_check_command "conmon" "conmon" required
  inline_check_command "buildah" "buildah" required
  inline_check_command "catatonit" "catatonit" recommended
  inline_check_command "passt" "passt" recommended
  inline_check_command "newuidmap" "newuidmap" recommended
  inline_check_command "fuse-overlayfs" "fuse-overlayfs" recommended
  inline_check_linux_shared_lib "libkrun.so" "libkrun.so" required
  inline_check_linux_shared_lib "libkrunfw.so" "libkrunfw.so" required
}

resolve_macos_gvproxy_path() {
  neovex_path="$(command -v neovex 2>/dev/null || true)"
  if [ -z "$neovex_path" ]; then
    return 1
  fi

  real_path="$(readlink "$neovex_path" 2>/dev/null || echo "$neovex_path")"
  if [ "${real_path#/}" = "$real_path" ]; then
    real_path="$(cd "$(dirname "$neovex_path")" && cd "$(dirname "$real_path")" && pwd)/$(basename "$real_path")"
  fi

  case "$real_path" in
    *Caskroom*)
      printf '%s\n' "$(dirname "$real_path")/libexec/gvproxy"
      return 0
      ;;
  esac

  brew_prefix="$(brew --prefix 2>/dev/null || echo "/opt/homebrew")"
  for candidate in "${brew_prefix}/bin/gvproxy" "/usr/local/bin/gvproxy"; do
    if [ -x "$candidate" ]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done

  return 1
}

verify_macos_inline() {
  inline_check_command "neovex" "neovex" required
  inline_check_command "krunkit" "krunkit" required

  if gvproxy_path="$(resolve_macos_gvproxy_path)"; then
    inline_print_line "gvproxy" "present path=$gvproxy_path"
  else
    inline_print_line "gvproxy" "missing"
    inline_mark_failure
  fi
}

verify_installation_inline() {
  inline_failures=0
  inline_warnings=0

  case "$PLATFORM" in
    linux)
      verify_linux_inline
      ;;
    darwin)
      verify_macos_inline
      ;;
    *)
      inline_print_line "host.support" "unsupported ($PLATFORM)"
      inline_mark_failure
      ;;
  esac

  say ""
  if [ "$inline_failures" -eq 0 ] && [ "$inline_warnings" -eq 0 ]; then
    inline_print_line "result" "supported (0 failures)"
    return 0
  fi
  if [ "$inline_failures" -eq 0 ]; then
    inline_print_line "result" "supported (0 failures, ${inline_warnings} warnings)"
    return 0
  fi

  inline_print_line "result" "unsupported (${inline_failures} failures, ${inline_warnings} warnings)"
  return 1
}

# --- Argument parsing -------------------------------------------------------

usage() {
  cat <<EOF
Neovex install script

Usage:
  install.sh [options]

Options:
  --version <tag>       Pin neovex version (e.g., v0.1.14)
                        Linux only; macOS installs the current Homebrew cask
  --crun-version <tag>  Pin neovex-crun version (Linux only)
  --prefix <path>       Install prefix (default: /usr/local, Linux only)
  --skip-deps           Skip system dependency installation
  --dry-run             Print what would happen without executing
  --uninstall           Remove neovex and neovex-crun
  -y, --yes             Skip interactive confirmation prompts
  -h, --help            Show this help message

Environment:
  GITHUB_TOKEN          Optional GitHub API auth for public release lookups
  HTTPS_PROXY           HTTP proxy for downloads
  HTTP_PROXY            HTTP proxy for downloads
  NO_PROXY              Hosts to exclude from proxy
  NEOVEX_REQUIRE_ATTESTATIONS
                        Fail closed if GitHub artifact attestation verification
                        cannot run or fails

Examples:
  # Install latest version
  curl -fsSL https://neovex.dev/install.sh | sh

  # Install specific version
  curl -fsSL https://neovex.dev/install.sh | sh -s -- --version v0.1.14

  # Dry run (see what would happen)
  curl -fsSL https://neovex.dev/install.sh | sh -s -- --dry-run

  # Uninstall
  curl -fsSL https://neovex.dev/install.sh | sh -s -- --uninstall
EOF
}

parse_args() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --version)
        shift
        if [ $# -eq 0 ]; then
          err "--version requires a value"
        fi
        NEOVEX_VERSION="$1"
        ;;
      --crun-version)
        shift
        if [ $# -eq 0 ]; then
          err "--crun-version requires a value"
        fi
        NEOVEX_CRUN_VERSION="$1"
        ;;
      --prefix)
        shift
        if [ $# -eq 0 ]; then
          err "--prefix requires a value"
        fi
        NEOVEX_PREFIX="$1"
        ;;
      --skip-deps)
        SKIP_DEPS="1"
        ;;
      --dry-run)
        DRY_RUN="1"
        ;;
      --uninstall)
        UNINSTALL="1"
        ;;
      -y|--yes)
        YES="1"
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        err "unknown option: $1"
        ;;
    esac
    shift
  done
}

# --- Main -------------------------------------------------------------------

main() {
  parse_args "$@"

  detect_platform
  check_platform_support
  detect_distro
  warn_ignored_args_for_platform

  if [ -n "$UNINSTALL" ]; then
    case "$PLATFORM" in
      linux)
        uninstall_linux
        ;;
      darwin)
        uninstall_macos
        ;;
    esac
    exit 0
  fi

  if [ -n "$DRY_RUN" ]; then
    if [ "$PLATFORM" = "linux" ]; then
      resolve_neovex_version
      resolve_crun_version
    fi
    print_install_plan
    say "[dry-run] No changes made"
    exit 0
  fi

  print_install_plan

  if ! confirm "Proceed with installation?"; then
    say "Installation cancelled"
    exit 0
  fi

  case "$PLATFORM" in
    linux)
      install_linux
      ;;
    darwin)
      install_macos
      ;;
  esac
}

main "$@"
