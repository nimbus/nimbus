#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-apt-repo-helper.XXXXXX")"
trap 'rm -rf "${output_dir}"' EXIT

packages_dir="${output_dir}/packages"
repo_dir="${output_dir}/repo"
signing_dir="${output_dir}/signing"
mkdir -p "${packages_dir}" "${repo_dir}" "${signing_dir}"

create_stub_deb() {
  local package_name="$1"
  local version="$2"
  local arch="$3"
  local destination_path="$4"
  shift 4
  local extra_control_lines=("$@")

  local package_root="${output_dir}/pkg-${package_name}-${arch}"
  local control_root="${package_root}/control"
  local data_root="${package_root}/data"
  local archive_name="${packages_dir}/${package_name}_${version}_${arch}.deb"

  rm -rf "${package_root}"
  mkdir -p "${control_root}" "$(dirname "${data_root}${destination_path}")"

  {
    printf 'Package: %s\n' "${package_name}"
    printf 'Version: %s\n' "${version}"
    printf 'Architecture: %s\n' "${arch}"
    printf 'Maintainer: Nimbus <oss@nimbus.dev>\n'
    printf 'Section: devel\n'
    printf 'Priority: optional\n'
    local control_line
    for control_line in "${extra_control_lines[@]}"; do
      printf '%s\n' "${control_line}"
    done
    printf 'Description: Stub package for apt repository verification\n'
  } >"${control_root}/control"

  printf 'stub %s %s\n' "${package_name}" "${arch}" >"${data_root}${destination_path}"

  (
    cd "${package_root}"
    printf '2.0\n' >debian-binary
    COPYFILE_DISABLE=1 tar --format ustar -czf control.tar.gz -C control .
    COPYFILE_DISABLE=1 tar --format ustar -czf data.tar.gz -C data .
    ar cr "${archive_name}" debian-binary control.tar.gz data.tar.gz
  )
}

create_stub_deb "nimbus" "0.1.10" "amd64" "/usr/bin/nimbus" \
  "Depends: nimbus-crun, buildah, conmon, netavark, aardvark-dns" \
  "Recommends: fuse-overlayfs, uidmap"
create_stub_deb "nimbus-crun" "0.1.4" "amd64" "/usr/libexec/nimbus/crun" \
  "Depends: libkrun, libkrunfw"
create_stub_deb "nimbus" "0.1.10" "arm64" "/usr/bin/nimbus" \
  "Depends: nimbus-crun, buildah, conmon, netavark, aardvark-dns" \
  "Recommends: fuse-overlayfs, uidmap"
create_stub_deb "nimbus-crun" "0.1.4" "arm64" "/usr/libexec/nimbus/crun" \
  "Depends: libkrun, libkrunfw"

cd "${repo_root}"

if command -v apt-ftparchive >/dev/null 2>&1; then
  signing_home="${signing_dir}/gnupg"
  mkdir -p "${signing_home}"
  chmod 0700 "${signing_home}"
  GNUPGHOME="${signing_home}" gpgconf --launch gpg-agent >/dev/null 2>&1
  GNUPGHOME="${signing_home}" gpg --batch --pinentry-mode loopback --passphrase '' \
    --quick-gen-key "Nimbus Apt Repo <apt@nimbus.dev>" ed25519 sign 0 >/dev/null 2>&1
  signing_fingerprint="$(
    GNUPGHOME="${signing_home}" gpg --batch --with-colons --list-secret-keys \
      | awk -F: '$1=="fpr"{print $10; exit}'
  )"
  GNUPGHOME="${signing_home}" gpg --batch --pinentry-mode loopback --passphrase '' \
    --armor --export-secret-keys "${signing_fingerprint}" >"${signing_dir}/private.asc"

  bash scripts/build-apt-repository.sh \
    --output-dir "${repo_dir}" \
    --packages-dir "${packages_dir}" \
    --distribution stable \
    --component main \
    --origin Nimbus \
    --label nimbus \
    --description "Nimbus apt repository" \
    --gpg-private-key "${signing_dir}/private.asc" \
    >"${output_dir}/build-summary.txt"
  verification_mode="local"
