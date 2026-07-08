#!/bin/sh
# shellcheck shell=sh
# SPDX-License-Identifier: LicenseRef-Nimbus-Community
# License text: see the LICENSE asset in the same GitHub Release, or
# https://github.com/nimbus/nimbus/releases/latest/download/LICENSE
# Nimbus install script — portable bootstrapper for all supported platforms.
#
# Usage:
#   curl -fsSL https://github.com/nimbus/nimbus/releases/latest/download/install.sh | sh
#   curl -fsSL https://github.com/nimbus/nimbus/releases/latest/download/install.sh | sh -s -- --version v0.1.14
#   curl -fsSL https://github.com/nimbus/nimbus/releases/latest/download/install.sh | sh -s -- --dry-run
#   curl -fsSL https://github.com/nimbus/nimbus/releases/latest/download/install.sh | sh -s -- --uninstall
#
# See docs/private/plans/install-script-plan.md for the full contract.

set -eu

# --- Globals ----------------------------------------------------------------

NIMBUS_VERSION=""
NIMBUS_CRUN_VERSION=""
NIMBUS_CRUN_UPSTREAM_VERSION=""
NIMBUS_LIBKRUN_VERSION=""
NIMBUS_LIBKRUN_UPSTREAM_VERSION=""
NIMBUS_PREFIX="/usr/local"
INSTALL_BUN_JSC_ADAPTER="${NIMBUS_INSTALL_BUN_JSC_ADAPTER:-}"
DRY_RUN=""
SKIP_DEPS=""
UNINSTALL=""
YES=""
REQUIRE_ATTESTATIONS="${NIMBUS_REQUIRE_ATTESTATIONS:-}"
PLATFORM=""
ARCH=""
DISTRO_ID=""
DISTRO_VERSION=""

# Single per-run scratch directory. Every download stages into a subdirectory
# of this root so one EXIT trap (registered once by ensure_workdir) cleans up
# everything. Do not set a per-function `trap ... EXIT`: POSIX sh keeps only one
# EXIT trap, so a second download would clobber the first one's cleanup and leak
# the earlier directory.
NIMBUS_WORKDIR=""

# Keep these constants aligned with scripts/bun-jsc-adapter-contract.sh and
# crates/nimbus-runtime/src/backends/bun_jsc/manifest.rs.
BUN_JSC_ADAPTER_SCHEMA_VERSION=1
BUN_JSC_ADAPTER_KIND="nimbus.bun_jsc.adapter"
BUN_JSC_ADAPTER_ABI_NAME="nimbus-bun-jsc-embedder"
BUN_JSC_ADAPTER_ABI_VERSION=1
BUN_JSC_ADAPTER_MEMORY_ENFORCEMENT="outer_quota_required"
BUN_JSC_ADAPTER_LIFECYCLE="fresh_discard"
BUN_JSC_ADAPTER_MANIFEST_FILE="nimbus-bun-jsc-adapter.json"
BUN_JSC_ADAPTER_CHECKSUMS_FILE="checksums-sha256.txt"
BUN_JSC_ADAPTER_README_FILE="README.md"
BUN_JSC_ADAPTER_SOURCE_REPOSITORY="https://github.com/nimbus/bun"
BUN_JSC_ADAPTER_SOURCE_REF="nimbus-bun-jsc-proof-main-20260708"
BUN_JSC_ADAPTER_SOURCE_REVISION="9c9ed55fd8859b1e27fed2afaae770a4b7527574"
BUN_JSC_ADAPTER_SBOM_FILE="nimbus-bun-jsc-adapter.sbom.cdx.json"
BUN_JSC_ADAPTER_SLSA_FILE="nimbus-bun-jsc-adapter.intoto.jsonl"

bun_jsc_adapter_required_exports() {
  cat <<'EOF'
nimbus_bun_embed_probe_construct_and_destroy_vm
nimbus_bun_embed_probe_sync_host_call
nimbus_bun_embed_probe_async_host_call
nimbus_bun_embed_probe_program_bundle_host_calls
nimbus_bun_embed_probe_timeout_and_cancel
nimbus_bun_embed_probe_permission_surface_inventory
nimbus_bun_embed_probe_memory_behavior
nimbus_bun_embed_probe_package_module_policy
nimbus_bun_embed_probe_lifecycle_reuse_stress
nimbus_bun_embed_invoke_program_wrapper_json
nimbus_bun_embed_invoke_program_wrapper_json_with_host_bridge
EOF
}

# GitHub API endpoints
NIMBUS_RELEASES_API="https://api.github.com/repos/nimbus/nimbus/releases"
NIMBUS_CRUN_RELEASES_API="https://api.github.com/repos/nimbus/nimbus-crun/releases"
NIMBUS_LIBKRUN_RELEASES_API="https://api.github.com/repos/nimbus/nimbus-libkrun/releases"

# Release asset base URLs
NIMBUS_RELEASES_DOWNLOAD="https://github.com/nimbus/nimbus/releases/download"
NIMBUS_CRUN_RELEASES_DOWNLOAD="https://github.com/nimbus/nimbus-crun/releases/download"
NIMBUS_LIBKRUN_RELEASES_DOWNLOAD="https://github.com/nimbus/nimbus-libkrun/releases/download"

# Default Linux VMM tuple. In a checked-out repo, load_linux_distribution_contract
# consumes packaging/linux-distribution-contract.env as the source of truth. The
# baked values keep standalone curl|sh installs pinned to the same validated
# tuple instead of resolving fork artifacts through GitHub "latest".
LINUX_DISTRIBUTION_CONTRACT_ENV="${NIMBUS_LINUX_DISTRIBUTION_CONTRACT_ENV:-packaging/linux-distribution-contract.env}"
DEFAULT_NIMBUS_CRUN_VERSION="v1.27.1-nimbus.2"
DEFAULT_NIMBUS_CRUN_UPSTREAM_VERSION="1.27.1"
DEFAULT_NIMBUS_LIBKRUN_VERSION="v1.18.1-nimbus.1"
DEFAULT_NIMBUS_LIBKRUN_UPSTREAM_VERSION="1.18.1"
LINUX_DISTRIBUTION_CONTRACT_LOADED=""

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

resolve_nimbus_version() {
  if [ -n "$NIMBUS_VERSION" ]; then
    return 0
  fi

  say_info "Resolving latest nimbus version..."

  response="$(download "${NIMBUS_RELEASES_API}/latest" 2>/dev/null || true)"

  if [ -z "$response" ]; then
    err "failed to fetch latest nimbus release — try --version <tag> or set GITHUB_TOKEN"
  fi

  # Simple JSON parsing for tag_name — avoids jq dependency
  NIMBUS_VERSION="$(echo "$response" | tr ',' '\n' | grep '"tag_name"' | head -1 | sed 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')"

  if [ -z "$NIMBUS_VERSION" ]; then
    # Check for rate limiting
    if echo "$response" | grep -qi "rate limit"; then
      err "GitHub API rate limit reached — try --version <tag> or set GITHUB_TOKEN"
    fi
    err "failed to parse latest nimbus version from GitHub API"
  fi

  say_info "Latest nimbus version: $NIMBUS_VERSION"
}

resolve_crun_version() {
  if [ -n "$NIMBUS_CRUN_VERSION" ]; then
    return 0
  fi

  if [ "$PLATFORM" != "linux" ]; then
    return 0
  fi

  load_linux_distribution_contract
}

resolve_libkrun_version() {
  if [ -n "$NIMBUS_LIBKRUN_VERSION" ]; then
    return 0
  fi

  if [ "$PLATFORM" != "linux" ]; then
    return 0
  fi

  load_linux_distribution_contract
}

load_linux_distribution_contract() {
  if [ -n "$LINUX_DISTRIBUTION_CONTRACT_LOADED" ]; then
    return 0
  fi
  LINUX_DISTRIBUTION_CONTRACT_LOADED="1"

  crun_override="$NIMBUS_CRUN_VERSION"
  libkrun_override="$NIMBUS_LIBKRUN_VERSION"
  contract_source="embedded installer defaults"
  if [ -r "$LINUX_DISTRIBUTION_CONTRACT_ENV" ]; then
    # shellcheck disable=SC1090
    . "$LINUX_DISTRIBUTION_CONTRACT_ENV"
    contract_source="$LINUX_DISTRIBUTION_CONTRACT_ENV"
  fi
  if [ -n "$crun_override" ]; then
    NIMBUS_CRUN_VERSION="$crun_override"
  fi
  if [ -n "$libkrun_override" ]; then
    NIMBUS_LIBKRUN_VERSION="$libkrun_override"
  fi

  if [ -z "$NIMBUS_CRUN_VERSION" ]; then
    NIMBUS_CRUN_VERSION="$DEFAULT_NIMBUS_CRUN_VERSION"
  fi
  if [ -z "$NIMBUS_CRUN_UPSTREAM_VERSION" ]; then
    NIMBUS_CRUN_UPSTREAM_VERSION="$DEFAULT_NIMBUS_CRUN_UPSTREAM_VERSION"
  fi
  if [ -z "$NIMBUS_LIBKRUN_VERSION" ]; then
    NIMBUS_LIBKRUN_VERSION="$DEFAULT_NIMBUS_LIBKRUN_VERSION"
  fi
  if [ -z "$NIMBUS_LIBKRUN_UPSTREAM_VERSION" ]; then
    NIMBUS_LIBKRUN_UPSTREAM_VERSION="$DEFAULT_NIMBUS_LIBKRUN_UPSTREAM_VERSION"
  fi

  say_info "Using validated Linux VMM tuple from ${contract_source}: nimbus-crun ${NIMBUS_CRUN_VERSION} (upstream ${NIMBUS_CRUN_UPSTREAM_VERSION}), nimbus-libkrun ${NIMBUS_LIBKRUN_VERSION} (upstream ${NIMBUS_LIBKRUN_UPSTREAM_VERSION})"
}

