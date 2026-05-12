#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: build-linux-release-packages.sh --output-dir <path> --nimbus-binary <path> --nimbus-crun-binary <path> --version <semver> [options]

Stage the Nimbus Linux package payloads, render nFPM manifests for Debian and
RPM formats, and optionally build the packages when `nfpm` is available.

Required:
  --output-dir <path>          Output root for staged payloads, manifests, and packages
  --nimbus-binary <path>       Linux `nimbus` binary to package at /usr/bin/nimbus
  --nimbus-crun-binary <path>  Linux patched `crun` binary to package at /usr/libexec/nimbus/crun
  --version <semver>           Nimbus package version (leading `v` accepted)

Optional:
  --crun-version <semver>      nimbus-crun package version (default: --version)
  --arch <amd64|arm64>         Package architecture (default: host architecture)
  --format <deb|rpm>           Package format to build; repeatable (default: deb + rpm)
  --nfpm <path>                Explicit nFPM binary path (default: `nfpm` on PATH)
  --render-only                Only stage payloads + manifests; do not run nFPM
  -h, --help                   Show this help text

Examples:
  bash scripts/build-linux-release-packages.sh \
    --output-dir /tmp/nimbus-linux-packages \
    --nimbus-binary /tmp/nimbus \
    --nimbus-crun-binary /tmp/nimbus-crun \
    --version 0.1.10 \
    --arch amd64

  bash scripts/build-linux-release-packages.sh \
    --output-dir /tmp/nimbus-linux-packages \
    --nimbus-binary /tmp/nimbus \
    --nimbus-crun-binary /tmp/nimbus-crun \
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

write_nimbus_readme() {
  local file_path="$1"
  local version="$2"
  cat >"$file_path" <<EOF
# nimbus

Version: ${version}
Repository: https://github.com/nimbus/nimbus

This package installs the Nimbus host CLI at /usr/bin/nimbus.

On Linux production hosts, Nimbus stays aligned with the existing service
execution stack instead of bundling Podman itself. The distro package depends
on the host container primitives (buildah, conmon, netavark, aardvark-dns)
plus the private nimbus-crun runtime package that installs to
/usr/libexec/nimbus/crun.
EOF
}

write_nimbus_crun_readme() {
  local file_path="$1"
  local version="$2"
  cat >"$file_path" <<EOF
# nimbus-crun

Version: ${version}
Repository: https://github.com/nimbus/nimbus-crun

This package installs the patched private runtime at /usr/libexec/nimbus/crun.

It does not replace the system crun package. Nimbus invokes this private path
explicitly so distro Podman/CRI-O flows can keep using the distro runtime
unmodified.
EOF
}

render_nimbus_manifest() {
  local manifest_path="$1"
  local version="$2"
  local arch="$3"
  local staged_root="$4"
  shift 4
  local dependencies=("$@")

  cat >"$manifest_path" <<EOF
# yaml-language-server: \$schema=https://nfpm.goreleaser.com/schema.json
name: nimbus
arch: ${arch}
platform: linux
version: ${version}
version_schema: semver
section: devel
priority: optional
maintainer: Nimbus
vendor: Nimbus
homepage: https://github.com/nimbus/nimbus
license: Nimbus-Community-1.0
description: |
  Self-hosted JavaScript backend runtime powered by V8.

  This Linux package installs the host Nimbus CLI and depends on the distro
  container stack plus the private nimbus-crun runtime package.
rpm:
  summary: Self-hosted JavaScript backend runtime powered by V8
  group: Applications/Internet
contents:
  - src: ${staged_root}/usr/bin/nimbus
    dst: /usr/bin/nimbus
    file_info:
      mode: 0755
  - src: ${staged_root}/usr/share/doc/nimbus/README.md
    dst: /usr/share/doc/nimbus/README.md
    file_info:
      mode: 0644
  - src: ${staged_root}/usr/share/doc/nimbus/LICENSE
    dst: /usr/share/doc/nimbus/LICENSE
    file_info:
      mode: 0644
EOF
  append_yaml_list "$manifest_path" "depends" "${dependencies[@]}"
  append_yaml_list "$manifest_path" "recommends" "fuse-overlayfs" "uidmap"
}

render_nimbus_crun_manifest() {
  local manifest_path="$1"
  local version="$2"
  local arch="$3"
  local staged_root="$4"
  shift 4
  local dependencies=("$@")

  cat >"$manifest_path" <<EOF
# yaml-language-server: \$schema=https://nfpm.goreleaser.com/schema.json
name: nimbus-crun
arch: ${arch}
platform: linux
version: ${version}
version_schema: semver
section: admin
priority: optional
maintainer: Nimbus
vendor: Nimbus
homepage: https://github.com/nimbus/nimbus-crun
license: Nimbus-Community-1.0
description: |
  Patched private crun runtime for Nimbus libkrun service execution.

  This package installs /usr/libexec/nimbus/crun and intentionally does not
  replace the system crun binary.
rpm:
  summary: Patched private crun runtime for Nimbus
  group: Applications/System
contents:
  - src: ${staged_root}/usr/libexec/nimbus/crun
    dst: /usr/libexec/nimbus/crun
    file_info:
      mode: 0755
  - src: ${staged_root}/usr/share/doc/nimbus-crun/README.md
    dst: /usr/share/doc/nimbus-crun/README.md
    file_info:
      mode: 0644
  - src: ${staged_root}/usr/share/doc/nimbus-crun/LICENSE
    dst: /usr/share/doc/nimbus-crun/LICENSE
    file_info:
      mode: 0644
EOF
  append_yaml_list "$manifest_path" "depends" "${dependencies[@]}"
}

