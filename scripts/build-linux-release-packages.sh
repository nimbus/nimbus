#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: build-linux-release-packages.sh --output-dir <path> --neovex-binary <path> --neovex-crun-binary <path> --version <semver> [options]

Stage the Neovex Linux package payloads, render nFPM manifests for Debian and
RPM formats, and optionally build the packages when `nfpm` is available.

Required:
  --output-dir <path>          Output root for staged payloads, manifests, and packages
  --neovex-binary <path>       Linux `neovex` binary to package at /usr/bin/neovex
  --neovex-crun-binary <path>  Linux patched `crun` binary to package at /usr/libexec/neovex/crun
  --version <semver>           Neovex package version (leading `v` accepted)

Optional:
  --crun-version <semver>      neovex-crun package version (default: --version)
  --arch <amd64|arm64>         Package architecture (default: host architecture)
  --format <deb|rpm>           Package format to build; repeatable (default: deb + rpm)
  --nfpm <path>                Explicit nFPM binary path (default: `nfpm` on PATH)
  --render-only                Only stage payloads + manifests; do not run nFPM
  -h, --help                   Show this help text

Examples:
  bash scripts/build-linux-release-packages.sh \
    --output-dir /tmp/neovex-linux-packages \
    --neovex-binary /tmp/neovex \
    --neovex-crun-binary /tmp/neovex-crun \
    --version 0.1.10 \
    --arch amd64

  bash scripts/build-linux-release-packages.sh \
    --output-dir /tmp/neovex-linux-packages \
    --neovex-binary /tmp/neovex \
    --neovex-crun-binary /tmp/neovex-crun \
    --version v0.1.10 \
    --crun-version 0.1.4 \
    --render-only
EOF
}

die() {
  printf '%s\n' "$*" >&2
  exit 64
}

normalize_arch() {
  case "$1" in
    amd64|x86_64)
      printf 'amd64\n'
      ;;
    arm64|aarch64)
      printf 'arm64\n'
      ;;
    *)
      die "unsupported architecture: $1 (expected amd64 or arm64)"
      ;;
  esac
}

sha256_file() {
  local output_path="$1"
  shift

  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$@" >"$output_path"
    return 0
  fi

  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$@" >"$output_path"
    return 0
  fi

  die "neither sha256sum nor shasum is available to checksum generated packages"
}

append_yaml_list() {
  local file_path="$1"
  local key="$2"
  shift 2

  if [[ "$#" -eq 0 ]]; then
    return 0
  fi

  {
    printf '%s:\n' "$key"
    local item
    for item in "$@"; do
      printf '  - %s\n' "$item"
    done
  } >>"$file_path"
}

write_neovex_readme() {
  local file_path="$1"
  local version="$2"
  cat >"$file_path" <<EOF
# neovex

Version: ${version}
Repository: https://github.com/agentstation/neovex

This package installs the Neovex host CLI at /usr/bin/neovex.

On Linux production hosts, Neovex stays aligned with the existing service
execution stack instead of bundling Podman itself. The distro package depends
on the host container primitives (buildah, conmon, netavark, aardvark-dns)
plus the private neovex-crun runtime package that installs to
/usr/libexec/neovex/crun.
EOF
}

write_neovex_crun_readme() {
  local file_path="$1"
  local version="$2"
  cat >"$file_path" <<EOF
# neovex-crun

Version: ${version}
Repository: https://github.com/agentstation/neovex-crun

This package installs the patched private runtime at /usr/libexec/neovex/crun.

It does not replace the system crun package. Neovex invokes this private path
explicitly so distro Podman/CRI-O flows can keep using the distro runtime
unmodified.
EOF
}

render_neovex_manifest() {
  local manifest_path="$1"
  local version="$2"
  local arch="$3"
  local staged_root="$4"
  shift 4
  local dependencies=("$@")

  cat >"$manifest_path" <<EOF
# yaml-language-server: \$schema=https://nfpm.goreleaser.com/schema.json
name: neovex
arch: ${arch}
platform: linux
version: ${version}
version_schema: semver
section: devel
priority: optional
maintainer: AgentStation
vendor: AgentStation
homepage: https://github.com/agentstation/neovex
license: Neovex-Community-1.0
description: |
  Self-hosted JavaScript backend runtime powered by V8.

  This Linux package installs the host Neovex CLI and depends on the distro
  container stack plus the private neovex-crun runtime package.
rpm:
  summary: Self-hosted JavaScript backend runtime powered by V8
  group: Applications/Internet
contents:
  - src: ${staged_root}/usr/bin/neovex
    dst: /usr/bin/neovex
    file_info:
      mode: 0755
  - src: ${staged_root}/usr/share/doc/neovex/README.md
    dst: /usr/share/doc/neovex/README.md
    file_info:
      mode: 0644
  - src: ${staged_root}/usr/share/doc/neovex/LICENSE
    dst: /usr/share/doc/neovex/LICENSE
    file_info:
      mode: 0644
EOF
  append_yaml_list "$manifest_path" "depends" "${dependencies[@]}"
  append_yaml_list "$manifest_path" "recommends" "fuse-overlayfs" "uidmap"
}