# --- Asset naming -----------------------------------------------------------

get_nimbus_asset_name() {
  case "$PLATFORM" in
    linux)
      case "$ARCH" in
        x86_64) echo "nimbus_linux_x86_64.tar.gz" ;;
        arm64) echo "nimbus_linux_arm64.tar.gz" ;;
      esac
      ;;
    darwin)
      echo "nimbus_darwin_arm64.tar.gz"
      ;;
  esac
}

get_crun_asset_name() {
  case "$ARCH" in
    x86_64) echo "nimbus-crun-linux-amd64" ;;
    arm64) echo "nimbus-crun-linux-arm64" ;;
  esac
}

get_libkrun_asset_name() {
  case "$ARCH" in
    x86_64) echo "nimbus-libkrun-linux-amd64.tar.gz" ;;
    arm64) echo "nimbus-libkrun-linux-arm64.tar.gz" ;;
  esac
}

get_bun_jsc_adapter_asset_name() {
  case "$PLATFORM:$ARCH" in
    linux:x86_64)
      echo "nimbus-bun-jsc-adapter-linux-x86_64.tar.gz"
      ;;
    darwin:arm64)
      echo "nimbus-bun-jsc-adapter-darwin-arm64.tar.gz"
      ;;
    *)
      err "optional Bun/JSC adapter release asset is not supported for $PLATFORM $ARCH"
      ;;
  esac
}

get_bun_jsc_adapter_target_triple() {
  case "$PLATFORM:$ARCH" in
    linux:x86_64)
      echo "x86_64-unknown-linux-gnu"
      ;;
    darwin:arm64)
      echo "aarch64-apple-darwin"
      ;;
    *)
      err "optional Bun/JSC adapter target triple is not supported for $PLATFORM $ARCH"
      ;;
  esac
}

get_bun_jsc_adapter_library_basename() {
  case "$PLATFORM" in
    darwin)
      echo "libnimbus_bun_jsc_embedder.dylib"
      ;;
    *)
      echo "libnimbus_bun_jsc_embedder.so"
      ;;
  esac
}

require_bun_jsc_adapter_verifier_tools() {
  need_cmd tar
  need_cmd python3
  need_cmd nm
  need_cmd diff
  if [ "$PLATFORM" = "linux" ]; then
    need_cmd readelf
  fi
}

verify_bun_jsc_adapter_archive_layout() {
  archive_path="$1"
  entries_path="$2"
  library_basename="$3"

  tar -tzf "$archive_path" >"$entries_path"
  while IFS= read -r entry; do
    case "$entry" in
      ""|/*|../*|*"/../"*|*".."*|*/*)
        err "unsafe Bun/JSC adapter archive entry: $entry"
        ;;
      "$library_basename"|"$BUN_JSC_ADAPTER_MANIFEST_FILE"|"$BUN_JSC_ADAPTER_CHECKSUMS_FILE"|"$BUN_JSC_ADAPTER_README_FILE"|"$BUN_JSC_ADAPTER_SBOM_FILE"|"$BUN_JSC_ADAPTER_SLSA_FILE")
        ;;
      *)
        err "unexpected Bun/JSC adapter archive entry: $entry"
        ;;
    esac
  done <"$entries_path"

  duplicate_entry="$(sort "$entries_path" | uniq -d | head -n 1 || true)"
  if [ -n "$duplicate_entry" ]; then
    err "duplicate Bun/JSC adapter archive entry: $duplicate_entry"
  fi

  for required_entry in "$library_basename" "$BUN_JSC_ADAPTER_MANIFEST_FILE" "$BUN_JSC_ADAPTER_CHECKSUMS_FILE" "$BUN_JSC_ADAPTER_README_FILE" "$BUN_JSC_ADAPTER_SBOM_FILE" "$BUN_JSC_ADAPTER_SLSA_FILE"; do
    if ! grep -qx "$required_entry" "$entries_path"; then
      err "Bun/JSC adapter archive is missing required entry: $required_entry"
    fi
  done
}

