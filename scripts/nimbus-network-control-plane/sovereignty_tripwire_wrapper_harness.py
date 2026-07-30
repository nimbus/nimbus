#!/usr/bin/env python3
"""Deterministic subprocess harness for the tripwire's privileged wrapper."""

from __future__ import annotations

import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile
from typing import Any


def run_isolated_wrapper(
    repo_root: Path,
) -> tuple[subprocess.CompletedProcess[str], dict[str, Any], bool, bool]:
    with tempfile.TemporaryDirectory() as temporary:
        parent = Path(temporary).resolve()
        parent.chmod(0o755)
        copied_root = parent / "copied-repo"
        copied_scripts = copied_root / "scripts"
        copied_scripts.mkdir(parents=True)
        wrapper = copied_scripts / "nimbus-network-sovereignty-tripwire.sh"
        shutil.copy2(
            repo_root / "scripts/nimbus-network-sovereignty-tripwire.sh",
            wrapper,
        )
        shutil.copytree(
            repo_root / "scripts/nimbus_network_sovereignty_tripwire",
            copied_scripts / "nimbus_network_sovereignty_tripwire",
        )

        output_parent = parent / "output"
        output_parent.mkdir(mode=0o777)
        output_parent.chmod(0o777)
        output = output_parent / "skip-evidence"
        shadow = parent / "shadow"
        malicious_package = shadow / "nimbus_network_sovereignty_tripwire"
        malicious_package.mkdir(parents=True)
        python_marker = output_parent / "python-path-imported"
        shell_marker = output_parent / "shell-startup-imported"
        shell_startup = shadow / "startup.sh"
        shell_startup.write_text(
            f"printf imported > {str(shell_marker)!r}\n",
            encoding="utf-8",
        )
        (malicious_package / "__init__.py").write_text("", encoding="utf-8")
        (malicious_package / "runner.py").write_text(
            "from pathlib import Path\n"
            f"Path({str(python_marker)!r}).write_text('imported')\n"
            "def main(): return 0\n",
            encoding="utf-8",
        )
        environment = os.environ.copy()
        environment.update(
            {
                "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
                "BASH_ENV": str(shell_startup),
                "ENV": str(shell_startup),
                "PYTHONHOME": str(shadow),
                "PYTHONPATH": str(shadow),
            }
        )
        drop_privilege = None
        if os.geteuid() == 0:

            def drop_privilege() -> None:
                os.setgroups([])
                os.setgid(65534)
                os.setuid(65534)

        result = subprocess.run(
            [
                str(wrapper),
                "--runner-id",
                "nnc47-wrapper",
                "--expected-hostname",
                platform.node(),
                "--host-class",
                "minicloud",
                "--provider-kind",
                "linuxkit",
                "--output-dir",
                str(output),
            ],
            cwd=shadow,
            env=environment,
            preexec_fn=drop_privilege,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )
        evidence = json.loads((output / "evidence.json").read_text(encoding="utf-8"))
        return result, evidence, python_marker.exists(), shell_marker.exists()