render_neovex_crun_manifest() {
  local manifest_path="$1"
  local version="$2"
  local arch="$3"
  local staged_root="$4"
  shift 4
  local dependencies=("$@")

  cat >"$manifest_path" <<EOF
# yaml-language-server: \$schema=https://nfpm.goreleaser.com/schema.json
name: neovex-crun
arch: ${arch}
platform: linux
version: ${version}
version_schema: semver
section: admin
priority: optional
maintainer: AgentStation
vendor: AgentStation
homepage: https://github.com/agentstation/neovex-crun
license: Neovex-Community-1.0
description: |
  Patched private crun runtime for Neovex libkrun service execution.

  This package installs /usr/libexec/neovex/crun and intentionally does not
  replace the system crun binary.
rpm:
  summary: Patched private crun runtime for Neovex
  group: Applications/System
contents:
  - src: ${staged_root}/usr/libexec/neovex/crun
    dst: /usr/libexec/neovex/crun
    file_info:
      mode: 0755
  - src: ${staged_root}/usr/share/doc/neovex-crun/README.md
    dst: /usr/share/doc/neovex-crun/README.md
    file_info:
      mode: 0644
  - src: ${staged_root}/usr/share/doc/neovex-crun/LICENSE
    dst: /usr/share/doc/neovex-crun/LICENSE
    file_info:
      mode: 0644
EOF
  append_yaml_list "$manifest_path" "depends" "${dependencies[@]}"
}

output_dir=""
neovex_binary=""
neovex_crun_binary=""
version=""
crun_version=""
arch=""
nfpm_bin="${NFPM_BIN:-nfpm}"
render_only=0
declare -a formats=()

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --output-dir)
      output_dir="${2:-}"
      shift 2
      ;;
    --neovex-binary)
      neovex_binary="${2:-}"
      shift 2
      ;;
    --neovex-crun-binary)
      neovex_crun_binary="${2:-}"
      shift 2
      ;;
    --version)
      version="${2:-}"
      shift 2
      ;;
    --crun-version)
      crun_version="${2:-}"
      shift 2
      ;;
    --arch)
      arch="$(normalize_arch "${2:-}")"
      shift 2
      ;;
    --format)
      case "${2:-}" in
        deb|rpm)
          formats+=("${2}")
          ;;
        *)
          die "unsupported format: ${2:-<empty>} (expected deb or rpm)"
          ;;
      esac
      shift 2
      ;;
    --nfpm)
      nfpm_bin="${2:-}"
      shift 2
      ;;
    --render-only)
      render_only=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

[[ -n "$output_dir" ]] || die "--output-dir is required"
[[ -n "$neovex_binary" ]] || die "--neovex-binary is required"
[[ -n "$neovex_crun_binary" ]] || die "--neovex-crun-binary is required"
[[ -n "$version" ]] || die "--version is required"

if [[ "${#formats[@]}" -eq 0 ]]; then
  formats=(deb rpm)
fi

version="${version#v}"
if [[ -z "$crun_version" ]]; then
  crun_version="$version"
else
  crun_version="${crun_version#v}"
fi

if [[ -z "$arch" ]]; then
  arch="$(normalize_arch "$(uname -m)")"
fi

[[ -f "$neovex_binary" ]] || die "neovex binary not found: $neovex_binary"
[[ -f "$neovex_crun_binary" ]] || die "neovex-crun binary not found: $neovex_crun_binary"

mkdir -p "$output_dir"
output_dir="$(cd "$output_dir" && pwd)"

