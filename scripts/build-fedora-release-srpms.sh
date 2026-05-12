#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: build-fedora-release-srpms.sh --output-dir <path> --nimbus-version <semver> --nimbus-linux-amd64-tarball <path> --nimbus-linux-arm64-tarball <path> --nimbus-crun-version <semver> --nimbus-crun-linux-amd64 <path> --nimbus-crun-linux-arm64 <path> [options]

Build deterministic Fedora/COPR source bundles and SRPMs for the Nimbus Linux
release artifacts without introducing a second source-build pipeline.

Required:
  --output-dir <path>                 Output root for source bundles, specs, and SRPMs
  --nimbus-version <semver>           Released nimbus version (leading `v` accepted)
  --nimbus-linux-amd64-tarball <path> `nimbus_linux_x86_64.tar.gz` release asset
  --nimbus-linux-arm64-tarball <path> `nimbus_linux_arm64.tar.gz` release asset
  --nimbus-crun-version <semver>      Released nimbus-crun version (leading `v` accepted)
  --nimbus-crun-linux-amd64 <path>    `nimbus-crun-linux-amd64` release asset
  --nimbus-crun-linux-arm64 <path>    `nimbus-crun-linux-arm64` release asset

Optional:
  --release <value>                   RPM release number (default: 1)
  --rpmbuild <path>                   Explicit rpmbuild binary path (default: `rpmbuild`)
  --render-only                       Only render source bundles + spec files; do not build SRPMs
  -h, --help                          Show this help text

Examples:
  bash scripts/build-fedora-release-srpms.sh \
    --output-dir /tmp/nimbus-fedora-srpms \
    --nimbus-version v0.1.10 \
    --nimbus-linux-amd64-tarball /tmp/nimbus_linux_x86_64.tar.gz \
    --nimbus-linux-arm64-tarball /tmp/nimbus_linux_arm64.tar.gz \
    --nimbus-crun-version v0.1.4 \
    --nimbus-crun-linux-amd64 /tmp/nimbus-crun-linux-amd64 \
    --nimbus-crun-linux-arm64 /tmp/nimbus-crun-linux-arm64
EOF
}

die() {
  printf '%s\n' "$*" >&2
  exit 64
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

  die "neither sha256sum nor shasum is available to checksum generated artifacts"
}

write_nimbus_readme() {
  local file_path="$1"
  local version="$2"

  cat >"$file_path" <<EOF
# nimbus

Version: ${version}
Repository: https://github.com/nimbus/nimbus

This Fedora/COPR source bundle wraps the published Nimbus Linux release
artifacts instead of rebuilding the V8 host binary from source inside COPR.

The resulting RPM installs the Nimbus host CLI at /usr/bin/nimbus and depends
on the distro container primitives (buildah, conmon, netavark, aardvark-dns)
plus the private nimbus-crun runtime package.
EOF
}

write_nimbus_crun_readme() {
  local file_path="$1"
  local version="$2"

  cat >"$file_path" <<EOF
# nimbus-crun

Version: ${version}
Repository: https://github.com/nimbus/nimbus-crun

This Fedora/COPR source bundle wraps the published patched private runtime
release assets instead of compiling crun again inside COPR.

The resulting RPM installs /usr/libexec/nimbus/crun. It intentionally does not
replace the system crun binary.
EOF
}

