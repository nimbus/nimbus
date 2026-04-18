#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: build-fedora-release-srpms.sh --output-dir <path> --neovex-version <semver> --neovex-linux-amd64-tarball <path> --neovex-linux-arm64-tarball <path> --neovex-crun-version <semver> --neovex-crun-linux-amd64 <path> --neovex-crun-linux-arm64 <path> [options]

Build deterministic Fedora/COPR source bundles and SRPMs for the Neovex Linux
release artifacts without introducing a second source-build pipeline.

Required:
  --output-dir <path>                 Output root for source bundles, specs, and SRPMs
  --neovex-version <semver>           Released neovex version (leading `v` accepted)
  --neovex-linux-amd64-tarball <path> `neovex_linux_x86_64.tar.gz` release asset
  --neovex-linux-arm64-tarball <path> `neovex_linux_arm64.tar.gz` release asset
  --neovex-crun-version <semver>      Released neovex-crun version (leading `v` accepted)
  --neovex-crun-linux-amd64 <path>    `neovex-crun-linux-amd64` release asset
  --neovex-crun-linux-arm64 <path>    `neovex-crun-linux-arm64` release asset

Optional:
  --release <value>                   RPM release number (default: 1)
  --rpmbuild <path>                   Explicit rpmbuild binary path (default: `rpmbuild`)
  --render-only                       Only render source bundles + spec files; do not build SRPMs
  -h, --help                          Show this help text

Examples:
  bash scripts/build-fedora-release-srpms.sh \
    --output-dir /tmp/neovex-fedora-srpms \
    --neovex-version v0.1.10 \
    --neovex-linux-amd64-tarball /tmp/neovex_linux_x86_64.tar.gz \
    --neovex-linux-arm64-tarball /tmp/neovex_linux_arm64.tar.gz \
    --neovex-crun-version v0.1.4 \
    --neovex-crun-linux-amd64 /tmp/neovex-crun-linux-amd64 \
    --neovex-crun-linux-arm64 /tmp/neovex-crun-linux-arm64
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

write_neovex_readme() {
  local file_path="$1"
  local version="$2"

  cat >"$file_path" <<EOF
# neovex

Version: ${version}
Repository: https://github.com/agentstation/neovex

This Fedora/COPR source bundle wraps the published Neovex Linux release
artifacts instead of rebuilding the V8 host binary from source inside COPR.

The resulting RPM installs the Neovex host CLI at /usr/bin/neovex and depends
on the distro container primitives (buildah, conmon, netavark, aardvark-dns)
plus the private neovex-crun runtime package.
EOF
}

write_neovex_crun_readme() {
  local file_path="$1"
  local version="$2"

  cat >"$file_path" <<EOF
# neovex-crun

Version: ${version}
Repository: https://github.com/agentstation/neovex-crun

This Fedora/COPR source bundle wraps the published patched private runtime
release assets instead of compiling crun again inside COPR.

The resulting RPM installs /usr/libexec/neovex/crun. It intentionally does not
replace the system crun binary.
EOF
}

render_neovex_spec() {
  local spec_path="$1"
  local version="$2"
  local release_number="$3"

  cat >"$spec_path" <<EOF
%global debug_package %{nil}
%global _build_id_links none

Name:           neovex
Version:        ${version}
Release:        ${release_number}%{?dist}
Summary:        Self-hosted JavaScript backend runtime powered by V8
License:        Neovex-Community-1.0
URL:            https://github.com/agentstation/neovex
Source0:        %{name}-%{version}-release-artifacts.tar.gz
ExclusiveArch:  x86_64 aarch64
Requires:       buildah
Requires:       conmon
Requires:       netavark
Requires:       aardvark-dns
Requires:       neovex-crun
Recommends:     fuse-overlayfs
Recommends:     passt
Recommends:     shadow-utils

%description
Self-hosted JavaScript backend runtime powered by V8.

This RPM intentionally mirrors the existing Neovex GitHub release artifacts
instead of introducing a second Fedora-only binary build pipeline.

%prep
%autosetup -n %{name}-%{version}

%install
rm -rf %{buildroot}

case "%{_arch}" in
  x86_64)
    neovex_archive="neovex_linux_x86_64.tar.gz"
    ;;
  aarch64)
    neovex_archive="neovex_linux_arm64.tar.gz"
    ;;
  *)
    echo "unsupported architecture: %{_arch}" >&2
    exit 1
    ;;
esac

build_tmp="%{_builddir}/%{name}-%{version}-payload"
rm -rf "\${build_tmp}"
mkdir -p "\${build_tmp}"
tar -xzf "\${neovex_archive}" -C "\${build_tmp}"