elif command -v docker >/dev/null 2>&1; then
  docker run --rm \
    -v "${repo_root}:/repo" \
    -v "${output_dir}:/work" \
    -w /repo \
    ubuntu:24.04 \
    bash -lc '
      set -euo pipefail
      export DEBIAN_FRONTEND=noninteractive
      apt-get update >/dev/null
      apt-get install -y apt-utils gnupg >/dev/null
      export GNUPGHOME=/work/signing/gnupg
      mkdir -p "$GNUPGHOME"
      chmod 0700 "$GNUPGHOME"
      gpgconf --launch gpg-agent >/dev/null 2>&1 || true
      gpg --batch --pinentry-mode loopback --passphrase "" \
        --quick-gen-key "Nimbus Apt Repo <apt@nimbus.dev>" ed25519 sign 0 >/dev/null 2>&1
      signing_fingerprint="$(
        gpg --batch --with-colons --list-secret-keys \
          | awk -F: '\''$1=="fpr"{print $10; exit}'\''
      )"
      bash scripts/build-apt-repository.sh \
        --output-dir /work/repo \
        --packages-dir /work/packages \
        --distribution stable \
        --component main \
        --origin Nimbus \
        --label nimbus \
        --description "Nimbus apt repository" \
        --gpg-key-id "$signing_fingerprint"
      verify_home=/work/signature-verify
      mkdir -p "$verify_home"
      chmod 0700 "$verify_home"
      GNUPGHOME="$verify_home" gpg --batch --import /work/repo/public/nimbus.asc >/dev/null 2>&1
      GNUPGHOME="$verify_home" gpg --batch --verify /work/repo/dists/stable/InRelease >/dev/null 2>&1
      GNUPGHOME="$verify_home" gpg --batch --verify \
        /work/repo/dists/stable/Release.gpg \
        /work/repo/dists/stable/Release >/dev/null 2>&1
      printf "verified=signatures\n" >/work/docker-signature-summary.txt
    ' >"${output_dir}/build-summary.txt"
  verification_mode="docker"
else
  echo "skipped: neither apt-ftparchive nor docker is available for apt repo verification" >&2
  exit 0
fi

test -f "${repo_dir}/dists/stable/main/binary-amd64/Packages"
test -f "${repo_dir}/dists/stable/main/binary-amd64/Packages.gz"
test -f "${repo_dir}/dists/stable/main/binary-arm64/Packages"
test -f "${repo_dir}/dists/stable/main/binary-arm64/Packages.gz"
test -f "${repo_dir}/dists/stable/Release"
test -f "${repo_dir}/dists/stable/Release.gpg"
test -f "${repo_dir}/dists/stable/InRelease"
test -f "${repo_dir}/public/nimbus.asc"
test -f "${repo_dir}/public/nimbus.gpg"

grep -F "Package: nimbus" "${repo_dir}/dists/stable/main/binary-amd64/Packages" >/dev/null
grep -F "Package: nimbus-crun" "${repo_dir}/dists/stable/main/binary-amd64/Packages" >/dev/null
grep -F "Filename: pool/main/amd64/nimbus_0.1.10_amd64.deb" "${repo_dir}/dists/stable/main/binary-amd64/Packages" >/dev/null
grep -F "Filename: pool/main/arm64/nimbus_0.1.10_arm64.deb" "${repo_dir}/dists/stable/main/binary-arm64/Packages" >/dev/null
grep -F "Architectures: amd64 arm64" "${repo_dir}/dists/stable/Release" >/dev/null
grep -F "Components: main" "${repo_dir}/dists/stable/Release" >/dev/null
grep -F "Codename: stable" "${repo_dir}/dists/stable/Release" >/dev/null
grep -F "Suite: stable" "${repo_dir}/dists/stable/Release" >/dev/null
grep -F "result=apt-repo-built" "${output_dir}/build-summary.txt" >/dev/null
grep -F "repo.keyring=" "${output_dir}/build-summary.txt" >/dev/null
grep -F "repo.keyring_ascii=" "${output_dir}/build-summary.txt" >/dev/null

if [[ "${verification_mode}" == "docker" ]]; then
  grep -F "verified=signatures" "${output_dir}/docker-signature-summary.txt" >/dev/null
else
  verify_home="${output_dir}/verify-gnupg"
  mkdir -p "${verify_home}"
  chmod 0700 "${verify_home}"
  GNUPGHOME="${verify_home}" gpg --batch --import "${repo_dir}/public/nimbus.asc" >/dev/null 2>&1
  GNUPGHOME="${verify_home}" gpg --batch --verify "${repo_dir}/dists/stable/InRelease" >/dev/null 2>&1
  GNUPGHOME="${verify_home}" gpg --batch --verify \
    "${repo_dir}/dists/stable/Release.gpg" \
    "${repo_dir}/dists/stable/Release" >/dev/null 2>&1
fi

printf 'verified: apt repository builder produced signed metadata via %s\n' "${verification_mode}"