render_nimbus_spec() {
  local spec_path="$1"
  local version="$2"
  local release_number="$3"

  cat >"$spec_path" <<EOF
%global debug_package %{nil}
%global _build_id_links none

Name:           nimbus
Version:        ${version}
Release:        ${release_number}%{?dist}
Summary:        Self-hosted JavaScript backend runtime powered by V8
License:        Nimbus-Community-1.0
URL:            https://github.com/nimbus/nimbus
Source0:        %{name}-%{version}-release-artifacts.tar.gz
ExclusiveArch:  x86_64 aarch64
Requires:       buildah
Requires:       conmon
Requires:       netavark
Requires:       aardvark-dns
Requires:       nimbus-crun
Recommends:     fuse-overlayfs
Recommends:     passt
Recommends:     shadow-utils

%description
Self-hosted JavaScript backend runtime powered by V8.

This RPM intentionally mirrors the existing Nimbus GitHub release artifacts
instead of introducing a second Fedora-only binary build pipeline.

%prep
%autosetup -n %{name}-%{version}

%install
rm -rf %{buildroot}

case "%{_arch}" in
  x86_64)
    nimbus_archive="nimbus_linux_x86_64.tar.gz"
    ;;
  aarch64)
    nimbus_archive="nimbus_linux_arm64.tar.gz"
    ;;
  *)
    echo "unsupported architecture: %{_arch}" >&2
    exit 1
    ;;
esac

build_tmp="%{_builddir}/%{name}-%{version}-payload"
rm -rf "\${build_tmp}"
mkdir -p "\${build_tmp}"
tar -xzf "\${nimbus_archive}" -C "\${build_tmp}"

install -D -m 0755 "\${build_tmp}/nimbus" "%{buildroot}%{_bindir}/nimbus"
install -D -m 0644 "README.package.md" "%{buildroot}%{_docdir}/%{name}/README.md"
install -D -m 0644 "LICENSE" "%{buildroot}%{_docdir}/%{name}/LICENSE"

%files
%{_bindir}/nimbus
%doc %{_docdir}/%{name}/README.md
%license %{_docdir}/%{name}/LICENSE

%changelog
* Sat Apr 18 2026 Nimbus <opensource@nimbus.github.io> - ${version}-${release_number}
- Package published release artifacts for Fedora/COPR
EOF
}

render_nimbus_crun_spec() {
  local spec_path="$1"
  local version="$2"
  local release_number="$3"

  cat >"$spec_path" <<EOF
%global debug_package %{nil}
%global _build_id_links none

Name:           nimbus-crun
Version:        ${version}
Release:        ${release_number}%{?dist}
Summary:        Patched private crun runtime for Nimbus
License:        Nimbus-Community-1.0
URL:            https://github.com/nimbus/nimbus-crun
Source0:        %{name}-%{version}-release-artifacts.tar.gz
ExclusiveArch:  x86_64 aarch64
Requires:       libkrun
Requires:       libkrunfw

%description
Patched private crun runtime for Nimbus.

This RPM intentionally mirrors the existing nimbus-crun GitHub release
artifacts instead of introducing a second Fedora-only runtime build pipeline.

%prep
%autosetup -n %{name}-%{version}

%install
rm -rf %{buildroot}

case "%{_arch}" in
  x86_64)
    crun_binary="nimbus-crun-linux-amd64"
    ;;
  aarch64)
    crun_binary="nimbus-crun-linux-arm64"
    ;;
  *)
    echo "unsupported architecture: %{_arch}" >&2
    exit 1
    ;;
esac

install -D -m 0755 "\${crun_binary}" "%{buildroot}%{_libexecdir}/nimbus/crun"
install -D -m 0644 "README.package.md" "%{buildroot}%{_docdir}/%{name}/README.md"
install -D -m 0644 "LICENSE" "%{buildroot}%{_docdir}/%{name}/LICENSE"

%files
%{_libexecdir}/nimbus/crun
%doc %{_docdir}/%{name}/README.md
%license %{_docdir}/%{name}/LICENSE

%changelog
* Sat Apr 18 2026 Nimbus <opensource@nimbus.github.io> - ${version}-${release_number}
- Package published release artifacts for Fedora/COPR
EOF
}