verify_bun_jsc_adapter_manifest_contract() {
  extract_dir="$1"
  target_triple="$2"
  adapter_platform="$3"
  library_basename="$4"

  manifest_path="$extract_dir/$BUN_JSC_ADAPTER_MANIFEST_FILE"
  library_path="$extract_dir/$library_basename"
  sbom_path="$extract_dir/$BUN_JSC_ADAPTER_SBOM_FILE"
  slsa_path="$extract_dir/$BUN_JSC_ADAPTER_SLSA_FILE"
  required_exports_manifest_file="$extract_dir/.nimbus-bun-jsc-required-exports-manifest.txt"
  required_exports_file="$extract_dir/.nimbus-bun-jsc-required-exports.txt"
  actual_exports_file="$extract_dir/.nimbus-bun-jsc-actual-exports.txt"
  library_sha256="$(sha256_file "$library_path")"

  bun_jsc_adapter_required_exports >"$required_exports_manifest_file"
  sort -u "$required_exports_manifest_file" >"$required_exports_file"

  export BUN_JSC_ADAPTER_SCHEMA_VERSION
  export BUN_JSC_ADAPTER_KIND
  export BUN_JSC_ADAPTER_ABI_NAME
  export BUN_JSC_ADAPTER_ABI_VERSION
  export BUN_JSC_ADAPTER_MEMORY_ENFORCEMENT
  export BUN_JSC_ADAPTER_LIFECYCLE
  export BUN_JSC_ADAPTER_MANIFEST_FILE
  export BUN_JSC_ADAPTER_README_FILE
  export BUN_JSC_ADAPTER_SOURCE_REPOSITORY
  export BUN_JSC_ADAPTER_SOURCE_REF
  export BUN_JSC_ADAPTER_SOURCE_REVISION
  export BUN_JSC_ADAPTER_CHECKSUMS_FILE
  export BUN_JSC_ADAPTER_SBOM_FILE
  export BUN_JSC_ADAPTER_SLSA_FILE
  export target_triple
  export adapter_platform
  export library_basename
  export library_sha256

  python3 - "$manifest_path" "$required_exports_manifest_file" <<'PY'
import json
import os
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
expected_exports_path = pathlib.Path(sys.argv[2])
manifest = json.loads(path.read_text())
allowed_top = {
    "schema_version",
    "kind",
    "adapter_version",
    "nimbus_version",
    "bun_source_repository",
    "bun_source_ref",
    "bun_source_revision",
    "target_triple",
    "platform",
    "library",
    "library_sha256",
    "abi",
    "memory_enforcement",
    "lifecycle",
    "provenance",
}
unknown = set(manifest) - allowed_top
if unknown:
    raise SystemExit(f"unknown manifest fields: {sorted(unknown)}")

expected = {
    "schema_version": int(os.environ["BUN_JSC_ADAPTER_SCHEMA_VERSION"]),
    "kind": os.environ["BUN_JSC_ADAPTER_KIND"],
    "bun_source_repository": os.environ["BUN_JSC_ADAPTER_SOURCE_REPOSITORY"],
    "bun_source_ref": os.environ["BUN_JSC_ADAPTER_SOURCE_REF"],
    "bun_source_revision": os.environ["BUN_JSC_ADAPTER_SOURCE_REVISION"],
    "target_triple": os.environ["target_triple"],
    "platform": os.environ["adapter_platform"],
    "library": os.environ["library_basename"],
    "library_sha256": os.environ["library_sha256"],
    "memory_enforcement": os.environ["BUN_JSC_ADAPTER_MEMORY_ENFORCEMENT"],
    "lifecycle": os.environ["BUN_JSC_ADAPTER_LIFECYCLE"],
}
for key, value in expected.items():
    if manifest.get(key) != value:
        raise SystemExit(f"manifest {key} mismatch: expected {value!r}, got {manifest.get(key)!r}")

for key in ("adapter_version", "nimbus_version"):
    if not isinstance(manifest.get(key), str) or not manifest[key].strip():
        raise SystemExit(f"manifest {key} must be a non-empty string")

abi = manifest.get("abi")
if not isinstance(abi, dict):
    raise SystemExit("manifest abi must be an object")
allowed_abi = {"name", "version", "required_exports"}
unknown_abi = set(abi) - allowed_abi
if unknown_abi:
    raise SystemExit(f"unknown abi fields: {sorted(unknown_abi)}")
if abi.get("name") != os.environ["BUN_JSC_ADAPTER_ABI_NAME"]:
    raise SystemExit("manifest abi.name mismatch")
if abi.get("version") != int(os.environ["BUN_JSC_ADAPTER_ABI_VERSION"]):
    raise SystemExit("manifest abi.version mismatch")
expected_exports = expected_exports_path.read_text().splitlines()
if abi.get("required_exports") != expected_exports:
    raise SystemExit("manifest abi.required_exports mismatch")

provenance = manifest.get("provenance")
if not isinstance(provenance, dict):
    raise SystemExit("manifest provenance must be an object")
allowed_provenance = {"checksum_file", "sbom", "slsa"}
unknown_provenance = set(provenance) - allowed_provenance
if unknown_provenance:
    raise SystemExit(f"unknown provenance fields: {sorted(unknown_provenance)}")
expected_provenance = {
    "checksum_file": os.environ["BUN_JSC_ADAPTER_CHECKSUMS_FILE"],
    "sbom": os.environ["BUN_JSC_ADAPTER_SBOM_FILE"],
    "slsa": os.environ["BUN_JSC_ADAPTER_SLSA_FILE"],
}
for key, value in expected_provenance.items():
    if provenance.get(key) != value:
        raise SystemExit(f"manifest provenance.{key} mismatch")
PY

  python3 - "$sbom_path" "$slsa_path" "$library_basename" "$library_sha256" <<'PY'
import json
import pathlib
import sys

sbom_path = pathlib.Path(sys.argv[1])
slsa_path = pathlib.Path(sys.argv[2])
library_name = sys.argv[3]
library_sha256 = sys.argv[4]

sbom = json.loads(sbom_path.read_text())
if sbom.get("bomFormat") != "CycloneDX":
    raise SystemExit("SBOM evidence must be CycloneDX JSON")
if not isinstance(sbom.get("components"), list):
    raise SystemExit("SBOM evidence must contain components")
component_names = {component.get("name") for component in sbom["components"] if isinstance(component, dict)}
if library_name not in component_names:
    raise SystemExit("SBOM evidence must identify the adapter shared library")
if "bun" not in component_names:
    raise SystemExit("SBOM evidence must identify the Bun source component")
if library_sha256 not in json.dumps(sbom, sort_keys=True):
    raise SystemExit("SBOM evidence must contain the adapter shared library SHA-256")

statements = [
    json.loads(line)
    for line in slsa_path.read_text().splitlines()
    if line.strip()
]
if len(statements) != 1:
    raise SystemExit("SLSA evidence must contain exactly one JSON statement")
statement = statements[0]
if statement.get("_type") != "https://in-toto.io/Statement/v1":
    raise SystemExit("SLSA evidence must be an in-toto statement")
if statement.get("predicateType") != "https://slsa.dev/provenance/v1":
    raise SystemExit("SLSA evidence must use the SLSA provenance v1 predicate")
subjects = statement.get("subject")
if not isinstance(subjects, list):
    raise SystemExit("SLSA evidence must contain subjects")
matched = False
for subject in subjects:
    if not isinstance(subject, dict):
        continue
    if subject.get("name") == library_name and subject.get("digest", {}).get("sha256") == library_sha256:
        matched = True
if not matched:
    raise SystemExit("SLSA evidence must bind the adapter shared library SHA-256")
PY

  nm -D --defined-only -C "$library_path" 2>/dev/null |
    awk '{ print $3 }' |
    sed -E 's/@@.*$//; s/@.*$//' |
    sort -u >"$actual_exports_file"

  leaked_count="$(nm -D --defined-only -C "$library_path" 2>/dev/null |
    awk -v pattern='v8::|hwy::|rust_eh_personality|simdutf::|simdutf__|nimbus_bun_simdutf::|nimbus_bun_simdutf__' '$0 ~ pattern { count++ } END { print count + 0 }')"
  if [ "$leaked_count" -ne 0 ]; then
    err "Bun/JSC adapter archive exports bundled native implementation symbols"
  fi

  if [ "$adapter_platform" = "linux" ]; then
    if readelf -d "$library_path" 2>/dev/null | grep -q TEXTREL; then
      err "Bun/JSC adapter archive has TEXTREL dynamic entries"
    fi
    if readelf -d "$library_path" 2>/dev/null | grep -q STATIC_TLS; then
      err "Bun/JSC adapter archive has STATIC_TLS and is not safe for late dlopen"
    fi
  fi

  if ! diff -u "$required_exports_file" "$actual_exports_file"; then
    err "Bun/JSC adapter archive export set drifted"
  fi
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

get_installed_nimbus_version() {
  if check_cmd nimbus; then
    nimbus --version 2>/dev/null | head -1 | sed 's/nimbus /v/'
  fi
}

get_installed_crun_version() {
  crun_path="/usr/libexec/nimbus/crun"
  if [ -x "$crun_path" ]; then
    "$crun_path" --version 2>/dev/null | head -1 | sed 's/crun version /v/' | sed 's/ .*//'
  fi
}

# --- Print install plan -----------------------------------------------------

print_install_plan() {
  say ""
  say "=== Nimbus Install Plan ==="
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
  say "  nimbus:      ${NIMBUS_VERSION:-latest}"
  if [ "$PLATFORM" = "linux" ]; then
    say "  nimbus-libkrun: ${NIMBUS_LIBKRUN_VERSION:-validated tuple} (upstream ${NIMBUS_LIBKRUN_UPSTREAM_VERSION:-unknown})"
    say "  nimbus-crun: ${NIMBUS_CRUN_VERSION:-validated tuple} (upstream ${NIMBUS_CRUN_UPSTREAM_VERSION:-unknown})"
    if [ -n "$INSTALL_BUN_JSC_ADAPTER" ]; then
      say "  nimbus-bun-jsc-adapter: ${NIMBUS_VERSION:-latest}"
    fi
  fi

  say ""
  say "Install paths:"
  if [ "$PLATFORM" = "linux" ]; then
    say "  nimbus:      ${NIMBUS_PREFIX}/bin/nimbus"
    say "  nimbus-libkrun: /usr/libexec/nimbus/lib"
    say "  nimbus-crun: /usr/libexec/nimbus/crun"
    if [ -n "$INSTALL_BUN_JSC_ADAPTER" ]; then
      say "  nimbus-bun-jsc-adapter: /usr/libexec/nimbus/runtime/bun-jsc/current"
    fi
  elif [ "$PLATFORM" = "darwin" ]; then
    say "  nimbus:      ${NIMBUS_PREFIX}/bin/nimbus"
    say "  gvproxy:     ${NIMBUS_PREFIX}/libexec/gvproxy (bundled, pinned)"
    say "  vfkit:       ${NIMBUS_PREFIX}/libexec/vfkit (bundled, pinned; opt-in NIMBUS_MACHINE_PROVIDER=vfkit)"
    say "  krunkit:     \$(brew --prefix)/bin/krunkit (default backend, via Homebrew libkrun/krun tap)"
    say "  libkrun:     \$(brew --prefix)/lib (optional, krunkit Homebrew dependency)"
  fi

  if [ "$PLATFORM" = "darwin" ] && [ -z "$SKIP_DEPS" ]; then
    say ""
    say "Optional macOS microVM dependencies (only needed for 'nimbus machine'):"
    if check_cmd brew; then
      say "  Homebrew install: brew install libkrun/krun/krunkit (krunkit + gvproxy + libkrun)"
    else
      say "  Homebrew not found — 'nimbus' runs without it; install Homebrew (https://brew.sh)"
      say "  then 'brew install libkrun/krun/krunkit' to enable the 'nimbus machine' dev flow"
    fi
    say "  vfkit backend (opt-in): NIMBUS_MACHINE_PROVIDER=vfkit uses the bundled"
    say "  ${NIMBUS_PREFIX}/libexec/vfkit; 'brew install vfkit' is only needed if you"
    say "  prefer the Homebrew copy. The default backend stays krunkit."
  fi

  if [ "$PLATFORM" = "linux" ] && [ -z "$SKIP_DEPS" ]; then
    say ""
    say "System dependencies to install:"
    pkg_mgr="$(get_package_manager)"
    case "$pkg_mgr" in
      apt)
        say "  apt-get install: conmon buildah catatonit passt uidmap fuse-overlayfs"
        ;;
      dnf)
        say "  dnf install: conmon buildah catatonit passt shadow-utils fuse-overlayfs"
        ;;
      *)
        say "  (unknown package manager — manual installation required)"
        ;;
    esac
  fi

  say ""
  say "Supply-chain verification:"
  say "  checksum:     enforced"
  if check_cmd gh; then
    say "  attestation:  GitHub provenance verification enabled for nimbus"
  elif [ -n "$REQUIRE_ATTESTATIONS" ]; then
    say "  attestation:  required, but gh CLI is missing"
  else
    say "  attestation:  best-effort (install gh or set NIMBUS_REQUIRE_ATTESTATIONS=1 to fail closed)"
  fi
  if [ "$PLATFORM" = "linux" ] && [ -n "$INSTALL_BUN_JSC_ADAPTER" ]; then
    say "  bun/jsc:      optional adapter checksum, manifest, SBOM/SLSA, export, and dlopen-safety checks enforced"
  fi

  say ""
}

