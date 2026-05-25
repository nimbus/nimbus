#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: build-linux-release-packages.sh --output-dir <path> --nimbus-binary <path> --nimbus-libkrun-archive <path> --nimbus-crun-binary <path> --version <semver> [options]

Stage the Nimbus Linux package payloads, render nFPM manifests for Debian and
RPM formats, and optionally build the packages when `nfpm` is available.

Required:
  --output-dir <path>          Output root for staged payloads, manifests, and packages
  --nimbus-binary <path>       Linux `nimbus` binary to package at /usr/bin/nimbus
  --nimbus-libkrun-archive <path>
                              `nimbus-libkrun-linux-<arch>.tar.gz` release archive
  --nimbus-crun-binary <path>  Linux patched `crun` binary to package at /usr/libexec/nimbus/crun
  --version <semver>           Nimbus package version (leading `v` accepted)

Optional:
  --libkrun-version <semver>   nimbus-libkrun package version (leading `v` accepted)
  --crun-version <semver>      nimbus-crun package version (default: --version)
  --bun-jsc-adapter-archive <path>
                              Optional `nimbus-bun-jsc-adapter-linux-<arch>.tar.gz`
                              release archive to package as nimbus-bun-jsc-adapter
  --arch <amd64|arm64>         Package architecture (default: host architecture)
  --format <deb|rpm>           Package format to build; repeatable (default: deb + rpm)
  --nfpm <path>                Explicit nFPM binary path (default: `nfpm` on PATH)
  --render-only                Only stage payloads + manifests; do not run nFPM
  -h, --help                   Show this help text

Examples:
  bash scripts/build-linux-release-packages.sh \
    --output-dir /tmp/nimbus-linux-packages \
    --nimbus-binary /tmp/nimbus \
    --nimbus-libkrun-archive /tmp/nimbus-libkrun-linux-amd64.tar.gz \
    --nimbus-crun-binary /tmp/nimbus-crun \
    --version 0.1.10 \
    --arch amd64

  bash scripts/build-linux-release-packages.sh \
    --output-dir /tmp/nimbus-linux-packages \
    --nimbus-binary /tmp/nimbus \
    --nimbus-libkrun-archive /tmp/nimbus-libkrun-linux-amd64.tar.gz \
    --nimbus-crun-binary /tmp/nimbus-crun \
    --version v0.1.10 \
    --libkrun-version v1.17.4-nimbus.1 \
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

write_nimbus_libkrun_readme() {
  local file_path="$1"
  local version="$2"
  cat >"$file_path" <<EOF
# nimbus-libkrun

Version: ${version}
Repository: https://github.com/nimbus/nimbus-libkrun

This package installs the Nimbus-private libkrun runtime stack under
/usr/libexec/nimbus/lib and its matching headers under
/usr/libexec/nimbus/include.

It does not replace distro libkrun or libkrunfw packages. Nimbus-private crun
resolves this package through its private runtime path.
EOF
}

write_nimbus_bun_jsc_adapter_readme() {
  local file_path="$1"
  local adapter_version="$2"
  cat >"$file_path" <<EOF
# nimbus-bun-jsc-adapter

Adapter version: ${adapter_version}
Repository: https://github.com/nimbus/nimbus

This optional package installs the in-process Bun/JSC runtime adapter under
/usr/libexec/nimbus/runtime/bun-jsc.

The default Nimbus binary works without this package and reports the Bun/JSC
lane as not_linked. Installing this package makes the adapter discoverable via
/usr/libexec/nimbus/runtime/bun-jsc/current/nimbus-bun-jsc-adapter.json without
requiring NIMBUS_BUN_EMBED_SHARED_LIBRARY.
EOF
}

linux_target_triple_for_arch() {
  case "$1" in
    amd64)
      printf 'x86_64-unknown-linux-gnu\n'
      ;;
    arm64)
      printf 'aarch64-unknown-linux-gnu\n'
      ;;
    *)
      die "unsupported Bun/JSC adapter package architecture: $1"
      ;;
  esac
}

bun_jsc_platform_arch_for_arch() {
  case "$1" in
    amd64)
      printf 'linux-x86_64\n'
      ;;
    arm64)
      printf 'linux-arm64\n'
      ;;
    *)
      die "unsupported Bun/JSC adapter package architecture: $1"
      ;;
  esac
}

