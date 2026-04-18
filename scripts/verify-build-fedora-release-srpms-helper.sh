#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="$(mktemp -d "${TMPDIR:-/tmp}/neovex-fedora-srpm-helper.XXXXXX")"
trap 'rm -rf "${output_dir}"' EXIT

make_neovex_tarball() {
  local tarball_path="$1"
  local version_label="$2"
  local staging_dir

  staging_dir="$(mktemp -d "${output_dir}/neovex-tarball.XXXXXX")"
  mkdir -p "${staging_dir}"

  cat >"${staging_dir}/neovex" <<EOF
#!/bin/sh
printf 'neovex ${version_label}\n'
EOF
  chmod 0755 "${staging_dir}/neovex"
  printf 'stub readme\n' >"${staging_dir}/README.md"
  printf 'stub license\n' >"${staging_dir}/LICENSE"

  COPYFILE_DISABLE=1 COPY_EXTENDED_ATTRIBUTES_DISABLE=1 tar -czf "${tarball_path}" -C "${staging_dir}" neovex README.md LICENSE
}

make_executable_stub() {
  local file_path="$1"
  local label="$2"

  cat >"${file_path}" <<EOF
#!/bin/sh
printf '${label}\n'
EOF
  chmod 0755 "${file_path}"
}

make_neovex_tarball "${output_dir}/neovex_linux_x86_64.tar.gz" "0.1.10-x86_64"
make_neovex_tarball "${output_dir}/neovex_linux_arm64.tar.gz" "0.1.10-aarch64"
make_executable_stub "${output_dir}/neovex-crun-linux-amd64" "neovex-crun 0.1.4 x86_64"
make_executable_stub "${output_dir}/neovex-crun-linux-arm64" "neovex-crun 0.1.4 aarch64"

cd "${repo_root}"

bash -n scripts/build-fedora-release-srpms.sh

if ! command -v docker >/dev/null 2>&1; then
  printf 'docker is required to verify Fedora/COPR SRPM generation on non-Fedora hosts\n' >&2
  exit 1
fi

docker run --rm \
  --platform linux/amd64 \
  -v "${repo_root}:/work/repo:ro" \
  -v "${output_dir}:/work/output" \
  -w /work/repo \
  fedora:42 \
  bash -lc '
    set -euo pipefail
    dnf install -y rpm-build rpm tar gzip findutils

    bash scripts/build-fedora-release-srpms.sh \
      --output-dir /work/output/amd64 \
      --neovex-version 0.1.10 \
      --neovex-linux-amd64-tarball /work/output/neovex_linux_x86_64.tar.gz \
      --neovex-linux-arm64-tarball /work/output/neovex_linux_arm64.tar.gz \
      --neovex-crun-version 0.1.4 \
      --neovex-crun-linux-amd64 /work/output/neovex-crun-linux-amd64 \
      --neovex-crun-linux-arm64 /work/output/neovex-crun-linux-arm64 \
      >/work/output/amd64-build-summary.txt

    rpmbuild --rebuild /work/output/amd64/srpms/neovex-0.1.10-1.src.rpm
    rpmbuild --rebuild /work/output/amd64/srpms/neovex-crun-0.1.4-1.src.rpm

    neovex_rpm="$(find /root/rpmbuild/RPMS -type f -name "neovex-[0-9]*.x86_64.rpm" | grep -v debuginfo | head -n 1)"
    neovex_crun_rpm="$(find /root/rpmbuild/RPMS -type f -name "neovex-crun-*.x86_64.rpm" | grep -v debuginfo | head -n 1)"

    test -n "${neovex_rpm}"
    test -n "${neovex_crun_rpm}"

    rpm -qp --requires "${neovex_rpm}" > /work/output/amd64-neovex.requires.txt
    rpm -qp --recommends "${neovex_rpm}" > /work/output/amd64-neovex.recommends.txt
    rpm -qp --requires "${neovex_crun_rpm}" > /work/output/amd64-neovex-crun.requires.txt
    rpm -qpl "${neovex_rpm}" > /work/output/amd64-neovex.files.txt
    rpm -qpl "${neovex_crun_rpm}" > /work/output/amd64-neovex-crun.files.txt

    dnf install -y "${neovex_crun_rpm}" "${neovex_rpm}"
    /usr/bin/neovex > /work/output/amd64-neovex.command.txt
    /usr/libexec/neovex/crun > /work/output/amd64-neovex-crun.command.txt
  '