warn_ignored_args_for_platform() {
  if [ "$PLATFORM" != "darwin" ]; then
    return 0
  fi

  if [ -n "$NIMBUS_CRUN_VERSION" ]; then
    say_warn "--crun-version is ignored on macOS (nimbus-crun is a Linux-only dependency)"
  fi
  if [ -n "$NIMBUS_LIBKRUN_VERSION" ]; then
    say_warn "--libkrun-version is ignored on macOS — libkrun ships via the krunkit Homebrew formula"
  fi
  if [ -n "$INSTALL_BUN_JSC_ADAPTER" ]; then
    say_warn "--with-bun-jsc is not installed by the macOS path yet — use the nimbus-bun-jsc-adapter release asset or package lane for the same tag"
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
}

install_deps_fedora() {
  if [ -n "$SKIP_DEPS" ]; then
    say_info "Skipping system dependency installation (--skip-deps)"
    return 0
  fi

  if [ -n "$DRY_RUN" ]; then
    say_info "[dry-run] Would install: conmon buildah catatonit passt shadow-utils fuse-overlayfs"
    return 0
  fi

  say_info "Installing system dependencies via dnf..."
  maybe_sudo dnf install -y conmon buildah catatonit passt shadow-utils fuse-overlayfs
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
      say_warn "Please install manually: conmon buildah catatonit passt uidmap fuse-overlayfs"
      ;;
  esac
}

# Create the per-run scratch root (once) and register the single EXIT trap that
# removes it. Idempotent: later calls reuse the existing directory. Each download
# allocates its own `download.XXXXXX` subdirectory underneath, so cleanup of the
# root removes every download's staging area without per-function traps.
ensure_workdir() {
  if [ -z "$NIMBUS_WORKDIR" ]; then
    NIMBUS_WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-install.XXXXXX")"
    trap 'rm -rf "$NIMBUS_WORKDIR"' EXIT
  fi
}

# The machine helpers the macOS archive bundles inside its libexec/. These
# travel under Nimbus provenance and the runtime resolves them bundled-first
# (see resolve_macos_bundled_helper). Linux archives carry no libexec, so the
# bundled set is empty there.
macos_bundled_helper_names() {
  printf '%s\n' "gvproxy" "vfkit"
}

# Succeed when every helper the install promises to bundle is already present in
# the prefix's libexec. On non-macOS platforms there are no bundled helpers, so
# the reconcile is vacuously satisfied. This predicate lets the same-version
# fast path stay network-free while still healing a partial helper set.
bundled_helpers_present() {
  [ "$PLATFORM" = "darwin" ] || return 0
  # Intentional word-splitting over the fixed, space-free helper names so the
  # loop runs in this shell (a `while`-over-pipe would swallow `return`).
  # shellcheck disable=SC2046
  for helper_name in $(macos_bundled_helper_names); do
    [ -x "${NIMBUS_PREFIX}/libexec/${helper_name}" ] || return 1
  done
  return 0
}