staging_dir="${output_dir}/staging"
manifests_dir="${output_dir}/manifests"
packages_dir="${output_dir}/packages"
package_checksums_path="${packages_dir}/checksums-sha256.txt"
rm -rf "$staging_dir" "$manifests_dir" "$packages_dir"
mkdir -p "$staging_dir" "$manifests_dir" "$packages_dir"

neovex_stage="${staging_dir}/neovex"
neovex_crun_stage="${staging_dir}/neovex-crun"

install -d "${neovex_stage}/usr/bin" \
  "${neovex_stage}/usr/share/doc/neovex" \
  "${neovex_crun_stage}/usr/libexec/neovex" \
  "${neovex_crun_stage}/usr/share/doc/neovex-crun"

install -m 0755 "$neovex_binary" "${neovex_stage}/usr/bin/neovex"
install -m 0755 "$neovex_crun_binary" "${neovex_crun_stage}/usr/libexec/neovex/crun"
install -m 0644 LICENSE "${neovex_stage}/usr/share/doc/neovex/LICENSE"
install -m 0644 LICENSE "${neovex_crun_stage}/usr/share/doc/neovex-crun/LICENSE"
write_neovex_readme "${neovex_stage}/usr/share/doc/neovex/README.md" "$version"
write_neovex_crun_readme "${neovex_crun_stage}/usr/share/doc/neovex-crun/README.md" "$crun_version"

neovex_deb_manifest="${manifests_dir}/neovex-deb.yaml"
neovex_rpm_manifest="${manifests_dir}/neovex-rpm.yaml"
neovex_crun_deb_manifest="${manifests_dir}/neovex-crun-deb.yaml"
neovex_crun_rpm_manifest="${manifests_dir}/neovex-crun-rpm.yaml"

render_neovex_manifest \
  "$neovex_deb_manifest" \
  "$version" \
  "$arch" \
  "$neovex_stage" \
  "buildah" "conmon" "netavark" "aardvark-dns" "neovex-crun"
render_neovex_manifest \
  "$neovex_rpm_manifest" \
  "$version" \
  "$arch" \
  "$neovex_stage" \
  "buildah" "conmon" "netavark" "aardvark-dns" "neovex-crun"
render_neovex_crun_manifest \
  "$neovex_crun_deb_manifest" \
  "$crun_version" \
  "$arch" \
  "$neovex_crun_stage" \
  "libkrun" "libkrunfw"
render_neovex_crun_manifest \
  "$neovex_crun_rpm_manifest" \
  "$crun_version" \
  "$arch" \
  "$neovex_crun_stage" \
  "libkrun" "libkrunfw"

printf 'stage.neovex=%s\n' "$neovex_stage"
printf 'stage.neovex_crun=%s\n' "$neovex_crun_stage"
printf 'manifest.neovex.deb=%s\n' "$neovex_deb_manifest"
printf 'manifest.neovex.rpm=%s\n' "$neovex_rpm_manifest"
printf 'manifest.neovex_crun.deb=%s\n' "$neovex_crun_deb_manifest"
printf 'manifest.neovex_crun.rpm=%s\n' "$neovex_crun_rpm_manifest"

if [[ "$render_only" -eq 1 ]]; then
  printf 'result=rendered\n'
  exit 0
fi

if ! command -v "$nfpm_bin" >/dev/null 2>&1; then
  die "nfpm not found: ${nfpm_bin} (use --render-only or install github.com/goreleaser/nfpm/v2/cmd/nfpm)"
fi

format=""
for format in "${formats[@]}"; do
  manifest_list=""
  case "$format" in
    deb)
      manifest_list="${neovex_deb_manifest} ${neovex_crun_deb_manifest}"
      ;;
    rpm)
      manifest_list="${neovex_rpm_manifest} ${neovex_crun_rpm_manifest}"
      ;;
    *)
      die "unsupported format in build loop: ${format}"
      ;;
  esac

  manifest_path=""
  for manifest_path in ${manifest_list}; do
    "$nfpm_bin" package \
      --config "$manifest_path" \
      --packager "$format" \
      --target "$packages_dir"
  done
done

(
  cd "$packages_dir"
  shopt -s nullglob
  package_files=( ./*.deb ./*.rpm )
  if [[ "${#package_files[@]}" -eq 0 ]]; then
    die "no generated package files found under ${packages_dir}"
  fi
  sha256_file "$package_checksums_path" "${package_files[@]}"
)

printf 'packages.dir=%s\n' "$packages_dir"
printf 'packages.checksums=%s\n' "$package_checksums_path"
printf 'result=packaged\n'
