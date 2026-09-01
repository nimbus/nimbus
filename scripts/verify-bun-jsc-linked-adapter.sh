#!/usr/bin/env bash
# Verifies the opt-in Bun/JSC linked-adapter build lane. This gate keeps the
# default Nimbus build fail-closed while checking the exact Bun proof source
# and source-owned shared adapter that the BJA4 execution adapter loads.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "${REPO_ROOT}/scripts/bun-jsc-adapter-contract.sh"
BUN_REPO="${NIMBUS_BUN_REPO:-${HOME}/src/github.com/nimbus/bun}"
EXPECTED_BUN_REF="${NIMBUS_BUN_EXPECTED_REF:-${BUN_JSC_ADAPTER_SOURCE_REF}}"
EXPECTED_BUN_REV="${NIMBUS_BUN_EXPECTED_REV:-${BUN_JSC_ADAPTER_SOURCE_REVISION}}"
BUN_SIMDUTF_NAMESPACE="${NIMBUS_BUN_SIMDUTF_NAMESPACE:-${BUN_JSC_ADAPTER_SIMDUTF_NAMESPACE}}"
BUN_EXECUTABLE="${NIMBUS_BUN_EXECUTABLE:-bun}"

host_triple="$(rustc -vV | awk '/^host:/ { print $2 }')"
case "${host_triple}" in
  x86_64-unknown-linux-gnu)
    DEFAULT_BUN_PROFILE=release-local
    DEFAULT_ENABLE_SIMDUTF_NAMESPACE=1
    DEFAULT_REQUIRE_SYMBOL_AUDIT=1
    SHARED_LIBRARY_BASENAME=libnimbus_bun_jsc_embedder.so
    DEFAULT_SHARED_ARTIFACT_AUDIT=1
    ;;
  *-apple-darwin)
    DEFAULT_BUN_PROFILE=release-local
    DEFAULT_ENABLE_SIMDUTF_NAMESPACE=1
    DEFAULT_REQUIRE_SYMBOL_AUDIT=0
    SHARED_LIBRARY_BASENAME=libnimbus_bun_jsc_embedder.dylib
    DEFAULT_SHARED_ARTIFACT_AUDIT=1
    ;;
  *)
    DEFAULT_BUN_PROFILE=release
    DEFAULT_ENABLE_SIMDUTF_NAMESPACE=0
    DEFAULT_REQUIRE_SYMBOL_AUDIT=0
    SHARED_LIBRARY_BASENAME=libnimbus_bun_jsc_embedder.so
    DEFAULT_SHARED_ARTIFACT_AUDIT=0
    ;;
esac

BUN_PROFILE="${NIMBUS_BUN_PROFILE:-${DEFAULT_BUN_PROFILE}}"
BUN_ENABLE_SIMDUTF_NAMESPACE="${NIMBUS_BUN_ENABLE_SIMDUTF_NAMESPACE:-${DEFAULT_ENABLE_SIMDUTF_NAMESPACE}}"
BUN_REQUIRE_SYMBOL_AUDIT="${NIMBUS_BUN_REQUIRE_SYMBOL_AUDIT:-${DEFAULT_REQUIRE_SYMBOL_AUDIT}}"
BUN_REQUIRE_SHARED_ARTIFACT_AUDIT="${NIMBUS_BUN_REQUIRE_SHARED_ARTIFACT_AUDIT:-${DEFAULT_SHARED_ARTIFACT_AUDIT}}"

is_enabled() {
  case "${1}" in
    1 | true | TRUE | yes | YES | on | ON) return 0 ;;
    *) return 1 ;;
  esac
}

reject_unsafe_linker_policy() {
  local flags="${RUSTFLAGS:-} ${CARGO_ENCODED_RUSTFLAGS:-}"
  case "${flags}" in
    *--allow-multiple-definition* | *"-z muldefs"* | *"-z,muldefs"*)
      printf 'unsafe linker policy detected in RUSTFLAGS/CARGO_ENCODED_RUSTFLAGS\n' >&2
      printf 'BJA4L forbids --allow-multiple-definition and -z muldefs because the Linux proof linked with that policy and then crashed with SIGSEGV.\n' >&2
      exit 1
      ;;
  esac
}

reject_unsafe_linker_manifest() {
  local manifest="${1}"
  if grep -E -- '--allow-multiple-definition|muldefs' "${manifest}" >/dev/null; then
    printf 'unsafe linker policy detected in Bun link manifest: %s\n' "${manifest}" >&2
    printf 'BJA4L forbids --allow-multiple-definition and muldefs because the Linux proof linked with that policy and then crashed with SIGSEGV.\n' >&2
    exit 1
  fi
}

