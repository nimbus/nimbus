#!/usr/bin/env python3
import contextlib
import importlib.util
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path

VERIFY_PATH = Path(__file__).with_name("verify.py")
SPEC = importlib.util.spec_from_file_location("release_readiness_verify", VERIFY_PATH)
assert SPEC is not None and SPEC.loader is not None
VERIFY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFY)

CANDIDATE = {
    "nimbus": "1" * 40,
    "desktop": "2" * 40,
    "deno": "3" * 40,
    "main": "4" * 40,
}


class ReleaseReadinessVerifierTests(unittest.TestCase):
    def run_verifier(
        self,
        *,
        proof_revisions: tuple[str, ...] = (
            CANDIDATE["nimbus"],
            CANDIDATE["desktop"],
            CANDIDATE["deno"],
            CANDIDATE["main"],
        ),
        header_revisions: tuple[str, ...] = (),
        blocked_conditions: tuple[str, ...] = (),
        proof_text: str | None = None,
        candidate: dict[str, str] | None = None,
    ) -> tuple[int, str]:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            proof = root / "proof.md"
            if proof_text is None:
                proof_text = (
                    "# Proof\n\n"
                    + "\n".join(header_revisions)
                    + "\n\n## Bound evidence\n\n"
                    + "\n".join(proof_revisions)
                    + "\n\n## Unrelated evidence\n"
                )
            proof.write_text(proof_text, encoding="utf-8")
            conditions = []
            for condition_id in VERIFY.EXPECTED_IDS:
                if condition_id in blocked_conditions:
                    conditions.append({"id": condition_id, "state": "blocked"})
                else:
                    conditions.append(
                        {
                            "id": condition_id,
                            "state": "pass",
                            "evidence": {
                                "path": "proof.md",
                                "anchor": "## Bound evidence",
                            },
                        }
                    )
            matrix = root / "matrix.json"
            matrix.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "candidate": CANDIDATE if candidate is None else candidate,
                        "conditions": conditions,
                    }
                ),
                encoding="utf-8",
            )
            previous_argv = sys.argv
            output = io.StringIO()
            try:
                sys.argv = [str(VERIFY_PATH), str(matrix)]
                with contextlib.redirect_stdout(output):
                    status = VERIFY.main()
            finally:
                sys.argv = previous_argv
            return status, output.getvalue()

    def test_exact_candidate_bound_proof_has_no_structural_error(self) -> None:
        status, output = self.run_verifier()
        self.assertEqual(status, 0)
        self.assertNotIn("ERROR:", output)
        self.assertIn("0 structural errors", output)

    def test_stale_pass_proof_is_rejected(self) -> None:
        status, output = self.run_verifier(
            proof_revisions=(CANDIDATE["desktop"], CANDIDATE["deno"], CANDIDATE["main"])
        )
        self.assertNotEqual(status, 0)
        self.assertIn("evidence is not bound to the nimbus candidate", output)

    def test_candidate_mention_outside_anchor_does_not_bind_evidence(self) -> None:
        status, output = self.run_verifier(
            proof_revisions=(
                CANDIDATE["desktop"],
                CANDIDATE["deno"],
                CANDIDATE["main"],
            ),
            header_revisions=(CANDIDATE["nimbus"],),
        )
        self.assertNotEqual(status, 0)
        self.assertIn("evidence is not bound to the nimbus candidate", output)

    def test_fenced_headings_do_not_create_or_truncate_evidence_sections(self) -> None:
        proof_text = "\n".join(
            (
                "# Proof",
                "",
                "```text",
                "## Bound evidence",
                "fake",
                "```",
                "",
                "## Bound evidence",
                "",
                "```sh",
                "# make ci",
                "```",
                CANDIDATE["nimbus"],
                CANDIDATE["desktop"],
                CANDIDATE["deno"],
                CANDIDATE["main"],
                "",
                "## Unrelated evidence",
                "",
            )
        )
        status, output = self.run_verifier(proof_text=proof_text)
        self.assertEqual(status, 0)
        self.assertIn("0 structural errors", output)

    def test_indented_code_heading_does_not_bind_evidence(self) -> None:
        proof_text = "\n".join(
            (
                "# Proof",
                "",
                "    ## Bound evidence",
                f"    {CANDIDATE['nimbus']}",
                "",
                "## Bound evidence",
                CANDIDATE["desktop"],
                CANDIDATE["deno"],
                CANDIDATE["main"],
                "",
                "## Unrelated evidence",
                "",
            )
        )
        status, output = self.run_verifier(proof_text=proof_text)
        self.assertNotEqual(status, 0)
        self.assertIn("evidence is not bound to the nimbus candidate", output)

    def test_mixed_tab_indentation_does_not_create_an_evidence_heading(self) -> None:
        proof_text = "\n".join(
            (
                "# Proof",
                "",
                " \t## Bound evidence",
                f" \t{CANDIDATE['nimbus']}",
                "",
                "## Bound evidence",
                CANDIDATE["desktop"],
                CANDIDATE["deno"],
                CANDIDATE["main"],
                "",
                "## Unrelated evidence",
                "",
            )
        )
        status, output = self.run_verifier(proof_text=proof_text)
        self.assertNotEqual(status, 0)
        self.assertIn("evidence is not bound to the nimbus candidate", output)

    def test_unclosed_fence_fails_closed_before_unrelated_candidate_shas(self) -> None:
        proof_text = "\n".join(
            (
                "# Proof",
                "",
                "## Bound evidence",
                "",
                "````text",
                "proof output",
                "```",
                "",
                "## Unrelated evidence",
                CANDIDATE["nimbus"],
                CANDIDATE["desktop"],
                CANDIDATE["deno"],
                CANDIDATE["main"],
                "",
            )
        )
        status, output = self.run_verifier(proof_text=proof_text)
        self.assertNotEqual(status, 0)
        self.assertIn("evidence proof has an unterminated Markdown fence", output)

    def test_backtick_in_fence_info_string_does_not_hide_peer_heading(self) -> None:
        proof_text = "\n".join(
            (
                "# Proof",
                "",
                "## Bound evidence",
                "```text`not-a-fence",
                "",
                "## Unrelated evidence",
                CANDIDATE["nimbus"],
                CANDIDATE["desktop"],
                CANDIDATE["deno"],
                CANDIDATE["main"],
                "",
            )
        )
        status, output = self.run_verifier(proof_text=proof_text)
        self.assertNotEqual(status, 0)
        self.assertIn("evidence is not bound to the nimbus candidate", output)

    def test_blocked_row_requires_no_candidate_binding(self) -> None:
        status, output = self.run_verifier(blocked_conditions=("tenant_lifecycle",))
        self.assertNotEqual(status, 0)
        self.assertIn("tenant_lifecycle: blocked", output)
        self.assertIn("0 structural errors", output)
        self.assertNotIn("tenant_lifecycle: evidence is not bound", output)

    def test_desktop_pass_requires_exact_desktop_revision(self) -> None:
        status, output = self.run_verifier(
            proof_revisions=(CANDIDATE["nimbus"], CANDIDATE["deno"], CANDIDATE["main"])
        )
        self.assertNotEqual(status, 0)
        self.assertIn("desktop_app: evidence is not bound to the desktop candidate", output)

    def test_candidate_revisions_must_be_full_lowercase_shas(self) -> None:
        malformed = dict(CANDIDATE)
        malformed["nimbus"] = "abc"
        status, output = self.run_verifier(candidate=malformed)
        self.assertNotEqual(status, 0)
        self.assertIn("candidate.nimbus must be a full lowercase commit SHA", output)


if __name__ == "__main__":
    unittest.main()