# Install every bundled helper carried in <extracted_dir>/libexec into the
# install prefix. macOS ships a self-contained libexec (gvproxy, vfkit); Linux
# archives carry none, so this is a no-op there. Idempotent: re-installing an
# existing helper refreshes it from the pinned, integrity-verified archive.
install_bundled_helpers() {
  extracted_dir="$1"
  [ -d "${extracted_dir}/libexec" ] || return 0
  maybe_sudo install -d "${NIMBUS_PREFIX}/libexec"
  for helper in "${extracted_dir}"/libexec/*; do
    [ -e "$helper" ] || continue
    helper_name="$(basename "$helper")"
    maybe_sudo install -m 0755 "$helper" "${NIMBUS_PREFIX}/libexec/${helper_name}"
    say_info "Installed bundled helper ${helper_name} to ${NIMBUS_PREFIX}/libexec/${helper_name}"
  done
}

download_and_install_nimbus() {
  if [ -n "$DRY_RUN" ]; then
    say_info "[dry-run] Would download and install nimbus $NIMBUS_VERSION to ${NIMBUS_PREFIX}/bin/nimbus"
    return 0
  fi

  asset_name="$(get_nimbus_asset_name)"
  download_url="${NIMBUS_RELEASES_DOWNLOAD}/${NIMBUS_VERSION}/${asset_name}"
  checksums_url="${NIMBUS_RELEASES_DOWNLOAD}/${NIMBUS_VERSION}/checksums-sha256.txt"

  # The binary version gates whether we rewrite ${NIMBUS_PREFIX}/bin/nimbus, but
  # the bundled machine helpers are reconciled independently. When nimbus is
  # already current AND every promised helper is present, this is a network-free
  # no-op. When the binary is current but a helper is missing (an install that
  # predates the helper, or one later removed), reconcile just the helpers from
  # the pinned, integrity-verified archive rather than forcing a full reinstall.
  installed_version="$(get_installed_nimbus_version)"
  reconcile_helpers_only=
  if [ "$installed_version" = "$NIMBUS_VERSION" ]; then
    if bundled_helpers_present; then
      say_info "nimbus $NIMBUS_VERSION is already installed — skipping"
      return 0
    fi
    say_info "nimbus $NIMBUS_VERSION is current; reconciling missing bundled helpers"
    reconcile_helpers_only=1
  elif [ -n "$installed_version" ]; then
    say_info "Upgrading nimbus from $installed_version to $NIMBUS_VERSION"
  fi

  ensure_workdir
  tmpdir="$(mktemp -d "${NIMBUS_WORKDIR}/download.XXXXXX")"

  say_info "Downloading checksums for nimbus ${NIMBUS_VERSION}..."
  download_to_file "$checksums_url" "$tmpdir/checksums-sha256.txt"

  say_info "Downloading nimbus ${NIMBUS_VERSION}..."
  download_to_file "$download_url" "$tmpdir/$asset_name"

  say_info "Verifying checksum..."
  verify_file_checksum "$tmpdir/$asset_name" "$tmpdir/checksums-sha256.txt" "$asset_name"
  verify_github_attestation \
    "$tmpdir/$asset_name" \
    "nimbus/nimbus" \
    "refs/tags/$NIMBUS_VERSION" \
    "nimbus/nimbus/.github/workflows/release.yml" \
    "$asset_name"

  say_info "Extracting and installing..."
  tar -xzf "$tmpdir/$asset_name" -C "$tmpdir"

  # Skip rewriting the binary when only the helper set needs healing; the
  # on-disk nimbus is already the requested version.
  if [ -z "$reconcile_helpers_only" ]; then
    maybe_sudo install -d "${NIMBUS_PREFIX}/bin"
    maybe_sudo install -m 0755 "$tmpdir/nimbus" "${NIMBUS_PREFIX}/bin/nimbus"
    say_info "Installed nimbus to ${NIMBUS_PREFIX}/bin/nimbus"
  fi

  # macOS ships a self-contained libexec (gvproxy, vfkit); Linux archives carry
  # none, so this is a no-op there. The runtime resolves these bundled-first via
  # ${NIMBUS_PREFIX}/libexec/<helper>.
  install_bundled_helpers "$tmpdir"

  # Hard post-condition (macOS): every helper the install promises to bundle must
  # now be present and executable. install_bundled_helpers only copies what the
  # archive's libexec actually contains, and the helpers-only reconcile path
  # above returns success even when it copied nothing, so a truncated or
  # malformed archive could otherwise leave the install "successful" with
  # vfkit/gvproxy missing — the machine path would then fail opaquely at first
  # boot. Fail loudly here instead. Vacuously true on Linux (no bundled helpers),
  # and past the dry-run guard so it only fires on a real install.
  if ! bundled_helpers_present; then
    err "bundled machine helpers are missing from ${NIMBUS_PREFIX}/libexec after install; the downloaded archive appears incomplete — re-run the installer, and if it persists report it at https://github.com/nimbus/nimbus/issues"
  fi
}

download_and_install_bun_jsc_adapter_linux() {
  if [ -z "$INSTALL_BUN_JSC_ADAPTER" ]; then
    return 0
  fi

  if [ -n "$DRY_RUN" ]; then
    say_info "[dry-run] Would download and install optional Bun/JSC adapter from nimbus $NIMBUS_VERSION to /usr/libexec/nimbus/runtime/bun-jsc/current"
    return 0
  fi

  asset_name="$(get_bun_jsc_adapter_asset_name)"
  target_triple="$(get_bun_jsc_adapter_target_triple)"
  adapter_platform="$PLATFORM"
  library_basename="$(get_bun_jsc_adapter_library_basename)"
  download_url="${NIMBUS_RELEASES_DOWNLOAD}/${NIMBUS_VERSION}/${asset_name}"
  adapter_checksums_url="${NIMBUS_RELEASES_DOWNLOAD}/${NIMBUS_VERSION}/nimbus-bun-jsc-adapter-checksums-sha256.txt"
  release_checksums_url="${NIMBUS_RELEASES_DOWNLOAD}/${NIMBUS_VERSION}/checksums-sha256.txt"
  require_bun_jsc_adapter_verifier_tools

  ensure_workdir
  tmpdir="$(mktemp -d "${NIMBUS_WORKDIR}/download.XXXXXX")"

  say_info "Downloading Bun/JSC adapter checksums for nimbus ${NIMBUS_VERSION}..."
  if download_to_file "$adapter_checksums_url" "$tmpdir/adapter-checksums-sha256.txt" 2>/dev/null; then
    checksums_path="$tmpdir/adapter-checksums-sha256.txt"
  else
    say_warn "Adapter-specific checksum file not found — falling back to release checksums"
    download_to_file "$release_checksums_url" "$tmpdir/checksums-sha256.txt"
    checksums_path="$tmpdir/checksums-sha256.txt"
  fi

  say_info "Downloading optional Bun/JSC adapter ${NIMBUS_VERSION}..."
  download_to_file "$download_url" "$tmpdir/$asset_name"

  say_info "Verifying Bun/JSC adapter checksum..."
  verify_file_checksum "$tmpdir/$asset_name" "$checksums_path" "$asset_name"
  verify_github_attestation \
    "$tmpdir/$asset_name" \
    "nimbus/nimbus" \
    "refs/tags/$NIMBUS_VERSION" \
    "nimbus/nimbus/.github/workflows/bun-jsc-adapter.yml" \
    "$asset_name"

  verify_bun_jsc_adapter_archive_layout "$tmpdir/$asset_name" "$tmpdir/adapter-archive-entries.txt" "$library_basename"

  say_info "Extracting optional Bun/JSC adapter..."
  tar -xzf "$tmpdir/$asset_name" -C "$tmpdir"
  manifest_path="$tmpdir/$BUN_JSC_ADAPTER_MANIFEST_FILE"
  verify_file_checksum "$tmpdir/$library_basename" "$tmpdir/$BUN_JSC_ADAPTER_CHECKSUMS_FILE" "$library_basename"
  verify_file_checksum "$manifest_path" "$tmpdir/$BUN_JSC_ADAPTER_CHECKSUMS_FILE" "$BUN_JSC_ADAPTER_MANIFEST_FILE"
  verify_file_checksum "$tmpdir/$BUN_JSC_ADAPTER_README_FILE" "$tmpdir/$BUN_JSC_ADAPTER_CHECKSUMS_FILE" "$BUN_JSC_ADAPTER_README_FILE"
  verify_file_checksum "$tmpdir/$BUN_JSC_ADAPTER_SBOM_FILE" "$tmpdir/$BUN_JSC_ADAPTER_CHECKSUMS_FILE" "$BUN_JSC_ADAPTER_SBOM_FILE"
  verify_file_checksum "$tmpdir/$BUN_JSC_ADAPTER_SLSA_FILE" "$tmpdir/$BUN_JSC_ADAPTER_CHECKSUMS_FILE" "$BUN_JSC_ADAPTER_SLSA_FILE"

  say_info "Verifying Bun/JSC adapter manifest, evidence, exports, and dlopen safety..."
  verify_bun_jsc_adapter_manifest_contract "$tmpdir" "$target_triple" "$adapter_platform" "$library_basename"

  adapter_version="$(python3 - "$manifest_path" <<'PY'
import json
import pathlib
import sys
print(json.loads(pathlib.Path(sys.argv[1]).read_text()).get("adapter_version", ""))
PY
)"
  case "$adapter_version" in
    ""|*/*|*..*)
      err "Bun/JSC adapter manifest has an unsafe adapter_version"
      ;;
  esac

  target_root="/usr/libexec/nimbus/runtime/bun-jsc"
  target_dir="${target_root}/${adapter_version}"
  maybe_sudo install -d "$target_dir"
  maybe_sudo install -m 0755 "$tmpdir/$library_basename" "$target_dir/$library_basename"
  maybe_sudo install -m 0644 "$tmpdir/$BUN_JSC_ADAPTER_MANIFEST_FILE" "$target_dir/$BUN_JSC_ADAPTER_MANIFEST_FILE"
  maybe_sudo install -m 0644 "$tmpdir/$BUN_JSC_ADAPTER_CHECKSUMS_FILE" "$target_dir/$BUN_JSC_ADAPTER_CHECKSUMS_FILE"
  maybe_sudo install -m 0644 "$tmpdir/$BUN_JSC_ADAPTER_README_FILE" "$target_dir/$BUN_JSC_ADAPTER_README_FILE"
  maybe_sudo install -m 0644 "$tmpdir/$BUN_JSC_ADAPTER_SBOM_FILE" "$target_dir/$BUN_JSC_ADAPTER_SBOM_FILE"
  maybe_sudo install -m 0644 "$tmpdir/$BUN_JSC_ADAPTER_SLSA_FILE" "$target_dir/$BUN_JSC_ADAPTER_SLSA_FILE"
  if [ -L "${target_root}/current" ]; then
    maybe_sudo rm -f "${target_root}/current"
  elif [ -e "${target_root}/current" ]; then
    err "refusing to replace non-symlink Bun/JSC current path: ${target_root}/current"
  fi
  maybe_sudo ln -s "$adapter_version" "${target_root}/current"

  say_info "Installed optional Bun/JSC adapter to ${target_root}/current"
}

get_installed_libkrun_version() {
  release_info="/usr/libexec/nimbus/NIMBUS_LIBKRUN_RELEASE.txt"
  if [ -f "$release_info" ]; then
    awk -F= '$1 == "nimbus-libkrun" { print $2; exit }' "$release_info" 2>/dev/null || true
  fi
}

download_and_install_libkrun() {
  if [ -n "$DRY_RUN" ]; then
    say_info "[dry-run] Would download and install nimbus-libkrun $NIMBUS_LIBKRUN_VERSION to /usr/libexec/nimbus/lib"
    return 0
  fi

  asset_name="$(get_libkrun_asset_name)"
  download_url="${NIMBUS_LIBKRUN_RELEASES_DOWNLOAD}/${NIMBUS_LIBKRUN_VERSION}/${asset_name}"
  checksums_url="${NIMBUS_LIBKRUN_RELEASES_DOWNLOAD}/${NIMBUS_LIBKRUN_VERSION}/checksums.txt"

  ensure_workdir
  tmpdir="$(mktemp -d "${NIMBUS_WORKDIR}/download.XXXXXX")"

  say_info "Downloading checksums for nimbus-libkrun ${NIMBUS_LIBKRUN_VERSION}..."
  download_to_file "$checksums_url" "$tmpdir/checksums.txt"

  installed_version="$(get_installed_libkrun_version)"
  if [ "$installed_version" = "$NIMBUS_LIBKRUN_VERSION" ] && [ -e /usr/libexec/nimbus/lib/libkrun.so.1 ]; then
    say_info "nimbus-libkrun $NIMBUS_LIBKRUN_VERSION is already installed — skipping"
    return 0
  elif [ -n "$installed_version" ]; then
    say_info "Upgrading nimbus-libkrun from $installed_version to $NIMBUS_LIBKRUN_VERSION"
  fi

  say_info "Downloading nimbus-libkrun ${NIMBUS_LIBKRUN_VERSION}..."
  download_to_file "$download_url" "$tmpdir/$asset_name"

  say_info "Verifying checksum..."
  verify_file_checksum "$tmpdir/$asset_name" "$tmpdir/checksums.txt" "$asset_name"
  verify_github_attestation \
    "$tmpdir/$asset_name" \
    "nimbus/nimbus-libkrun" \
    "refs/heads/main" \
    "nimbus/nimbus-libkrun/.github/workflows/release.yml" \
    "$asset_name"

  say_info "Installing nimbus-libkrun..."
  maybe_sudo install -d /usr/libexec/nimbus
  maybe_sudo tar -xzf "$tmpdir/$asset_name" -C /usr/libexec/nimbus

  say_info "Installed nimbus-libkrun to /usr/libexec/nimbus/lib"
}