reject_unsafe_generated_build_graph() {
  local build_graph="${1}"
  if [[ ! -f "${build_graph}" ]]; then
    printf 'missing Bun generated build graph: %s\n' "${build_graph}" >&2
    exit 1
  fi
  if grep -E -- '--allow-multiple-definition|muldefs' "${build_graph}" >/dev/null; then
    printf 'unsafe linker policy detected in Bun generated build graph: %s\n' "${build_graph}" >&2
    printf 'BJA4L forbids --allow-multiple-definition and muldefs because the Linux proof linked with that policy and then crashed with SIGSEGV.\n' >&2
    exit 1
  fi
  if grep -E -- '-ftls-model=(initial-exec|local-exec)' "${build_graph}" >/dev/null; then
    printf 'unsafe static TLS model detected in Bun generated build graph: %s\n' "${build_graph}" >&2
    printf 'BJA4L shared adapters must be dlopen-safe after V8/Deno startup; use local-dynamic/global-dynamic TLS instead.\n' >&2
    exit 1
  fi
}

count_demangled_symbols() {
  local artifact="${1}"
  local needle="${2}"
  nm -g --defined-only -C "${artifact}" 2>/dev/null |
    awk -v needle="${needle}" 'index($0, needle) { count++ } END { print count + 0 }'
}

count_raw_symbols() {
  local artifact="${1}"
  local pattern="${2}"
  nm -g --defined-only "${artifact}" 2>/dev/null |
    awk -v pattern="${pattern}" '$0 ~ pattern { count++ } END { print count + 0 }'
}

audit_simdutf_symbols() {
  if ! is_enabled "${BUN_REQUIRE_SYMBOL_AUDIT}"; then
    printf 'symbol audit skipped for host %s; set NIMBUS_BUN_REQUIRE_SYMBOL_AUDIT=1 to require it\n' "${host_triple}"
    return
  fi
  if ! is_enabled "${BUN_ENABLE_SIMDUTF_NAMESPACE}"; then
    printf 'symbol audit requires NIMBUS_BUN_ENABLE_SIMDUTF_NAMESPACE=1\n' >&2
    exit 1
  fi

  local wtf="${BUN_BUILD_DIR}/deps/WebKit/lib/libWTF.a"
  local jsc="${BUN_BUILD_DIR}/deps/WebKit/lib/libJavaScriptCore.a"
  local wrapper="${BUN_BUILD_DIR}/obj/src/simdutf_sys/bun-simdutf.cpp.o"
  for artifact in "${wtf}" "${jsc}" "${wrapper}"; do
    if [[ ! -e "${artifact}" ]]; then
      printf 'missing required symbol-audit artifact: %s\n' "${artifact}" >&2
      exit 1
    fi
  done

  local wtf_nimbus_cpp wtf_plain_cpp jsc_nimbus_cpp jsc_plain_cpp
  local wrapper_prefixed_c wrapper_plain_c
  wtf_nimbus_cpp="$(count_demangled_symbols "${wtf}" "${BUN_SIMDUTF_NAMESPACE}::")"
  wtf_plain_cpp="$(count_demangled_symbols "${wtf}" " simdutf::")"
  jsc_nimbus_cpp="$(count_demangled_symbols "${jsc}" "${BUN_SIMDUTF_NAMESPACE}::")"
  jsc_plain_cpp="$(count_demangled_symbols "${jsc}" " simdutf::")"
  wrapper_prefixed_c="$(count_raw_symbols "${wrapper}" "(^| )${BUN_SIMDUTF_NAMESPACE}__")"
  wrapper_plain_c="$(count_raw_symbols "${wrapper}" "(^| )simdutf__")"

  printf '  libWTF.a %s:: definitions: %s\n' "${BUN_SIMDUTF_NAMESPACE}" "${wtf_nimbus_cpp}"
  printf '  libWTF.a plain simdutf:: definitions: %s\n' "${wtf_plain_cpp}"
  printf '  libJavaScriptCore.a %s:: definitions: %s\n' "${BUN_SIMDUTF_NAMESPACE}" "${jsc_nimbus_cpp}"
  printf '  libJavaScriptCore.a plain simdutf:: definitions: %s\n' "${jsc_plain_cpp}"
  printf '  bun-simdutf.cpp.o %s__ definitions: %s\n' "${BUN_SIMDUTF_NAMESPACE}" "${wrapper_prefixed_c}"
  printf '  bun-simdutf.cpp.o plain simdutf__ definitions: %s\n' "${wrapper_plain_c}"

  if [[ "${wtf_nimbus_cpp}" -le 0 || "${wtf_plain_cpp}" -ne 0 ]]; then
    printf 'WebKit/WTF simdutf namespace audit failed\n' >&2
    exit 1
  fi
  if [[ "${jsc_plain_cpp}" -ne 0 ]]; then
    printf 'JavaScriptCore unexpectedly defines plain simdutf:: symbols\n' >&2
    exit 1
  fi
  if [[ "${wrapper_prefixed_c}" -le 0 || "${wrapper_plain_c}" -ne 0 ]]; then
    printf 'Bun simdutf C wrapper namespace audit failed\n' >&2
    exit 1
  fi

  local cargo_target_dir="${CARGO_TARGET_DIR:-target}"
  mapfile -t v8_artifacts < <(
    find "${cargo_target_dir}/debug/gn_out/obj" "${cargo_target_dir}/debug/deps" \
      \( -name 'librusty_v8.a' -o -name 'libv8-*.rlib' \) 2>/dev/null | sort
  )
  if [[ "${#v8_artifacts[@]}" -eq 0 ]]; then
    printf 'no local V8 artifacts found for namespace audit\n' >&2
    exit 1
  fi
  for artifact in "${v8_artifacts[@]}"; do
    local v8_plain_cpp v8_nimbus_cpp v8_plain_c v8_nimbus_c
    v8_plain_cpp="$(count_demangled_symbols "${artifact}" " simdutf::")"
    v8_nimbus_cpp="$(count_demangled_symbols "${artifact}" "${BUN_SIMDUTF_NAMESPACE}::")"
    v8_plain_c="$(count_raw_symbols "${artifact}" "(^| )simdutf__")"
    v8_nimbus_c="$(count_raw_symbols "${artifact}" "(^| )${BUN_SIMDUTF_NAMESPACE}__")"
    printf '  %s plain simdutf::=%s plain simdutf__=%s %s::=%s %s__=%s\n' \
      "${artifact}" \
      "${v8_plain_cpp}" \
      "${v8_plain_c}" \
      "${BUN_SIMDUTF_NAMESPACE}" \
      "${v8_nimbus_cpp}" \
      "${BUN_SIMDUTF_NAMESPACE}" \
      "${v8_nimbus_c}"
    if [[ "${v8_nimbus_cpp}" -ne 0 || "${v8_nimbus_c}" -ne 0 ]]; then
      printf 'V8/rusty_v8 unexpectedly owns Bun private simdutf namespace symbols\n' >&2
      exit 1
    fi
  done
}