output_dir=""
nimbus_version=""
nimbus_linux_amd64_tarball=""
nimbus_linux_arm64_tarball=""
nimbus_crun_version=""
nimbus_crun_linux_amd64=""
nimbus_crun_linux_arm64=""
release_number="1"
rpmbuild_bin="${RPMBUILD_BIN:-rpmbuild}"
render_only=0

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --output-dir)
      output_dir="${2:-}"
      shift 2
      ;;
    --nimbus-version)
      nimbus_version="${2:-}"
      shift 2
      ;;
    --nimbus-linux-amd64-tarball)
      nimbus_linux_amd64_tarball="${2:-}"
      shift 2
      ;;
    --nimbus-linux-arm64-tarball)
      nimbus_linux_arm64_tarball="${2:-}"
      shift 2
      ;;
    --nimbus-crun-version)
      nimbus_crun_version="${2:-}"
      shift 2
      ;;
    --nimbus-crun-linux-amd64)
      nimbus_crun_linux_amd64="${2:-}"
      shift 2
      ;;
    --nimbus-crun-linux-arm64)
      nimbus_crun_linux_arm64="${2:-}"
      shift 2
      ;;
    --release)
      release_number="${2:-}"
      shift 2
      ;;
    --rpmbuild)
      rpmbuild_bin="${2:-}"
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
[[ -n "$nimbus_version" ]] || die "--nimbus-version is required"
[[ -n "$nimbus_linux_amd64_tarball" ]] || die "--nimbus-linux-amd64-tarball is required"
[[ -n "$nimbus_linux_arm64_tarball" ]] || die "--nimbus-linux-arm64-tarball is required"
[[ -n "$nimbus_crun_version" ]] || die "--nimbus-crun-version is required"
[[ -n "$nimbus_crun_linux_amd64" ]] || die "--nimbus-crun-linux-amd64 is required"
[[ -n "$nimbus_crun_linux_arm64" ]] || die "--nimbus-crun-linux-arm64 is required"

nimbus_version="${nimbus_version#v}"
nimbus_crun_version="${nimbus_crun_version#v}"

[[ -f "$nimbus_linux_amd64_tarball" ]] || die "nimbus amd64 tarball not found: $nimbus_linux_amd64_tarball"
[[ -f "$nimbus_linux_arm64_tarball" ]] || die "nimbus arm64 tarball not found: $nimbus_linux_arm64_tarball"
[[ -f "$nimbus_crun_linux_amd64" ]] || die "nimbus-crun amd64 asset not found: $nimbus_crun_linux_amd64"
[[ -f "$nimbus_crun_linux_arm64" ]] || die "nimbus-crun arm64 asset not found: $nimbus_crun_linux_arm64"

mkdir -p "$output_dir"
output_dir="$(cd "$output_dir" && pwd)"

build_dir="${output_dir}/build"
source_bundles_dir="${output_dir}/source-bundles"
specs_dir="${output_dir}/specs"
srpms_dir="${output_dir}/srpms"
checksums_path="${output_dir}/checksums-sha256.txt"
rpmbuild_topdir="${output_dir}/rpmbuild"

rm -rf "$build_dir" "$source_bundles_dir" "$specs_dir" "$srpms_dir" "$rpmbuild_topdir"
mkdir -p "$build_dir" "$source_bundles_dir" "$specs_dir" "$srpms_dir" "$rpmbuild_topdir"

nimbus_source_root="${build_dir}/nimbus-${nimbus_version}"
mkdir -p "$nimbus_source_root"
install -m 0644 LICENSE "${nimbus_source_root}/LICENSE"
install -m 0644 "$nimbus_linux_amd64_tarball" "${nimbus_source_root}/nimbus_linux_x86_64.tar.gz"
install -m 0644 "$nimbus_linux_arm64_tarball" "${nimbus_source_root}/nimbus_linux_arm64.tar.gz"
write_nimbus_readme "${nimbus_source_root}/README.package.md" "$nimbus_version"

nimbus_source_bundle="${source_bundles_dir}/nimbus-${nimbus_version}-release-artifacts.tar.gz"
COPYFILE_DISABLE=1 COPY_EXTENDED_ATTRIBUTES_DISABLE=1 tar -czf "$nimbus_source_bundle" -C "$build_dir" "nimbus-${nimbus_version}"