install -D -m 0755 "\${build_tmp}/neovex" "%{buildroot}%{_bindir}/neovex"
install -D -m 0644 "README.package.md" "%{buildroot}%{_docdir}/%{name}/README.md"
install -D -m 0644 "LICENSE" "%{buildroot}%{_docdir}/%{name}/LICENSE"

%files
%{_bindir}/neovex
%doc %{_docdir}/%{name}/README.md
%license %{_docdir}/%{name}/LICENSE

%changelog
* Sat Apr 18 2026 AgentStation <opensource@agentstation.ai> - ${version}-${release_number}
- Package published release artifacts for Fedora/COPR
EOF
}

render_neovex_crun_spec() {
  local spec_path="$1"
  local version="$2"
  local release_number="$3"

  cat >"$spec_path" <<EOF
%global debug_package %{nil}
%global _build_id_links none

Name:           neovex-crun
Version:        ${version}
Release:        ${release_number}%{?dist}
Summary:        Patched private crun runtime for Neovex
License:        Neovex-Community-1.0
URL:            https://github.com/agentstation/neovex-crun
Source0:        %{name}-%{version}-release-artifacts.tar.gz
ExclusiveArch:  x86_64 aarch64
Requires:       libkrun
Requires:       libkrunfw

%description
Patched private crun runtime for Neovex.

This RPM intentionally mirrors the existing neovex-crun GitHub release
artifacts instead of introducing a second Fedora-only runtime build pipeline.

%prep
%autosetup -n %{name}-%{version}

%install
rm -rf %{buildroot}

case "%{_arch}" in
  x86_64)
    crun_binary="neovex-crun-linux-amd64"
    ;;
  aarch64)
    crun_binary="neovex-crun-linux-arm64"
    ;;
  *)
    echo "unsupported architecture: %{_arch}" >&2
    exit 1
    ;;
esac

install -D -m 0755 "\${crun_binary}" "%{buildroot}%{_libexecdir}/neovex/crun"
install -D -m 0644 "README.package.md" "%{buildroot}%{_docdir}/%{name}/README.md"
install -D -m 0644 "LICENSE" "%{buildroot}%{_docdir}/%{name}/LICENSE"

%files
%{_libexecdir}/neovex/crun
%doc %{_docdir}/%{name}/README.md
%license %{_docdir}/%{name}/LICENSE

%changelog
* Sat Apr 18 2026 AgentStation <opensource@agentstation.ai> - ${version}-${release_number}
- Package published release artifacts for Fedora/COPR
EOF
}

output_dir=""
neovex_version=""
neovex_linux_amd64_tarball=""
neovex_linux_arm64_tarball=""
neovex_crun_version=""
neovex_crun_linux_amd64=""
neovex_crun_linux_arm64=""
release_number="1"
rpmbuild_bin="${RPMBUILD_BIN:-rpmbuild}"
render_only=0

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --output-dir)
      output_dir="${2:-}"
      shift 2
      ;;
    --neovex-version)
      neovex_version="${2:-}"
      shift 2
      ;;
    --neovex-linux-amd64-tarball)
      neovex_linux_amd64_tarball="${2:-}"
      shift 2
      ;;
    --neovex-linux-arm64-tarball)
      neovex_linux_arm64_tarball="${2:-}"
      shift 2
      ;;
    --neovex-crun-version)
      neovex_crun_version="${2:-}"
      shift 2
      ;;
    --neovex-crun-linux-amd64)
      neovex_crun_linux_amd64="${2:-}"
      shift 2
      ;;
    --neovex-crun-linux-arm64)
      neovex_crun_linux_arm64="${2:-}"
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
[[ -n "$neovex_version" ]] || die "--neovex-version is required"
[[ -n "$neovex_linux_amd64_tarball" ]] || die "--neovex-linux-amd64-tarball is required"
[[ -n "$neovex_linux_arm64_tarball" ]] || die "--neovex-linux-arm64-tarball is required"
[[ -n "$neovex_crun_version" ]] || die "--neovex-crun-version is required"
[[ -n "$neovex_crun_linux_amd64" ]] || die "--neovex-crun-linux-amd64 is required"
[[ -n "$neovex_crun_linux_arm64" ]] || die "--neovex-crun-linux-arm64 is required"

neovex_version="${neovex_version#v}"
neovex_crun_version="${neovex_crun_version#v}"