list_shared_adapter_exports() {
  local shared_library="${1}"
  case "${host_triple}" in
    *-apple-darwin)
      nm -gU "${shared_library}" 2>/dev/null |
        awk '{ print $3 }' |
        sed -E 's/^_//; s/@.*$//' |
        sort -u
      ;;
    *)
      nm -D --defined-only -C "${shared_library}" 2>/dev/null |
        awk '{ print $3 }' |
        sed -E 's/@@.*$//; s/@.*$//' |
        sort -u
      ;;
  esac
}

audit_shared_adapter_exports() {
  local shared_library="${1}"
  if ! is_enabled "${BUN_REQUIRE_SHARED_ARTIFACT_AUDIT}"; then
    printf 'shared artifact audit skipped for host %s; set NIMBUS_BUN_REQUIRE_SHARED_ARTIFACT_AUDIT=1 to require it\n' "${host_triple}"
    return
  fi
  if [[ ! -f "${shared_library}" ]]; then
    printf 'missing Bun/JSC shared adapter artifact: %s\n' "${shared_library}" >&2
    exit 1
  fi

  local expected_file actual_file leaked_count
  local leak_pattern='v8::|hwy::|rust_eh_personality|simdutf::|simdutf__|nimbus_bun_simdutf::|nimbus_bun_simdutf__'
  expected_file="$(mktemp)"
  actual_file="$(mktemp)"
  trap 'rm -f "${expected_file}" "${actual_file}"' RETURN
  printf '%s\n' "${REQUIRED_EXPORTS[@]}" | sort -u >"${expected_file}"
  list_shared_adapter_exports "${shared_library}" >"${actual_file}"

  printf '  shared adapter artifact: %s\n' "${shared_library}"
  printf '  defined dynamic exports:\n'
  sed 's/^/    /' "${actual_file}"

  if ! diff -u "${expected_file}" "${actual_file}"; then
    printf 'Bun/JSC shared adapter export set drifted\n' >&2
    exit 1
  fi

  case "${host_triple}" in
    *-apple-darwin)
      leaked_count="$(nm -gU -C "${shared_library}" 2>/dev/null |
        awk -v pattern="${leak_pattern}" '$0 ~ pattern { count++ } END { print count + 0 }')"
      ;;
    *)
      leaked_count="$(nm -D --defined-only -C "${shared_library}" 2>/dev/null |
        awk -v pattern="${leak_pattern}" '$0 ~ pattern { count++ } END { print count + 0 }')"
      if command -v readelf >/dev/null 2>&1 &&
        readelf -d "${shared_library}" 2>/dev/null | grep -q TEXTREL; then
        printf 'Bun/JSC shared adapter has TEXTREL dynamic entries\n' >&2
        exit 1
      fi
      if command -v readelf >/dev/null 2>&1 &&
        readelf -d "${shared_library}" 2>/dev/null | grep -q STATIC_TLS; then
        printf 'Bun/JSC shared adapter has STATIC_TLS and is not safe for late dlopen\n' >&2
        exit 1
      fi
      ;;
  esac

  printf '  leaked native defined symbols: %s\n' "${leaked_count}"
  if [[ "${leaked_count}" -ne 0 ]]; then
    printf 'Bun/JSC shared adapter exported bundled native implementation symbols\n' >&2
    exit 1
  fi
}

