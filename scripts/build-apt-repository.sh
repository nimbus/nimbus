#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: build-apt-repository.sh --output-dir <path> --packages-dir <path> [options]

Build a static Debian/Ubuntu apt repository tree from prebuilt `.deb` packages.

Required:
  --output-dir <path>            Output root for the repository bundle
  --packages-dir <path>          Directory containing `.deb` packages

Optional:
  --distribution <name>          Apt distribution/codename (default: stable)
  --suite <name>                 Apt suite name (default: same as distribution)
  --component <name>             Apt component name (default: main)
  --origin <name>                Release metadata Origin field (default: Nimbus)
  --label <name>                 Release metadata Label field (default: nimbus)
  --description <text>           Release metadata Description field
  --arch <amd64|arm64>           Architectures to include; repeatable (default: infer from packages)
  --apt-ftparchive <path>        Explicit apt-ftparchive binary path
  --gpg <path>                   Explicit gpg binary path
  --gpg-private-key <path>       ASCII-armored private signing key to import
  --gpg-key-id <id>              Existing GPG key id/fingerprint to sign with
  --gpg-passphrase-file <path>   File containing the signing-key passphrase
  --keyring-name <name>          Output keyring basename (default: nimbus)
  -h, --help                     Show this help text

Examples:
  bash scripts/build-apt-repository.sh \
    --output-dir /tmp/nimbus-apt-repo \
    --packages-dir /tmp/nimbus-linux-packages/packages \
    --distribution stable \
    --component main

  bash scripts/build-apt-repository.sh \
    --output-dir /tmp/nimbus-apt-repo \
    --packages-dir /tmp/nimbus-linux-packages/packages \
    --distribution stable \
    --gpg-private-key /tmp/nimbus-apt-private-key.asc \
    --gpg-passphrase-file /tmp/nimbus-apt-passphrase.txt
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

package_arch_from_name() {
  local package_name="$1"
  local stem="${package_name%.deb}"
  normalize_arch "${stem##*_}"
}

append_unique() {
  local value="$1"
  shift
  local existing
  for existing in "$@"; do
    if [[ "$existing" == "$value" ]]; then
      return 0
    fi
  done
  printf '%s\n' "$value"
}

gpg_command() {
  local gpg_bin="$1"
  shift

  if [[ -n "${GPG_PASSPHRASE_FILE:-}" ]]; then
    "$gpg_bin" --batch --yes --pinentry-mode loopback --passphrase-file "$GPG_PASSPHRASE_FILE" "$@"
  else
    "$gpg_bin" --batch --yes --pinentry-mode loopback "$@"
  fi
}

output_dir=""
packages_dir=""
distribution="stable"
suite=""
component="main"
origin="Nimbus"
label="nimbus"
description="Nimbus apt repository"
apt_ftparchive_bin="${APT_FTPARCHIVE_BIN:-apt-ftparchive}"
gpg_bin="${GPG_BIN:-gpg}"
gpg_private_key=""
gpg_key_id=""
GPG_PASSPHRASE_FILE=""
keyring_name="nimbus"
declare -a requested_arches=()

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --output-dir)
      output_dir="${2:-}"
      shift 2
      ;;
    --packages-dir)
      packages_dir="${2:-}"
      shift 2
      ;;
    --distribution)
      distribution="${2:-}"
      shift 2
      ;;
    --suite)
      suite="${2:-}"
      shift 2
      ;;
    --component)
      component="${2:-}"
      shift 2
      ;;
    --origin)
      origin="${2:-}"
      shift 2
      ;;
    --label)
      label="${2:-}"
      shift 2
      ;;
    --description)
      description="${2:-}"
      shift 2
      ;;
    --arch)
      requested_arches+=("$(normalize_arch "${2:-}")")
      shift 2
      ;;
    --apt-ftparchive)
      apt_ftparchive_bin="${2:-}"
      shift 2
      ;;
    --gpg)
      gpg_bin="${2:-}"
      shift 2
      ;;
    --gpg-private-key)
      gpg_private_key="${2:-}"
      shift 2
      ;;
    --gpg-key-id)
      gpg_key_id="${2:-}"
      shift 2
      ;;
    --gpg-passphrase-file)
      GPG_PASSPHRASE_FILE="${2:-}"
      shift 2
      ;;
    --keyring-name)
      keyring_name="${2:-}"
      shift 2
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
[[ -n "$packages_dir" ]] || die "--packages-dir is required"

if [[ -z "$suite" ]]; then
  suite="$distribution"
fi

[[ -d "$packages_dir" ]] || die "packages dir not found: $packages_dir"
if ! command -v "$apt_ftparchive_bin" >/dev/null 2>&1; then
  die "apt-ftparchive not found: ${apt_ftparchive_bin}"
fi

if [[ -n "$gpg_private_key" || -n "$gpg_key_id" ]]; then
  command -v "$gpg_bin" >/dev/null 2>&1 || die "gpg not found: ${gpg_bin}"
fi

if [[ -n "$gpg_private_key" ]]; then
  [[ -f "$gpg_private_key" ]] || die "gpg private key not found: $gpg_private_key"
fi

if [[ -n "$GPG_PASSPHRASE_FILE" ]]; then
  [[ -f "$GPG_PASSPHRASE_FILE" ]] || die "gpg passphrase file not found: $GPG_PASSPHRASE_FILE"
fi

packages_dir="$(cd "$packages_dir" && pwd)"
mkdir -p "$output_dir"
output_dir="$(cd "$output_dir" && pwd)"