download_and_install_crun() {
  if [ -n "$DRY_RUN" ]; then
    say_info "[dry-run] Would download and install nimbus-crun $NIMBUS_CRUN_VERSION to /usr/libexec/nimbus/crun"
    return 0
  fi

  asset_name="$(get_crun_asset_name)"
  download_url="${NIMBUS_CRUN_RELEASES_DOWNLOAD}/${NIMBUS_CRUN_VERSION}/${asset_name}"
  checksums_url="${NIMBUS_CRUN_RELEASES_DOWNLOAD}/${NIMBUS_CRUN_VERSION}/checksums.txt"

  ensure_workdir
  tmpdir="$(mktemp -d "${NIMBUS_WORKDIR}/download.XXXXXX")"

  say_info "Downloading checksums for nimbus-crun ${NIMBUS_CRUN_VERSION}..."
  download_to_file "$checksums_url" "$tmpdir/checksums.txt"

  # Check if the installed binary already matches the target release.
  crun_path="/usr/libexec/nimbus/crun"
  if [ -x "$crun_path" ]; then
    crun_version="$("$crun_path" --version 2>/dev/null || true)"
    if echo "$crun_version" | grep -q '+LIBKRUN' && file_matches_manifest_checksum "$crun_path" "$tmpdir/checksums.txt" "$asset_name"; then
      say_info "nimbus-crun $NIMBUS_CRUN_VERSION is already installed — skipping"
      return 0
    fi
  fi

  say_info "Downloading nimbus-crun ${NIMBUS_CRUN_VERSION}..."
  download_to_file "$download_url" "$tmpdir/$asset_name"

  say_info "Verifying checksum..."
  verify_file_checksum "$tmpdir/$asset_name" "$tmpdir/checksums.txt" "$asset_name"
  verify_github_attestation \
    "$tmpdir/$asset_name" \
    "nimbus/nimbus-crun" \
    "refs/tags/$NIMBUS_CRUN_VERSION" \
    "nimbus/nimbus-crun/.github/workflows/build.yml" \
    "$asset_name"

  say_info "Installing nimbus-crun..."
  maybe_sudo install -d /usr/libexec/nimbus
  maybe_sudo install -m 0755 "$tmpdir/$asset_name" /usr/libexec/nimbus/crun

  say_info "Installed nimbus-crun to /usr/libexec/nimbus/crun"
}

install_linux() {
  check_kvm_access
  install_system_deps
  resolve_nimbus_version
  resolve_libkrun_version
  resolve_crun_version
  download_and_install_nimbus
  download_and_install_bun_jsc_adapter_linux
  download_and_install_libkrun
  download_and_install_crun
  verify_installation
  print_getting_started_linux
}

print_getting_started_linux() {
  say ""
  say "=== Getting Started ==="
  say ""
  say "Nimbus is installed! To start the server:"
  say ""
  say "  nimbus start"
  say ""
  say "For more information:"
  say "  nimbus --help"
  say "  https://github.com/nimbus/nimbus#readme"
  say ""
}

# --- macOS installation -----------------------------------------------------

# The macOS `nimbus machine` dev flow boots a Linux outer VM through the krunkit
# microVM chain (krunkit VMM + gvproxy network helper + libkrun). That chain is
# published by the official libkrun/krun Homebrew tap. We install it as an
# OPTIONAL fast-path: when Homebrew is present we tap + trust + install it, and
# when it is absent we print guidance and continue. The nimbus binary itself is
# already installed directly (curl|sh, like Linux) and the server runs without
# the machine flow, so a missing Homebrew is never a hard failure on macOS.
install_macos_microvm_deps() {
  if [ -n "$SKIP_DEPS" ]; then
    say_info "Skipping macOS microVM dependency installation (--skip-deps)"
    say_info "Install the krunkit chain later with: brew install libkrun/krun/krunkit"
    return 0
  fi

  if ! check_cmd brew; then
    say_warn "Homebrew not found — skipping the optional macOS microVM dependency chain"
    say_warn "The 'nimbus' server is installed and runs without it."
    say_warn "The 'nimbus machine' dev flow needs krunkit, gvproxy, and libkrun."
    say_warn "Install Homebrew (https://brew.sh), then run: brew install libkrun/krun/krunkit"
    return 0
  fi

  if [ -n "$DRY_RUN" ]; then
    say_info "[dry-run] Would install the macOS microVM chain via Homebrew: krunkit + gvproxy + libkrun (brew install libkrun/krun/krunkit)"
    return 0
  fi

  say_info "Tapping libkrun/krun..."
  brew tap libkrun/krun 2>/dev/null || true

  # Homebrew 6.0 won't load formulae from third-party taps until they are
  # explicitly trusted (HOMEBREW_REQUIRE_TAP_TRUST, default true since 6.0). An
  # untrusted tap is a hard error (UntrustedTapError) with no interactive
  # prompt. `brew trust` is idempotent and records to trust.json whether or not
  # the tap is yet tapped. One trust of libkrun/krun covers the whole microVM
  # chain: krunkit -> libkrun + gvproxy -> virglrenderer/libepoxy/libkrunfw.
  say_info "Trusting the libkrun/krun tap (Homebrew 6.0 tap-trust gate)..."
  brew trust --tap libkrun/krun || true

  # The whole microVM chain is an optional fast-path. Under `set -eu` an install
  # failure (network blip, tap trust declined, Rosetta/arch mismatch) would
  # otherwise abort the entire installer even though `nimbus` itself is already
  # installed and runs without it. Degrade to a warning and continue.
  say_info "Installing the macOS microVM chain (krunkit + gvproxy + libkrun)..."
  if brew install libkrun/krun/krunkit; then
    say_info "Installed the macOS microVM chain via Homebrew"
  else
    say_warn "Could not install the krunkit chain via Homebrew (exit $?)"
    say_warn "The 'nimbus' server is installed and runs without it."
    say_warn "Retry later with: brew install libkrun/krun/krunkit"
  fi
  return 0
}

# Fail fast when the install prefix is not writable and we have no way to gain
# privilege, instead of letting the first `maybe_sudo install` abort mid-run
# with a bare "need sudo to install to system paths" after the user has already
# waited on a download. What governs success is the *writability* of the prefix,
# not whether we are root: a user-owned --prefix (e.g. $HOME/.local) needs no
# sudo at all, while the default /usr/local typically does. The leaf prefix
# directories may not exist yet, so probe the deepest existing ancestor.
preflight_prefix_access() {
  [ -n "$DRY_RUN" ] && return 0

  probe="$NIMBUS_PREFIX"
  while [ -n "$probe" ] && [ ! -e "$probe" ]; do
    parent="$(dirname "$probe")"
    # Stop if dirname stops making progress (reached "/" or ".").
    [ "$parent" = "$probe" ] && break
    probe="$parent"
  done
  [ -n "$probe" ] || probe="/"

  # A writable prefix needs no elevation, whatever the uid.
  if [ -w "$probe" ]; then
    return 0
  fi

  # Already root: the install writes succeed without sudo.
  if [ "$(id -u)" -eq 0 ]; then
    return 0
  fi

  # Non-root: sudo can still carry the install, but only if it will not block on
  # a password we cannot answer. An interactive terminal can satisfy a prompt;
  # otherwise require passwordless sudo (`sudo -n`) so a piped `curl | sh` does
  # not hang or fail deep inside the install. This mirrors maybe_sudo's runtime
  # choice, just surfaced up front with an actionable message.
  if check_cmd sudo; then
    if is_interactive || sudo -n true 2>/dev/null; then
      return 0
    fi
  fi

  err "cannot write to ${NIMBUS_PREFIX} (needed for ${NIMBUS_PREFIX}/bin and ${NIMBUS_PREFIX}/libexec) and cannot elevate. Re-run with sudo (e.g. 'curl ... | sudo sh'), run as root, or set --prefix to a writable location such as \$HOME/.local."
}

install_macos() {
  check_macos_version
  preflight_prefix_access
  resolve_nimbus_version
  download_and_install_nimbus
  install_macos_microvm_deps
  verify_installation
  print_getting_started_macos
}

print_getting_started_macos() {
  say ""
  say "=== Getting Started ==="
  say ""
  say "Nimbus is installed! To initialize and start the machine VM:"
  say ""
  say "  nimbus machine init"
  say "  nimbus start"
  say ""
  say "For more information:"
  say "  nimbus --help"
  say "  https://github.com/nimbus/nimbus#readme"
  say ""
}

# --- Uninstall --------------------------------------------------------------