if is_enabled "${BUN_ENABLE_SIMDUTF_NAMESPACE}" && [[ -z "${BUN_WEBKIT_PATH:-}" ]]; then
  if [[ -d "${HOME}/src/github.com/oven-sh/WebKit" ]]; then
    export BUN_WEBKIT_PATH="${HOME}/src/github.com/oven-sh/WebKit"
  fi
fi

if [[ -d /private/tmp ]] && ! is_enabled "${BUN_ENABLE_SIMDUTF_NAMESPACE}"; then
  BUN_BUILD_DIR="${NIMBUS_BUN_BUILD_DIR:-/private/tmp/nimbus-bun-shared-adapter-${BUN_PROFILE}}"
  BUN_CACHE_DIR="${NIMBUS_BUN_CACHE_DIR:-/private/tmp/nimbus-bun-cache}"
  BUN_CARGO_TARGET_DIR="${NIMBUS_BUN_CARGO_TARGET_DIR:-/private/tmp/nimbus-bun-proof-target-${BUN_PROFILE}}"
else
  BUN_PROOF_ROOT="${NIMBUS_BUN_PROOF_ROOT:-${XDG_CACHE_HOME:-${HOME}/.cache}/nimbus-bun-proof}"
  if is_enabled "${BUN_ENABLE_SIMDUTF_NAMESPACE}"; then
    BUN_BUILD_DIR="${NIMBUS_BUN_BUILD_DIR:-${BUN_PROOF_ROOT}/shared-adapter-${BUN_PROFILE}-namespaced}"
    BUN_CACHE_DIR="${NIMBUS_BUN_CACHE_DIR:-${BUN_PROOF_ROOT}/cache-shared}"
    BUN_CARGO_TARGET_DIR="${NIMBUS_BUN_CARGO_TARGET_DIR:-${BUN_PROOF_ROOT}/bun-cargo-target-${BUN_PROFILE}-shared}"
  else
    BUN_BUILD_DIR="${NIMBUS_BUN_BUILD_DIR:-${BUN_PROOF_ROOT}/shared-adapter-${BUN_PROFILE}}"
    BUN_CACHE_DIR="${NIMBUS_BUN_CACHE_DIR:-${BUN_PROOF_ROOT}/bun-cache}"
    BUN_CARGO_TARGET_DIR="${NIMBUS_BUN_CARGO_TARGET_DIR:-${BUN_PROOF_ROOT}/bun-cargo-target-${BUN_PROFILE}}"
  fi
fi
SHARED_LIBRARY="${NIMBUS_BUN_EMBED_SHARED_LIBRARY:-${BUN_BUILD_DIR}/${SHARED_LIBRARY_BASENAME}}"

REQUIRED_EXPORTS=(
  "${BUN_JSC_ADAPTER_REQUIRED_EXPORTS[@]}"
)

cd "${REPO_ROOT}"

