#!/usr/bin/env bash
# Verifies the Nimbus-owned upstream fork inventory and remote contract.

set -euo pipefail

usage() {
  cat <<'EOF'
usage: verify-fork-upstream-standardization.sh [--offline]

Print a tab-separated inventory for the Nimbus-owned source forks and fail when
the current remotes or upstream release heads drift from the fork standard.

Options:
  --offline  Skip git ls-remote checks and only inspect local repositories.
EOF
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
offline=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --offline)
      offline=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'unknown argument: %s\n' "$1" >&2
      usage >&2
      exit 64
      ;;
  esac
done

forks_tsv=$(cat <<EOF
nimbus/deno	${HOME}/src/github.com/nimbus/deno	full_source	git@github.com:nimbus/deno.git	git@github.com:denoland/deno.git	v2.8.0	v2.8.0-nimbus.2	v[0-9]*	yes	v2.7.14
nimbus/rusty_v8	${HOME}/src/github.com/nimbus/rusty_v8	full_source	git@github.com:nimbus/rusty_v8.git	git@github.com:denoland/rusty_v8.git	v149.0.0	v149.0.0-nimbus.1	v[0-9]*	no	v147.4.0
nimbus/bun	${HOME}/src/github.com/nimbus/bun	full_source	git@github.com:nimbus/bun.git	git@github.com:oven-sh/bun.git	bun-v1.3.14	nimbus-bun-jsc-proof-main-20260525	bun-v[0-9]*	yes	f161e0311d56
nimbus/nimbus-crun	${HOME}/src/github.com/nimbus/nimbus-crun	patch_carrier	git@github.com:nimbus/nimbus-crun.git	git@github.com:containers/crun.git	1.27.1	v1.27.1-nimbus.2	[0-9]*	yes	1.27.1
nimbus/nimbus-libkrun	${HOME}/src/github.com/nimbus/nimbus-libkrun	full_source	git@github.com:nimbus/nimbus-libkrun.git	git@github.com:containers/libkrun.git	v1.18.1	v1.18.1-nimbus.1	v[0-9]*	yes	v1.18.1
EOF
)

issue_count=0

record_issue() {
  local fork="$1"
  local message="$2"
  printf 'fork-standardization: %s: %s\n' "${fork}" "${message}" >&2
  issue_count=$((issue_count + 1))
}

git_value() {
  local repo="$1"
  shift
  git -C "${repo}" "$@" 2>/dev/null || true
}

remote_url() {
  local repo="$1"
  local remote="$2"
  git_value "${repo}" remote get-url "${remote}"
}

remote_head() {
  local remote="$1"

  if [[ "${offline}" -eq 1 || -z "${remote}" ]]; then
    printf 'skipped'
    return 0
  fi

  git ls-remote --symref "${remote}" HEAD 2>/dev/null \
    | awk '/^ref:/ { sub("^refs/heads/", "", $2); print $2; exit }' \
    || true
}

remote_latest_tag() {
  local remote="$1"
  local pattern="$2"

  if [[ "${offline}" -eq 1 || -z "${remote}" ]]; then
    printf 'skipped'
    return 0
  fi

  git ls-remote --tags --refs "${remote}" "${pattern}" 2>/dev/null \
    | awk '{ sub("^refs/tags/", "", $2); print $2 }' \
    | sort -V \
    | tail -n 1 \
    || true
}

remote_tag_state() {
  local remote="$1"
  local tag="$2"

  if [[ "${offline}" -eq 1 || -z "${remote}" ]]; then
    printf 'skipped'
    return 0
  fi

  if git ls-remote --exit-code --tags --refs "${remote}" "${tag}" >/dev/null 2>&1; then
    printf 'present'
  else
    printf 'missing'
  fi
}

local_tag_state() {
  local repo="$1"
  local tag="$2"

  if git -C "${repo}" rev-parse -q --verify "refs/tags/${tag}^{commit}" >/dev/null 2>&1; then
    printf 'present'
  else
    printf 'missing'
  fi
}

