#!/usr/bin/env python3
import os
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
VERIFY_SCRIPT = REPO_ROOT / "scripts" / "verify-profile-aware-isolate-runtime.sh"


class RuntimeStrategyGateTests(unittest.TestCase):
    def test_rejected_symbol_search_error_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            fake_bin = Path(tempdir)
            fake_rg = fake_bin / "rg"
            fake_rg.write_text("#!/bin/sh\nexit 2\n", encoding="utf-8")
            fake_rg.chmod(0o755)
            env = os.environ.copy()
            env["PATH"] = f"{fake_bin}:/usr/bin:/bin"

            result = subprocess.run(
                ["/bin/bash", str(VERIFY_SCRIPT)],
                cwd=REPO_ROOT,
                env=env,
                text=True,
                capture_output=True,
                check=False,
            )

        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assertIn("rejected-symbol search could not run", result.stderr)
        self.assertNotIn("PASS rejected fresh-realm product symbols", result.stdout)


if __name__ == "__main__":
    unittest.main()