output_dir=""
nimbus_binary=""
nimbus_crun_binary=""
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
    --nimbus-binary)
      nimbus_binary="${2:-}"
      shift 2
      ;;
    --nimbus-crun-binary)
      nimbus_crun_binary="${2:-}"
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
[[ -n "$nimbus_binary" ]] || die "--nimbus-binary is required"
[[ -n "$nimbus_crun_binary" ]] || die "--nimbus-crun-binary is required"
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

[[ -f "$nimbus_binary" ]] || die "nimbus binary not found: $nimbus_binary"
[[ -f "$nimbus_crun_binary" ]] || die "nimbus-crun binary not found: $nimbus_crun_binary"

mkdir -p "$output_dir"
output_dir="$(cd "$output_dir" && pwd)"

staging_dir="${output_dir}/staging"
manifests_dir="${output_dir}/manifests"
packages_dir="${output_dir}/packages"
package_checksums_path="${packages_dir}/checksums-sha256.txt"
rm -rf "$staging_dir" "$manifests_dir" "$packages_dir"
mkdir -p "$staging_dir" "$manifests_dir" "$packages_dir"

nimbus_stage="${staging_dir}/nimbus"
nimbus_crun_stage="${staging_dir}/nimbus-crun"

install -d "${nimbus_stage}/usr/bin" \
  "${nimbus_stage}/usr/share/doc/nimbus" \
  "${nimbus_crun_stage}/usr/libexec/nimbus" \
  "${nimbus_crun_stage}/usr/share/doc/nimbus-crun"

install -m 0755 "$nimbus_binary" "${nimbus_stage}/usr/bin/nimbus"
install -m 0755 "$nimbus_crun_binary" "${nimbus_crun_stage}/usr/libexec/nimbus/crun"
install -m 0644 LICENSE "${nimbus_stage}/usr/share/doc/nimbus/LICENSE"
install -m 0644 LICENSE "${nimbus_crun_stage}/usr/share/doc/nimbus-crun/LICENSE"
write_nimbus_readme "${nimbus_stage}/usr/share/doc/nimbus/README.md" "$version"
write_nimbus_crun_readme "${nimbus_crun_stage}/usr/share/doc/nimbus-crun/README.md" "$crun_version"

nimbus_deb_manifest="${manifests_dir}/nimbus-deb.yaml"
nimbus_rpm_manifest="${manifests_dir}/nimbus-rpm.yaml"
nimbus_crun_deb_manifest="${manifests_dir}/nimbus-crun-deb.yaml"
nimbus_crun_rpm_manifest="${manifests_dir}/nimbus-crun-rpm.yaml"

render_nimbus_manifest \
  "$nimbus_deb_manifest" \
  "$version" \
  "$arch" \
  "$nimbus_stage" \
  "buildah" "conmon" "netavark" "aardvark-dns" "nimbus-crun"
render_nimbus_manifest \
  "$nimbus_rpm_manifest" \
  "$version" \
  "$arch" \
  "$nimbus_stage" \
  "buildah" "conmon" "netavark" "aardvark-dns" "nimbus-crun"
render_nimbus_crun_manifest \
  "$nimbus_crun_deb_manifest" \
  "$crun_version" \
  "$arch" \
  "$nimbus_crun_stage" \
  "libkrun" "libkrunfw"
render_nimbus_crun_manifest \
  "$nimbus_crun_rpm_manifest" \
  "$crun_version" \
  "$arch" \
  "$nimbus_crun_stage" \
  "libkrun" "libkrunfw"

printf 'stage.nimbus=%s\n' "$nimbus_stage"
printf 'stage.nimbus_crun=%s\n' "$nimbus_crun_stage"
printf 'manifest.nimbus.deb=%s\n' "$nimbus_deb_manifest"
printf 'manifest.nimbus.rpm=%s\n' "$nimbus_rpm_manifest"
printf 'manifest.nimbus_crun.deb=%s\n' "$nimbus_crun_deb_manifest"
printf 'manifest.nimbus_crun.rpm=%s\n' "$nimbus_crun_rpm_manifest"

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
      manifest_list="${nimbus_deb_manifest} ${nimbus_crun_deb_manifest}"
      ;;
    rpm)
      manifest_list="${nimbus_rpm_manifest} ${nimbus_crun_rpm_manifest}"
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