docker run --rm \
  --platform linux/arm64 \
  -v "${repo_root}:/work/repo:ro" \
  -v "${output_dir}:/work/output" \
  -w /work/repo \
  fedora:42 \
  bash -lc '
    set -euo pipefail
    dnf install -y rpm-build rpm tar gzip findutils

    bash scripts/build-fedora-release-srpms.sh \
      --output-dir /work/output/arm64 \
      --neovex-version 0.1.10 \
      --neovex-linux-amd64-tarball /work/output/neovex_linux_x86_64.tar.gz \
      --neovex-linux-arm64-tarball /work/output/neovex_linux_arm64.tar.gz \
      --neovex-crun-version 0.1.4 \
      --neovex-crun-linux-amd64 /work/output/neovex-crun-linux-amd64 \
      --neovex-crun-linux-arm64 /work/output/neovex-crun-linux-arm64 \
      >/work/output/arm64-build-summary.txt

    rpmbuild --rebuild /work/output/arm64/srpms/neovex-0.1.10-1.src.rpm
    rpmbuild --rebuild /work/output/arm64/srpms/neovex-crun-0.1.4-1.src.rpm

    neovex_rpm="$(find /root/rpmbuild/RPMS -type f -name "neovex-[0-9]*.aarch64.rpm" | grep -v debuginfo | head -n 1)"
    neovex_crun_rpm="$(find /root/rpmbuild/RPMS -type f -name "neovex-crun-*.aarch64.rpm" | grep -v debuginfo | head -n 1)"

    test -n "${neovex_rpm}"
    test -n "${neovex_crun_rpm}"

    rpm -qp --requires "${neovex_rpm}" > /work/output/arm64-neovex.requires.txt
    rpm -qp --recommends "${neovex_rpm}" > /work/output/arm64-neovex.recommends.txt
    rpm -qp --requires "${neovex_crun_rpm}" > /work/output/arm64-neovex-crun.requires.txt
    rpm -qpl "${neovex_rpm}" > /work/output/arm64-neovex.files.txt
    rpm -qpl "${neovex_crun_rpm}" > /work/output/arm64-neovex-crun.files.txt

    dnf install -y "${neovex_crun_rpm}" "${neovex_rpm}"
    /usr/bin/neovex > /work/output/arm64-neovex.command.txt
    /usr/libexec/neovex/crun > /work/output/arm64-neovex-crun.command.txt
  '

test -f "${output_dir}/amd64/srpms/neovex-0.1.10-1.src.rpm"
test -f "${output_dir}/amd64/srpms/neovex-crun-0.1.4-1.src.rpm"
test -f "${output_dir}/arm64/srpms/neovex-0.1.10-1.src.rpm"
test -f "${output_dir}/arm64/srpms/neovex-crun-0.1.4-1.src.rpm"
test -f "${output_dir}/amd64/checksums-sha256.txt"
test -f "${output_dir}/arm64/checksums-sha256.txt"

grep -F "Requires:       buildah" "${output_dir}/amd64/specs/neovex.spec" >/dev/null
grep -F "Requires:       libkrun" "${output_dir}/amd64/specs/neovex-crun.spec" >/dev/null
grep -F "result=srpm-built" "${output_dir}/amd64-build-summary.txt" >/dev/null
grep -F "result=srpm-built" "${output_dir}/arm64-build-summary.txt" >/dev/null

grep -F "buildah" "${output_dir}/amd64-neovex.requires.txt" >/dev/null
grep -F "conmon" "${output_dir}/amd64-neovex.requires.txt" >/dev/null
grep -F "netavark" "${output_dir}/amd64-neovex.requires.txt" >/dev/null
grep -F "aardvark-dns" "${output_dir}/amd64-neovex.requires.txt" >/dev/null
grep -F "neovex-crun" "${output_dir}/amd64-neovex.requires.txt" >/dev/null
grep -F "fuse-overlayfs" "${output_dir}/amd64-neovex.recommends.txt" >/dev/null
grep -F "passt" "${output_dir}/amd64-neovex.recommends.txt" >/dev/null
grep -F "shadow-utils" "${output_dir}/amd64-neovex.recommends.txt" >/dev/null
grep -F "libkrun" "${output_dir}/amd64-neovex-crun.requires.txt" >/dev/null
grep -F "libkrunfw" "${output_dir}/amd64-neovex-crun.requires.txt" >/dev/null
grep -F "/usr/bin/neovex" "${output_dir}/amd64-neovex.files.txt" >/dev/null
grep -F "/usr/libexec/neovex/crun" "${output_dir}/amd64-neovex-crun.files.txt" >/dev/null
grep -F "neovex 0.1.10-x86_64" "${output_dir}/amd64-neovex.command.txt" >/dev/null
grep -F "neovex-crun 0.1.4 x86_64" "${output_dir}/amd64-neovex-crun.command.txt" >/dev/null

grep -F "buildah" "${output_dir}/arm64-neovex.requires.txt" >/dev/null
grep -F "conmon" "${output_dir}/arm64-neovex.requires.txt" >/dev/null
grep -F "netavark" "${output_dir}/arm64-neovex.requires.txt" >/dev/null
grep -F "aardvark-dns" "${output_dir}/arm64-neovex.requires.txt" >/dev/null
grep -F "neovex-crun" "${output_dir}/arm64-neovex.requires.txt" >/dev/null
grep -F "fuse-overlayfs" "${output_dir}/arm64-neovex.recommends.txt" >/dev/null
grep -F "passt" "${output_dir}/arm64-neovex.recommends.txt" >/dev/null
grep -F "shadow-utils" "${output_dir}/arm64-neovex.recommends.txt" >/dev/null
grep -F "libkrun" "${output_dir}/arm64-neovex-crun.requires.txt" >/dev/null
grep -F "libkrunfw" "${output_dir}/arm64-neovex-crun.requires.txt" >/dev/null
grep -F "/usr/bin/neovex" "${output_dir}/arm64-neovex.files.txt" >/dev/null
grep -F "/usr/libexec/neovex/crun" "${output_dir}/arm64-neovex-crun.files.txt" >/dev/null
grep -F "neovex 0.1.10-aarch64" "${output_dir}/arm64-neovex.command.txt" >/dev/null
grep -F "neovex-crun 0.1.4 aarch64" "${output_dir}/arm64-neovex-crun.command.txt" >/dev/null

printf 'verified: Fedora/COPR SRPM builder produced reusable source RPMs and installable x86_64/aarch64 RPMs from release artifacts\n'