local_release_tag_state() {
  local repo="$1"
  local tag="$2"

  if git -C "${repo}" rev-parse -q --verify "refs/tags/${tag}^{commit}" >/dev/null 2>&1; then
    printf 'present'
  else
    printf 'missing'
  fi
}

printf 'fork\tpath\tkind\tbranch\torigin_url\tupstream_url\torigin_head\tupstream_head\tselected_source_tag\tselected_release_tag\tlocal_source_tag\tremote_source_tag\tlocal_release_tag\tlatest_upstream_tag\ttracks_latest\tclean_state\tdelta_base\n'

while IFS=$'\t' read -r fork path kind expected_origin expected_upstream selected_source_tag selected_release_tag tag_pattern tracks_latest delta_base; do
  if [[ ! -d "${path}" ]] || ! git -C "${path}" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    record_issue "${fork}" "missing git checkout at ${path}"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
      "${fork}" "${path}" "${kind}" "<missing>" "<missing>" "<missing>" "missing" "missing" \
      "${selected_source_tag}" "${selected_release_tag}" "missing" "missing" "missing" "missing" \
      "${tracks_latest}" "missing" "${delta_base}"
    continue
  fi

  branch="$(git_value "${path}" branch --show-current)"
  if [[ -z "${branch}" ]]; then
    branch="$(git_value "${path}" rev-parse --short HEAD)"
  fi

  origin="$(remote_url "${path}" origin)"
  upstream="$(remote_url "${path}" upstream)"
  origin_head="$(remote_head "${origin}")"
  upstream_head="$(remote_head "${upstream}")"
  local_source_tag="$(local_tag_state "${path}" "${selected_source_tag}")"
  remote_source_tag="$(remote_tag_state "${expected_upstream}" "${selected_source_tag}")"
  local_release_tag="$(local_release_tag_state "${path}" "${selected_release_tag}")"
  latest_tag="$(remote_latest_tag "${expected_upstream}" "${tag_pattern}")"
  dirty_count="$(git -C "${path}" status --short | wc -l | tr -d ' ')"
  clean_state="clean"
  if [[ "${dirty_count}" != "0" ]]; then
    clean_state="dirty:${dirty_count}"
  fi

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "${fork}" "${path}" "${kind}" "${branch}" "${origin:-<missing>}" "${upstream:-<missing>}" \
    "${origin_head:-unavailable}" "${upstream_head:-unavailable}" "${selected_source_tag}" \
    "${selected_release_tag}" "${local_source_tag}" "${remote_source_tag}" "${local_release_tag}" \
    "${latest_tag:-unavailable}" "${tracks_latest}" "${clean_state}" "${delta_base}"

  if [[ "${origin}" != "${expected_origin}" ]]; then
    record_issue "${fork}" "origin mismatch expected=${expected_origin} actual=${origin:-<missing>}"
  fi

  if [[ "${upstream}" != "${expected_upstream}" ]]; then
    record_issue "${fork}" "upstream mismatch expected=${expected_upstream} actual=${upstream:-<missing>}"
  fi

  if [[ "${remote_source_tag}" == "missing" ]]; then
    record_issue "${fork}" "selected source tag missing upstream: ${selected_source_tag}"
  fi

  if [[ "${kind}" == "full_source" && "${local_source_tag}" == "missing" ]]; then
    record_issue "${fork}" "selected source tag missing locally: ${selected_source_tag}"
  fi

  if [[ "${tracks_latest}" == "yes" && "${latest_tag}" != "skipped" && "${latest_tag}" != "unavailable" && "${latest_tag}" != "${selected_source_tag}" ]]; then
    record_issue "${fork}" "newer upstream tag detected expected=${selected_source_tag} latest=${latest_tag}"
  fi
done <<<"${forks_tsv}"

if [[ "${issue_count}" -ne 0 ]]; then
  printf 'fork-standardization: %s issue(s) detected\n' "${issue_count}" >&2
  exit 1
fi

printf 'fork-standardization: pass\n' >&2