read_bun_jsc_adapter_version() {
  local manifest_path="$1"
  command -v python3 >/dev/null 2>&1 || die "python3 is required to read the Bun/JSC adapter manifest"
  python3 - "$manifest_path" <<'PY'
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text())
version = manifest.get("adapter_version")
if not isinstance(version, str) or not version.strip():
    raise SystemExit("adapter_version must be a non-empty string")
print(version)
PY
}

stage_bun_jsc_adapter_archive() {
  local archive_path="$1"
  local staged_root="$2"
  local package_arch="$3"
  local target_triple platform_arch extract_dir adapter_version version_dir
  local manifest_name="nimbus-bun-jsc-adapter.json"
  local checksums_name="checksums-sha256.txt"
  local readme_name="README.md"
  local library_name="libnimbus_bun_jsc_embedder.so"

  target_triple="$(linux_target_triple_for_arch "$package_arch")"
  platform_arch="$(bun_jsc_platform_arch_for_arch "$package_arch")"
  case "$(basename "$archive_path")" in
    nimbus-bun-jsc-adapter-"${platform_arch}".tar.gz) ;;
    *)
      die "Bun/JSC adapter archive name must match ${platform_arch}: $(basename "$archive_path")"
      ;;
  esac

  verify_args=(
    bash scripts/verify-bun-jsc-adapter-package.sh
    --archive "$archive_path"
    --target-triple "$target_triple"
  )
  if [[ -n "${NIMBUS_BUN_JSC_ADAPTER_NM:-}" ]]; then
    verify_args+=(--nm "${NIMBUS_BUN_JSC_ADAPTER_NM}")
  fi
  "${verify_args[@]}"

  extract_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-bun-jsc-adapter-package.XXXXXX")"
  tar -xzf "$archive_path" -C "$extract_dir"
  adapter_version="$(read_bun_jsc_adapter_version "${extract_dir}/${manifest_name}")"
  case "$adapter_version" in
    ""|*/*|*..*)
      die "unsafe Bun/JSC adapter version for install path: ${adapter_version}"
      ;;
  esac

  version_dir="${staged_root}/usr/libexec/nimbus/runtime/bun-jsc/${adapter_version}"
  install -d "$version_dir" \
    "${staged_root}/usr/share/doc/nimbus-bun-jsc-adapter"
  install -m 0755 "${extract_dir}/${library_name}" "${version_dir}/${library_name}"
  install -m 0644 "${extract_dir}/${manifest_name}" "${version_dir}/${manifest_name}"
  install -m 0644 "${extract_dir}/${checksums_name}" "${version_dir}/${checksums_name}"
  install -m 0644 "${extract_dir}/${readme_name}" "${version_dir}/${readme_name}"
  if [[ -f "${extract_dir}/nimbus-bun-jsc-adapter.sbom.cdx.json" ]]; then
    install -m 0644 \
      "${extract_dir}/nimbus-bun-jsc-adapter.sbom.cdx.json" \
      "${version_dir}/nimbus-bun-jsc-adapter.sbom.cdx.json"
  fi
  if [[ -f "${extract_dir}/nimbus-bun-jsc-adapter.intoto.jsonl" ]]; then
    install -m 0644 \
      "${extract_dir}/nimbus-bun-jsc-adapter.intoto.jsonl" \
      "${version_dir}/nimbus-bun-jsc-adapter.intoto.jsonl"
  fi
  ln -sfn "$adapter_version" "${staged_root}/usr/libexec/nimbus/runtime/bun-jsc/current"
  install -m 0644 LICENSE "${staged_root}/usr/share/doc/nimbus-bun-jsc-adapter/LICENSE"
  write_nimbus_bun_jsc_adapter_readme \
    "${staged_root}/usr/share/doc/nimbus-bun-jsc-adapter/README.md" \
    "$adapter_version"
  rm -rf "$extract_dir"
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

render_nimbus_libkrun_manifest() {
  local manifest_path="$1"
  local version="$2"
  local arch="$3"
  local staged_root="$4"

  cat >"$manifest_path" <<EOF
# yaml-language-server: \$schema=https://nfpm.goreleaser.com/schema.json
name: nimbus-libkrun
arch: ${arch}
platform: linux
version: ${version}
version_schema: semver
section: admin
priority: optional
maintainer: Nimbus
vendor: Nimbus
homepage: https://github.com/nimbus/nimbus-libkrun
license: Nimbus-Community-1.0
description: |
  Nimbus-private libkrun runtime stack for KVM-based service execution.

  This package installs the validated libkrun and libkrunfw runtime libraries
  under /usr/libexec/nimbus/lib for use by nimbus-crun.
rpm:
  summary: Nimbus-private libkrun runtime stack
  group: Applications/System
contents:
  - src: ${staged_root}/usr/libexec/nimbus/lib
    dst: /usr/libexec/nimbus/lib
    type: tree
  - src: ${staged_root}/usr/libexec/nimbus/include
    dst: /usr/libexec/nimbus/include
    type: tree
  - src: ${staged_root}/usr/libexec/nimbus/NIMBUS_LIBKRUN_RELEASE.txt
    dst: /usr/libexec/nimbus/NIMBUS_LIBKRUN_RELEASE.txt
    file_info:
      mode: 0644
  - src: ${staged_root}/usr/share/doc/nimbus-libkrun/README.md
    dst: /usr/share/doc/nimbus-libkrun/README.md
    file_info:
      mode: 0644
  - src: ${staged_root}/usr/share/doc/nimbus-libkrun/LICENSE
    dst: /usr/share/doc/nimbus-libkrun/LICENSE
    file_info:
      mode: 0644
EOF
}

render_nimbus_bun_jsc_adapter_manifest() {
  local manifest_path="$1"
  local version="$2"
  local arch="$3"
  local staged_root="$4"

  cat >"$manifest_path" <<EOF
# yaml-language-server: \$schema=https://nfpm.goreleaser.com/schema.json
name: nimbus-bun-jsc-adapter
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
  Optional in-process Bun/JSC runtime adapter for Nimbus.

  This package installs the verified adapter manifest and shared library under
  /usr/libexec/nimbus/runtime/bun-jsc and points current/ at the packaged
  adapter version.
rpm:
  summary: Optional Bun/JSC runtime adapter for Nimbus
  group: Applications/Internet
contents:
  - src: ${staged_root}/usr/libexec/nimbus/runtime/bun-jsc
    dst: /usr/libexec/nimbus/runtime/bun-jsc
    type: tree
  - src: ${staged_root}/usr/share/doc/nimbus-bun-jsc-adapter/README.md
    dst: /usr/share/doc/nimbus-bun-jsc-adapter/README.md
    file_info:
      mode: 0644
  - src: ${staged_root}/usr/share/doc/nimbus-bun-jsc-adapter/LICENSE
    dst: /usr/share/doc/nimbus-bun-jsc-adapter/LICENSE
    file_info:
      mode: 0644
EOF
  append_yaml_list "$manifest_path" "depends" "nimbus"
}

output_dir=""
nimbus_binary=""
nimbus_libkrun_archive=""
nimbus_crun_binary=""
nimbus_bun_jsc_adapter_archive=""
version=""
libkrun_version=""
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
    --nimbus-libkrun-archive)
      nimbus_libkrun_archive="${2:-}"
      shift 2
      ;;
    --nimbus-crun-binary)
      nimbus_crun_binary="${2:-}"
      shift 2
      ;;
    --bun-jsc-adapter-archive)
      nimbus_bun_jsc_adapter_archive="${2:-}"
      shift 2
      ;;
    --version)
      version="${2:-}"
      shift 2
      ;;
    --libkrun-version)
      libkrun_version="${2:-}"
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
[[ -n "$nimbus_libkrun_archive" ]] || die "--nimbus-libkrun-archive is required"
[[ -n "$nimbus_crun_binary" ]] || die "--nimbus-crun-binary is required"
[[ -n "$version" ]] || die "--version is required"

if [[ "${#formats[@]}" -eq 0 ]]; then
  formats=(deb rpm)
fi

version="${version#v}"
if [[ -z "$libkrun_version" ]]; then
  die "--libkrun-version is required"
else
  libkrun_version="${libkrun_version#v}"
fi
if [[ -z "$crun_version" ]]; then
  crun_version="$version"
else
  crun_version="${crun_version#v}"
fi

if [[ -z "$arch" ]]; then
  arch="$(normalize_arch "$(uname -m)")"
fi

[[ -f "$nimbus_binary" ]] || die "nimbus binary not found: $nimbus_binary"
[[ -f "$nimbus_libkrun_archive" ]] || die "nimbus-libkrun archive not found: $nimbus_libkrun_archive"
[[ -f "$nimbus_crun_binary" ]] || die "nimbus-crun binary not found: $nimbus_crun_binary"
if [[ -n "$nimbus_bun_jsc_adapter_archive" ]]; then
  [[ -f "$nimbus_bun_jsc_adapter_archive" ]] || die "Bun/JSC adapter archive not found: $nimbus_bun_jsc_adapter_archive"
fi

mkdir -p "$output_dir"
output_dir="$(cd "$output_dir" && pwd)"

staging_dir="${output_dir}/staging"
manifests_dir="${output_dir}/manifests"
packages_dir="${output_dir}/packages"
package_checksums_path="${packages_dir}/checksums-sha256.txt"
rm -rf "$staging_dir" "$manifests_dir" "$packages_dir"
mkdir -p "$staging_dir" "$manifests_dir" "$packages_dir"

nimbus_stage="${staging_dir}/nimbus"
nimbus_libkrun_stage="${staging_dir}/nimbus-libkrun"
nimbus_crun_stage="${staging_dir}/nimbus-crun"
nimbus_bun_jsc_adapter_stage="${staging_dir}/nimbus-bun-jsc-adapter"

install -d "${nimbus_stage}/usr/bin" \
  "${nimbus_stage}/usr/share/doc/nimbus" \
  "${nimbus_libkrun_stage}/usr/libexec/nimbus" \
  "${nimbus_libkrun_stage}/usr/share/doc/nimbus-libkrun" \
  "${nimbus_crun_stage}/usr/libexec/nimbus" \
  "${nimbus_crun_stage}/usr/share/doc/nimbus-crun"

install -m 0755 "$nimbus_binary" "${nimbus_stage}/usr/bin/nimbus"
tar -xzf "$nimbus_libkrun_archive" -C "${nimbus_libkrun_stage}/usr/libexec/nimbus"
install -m 0755 "$nimbus_crun_binary" "${nimbus_crun_stage}/usr/libexec/nimbus/crun"
install -m 0644 LICENSE "${nimbus_stage}/usr/share/doc/nimbus/LICENSE"
install -m 0644 LICENSE "${nimbus_libkrun_stage}/usr/share/doc/nimbus-libkrun/LICENSE"
install -m 0644 LICENSE "${nimbus_crun_stage}/usr/share/doc/nimbus-crun/LICENSE"
write_nimbus_readme "${nimbus_stage}/usr/share/doc/nimbus/README.md" "$version"
write_nimbus_libkrun_readme "${nimbus_libkrun_stage}/usr/share/doc/nimbus-libkrun/README.md" "$libkrun_version"
write_nimbus_crun_readme "${nimbus_crun_stage}/usr/share/doc/nimbus-crun/README.md" "$crun_version"
if [[ -n "$nimbus_bun_jsc_adapter_archive" ]]; then
  stage_bun_jsc_adapter_archive "$nimbus_bun_jsc_adapter_archive" "$nimbus_bun_jsc_adapter_stage" "$arch"
fi

[[ -e "${nimbus_libkrun_stage}/usr/libexec/nimbus/lib/libkrun.so.1" ]] || die "nimbus-libkrun archive is missing lib/libkrun.so.1"
[[ -e "${nimbus_libkrun_stage}/usr/libexec/nimbus/lib/libkrunfw.so.5" ]] || die "nimbus-libkrun archive is missing lib/libkrunfw.so.5"
[[ -f "${nimbus_libkrun_stage}/usr/libexec/nimbus/NIMBUS_LIBKRUN_RELEASE.txt" ]] || die "nimbus-libkrun archive is missing NIMBUS_LIBKRUN_RELEASE.txt"

nimbus_deb_manifest="${manifests_dir}/nimbus-deb.yaml"
nimbus_rpm_manifest="${manifests_dir}/nimbus-rpm.yaml"
nimbus_libkrun_deb_manifest="${manifests_dir}/nimbus-libkrun-deb.yaml"
nimbus_libkrun_rpm_manifest="${manifests_dir}/nimbus-libkrun-rpm.yaml"
nimbus_crun_deb_manifest="${manifests_dir}/nimbus-crun-deb.yaml"
nimbus_crun_rpm_manifest="${manifests_dir}/nimbus-crun-rpm.yaml"
nimbus_bun_jsc_adapter_deb_manifest="${manifests_dir}/nimbus-bun-jsc-adapter-deb.yaml"
nimbus_bun_jsc_adapter_rpm_manifest="${manifests_dir}/nimbus-bun-jsc-adapter-rpm.yaml"

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
render_nimbus_libkrun_manifest \
  "$nimbus_libkrun_deb_manifest" \
  "$libkrun_version" \
  "$arch" \
  "$nimbus_libkrun_stage"
render_nimbus_libkrun_manifest \
  "$nimbus_libkrun_rpm_manifest" \
  "$libkrun_version" \
  "$arch" \
  "$nimbus_libkrun_stage"
render_nimbus_crun_manifest \
  "$nimbus_crun_deb_manifest" \
  "$crun_version" \
  "$arch" \
  "$nimbus_crun_stage" \
  "nimbus-libkrun"
render_nimbus_crun_manifest \
  "$nimbus_crun_rpm_manifest" \
  "$crun_version" \
  "$arch" \
  "$nimbus_crun_stage" \
  "nimbus-libkrun"
if [[ -n "$nimbus_bun_jsc_adapter_archive" ]]; then
  render_nimbus_bun_jsc_adapter_manifest \
    "$nimbus_bun_jsc_adapter_deb_manifest" \
    "$version" \
    "$arch" \
    "$nimbus_bun_jsc_adapter_stage"
  render_nimbus_bun_jsc_adapter_manifest \
    "$nimbus_bun_jsc_adapter_rpm_manifest" \
    "$version" \
    "$arch" \
    "$nimbus_bun_jsc_adapter_stage"
fi

printf 'stage.nimbus=%s\n' "$nimbus_stage"
printf 'stage.nimbus_libkrun=%s\n' "$nimbus_libkrun_stage"
printf 'stage.nimbus_crun=%s\n' "$nimbus_crun_stage"
if [[ -n "$nimbus_bun_jsc_adapter_archive" ]]; then
  printf 'stage.nimbus_bun_jsc_adapter=%s\n' "$nimbus_bun_jsc_adapter_stage"
fi
printf 'manifest.nimbus.deb=%s\n' "$nimbus_deb_manifest"
printf 'manifest.nimbus.rpm=%s\n' "$nimbus_rpm_manifest"
printf 'manifest.nimbus_libkrun.deb=%s\n' "$nimbus_libkrun_deb_manifest"
printf 'manifest.nimbus_libkrun.rpm=%s\n' "$nimbus_libkrun_rpm_manifest"
printf 'manifest.nimbus_crun.deb=%s\n' "$nimbus_crun_deb_manifest"
printf 'manifest.nimbus_crun.rpm=%s\n' "$nimbus_crun_rpm_manifest"
if [[ -n "$nimbus_bun_jsc_adapter_archive" ]]; then
  printf 'manifest.nimbus_bun_jsc_adapter.deb=%s\n' "$nimbus_bun_jsc_adapter_deb_manifest"
  printf 'manifest.nimbus_bun_jsc_adapter.rpm=%s\n' "$nimbus_bun_jsc_adapter_rpm_manifest"
fi

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
      manifest_list="${nimbus_deb_manifest} ${nimbus_libkrun_deb_manifest} ${nimbus_crun_deb_manifest}"
      if [[ -n "$nimbus_bun_jsc_adapter_archive" ]]; then
        manifest_list="${manifest_list} ${nimbus_bun_jsc_adapter_deb_manifest}"
      fi
      ;;
    rpm)
      manifest_list="${nimbus_rpm_manifest} ${nimbus_libkrun_rpm_manifest} ${nimbus_crun_rpm_manifest}"
      if [[ -n "$nimbus_bun_jsc_adapter_archive" ]]; then
        manifest_list="${manifest_list} ${nimbus_bun_jsc_adapter_rpm_manifest}"
      fi
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