shopt -s nullglob
package_files=( "$packages_dir"/*.deb )
if [[ "${#package_files[@]}" -eq 0 ]]; then
  die "no .deb packages found under ${packages_dir}"
fi

declare -a inferred_arches=()
package_path=""
for package_path in "${package_files[@]}"; do
  package_name="$(basename "$package_path")"
  package_arch="$(package_arch_from_name "$package_name")"
  if [[ "${#requested_arches[@]}" -gt 0 ]]; then
    include_arch=0
    requested_arch=""
    for requested_arch in "${requested_arches[@]}"; do
      if [[ "$requested_arch" == "$package_arch" ]]; then
        include_arch=1
        break
      fi
    done
    if [[ "$include_arch" -eq 0 ]]; then
      continue
    fi
  fi
  maybe_arch="$(append_unique "$package_arch" "${inferred_arches[@]}")"
  if [[ -n "$maybe_arch" ]]; then
    inferred_arches+=("$maybe_arch")
  fi
done

if [[ "${#inferred_arches[@]}" -eq 0 ]]; then
  die "no matching .deb packages found for the requested architectures"
fi

pool_root="${output_dir}/pool/${component}"
dist_root="${output_dir}/dists/${distribution}"
public_root="${output_dir}/public"
rm -rf "$pool_root" "$dist_root" "$public_root"
mkdir -p "$pool_root" "$dist_root" "$public_root"

for package_arch in "${inferred_arches[@]}"; do
  mkdir -p "${pool_root}/${package_arch}" "${dist_root}/${component}/binary-${package_arch}"
done

for package_path in "${package_files[@]}"; do
  package_name="$(basename "$package_path")"
  package_arch="$(package_arch_from_name "$package_name")"
  include_arch=0
  for requested_arch in "${inferred_arches[@]}"; do
    if [[ "$requested_arch" == "$package_arch" ]]; then
      include_arch=1
      break
    fi
  done
  if [[ "$include_arch" -eq 1 ]]; then
    cp "$package_path" "${pool_root}/${package_arch}/${package_name}"
  fi
done

repo_signing_home=""
cleanup() {
  if [[ -n "$repo_signing_home" && -d "$repo_signing_home" ]]; then
    rm -rf "$repo_signing_home"
  fi
}
trap cleanup EXIT

if [[ -n "$gpg_private_key" ]]; then
  repo_signing_home="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-apt-signing.XXXXXX")"
  chmod 0700 "$repo_signing_home"
  GNUPGHOME="$repo_signing_home" gpg_command "$gpg_bin" --import "$gpg_private_key" >/dev/null 2>&1
  if [[ -z "$gpg_key_id" ]]; then
    gpg_key_id="$(
      GNUPGHOME="$repo_signing_home" "$gpg_bin" --batch --with-colons --list-secret-keys \
        | awk -F: '$1=="fpr"{print $10; exit}'
    )"
  fi
fi

if [[ -n "$gpg_key_id" && -z "$repo_signing_home" ]]; then
  repo_signing_home="${GNUPGHOME:-$HOME/.gnupg}"
fi

(
  cd "$output_dir"

  for package_arch in "${inferred_arches[@]}"; do
    packages_index="dists/${distribution}/${component}/binary-${package_arch}/Packages"
    "$apt_ftparchive_bin" packages "pool/${component}/${package_arch}" >"$packages_index"
    gzip -n -9 -c "$packages_index" >"${packages_index}.gz"
  done

  architectures_line="$(printf '%s ' "${inferred_arches[@]}")"
  architectures_line="${architectures_line% }"
  release_path="dists/${distribution}/Release"

  "$apt_ftparchive_bin" \
    -o "APT::FTPArchive::Release::Origin=${origin}" \
    -o "APT::FTPArchive::Release::Label=${label}" \
    -o "APT::FTPArchive::Release::Suite=${suite}" \
    -o "APT::FTPArchive::Release::Codename=${distribution}" \
    -o "APT::FTPArchive::Release::Architectures=${architectures_line}" \
    -o "APT::FTPArchive::Release::Components=${component}" \
    -o "APT::FTPArchive::Release::Description=${description}" \
    release "dists/${distribution}" >"$release_path"

  if [[ -n "$gpg_key_id" ]]; then
    GNUPGHOME="$repo_signing_home" gpg_command "$gpg_bin" --armor --export "$gpg_key_id" >"public/${keyring_name}.asc"
    GNUPGHOME="$repo_signing_home" gpg_command "$gpg_bin" --export "$gpg_key_id" >"public/${keyring_name}.gpg"
    GNUPGHOME="$repo_signing_home" gpg_command "$gpg_bin" --local-user "$gpg_key_id" --detach-sign --output "dists/${distribution}/Release.gpg" "$release_path"
    GNUPGHOME="$repo_signing_home" gpg_command "$gpg_bin" --local-user "$gpg_key_id" --clearsign --output "dists/${distribution}/InRelease" "$release_path"
  fi
)

printf 'repo.root=%s\n' "$output_dir"
printf 'repo.distribution=%s\n' "$distribution"
printf 'repo.component=%s\n' "$component"
printf 'repo.architectures=%s\n' "$(printf '%s,' "${inferred_arches[@]}" | sed 's/,$//')"
printf 'repo.release=%s\n' "${dist_root}/Release"
if [[ -n "$gpg_key_id" ]]; then
  printf 'repo.inrelease=%s\n' "${dist_root}/InRelease"
  printf 'repo.release_gpg=%s\n' "${dist_root}/Release.gpg"
  printf 'repo.keyring=%s\n' "${public_root}/${keyring_name}.gpg"
  printf 'repo.keyring_ascii=%s\n' "${public_root}/${keyring_name}.asc"
fi
printf 'result=apt-repo-built\n'