nimbus_crun_source_root="${build_dir}/nimbus-crun-${nimbus_crun_version}"
mkdir -p "$nimbus_crun_source_root"
install -m 0644 LICENSE "${nimbus_crun_source_root}/LICENSE"
install -m 0644 "$nimbus_crun_linux_amd64" "${nimbus_crun_source_root}/nimbus-crun-linux-amd64"
install -m 0644 "$nimbus_crun_linux_arm64" "${nimbus_crun_source_root}/nimbus-crun-linux-arm64"
write_nimbus_crun_readme "${nimbus_crun_source_root}/README.package.md" "$nimbus_crun_version"

nimbus_crun_source_bundle="${source_bundles_dir}/nimbus-crun-${nimbus_crun_version}-release-artifacts.tar.gz"
COPYFILE_DISABLE=1 COPY_EXTENDED_ATTRIBUTES_DISABLE=1 tar -czf "$nimbus_crun_source_bundle" -C "$build_dir" "nimbus-crun-${nimbus_crun_version}"

nimbus_spec="${specs_dir}/nimbus.spec"
nimbus_crun_spec="${specs_dir}/nimbus-crun.spec"
render_nimbus_spec "$nimbus_spec" "$nimbus_version" "$release_number"
render_nimbus_crun_spec "$nimbus_crun_spec" "$nimbus_crun_version" "$release_number"

artifact_paths=(
  "$nimbus_source_bundle"
  "$nimbus_crun_source_bundle"
  "$nimbus_spec"
  "$nimbus_crun_spec"
)

printf 'source_bundle.nimbus=%s\n' "$nimbus_source_bundle"
printf 'source_bundle.nimbus_crun=%s\n' "$nimbus_crun_source_bundle"
printf 'spec.nimbus=%s\n' "$nimbus_spec"
printf 'spec.nimbus_crun=%s\n' "$nimbus_crun_spec"

if [[ "$render_only" -eq 0 ]]; then
  if ! command -v "$rpmbuild_bin" >/dev/null 2>&1; then
    die "rpmbuild not found: ${rpmbuild_bin} (use --render-only or install rpm-build)"
  fi

  "$rpmbuild_bin" \
    --define "_topdir ${rpmbuild_topdir}" \
    --define "_sourcedir ${source_bundles_dir}" \
    --define "_specdir ${specs_dir}" \
    --define "_srcrpmdir ${srpms_dir}" \
    --define "_rpmdir ${output_dir}/rpms" \
    --define "dist %{nil}" \
    -bs "$nimbus_spec"

  "$rpmbuild_bin" \
    --define "_topdir ${rpmbuild_topdir}" \
    --define "_sourcedir ${source_bundles_dir}" \
    --define "_specdir ${specs_dir}" \
    --define "_srcrpmdir ${srpms_dir}" \
    --define "_rpmdir ${output_dir}/rpms" \
    --define "dist %{nil}" \
    -bs "$nimbus_crun_spec"

  nimbus_srpm="${srpms_dir}/nimbus-${nimbus_version}-${release_number}.src.rpm"
  nimbus_crun_srpm="${srpms_dir}/nimbus-crun-${nimbus_crun_version}-${release_number}.src.rpm"
  [[ -f "$nimbus_srpm" ]] || die "expected SRPM not found: $nimbus_srpm"
  [[ -f "$nimbus_crun_srpm" ]] || die "expected SRPM not found: $nimbus_crun_srpm"

  artifact_paths+=("$nimbus_srpm" "$nimbus_crun_srpm")
  printf 'srpm.nimbus=%s\n' "$nimbus_srpm"
  printf 'srpm.nimbus_crun=%s\n' "$nimbus_crun_srpm"
fi

sha256_file "$checksums_path" "${artifact_paths[@]}"
printf 'artifacts.checksums=%s\n' "$checksums_path"

if [[ "$render_only" -eq 1 ]]; then
  printf 'result=rendered\n'
else
  printf 'result=srpm-built\n'
fi
