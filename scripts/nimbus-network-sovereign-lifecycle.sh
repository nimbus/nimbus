#!/bin/bash -p
# Thin, exit-preserving entry point for the NNC9.2 privileged lifecycle proof.

set -euo pipefail
unset BASH_ENV ENV CDPATH GLOBIGNORE

SCRIPT_PATH="${BASH_SOURCE[0]}"
SCRIPT_DIR="${SCRIPT_PATH%/*}"
if [ "${SCRIPT_DIR}" = "${SCRIPT_PATH}" ]; then
  SCRIPT_DIR="."
fi
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd -P)"
if [ "${EUID}" -eq 0 ]; then
  PYTHON_BIN="/usr/bin/python3"
else
  PYTHON_BIN="$(command -v python3)"
fi
if [ ! -x "${PYTHON_BIN}" ]; then
  printf 'trusted Python interpreter is unavailable: %s\n' "${PYTHON_BIN}" >&2
  exit 64
fi
unset PYTHONHOME PYTHONPATH
exec "${PYTHON_BIN}" -I -S -c \
  'import sys; sys.path.append(sys.argv.pop(1)); from nimbus_network_sovereignty_tripwire.lifecycle import main; raise SystemExit(main())' \
  "${REPO_ROOT}/scripts" "$@"