uninstall_linux() {
  say_info "Uninstalling nimbus from Linux..."

  if [ -n "$DRY_RUN" ]; then
    say_info "[dry-run] Would remove ${NIMBUS_PREFIX}/bin/nimbus"
    say_info "[dry-run] Would remove /usr/libexec/nimbus/crun"
    say_info "[dry-run] Would remove /usr/libexec/nimbus/lib"
    say_info "[dry-run] Would remove /usr/libexec/nimbus/include"
    say_info "[dry-run] Would remove /usr/libexec/nimbus/runtime/bun-jsc"
    return 0
  fi

  if [ -f "${NIMBUS_PREFIX}/bin/nimbus" ]; then
    maybe_sudo rm -f "${NIMBUS_PREFIX}/bin/nimbus"
    say_info "Removed ${NIMBUS_PREFIX}/bin/nimbus"
  fi

  if [ -f "/usr/libexec/nimbus/crun" ]; then
    maybe_sudo rm -f "/usr/libexec/nimbus/crun"
    say_info "Removed /usr/libexec/nimbus/crun"
  fi

  if [ -d "/usr/libexec/nimbus/lib" ]; then
    maybe_sudo rm -rf "/usr/libexec/nimbus/lib"
    say_info "Removed /usr/libexec/nimbus/lib"
  fi

  if [ -d "/usr/libexec/nimbus/include" ]; then
    maybe_sudo rm -rf "/usr/libexec/nimbus/include"
    say_info "Removed /usr/libexec/nimbus/include"
  fi

  if [ -d "/usr/libexec/nimbus/runtime/bun-jsc" ]; then
    maybe_sudo rm -rf "/usr/libexec/nimbus/runtime/bun-jsc"
    say_info "Removed /usr/libexec/nimbus/runtime/bun-jsc"
  fi

  if [ -f "/usr/libexec/nimbus/NIMBUS_LIBKRUN_RELEASE.txt" ]; then
    maybe_sudo rm -f "/usr/libexec/nimbus/NIMBUS_LIBKRUN_RELEASE.txt"
    say_info "Removed /usr/libexec/nimbus/NIMBUS_LIBKRUN_RELEASE.txt"
  fi

  if [ -d "/usr/libexec/nimbus" ]; then
    maybe_sudo rmdir "/usr/libexec/nimbus/runtime" 2>/dev/null || true
    maybe_sudo rmdir "/usr/libexec/nimbus" 2>/dev/null || true
  fi

  say_info "Nimbus uninstalled"
  say ""
  say "System dependencies (conmon, buildah, etc.) were not removed."
  say "Remove them manually if no longer needed."
}

uninstall_macos() {
  say_info "Uninstalling nimbus from macOS..."

  # macOS has two install channels that land the binary in different prefixes:
  #   curl | sh   -> ${NIMBUS_PREFIX}/bin/nimbus    (default /usr/local/bin)
  #   brew install -> $(brew --prefix)/bin/nimbus   (Homebrew-managed symlink)
  # This script only owns the curl|sh copy. If the Homebrew cask is installed we
  # defer to `brew uninstall` so Homebrew's receipts and the optional krunkit
  # dependency chain stay consistent instead of leaving a dangling symlink.
  cask_installed=""
  if check_cmd brew && brew list --cask nimbus >/dev/null 2>&1; then
    cask_installed=1
  fi

  if [ -n "$DRY_RUN" ]; then
    say_info "[dry-run] Would remove ${NIMBUS_PREFIX}/bin/nimbus"
    # shellcheck disable=SC2046
    for helper_name in $(macos_bundled_helper_names); do
      say_info "[dry-run] Would remove ${NIMBUS_PREFIX}/libexec/${helper_name}"
    done
    if [ -n "$cask_installed" ]; then
      say_info "[dry-run] Detected the 'nimbus' Homebrew cask — defer to: brew uninstall --cask nimbus"
    fi
    return 0
  fi

  if [ -n "$cask_installed" ]; then
    say_info "nimbus is also installed via the Homebrew cask."
    say_info "Remove that copy with: brew uninstall --cask nimbus"
  fi

  if [ -f "${NIMBUS_PREFIX}/bin/nimbus" ]; then
    maybe_sudo rm -f "${NIMBUS_PREFIX}/bin/nimbus"
    say_info "Removed ${NIMBUS_PREFIX}/bin/nimbus"
  elif [ -n "$cask_installed" ]; then
    say_info "No curl|sh-installed binary at ${NIMBUS_PREFIX}/bin/nimbus (cask copy left to Homebrew)"
  else
    say_info "nimbus binary not found at ${NIMBUS_PREFIX}/bin/nimbus"
  fi

  # Remove only the direct-install helpers this script owns. The Homebrew
  # krunkit/libkrun chain remains Homebrew-owned and is reported below.
  # shellcheck disable=SC2046
  for helper_name in $(macos_bundled_helper_names); do
    helper_path="${NIMBUS_PREFIX}/libexec/${helper_name}"
    if [ -f "$helper_path" ] || [ -L "$helper_path" ]; then
      maybe_sudo rm -f "$helper_path"
      say_info "Removed ${helper_path}"
    fi
  done
  if [ -d "${NIMBUS_PREFIX}/libexec" ]; then
    maybe_sudo rmdir "${NIMBUS_PREFIX}/libexec" 2>/dev/null || true
  fi

  say ""
  say "The macOS microVM chain (krunkit, gvproxy, libkrun) was not removed."
  say "Run 'brew uninstall libkrun/krun/krunkit' (and 'brew autoremove') if no longer needed."
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
    if NIMBUS_PREFIX="$NIMBUS_PREFIX" bash "${script_dir}/verify-install.sh"; then
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

inline_check_private_libkrun_stack() {
  lib_root="/usr/libexec/nimbus/lib"
  release_info="/usr/libexec/nimbus/NIMBUS_LIBKRUN_RELEASE.txt"

  if [ -f "$release_info" ]; then
    installed_libkrun_version="$(awk -F= '$1 == "nimbus-libkrun" { print $2; exit }' "$release_info" 2>/dev/null || true)"
    inline_print_line "nimbus-libkrun" "present path=$lib_root version=${installed_libkrun_version:-unknown}"
  else
    inline_print_line "nimbus-libkrun" "missing path=$release_info"
    inline_mark_failure
  fi

  if [ -f "$lib_root/libkrun.so.1" ]; then
    inline_print_line "libkrun.so" "present path=$lib_root/libkrun.so.1"
  else
    inline_print_line "libkrun.so" "missing path=$lib_root/libkrun.so.1"
    inline_mark_failure
  fi

  if [ -f "$lib_root/libkrunfw.so.5" ]; then
    inline_print_line "libkrunfw.so" "present path=$lib_root/libkrunfw.so.5"
  else
    inline_print_line "libkrunfw.so" "missing path=$lib_root/libkrunfw.so.5"
    inline_mark_failure
  fi

  if check_cmd nm && [ -f "$lib_root/libkrun.so.1" ]; then
    nm_output="$(nm -D "$lib_root/libkrun.so.1" 2>/dev/null || true)"
    if echo "$nm_output" | grep -q " krun_set_port_map_with_bind_address"; then
      inline_print_line "libkrun.symbol" "present krun_set_port_map_with_bind_address"
    else
      inline_print_line "libkrun.symbol" "missing krun_set_port_map_with_bind_address"
      inline_mark_failure
    fi
  else
    inline_print_line "libkrun.symbol" "skipped (nm or libkrun missing)"
    inline_mark_warning
  fi

  crun_path="/usr/libexec/nimbus/crun"
  if check_cmd readelf && [ -x "$crun_path" ]; then
    dynamic_entries="$(readelf -d "$crun_path" 2>/dev/null || true)"
    case "$dynamic_entries" in
      *'$ORIGIN/lib'*)
        inline_print_line "nimbus-crun.runpath" 'present $ORIGIN/lib'
        ;;
      *)
        inline_print_line "nimbus-crun.runpath" 'missing $ORIGIN/lib'
        inline_mark_failure
        ;;
    esac
  else
    inline_print_line "nimbus-crun.runpath" "skipped (readelf or crun missing)"
    inline_mark_warning
  fi
}

inline_check_bun_jsc_adapter() {
  manifest_path="/usr/libexec/nimbus/runtime/bun-jsc/current/nimbus-bun-jsc-adapter.json"
  adapter_dir="/usr/libexec/nimbus/runtime/bun-jsc/current"
  if [ ! -f "$manifest_path" ]; then
    inline_print_line "nimbus-bun-jsc" "absent optional"
    return 0
  fi

  adapter_version="$(sed -n 's/^[[:space:]]*"adapter_version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$manifest_path" | head -n 1)"
  inline_print_line "nimbus-bun-jsc" "present path=$manifest_path version=${adapter_version:-unknown}"
  if [ -x "$adapter_dir/libnimbus_bun_jsc_embedder.so" ]; then
    inline_print_line "bun-jsc.library" "present path=$adapter_dir/libnimbus_bun_jsc_embedder.so"
  else
    inline_print_line "bun-jsc.library" "missing path=$adapter_dir/libnimbus_bun_jsc_embedder.so"
    inline_mark_failure
  fi
}

