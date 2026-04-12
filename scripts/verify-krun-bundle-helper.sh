#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixture_path="${repo_root}/scripts/fixtures/crun-spec-config.json"
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/neovex-krun-bundle-verify.XXXXXX")"
trap 'rm -rf "${tmp_dir}"' EXIT

config_path="${tmp_dir}/config.json"
cp "${fixture_path}" "${config_path}"

bash "${repo_root}/scripts/prepare-krun-bundle.sh" \
  --config-path "${config_path}" \
  --skip-spec \
  --rootfs /srv/neovex/rootfs \
  --host-port 15432 \
  --guest-port 5432 \
  --process-arg /usr/bin/postgres \
  --process-arg -D \
  --process-arg /var/lib/postgresql/data

python3 - "${config_path}" <<'PY'
import json
import sys
from pathlib import Path

config_path = Path(sys.argv[1])
with config_path.open("r", encoding="utf-8") as fh:
    config = json.load(fh)

assert config["root"]["path"] == "/srv/neovex/rootfs"
assert config["root"]["readonly"] is False
assert config["process"]["cwd"] == "/"
assert config["process"]["args"] == [
    "/usr/bin/postgres",
    "-D",
    "/var/lib/postgresql/data",
]
assert config["annotations"]["run.oci.handler"] == "krun"
assert config["annotations"]["krun.port_map"] == "15432:5432"
PY

echo "verified: krun bundle helper updated ${config_path}"