reject_unsafe_linker_policy
if [[ "${BUN_EXECUTABLE}" == */* ]]; then
  if [[ ! -x "${BUN_EXECUTABLE}" ]]; then
    printf 'Bun executable is not executable: %s\n' "${BUN_EXECUTABLE}" >&2
    exit 1
  fi
elif ! command -v "${BUN_EXECUTABLE}" >/dev/null 2>&1; then
  printf 'Bun executable not found: %s\n' "${BUN_EXECUTABLE}" >&2
  printf 'set NIMBUS_BUN_EXECUTABLE to a pinned Bun CLI for source-backed adapter builds\n' >&2
  exit 1
fi

printf 'Bun/JSC linked adapter gate\n'
printf 'Nimbus repo: %s\n' "${REPO_ROOT}"
printf 'Bun repo:    %s\n' "${BUN_REPO}"
printf 'Bun ref:     %s\n' "${EXPECTED_BUN_REF}"
printf 'Bun rev:     %s\n' "${EXPECTED_BUN_REV}"
printf 'Bun CLI:     %s\n' "${BUN_EXECUTABLE}"
printf 'Bun profile: %s\n' "${BUN_PROFILE}"
printf 'Bun simdutf namespace enabled: %s\n' "${BUN_ENABLE_SIMDUTF_NAMESPACE}"
printf 'Bun symbol audit required: %s\n\n' "${BUN_REQUIRE_SYMBOL_AUDIT}"
printf 'Bun shared artifact audit required: %s\n' "${BUN_REQUIRE_SHARED_ARTIFACT_AUDIT}"
printf 'Bun shared library: %s\n\n' "${SHARED_LIBRARY}"

if [[ ! -f "${BUN_REPO}/src/embed_probe/lib.rs" ]]; then
  printf 'missing Bun checkout: expected %s/src/embed_probe/lib.rs\n' "${BUN_REPO}" >&2
  printf 'set NIMBUS_BUN_REPO to the Nimbus Bun fork checkout\n' >&2
  exit 1
fi

ref_rev="$(git -C "${BUN_REPO}" rev-parse "${EXPECTED_BUN_REF}^{commit}" 2>/dev/null || true)"
if [[ "${ref_rev}" != "${EXPECTED_BUN_REV}" ]]; then
  printf 'unexpected Bun source ref: expected %s at %s, got %s\n' \
    "${EXPECTED_BUN_REF}" "${EXPECTED_BUN_REV}" "${ref_rev:-missing}" >&2
  exit 1
fi

actual_bun_rev="$(git -C "${BUN_REPO}" rev-parse HEAD)"
if [[ "${actual_bun_rev}" != "${EXPECTED_BUN_REV}" ]]; then
  printf 'unexpected Bun revision: expected %s, got %s\n' \
    "${EXPECTED_BUN_REV}" "${actual_bun_rev}" >&2
  printf 'set NIMBUS_BUN_EXPECTED_REV only when the plan has recorded a new source baseline\n' >&2
  exit 1
fi

bun_status="$(git -C "${BUN_REPO}" status --short)"
if [[ -n "${bun_status}" ]]; then
  printf 'Bun proof worktree must be clean for linked-adapter verification:\n%s\n' \
    "${bun_status}" >&2
  exit 1
fi

printf '[1/11] Default no-link runtime contract\n'
make verify-bun-jsc-runtime-contract

printf '\n[2/11] Linked adapter feature compile and no-shared-library unit contract\n'
env -u NIMBUS_BUN_EMBED_LINK_ARGS -u NIMBUS_BUN_EMBED_SHARED_LIBRARY \
  cargo test -p nimbus-runtime --features bun-jsc-linked-adapter --lib backends::bun_jsc

printf '\n[3/11] Bun proof source exports\n'
for export in "${REQUIRED_EXPORTS[@]}"; do
  git -C "${BUN_REPO}" grep -q --fixed-strings "${export}" -- \
    src/embed_probe/lib.rs scripts/build/bun.ts
  printf '  %s\n' "${export}"
done

printf '\n[4/11] Bun Rust format\n'
(cd "${BUN_REPO}" && cargo fmt --all --check)

printf '\n[5/11] Bun native shared adapter build\n'
mkdir -p "${BUN_BUILD_DIR}" "${BUN_CACHE_DIR}" "${BUN_CARGO_TARGET_DIR}"
BUN_BUILD_ARGS=(
  "${BUN_EXECUTABLE}" scripts/build.ts
  "--profile=${BUN_PROFILE}"
  "--webkit=local"
  "--embedder-shared=on"
  "--build-dir=${BUN_BUILD_DIR}"
  "--cache-dir=${BUN_CACHE_DIR}"
  "--target=check-bun-embed-shared"
)
if is_enabled "${BUN_ENABLE_SIMDUTF_NAMESPACE}"; then
  BUN_BUILD_ARGS+=("--simdutf-namespace=${BUN_SIMDUTF_NAMESPACE}")
fi
(cd "${BUN_REPO}" && CARGO_TARGET_DIR="${BUN_CARGO_TARGET_DIR}" "${BUN_BUILD_ARGS[@]}")

if [[ ! -f "${SHARED_LIBRARY}" ]]; then
  printf 'missing Bun/JSC shared adapter artifact: %s\n' "${SHARED_LIBRARY}" >&2
  exit 1
fi

printf '\n[6/11] Generated build graph safety policy\n'
reject_unsafe_generated_build_graph "${BUN_BUILD_DIR}/build.ninja"

printf '\n[7/11] Bun/JSC shared adapter export audit and symbol audit\n'
audit_shared_adapter_exports "${SHARED_LIBRARY}"
audit_simdutf_symbols

case "${host_triple}" in
  aarch64-apple-darwin)
    if [[ -z "${CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER:-}" ]]; then
      if [[ -x /opt/homebrew/opt/llvm@21/bin/clang++ ]]; then
        export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=/opt/homebrew/opt/llvm@21/bin/clang++
      else
        export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER="$(command -v clang++ || command -v c++)"
      fi
    fi
    ;;
  x86_64-apple-darwin)
    if [[ -z "${CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER:-}" ]]; then
      if [[ -x /opt/homebrew/opt/llvm@21/bin/clang++ ]]; then
        export CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER=/opt/homebrew/opt/llvm@21/bin/clang++
      else
        export CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER="$(command -v clang++ || command -v c++)"
      fi
    fi
    ;;
  x86_64-unknown-linux-gnu)
    if [[ -z "${CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER:-}" ]]; then
      if command -v clang++-21 >/dev/null 2>&1; then
        export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=clang++-21
      elif command -v clang++ >/dev/null 2>&1; then
        export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=clang++
      else
        export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$(command -v c++)"
      fi
    fi
    if command -v ld.lld >/dev/null 2>&1; then
      linux_rustflags="${CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS:-}"
      case " ${linux_rustflags} " in
        *" -C link-arg=-fuse-ld="*) ;;
        *)
          export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="${linux_rustflags:+${linux_rustflags} }-C link-arg=-fuse-ld=lld"
          ;;
      esac
    fi
    ;;
esac

printf '\n[8/11] Bun embedder FFI and same-process V8+Bun/JSC proof\n'
LINKED_CARGO_JOBS="${NIMBUS_BUN_LINKED_CARGO_JOBS:-1}"
NIMBUS_BUN_EMBED_SHARED_LIBRARY="${SHARED_LIBRARY}" \
  CARGO_BUILD_JOBS="${LINKED_CARGO_JOBS}" \
  cargo test -p nimbus-runtime --features bun-jsc-linked-adapter --test \
    bun_jsc_concurrent_init -- --nocapture
NIMBUS_BUN_EMBED_SHARED_LIBRARY="${SHARED_LIBRARY}" \
  CARGO_BUILD_JOBS="${LINKED_CARGO_JOBS}" \
  cargo test -p nimbus-runtime --features bun-jsc-linked-adapter --lib \
    backends::bun_jsc -- --nocapture
NIMBUS_BUN_EMBED_SHARED_LIBRARY="${SHARED_LIBRARY}" \
  CARGO_BUILD_JOBS="${LINKED_CARGO_JOBS}" \
  cargo test -p nimbus-runtime --features bun-jsc-linked-adapter --test \
    bun_jsc_linked_adapter -- --nocapture

printf '\n[9/11] Server linked-lane diagnostics proof\n'
NIMBUS_BUN_EMBED_SHARED_LIBRARY="${SHARED_LIBRARY}" \
  CARGO_BUILD_JOBS="${LINKED_CARGO_JOBS}" \
  cargo test -p nimbus-server --features bun-jsc-linked-adapter \
    registry_and_license::registry::convex_registry_bun_jsc_lane_diagnostics_reflect_runtime_adapter_state \
    -- --nocapture

printf '\n[10/11] Nimbus whitespace diff check\n'
git diff --check

printf '\n[11/11] Bun whitespace diff check\n'
(cd "${BUN_REPO}" && git diff --check)

printf '\nBun/JSC linked adapter gate: pass\n'