[[ -f "$neovex_linux_amd64_tarball" ]] || die "neovex amd64 tarball not found: $neovex_linux_amd64_tarball"
[[ -f "$neovex_linux_arm64_tarball" ]] || die "neovex arm64 tarball not found: $neovex_linux_arm64_tarball"
[[ -f "$neovex_crun_linux_amd64" ]] || die "neovex-crun amd64 asset not found: $neovex_crun_linux_amd64"
[[ -f "$neovex_crun_linux_arm64" ]] || die "neovex-crun arm64 asset not found: $neovex_crun_linux_arm64"

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

neovex_source_root="${build_dir}/neovex-${neovex_version}"
mkdir -p "$neovex_source_root"
install -m 0644 LICENSE "${neovex_source_root}/LICENSE"
install -m 0644 "$neovex_linux_amd64_tarball" "${neovex_source_root}/neovex_linux_x86_64.tar.gz"
install -m 0644 "$neovex_linux_arm64_tarball" "${neovex_source_root}/neovex_linux_arm64.tar.gz"
write_neovex_readme "${neovex_source_root}/README.package.md" "$neovex_version"

neovex_source_bundle="${source_bundles_dir}/neovex-${neovex_version}-release-artifacts.tar.gz"
COPYFILE_DISABLE=1 COPY_EXTENDED_ATTRIBUTES_DISABLE=1 tar -czf "$neovex_source_bundle" -C "$build_dir" "neovex-${neovex_version}"

neovex_crun_source_root="${build_dir}/neovex-crun-${neovex_crun_version}"
mkdir -p "$neovex_crun_source_root"
install -m 0644 LICENSE "${neovex_crun_source_root}/LICENSE"
install -m 0644 "$neovex_crun_linux_amd64" "${neovex_crun_source_root}/neovex-crun-linux-amd64"
install -m 0644 "$neovex_crun_linux_arm64" "${neovex_crun_source_root}/neovex-crun-linux-arm64"
write_neovex_crun_readme "${neovex_crun_source_root}/README.package.md" "$neovex_crun_version"

neovex_crun_source_bundle="${source_bundles_dir}/neovex-crun-${neovex_crun_version}-release-artifacts.tar.gz"
COPYFILE_DISABLE=1 COPY_EXTENDED_ATTRIBUTES_DISABLE=1 tar -czf "$neovex_crun_source_bundle" -C "$build_dir" "neovex-crun-${neovex_crun_version}"

neovex_spec="${specs_dir}/neovex.spec"
neovex_crun_spec="${specs_dir}/neovex-crun.spec"
render_neovex_spec "$neovex_spec" "$neovex_version" "$release_number"
render_neovex_crun_spec "$neovex_crun_spec" "$neovex_crun_version" "$release_number"

artifact_paths=(
  "$neovex_source_bundle"
  "$neovex_crun_source_bundle"
  "$neovex_spec"
  "$neovex_crun_spec"
)

printf 'source_bundle.neovex=%s\n' "$neovex_source_bundle"
printf 'source_bundle.neovex_crun=%s\n' "$neovex_crun_source_bundle"
printf 'spec.neovex=%s\n' "$neovex_spec"
printf 'spec.neovex_crun=%s\n' "$neovex_crun_spec"

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
    -bs "$neovex_spec"

  "$rpmbuild_bin" \
    --define "_topdir ${rpmbuild_topdir}" \
    --define "_sourcedir ${source_bundles_dir}" \
    --define "_specdir ${specs_dir}" \
    --define "_srcrpmdir ${srpms_dir}" \
    --define "_rpmdir ${output_dir}/rpms" \
    --define "dist %{nil}" \
    -bs "$neovex_crun_spec"

  neovex_srpm="${srpms_dir}/neovex-${neovex_version}-${release_number}.src.rpm"
  neovex_crun_srpm="${srpms_dir}/neovex-crun-${neovex_crun_version}-${release_number}.src.rpm"
  [[ -f "$neovex_srpm" ]] || die "expected SRPM not found: $neovex_srpm"
  [[ -f "$neovex_crun_srpm" ]] || die "expected SRPM not found: $neovex_crun_srpm"

  artifact_paths+=("$neovex_srpm" "$neovex_crun_srpm")
  printf 'srpm.neovex=%s\n' "$neovex_srpm"
  printf 'srpm.neovex_crun=%s\n' "$neovex_crun_srpm"
fi

sha256_file "$checksums_path" "${artifact_paths[@]}"
printf 'artifacts.checksums=%s\n' "$checksums_path"

if [[ "$render_only" -eq 1 ]]; then
  printf 'result=rendered\n'
else
  printf 'result=srpm-built\n'
fi