inline_check_macos_bun_jsc_adapter() {
  brew_prefix="$(brew --prefix 2>/dev/null || echo "/opt/homebrew")"
  manifest_path="${brew_prefix}/opt/nimbus/libexec/runtime/bun-jsc/current/nimbus-bun-jsc-adapter.json"
  adapter_dir="$(dirname "$manifest_path")"
  if [ ! -f "$manifest_path" ]; then
    inline_print_line "nimbus-bun-jsc" "absent optional"
    return 0
  fi

  adapter_version="$(sed -n 's/^[[:space:]]*"adapter_version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$manifest_path" | head -n 1)"
  inline_print_line "nimbus-bun-jsc" "present path=$manifest_path version=${adapter_version:-unknown}"
  if [ -x "$adapter_dir/libnimbus_bun_jsc_embedder.dylib" ]; then
    inline_print_line "bun-jsc.library" "present path=$adapter_dir/libnimbus_bun_jsc_embedder.dylib"
  else
    inline_print_line "bun-jsc.library" "missing path=$adapter_dir/libnimbus_bun_jsc_embedder.dylib"
    inline_mark_failure
  fi
}

verify_linux_inline() {
  if [ -x "${NIMBUS_PREFIX}/bin/nimbus" ]; then
    inline_print_line "nimbus" "present path=${NIMBUS_PREFIX}/bin/nimbus"
  elif command -v nimbus >/dev/null 2>&1; then
    inline_print_line "nimbus" "present path=$(command -v nimbus)"
  else
    inline_print_line "nimbus" "missing"
    inline_mark_failure
  fi

  crun_path="/usr/libexec/nimbus/crun"
  if [ -x "$crun_path" ]; then
    crun_version="$("$crun_path" --version 2>/dev/null || true)"
    if echo "$crun_version" | grep -q '+LIBKRUN'; then
      inline_print_line "nimbus-crun" "present path=$crun_path version=$(printf '%s' "$crun_version" | tr '\n' ' ' | sed -e 's/[[:space:]]\+/ /g' -e 's/^ //' -e 's/ $//')"
    else
      inline_print_line "nimbus-crun" "present path=$crun_path (missing +LIBKRUN flag)"
      inline_mark_failure
    fi
  else
    inline_print_line "nimbus-crun" "missing path=$crun_path"
    inline_mark_failure
  fi

  inline_check_command "conmon" "conmon" required
  inline_check_command "buildah" "buildah" required
  inline_check_command "catatonit" "catatonit" recommended
  inline_check_command "passt" "passt" recommended
  inline_check_command "newuidmap" "newuidmap" recommended
  inline_check_command "fuse-overlayfs" "fuse-overlayfs" recommended
  inline_check_private_libkrun_stack
  inline_check_bun_jsc_adapter
}

# Resolve a bundled-first macOS machine helper. Mirrors the Rust runtime
# resolver (bundled_helper_candidates_for_executable in machine/manager/helpers.rs):
# the Nimbus-pinned helper shipped in the archive's libexec/ is AUTHORITATIVE
# and resolved first, then the Homebrew prefix and PATH as fallbacks.
resolve_macos_bundled_helper() {
  helper_name="$1"

  # 1) Bundled in the install prefix's libexec (direct no-brew install path).
  if [ -x "${NIMBUS_PREFIX}/libexec/${helper_name}" ]; then
    printf '%s\n' "${NIMBUS_PREFIX}/libexec/${helper_name}"
    return 0
  fi

  # 2) Bundled beside the resolved nimbus binary (e.g. Homebrew Caskroom, where
  #    libexec sits next to the real binary the bin symlink points at).
  nimbus_path="$(command -v nimbus 2>/dev/null || true)"
  if [ -n "$nimbus_path" ]; then
    real_path="$(readlink "$nimbus_path" 2>/dev/null || echo "$nimbus_path")"
    if [ "${real_path#/}" = "$real_path" ]; then
      real_path="$(cd "$(dirname "$nimbus_path")" && cd "$(dirname "$real_path")" && pwd)/$(basename "$real_path")"
    fi
    real_dir="$(dirname "$real_path")"
    if [ -x "${real_dir}/libexec/${helper_name}" ]; then
      printf '%s\n' "${real_dir}/libexec/${helper_name}"
      return 0
    fi
  fi

  # 3) Homebrew prefix / standard bin fallbacks, then PATH.
  brew_prefix="$(brew --prefix 2>/dev/null || echo "/opt/homebrew")"
  for candidate in "${brew_prefix}/bin/${helper_name}" "/usr/local/bin/${helper_name}"; do
    if [ -x "$candidate" ]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done

  if helper_path="$(command -v "${helper_name}" 2>/dev/null)"; then
    printf '%s\n' "$helper_path"
    return 0
  fi

  return 1
}

resolve_macos_gvproxy_path() {
  resolve_macos_bundled_helper "gvproxy"
}

resolve_macos_vfkit_path() {
  resolve_macos_bundled_helper "vfkit"
}

verify_macos_inline() {
  inline_check_command "nimbus" "nimbus" required
  inline_check_command "krunkit" "krunkit" recommended
  inline_check_macos_bun_jsc_adapter

  if gvproxy_path="$(resolve_macos_gvproxy_path)"; then
    inline_print_line "gvproxy" "present path=$gvproxy_path"
  else
    inline_print_line "gvproxy" "missing (expected bundled at ${NIMBUS_PREFIX}/libexec/gvproxy)"
    inline_mark_warning
  fi

  # vfkit is the opt-in backend (NIMBUS_MACHINE_PROVIDER=vfkit); the default
  # stays krunkit, so a missing vfkit is informational rather than a warning.
  if vfkit_path="$(resolve_macos_vfkit_path)"; then
    inline_print_line "vfkit" "present path=$vfkit_path (opt-in: NIMBUS_MACHINE_PROVIDER=vfkit)"
  else
    inline_print_line "vfkit" "absent (opt-in backend; bundled at ${NIMBUS_PREFIX}/libexec/vfkit when shipped)"
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
Nimbus install script

Usage:
  install.sh [options]

Options:
  --version <tag>       Pin nimbus version (e.g., v0.1.14)
  --crun-version <tag>  Pin nimbus-crun version (Linux only)
  --libkrun-version <tag>
                        Pin nimbus-libkrun version (Linux only)
  --with-bun-jsc        Install optional in-process Bun/JSC adapter when a
                        matching release asset exists (Linux x86_64 today)
  --prefix <path>       Install prefix (default: /usr/local)
  --skip-deps           Skip system dependency installation
  --dry-run             Print what would happen without executing
  --uninstall           Remove nimbus, nimbus-libkrun, and nimbus-crun
  -y, --yes             Skip interactive confirmation prompts
  -h, --help            Show this help message

Environment:
  GITHUB_TOKEN          Optional GitHub API auth for public release lookups
  HTTPS_PROXY           HTTP proxy for downloads
  HTTP_PROXY            HTTP proxy for downloads
  NO_PROXY              Hosts to exclude from proxy
  NIMBUS_REQUIRE_ATTESTATIONS
                        Fail closed if GitHub artifact attestation verification
                        cannot run or fails
  NIMBUS_INSTALL_BUN_JSC_ADAPTER
                        Set to a non-empty value to behave like --with-bun-jsc

Examples:
  # Install latest version
  curl -fsSL https://github.com/nimbus/nimbus/releases/latest/download/install.sh | sh

  # Install specific version
  curl -fsSL https://github.com/nimbus/nimbus/releases/latest/download/install.sh | sh -s -- --version v0.1.14

  # Dry run (see what would happen)
  curl -fsSL https://github.com/nimbus/nimbus/releases/latest/download/install.sh | sh -s -- --dry-run

  # Uninstall
  curl -fsSL https://github.com/nimbus/nimbus/releases/latest/download/install.sh | sh -s -- --uninstall
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
        NIMBUS_VERSION="$1"
        ;;
      --crun-version)
        shift
        if [ $# -eq 0 ]; then
          err "--crun-version requires a value"
        fi
        NIMBUS_CRUN_VERSION="$1"
        ;;
      --libkrun-version)
        shift
        if [ $# -eq 0 ]; then
          err "--libkrun-version requires a value"
        fi
        NIMBUS_LIBKRUN_VERSION="$1"
        ;;
      --with-bun-jsc)
        INSTALL_BUN_JSC_ADAPTER="1"
        ;;
      --prefix)
        shift
        if [ $# -eq 0 ]; then
          err "--prefix requires a value"
        fi
        NIMBUS_PREFIX="$1"
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
  if [ "$PLATFORM" = "linux" ]; then
    load_linux_distribution_contract
  fi

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
    # Resolve the nimbus version on every platform so the plan shows the real
    # tag instead of "latest". resolve_nimbus_version short-circuits when the
    # version was passed explicitly, so a forced --version stays network-free.
    # The libkrun/crun helper components are Linux-only.
    resolve_nimbus_version
    if [ "$PLATFORM" = "linux" ]; then
      resolve_libkrun_version
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
