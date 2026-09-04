import datetime as dt
import importlib.util
import sys
import textwrap
import unittest
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("test-taxonomy.py")
SPEC = importlib.util.spec_from_file_location("test_taxonomy", SCRIPT_PATH)
taxonomy = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = taxonomy
SPEC.loader.exec_module(taxonomy)


def row(
    pattern="test(/ignored/)",
    reason="heavy-resource",
    expiry=None,
    scope="filter",
    evidence="measured with nextest list",
):
    return taxonomy.Exclusion(
        pattern=pattern,
        reason=reason,
        evidence=evidence,
        measured_at="2026-07-05",
        owner="test-infra",
        issue="PLAN.md#b1",
        scope=scope,
        expiry=expiry,
    )


class GenerateNextestTests(unittest.TestCase):
    def test_generate_nextest_emits_marked_filter_section(self):
        generated = taxonomy.generate_nextest_section([row("test(/a/)"), row("test(/b/)")])
        self.assertIn(taxonomy.GENERATED_BEGIN, generated)
        self.assertIn("default-filter = 'not (test(/a/) or test(/b/))'", generated)
        self.assertIn(taxonomy.GENERATED_END, generated)

    def test_generate_nextest_empty_ledger_keeps_all_tests(self):
        self.assertIn("default-filter = 'all()'", taxonomy.generate_nextest_section([]))

    def test_generate_nextest_omits_ignored_scope_rows_from_filter(self):
        rows = [row("test(/doc_only/)", scope="ignored"), row("test(/real_exclusion/)")]
        generated = taxonomy.generate_nextest_section(rows)
        self.assertIn("default-filter = 'not (test(/real_exclusion/))'", generated)
        self.assertNotIn("doc_only", generated)

    def test_generate_nextest_all_ignored_scope_yields_default_include(self):
        rows = [row("test(/doc_only/)", scope="ignored")]
        self.assertIn("default-filter = 'all()'", taxonomy.generate_nextest_section(rows))

    def test_validate_rejects_unknown_scope(self):
        errors = taxonomy.validate_exclusions([row(scope="sometimes")], dt.date(2026, 7, 5))
        self.assertTrue(any("invalid scope" in error for error in errors))

    def test_extract_generated_section_rejects_missing_markers(self):
        self.assertIsNone(taxonomy.extract_generated_section("[profile.ci-pr]\n"))


class CheckTests(unittest.TestCase):
    def test_check_accepts_matching_config_and_ledgered_ignore(self):
        exclusions = [row("test(/ignored_case/)", scope="ignored")]
        test = taxonomy.RustTest(
            path="crates/demo/src/tests.rs",
            line=3,
            name="ignored_case",
            crate="demo",
            ignored=True,
            canonical_id="tests::ignored_case",
        )
        errors = taxonomy.check_taxonomy(
            exclusions=exclusions,
            nextest_config_text=taxonomy.generate_nextest_section(exclusions),
            tests=[test],
            env_violations=[],
            today=dt.date(2026, 7, 5),
        )
        self.assertEqual(errors, [])

    def test_check_reports_config_drift(self):
        errors = taxonomy.check_taxonomy(
            exclusions=[row("test(/ignored/)")],
            nextest_config_text=taxonomy.generate_nextest_section([row("test(/other/)")]),
            tests=[],
            env_violations=[],
            today=dt.date(2026, 7, 5),
        )
        self.assertTrue(any("out of date" in error for error in errors))

    def test_check_reports_ignore_without_ledger(self):
        test = taxonomy.RustTest(
            path="crates/demo/src/tests.rs",
            line=8,
            name="ignored_case",
            crate="demo",
            ignored=True,
            canonical_id="tests::ignored_case",
        )
        errors = taxonomy.check_taxonomy(
            exclusions=[row("test(/other/)")],
            nextest_config_text=taxonomy.generate_nextest_section([row("test(/other/)")]),
            tests=[test],
            env_violations=[],
            today=dt.date(2026, 7, 5),
        )
        self.assertTrue(any("has no exclusions ledger row" in error for error in errors))

    def test_check_reports_expired_quarantine_and_env_violation(self):
        flaky = row("test(/flaky/)", reason="flaky-quarantine", expiry="2026-07-01")
        errors = taxonomy.check_taxonomy(
            exclusions=[flaky],
            nextest_config_text=taxonomy.generate_nextest_section([flaky]),
            tests=[],
            env_violations=["crates/demo/tests/demo.rs:4: compile-time Cargo env macro in test tree"],
            today=dt.date(2026, 7, 5),
        )
        self.assertTrue(any("expired" in error for error in errors))
        self.assertTrue(any("compile-time Cargo env macro" in error for error in errors))


class InventoryAndCoverageTests(unittest.TestCase):
    def test_inventory_counts_attributes_ignored_and_crates(self):
        tests = [
            taxonomy.RustTest("crates/a/src/lib.rs", 1, "one", "a", False, "one"),
            taxonomy.RustTest("crates/a/src/lib.rs", 2, "two", "a", True, "two"),
            taxonomy.RustTest("crates/b/src/lib.rs", 3, "three", "b", False, "three"),
        ]
        report = taxonomy.inventory_report(tests, nextest_visible=2)
        self.assertIn("test_attributes = 3", report)
        self.assertIn("nextest_visible = 2", report)
        self.assertIn("a: test_attributes=2 ignored=1", report)

    def test_coverage_report_counts_reasons_and_ledgered_ignores(self):
        exclusions = [row("test(/ignored/)"), row("test(/other/)", reason="privileged")]
        tests = [
            taxonomy.RustTest("crates/a/src/lib.rs", 1, "ignored_case", "a", True, "ignored_case"),
            taxonomy.RustTest("crates/a/src/lib.rs", 2, "plain_case", "a", False, "plain_case"),
        ]
        report = taxonomy.coverage_report(exclusions, tests)
        self.assertIn("exclusions = 2", report)
        self.assertIn("ignored_tests_with_ledger_row = 1", report)
        self.assertIn("privileged: 1", report)

    def test_nextest_visible_from_json_ignores_ignored_cases(self):
        payload = {
            "rust-suites": {
                "demo": {
                    "testcases": {
                        "a": {"ignored": False, "filter-match": {"status": "matches"}},
                        "b": {"ignored": True, "filter-match": {"status": "matches"}},
                        "c": {"ignored": False, "filter-match": {"status": "mismatch"}},
                    }
                }
            }
        }
        self.assertEqual(taxonomy.nextest_visible_from_json(__import__("json").dumps(payload)), 1)


class ScopeGateTests(unittest.TestCase):
    def nextest_case(self, name="demo::slow_case", ignored=False):
        return taxonomy.NextestTest(
            path="target/debug/deps/demo",
            line=0,
            name=name,
            crate="demo",
            ignored=ignored,
            canonical_id=name,
            binary_name="demo",
            kind="lib",
        )

    def test_check_fails_closed_on_filter_scope_rows_without_authoritative_json(self):
        filter_row = row("test(/slow_case/)", scope="filter")
        errors = taxonomy.check_taxonomy(
            exclusions=[filter_row],
            nextest_config_text=taxonomy.generate_nextest_section([filter_row]),
            tests=[],
            env_violations=[],
            today=dt.date(2026, 7, 5),
        )
        self.assertTrue(any("gated until the B2" in error for error in errors))

    def test_check_accepts_filter_scope_when_authoritative_json_matches_non_ignored_test(self):
        filter_row = row(
            "test(/slow_case/)",
            scope="filter",
            evidence="duration 62s in ci-pr lane",
        )
        errors = taxonomy.check_taxonomy(
            exclusions=[filter_row],
            nextest_config_text=taxonomy.generate_nextest_section([filter_row]),
            tests=[],
            nextest_tests=[self.nextest_case()],
            env_violations=[],
            today=dt.date(2026, 7, 5),
        )
        self.assertEqual(errors, [])

    def test_check_rejects_filter_scope_matching_only_ignored_nextest_tests(self):
        filter_row = row(
            "test(/slow_case/)",
            scope="filter",
            evidence="duration 62s in ci-pr lane",
        )
        errors = taxonomy.check_taxonomy(
            exclusions=[filter_row],
            nextest_config_text=taxonomy.generate_nextest_section([filter_row]),
            tests=[],
            nextest_tests=[self.nextest_case(ignored=True)],
            env_violations=[],
            today=dt.date(2026, 7, 5),
        )
        self.assertTrue(any("matches zero real non-ignored" in error for error in errors))

    def test_check_rejects_filter_scope_without_duration_or_lane_evidence(self):
        filter_row = row("test(/slow_case/)", scope="filter", evidence="measured manually")
        errors = taxonomy.check_taxonomy(
            exclusions=[filter_row],
            nextest_config_text=taxonomy.generate_nextest_section([filter_row]),
            tests=[],
            nextest_tests=[self.nextest_case()],
            env_violations=[],
            today=dt.date(2026, 7, 5),
        )
        self.assertTrue(any("evidence must cite a measured duration" in error for error in errors))

    def test_profile_scope_passes_when_pattern_in_config(self):
        profile_row = row("test(/pool_reuse::isol_/)", scope="profile")
        config = (
            taxonomy.generate_nextest_section([profile_row])
            + "\n[profile.ci-runtime]\ndefault-filter = 'not test(/pool_reuse::isol_/)'\n"
        )
        errors = taxonomy.check_taxonomy(
            exclusions=[profile_row],
            nextest_config_text=config,
            tests=[],
            env_violations=[],
            today=dt.date(2026, 7, 5),
        )
        self.assertEqual(errors, [])

    def test_profile_scope_fails_when_pattern_absent_from_config(self):
        profile_row = row("test(/pool_reuse::isol_/)", scope="profile")
        errors = taxonomy.check_taxonomy(
            exclusions=[profile_row],
            nextest_config_text=taxonomy.generate_nextest_section([profile_row]),
            tests=[],
            env_violations=[],
            today=dt.date(2026, 7, 5),
        )
        self.assertTrue(any("not found in any hand-written" in error for error in errors))


class ScannerTests(unittest.TestCase):
    def test_scanner_includes_inline_test_module_in_canonical_id(self):
        import tempfile
        source = (
            "pub fn real() {}\n"
            "#[cfg(test)]\n"
            "mod tests {\n"
            "    #[test]\n"
            "    fn inline_case() {}\n"
            "}\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            file = Path(tmp) / "crates" / "demo" / "src" / "lib.rs"
            file.parent.mkdir(parents=True)
            file.write_text(source)
            tests = taxonomy.scan_rust_tests(Path(tmp))
            self.assertEqual([test.canonical_id for test in tests], ["tests::inline_case"])

    def test_scanner_ignores_raw_string_braces_when_tracking_inline_modules(self):
        import tempfile
        source = (
            "#[cfg(test)]\n"
            "mod tests {\n"
            "    #[test]\n"
            "    fn first_case() {\n"
            "        let _json = r#\"{\n"
            "  \"x\": 1\n"
            "}\"#;\n"
            "    }\n"
            "    #[test]\n"
            "    fn second_case() {}\n"
            "}\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            file = Path(tmp) / "crates" / "demo" / "src" / "lib.rs"
            file.parent.mkdir(parents=True)
            file.write_text(source)
            tests = taxonomy.scan_rust_tests(Path(tmp))
            self.assertEqual(
                [test.canonical_id for test in tests],
                ["tests::first_case", "tests::second_case"],
            )

    def test_scanner_sees_test_behind_multiline_ignore_string(self):
        import tempfile
        source = (
            "#[test]\n"
            '#[ignore = "MANUAL diagnostic, demoted from the cage lane. Long reason \\\n'
            '            spanning several lines with details like vector.h:415 \\\n'
            '            and more."]\n'
            "fn nodefull_anchor_first_then_refill_does_not_abort() {\n"
            "}\n"
            "#[test]\n"
            "fn plain_case_still_scanned() {}\n"
        )
        with tempfile.TemporaryDirectory() as tmp:
            file = Path(tmp) / "crates" / "demo" / "src" / "tests" / "pool.rs"
            file.parent.mkdir(parents=True)
            file.write_text(source)
            tests = taxonomy.scan_rust_tests(Path(tmp))
            names = {t.name: t.ignored for t in tests}
            self.assertEqual(
                names,
                {
                    "nodefull_anchor_first_then_refill_does_not_abort": True,
                    "plain_case_still_scanned": False,
                },
            )


class EnvBaselineTests(unittest.TestCase):
    def _tree_with_hits(self, tmp, rel_path, hits):
        file = Path(tmp) / rel_path
        file.parent.mkdir(parents=True, exist_ok=True)
        body = "\n".join(['let p = env!("CARGO_MANIFEST_DIR");'] * hits) + "\nfn other() {}\n"
        file.write_text(body)

    def test_env_gate_flags_file_not_in_baseline(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            self._tree_with_hits(tmp, "crates/demo/tests/new_case.rs", 1)
            violations = taxonomy.find_compile_time_env_violations(Path(tmp), baseline={})
            self.assertEqual(len(violations), 1)
            self.assertIn("baseline 0", violations[0])

    def test_env_gate_accepts_counts_within_baseline_regardless_of_lines(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            self._tree_with_hits(tmp, "crates/demo/tests/known.rs", 2)
            violations = taxonomy.find_compile_time_env_violations(
                Path(tmp), baseline={"crates/demo/tests/known.rs": 2}
            )
            self.assertEqual(violations, [])

    def test_env_gate_scans_inline_cfg_test_modules_outside_tests_trees(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            file = Path(tmp) / "crates" / "demo" / "src" / "lib.rs"
            file.parent.mkdir(parents=True)
            file.write_text(
                "pub fn real() {}\n#[cfg(test)]\nmod tests {\n"
                '    const P: &str = env!("CARGO_MANIFEST_DIR");\n}\n'
            )
            violations = taxonomy.find_compile_time_env_violations(Path(tmp), baseline={})
            self.assertEqual(len(violations), 1)
            self.assertIn("crates/demo/src/lib.rs", violations[0])

    def test_env_gate_flags_count_exceeding_baseline(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            self._tree_with_hits(tmp, "crates/demo/tests/known.rs", 3)
            violations = taxonomy.find_compile_time_env_violations(
                Path(tmp), baseline={"crates/demo/tests/known.rs": 2}
            )
            self.assertEqual(len(violations), 1)
            self.assertIn("3 compile-time", violations[0])

    def test_env_gate_allows_cargo_bin_option_env_fallback(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            file = Path(tmp) / "crates" / "demo" / "tests" / "bin.rs"
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text('let p = option_env!("CARGO_BIN_EXE_demo");\n')
            violations = taxonomy.find_compile_time_env_violations(Path(tmp), baseline={})
            self.assertEqual(violations, [])


class CaseMatrixTests(unittest.TestCase):
    def test_absent_case_matrix_is_warning_not_failure(self):
        status, out, err = taxonomy.run_case_matrix_check(Path("/definitely/missing/case-matrix.toml"))
        self.assertEqual(status, 0)
        self.assertEqual(out, "")
        self.assertIn("warning:", err)

    def test_case_matrix_accepts_tracked_gap(self):
        text = textwrap.dedent(
            """
            [[surfaces]]
            surface = "storage"
            mission_critical = true
            cases = [
              { class = "main", tests = ["storage_main"] },
              { class = "error", tests = ["storage_error"] },
              { class = "recovery", gap = "tracked in B8" },
            ]
            """
        )
        self.assertEqual(taxonomy.validate_case_matrix_text(text), [])

    def test_case_matrix_rejects_empty_untracked_case(self):
        text = textwrap.dedent(
            """
            [[surfaces]]
            surface = "storage"
            cases = [
              { class = "main", tests = [] },
            ]
            """
        )
        self.assertTrue(any("empty tests require" in error for error in taxonomy.validate_case_matrix_text(text)))


if __name__ == "__main__":
    unittest.main()


class LexStateTrackerTests(unittest.TestCase):
    def test_brace_in_char_literal_does_not_change_depth(self):
        delta, state = taxonomy._brace_delta_with_raw_state("let a = '{';", taxonomy._LexState())
        self.assertEqual(delta, 0)
        self.assertEqual(state.comment_depth, 0)
        delta, _ = taxonomy._brace_delta_with_raw_state("let b = '}';", taxonomy._LexState())
        self.assertEqual(delta, 0)

    def test_brace_in_block_comment_ignored_and_state_crosses_lines(self):
        state = taxonomy._LexState()
        delta, state = taxonomy._brace_delta_with_raw_state("/* opening { brace", state)
        self.assertEqual(delta, 0)
        self.assertEqual(state.comment_depth, 1)
        delta, state = taxonomy._brace_delta_with_raw_state("still { inside } */ fn x() {", state)
        self.assertEqual(state.comment_depth, 0)
        self.assertEqual(delta, 1)

    def test_nested_block_comments_track_depth(self):
        state = taxonomy._LexState()
        delta, state = taxonomy._brace_delta_with_raw_state("/* outer /* inner { */ still } */", state)
        self.assertEqual(delta, 0)
        self.assertEqual(state.comment_depth, 0)

    def test_unbalanced_raw_string_brace_crosses_lines(self):
        state = taxonomy._LexState()
        delta, state = taxonomy._brace_delta_with_raw_state('let s = r#"open { brace', state)
        self.assertEqual(delta, 0)
        self.assertIsNotNone(state.raw_terminator)
        delta, state = taxonomy._brace_delta_with_raw_state('still { unbalanced "# ; }', state)
        self.assertIsNone(state.raw_terminator)
        self.assertEqual(delta, -1)

    def test_lifetime_quote_does_not_swallow_braces(self):
        delta, _ = taxonomy._brace_delta_with_raw_state("fn f<'a>(x: &'a str) {", taxonomy._LexState())
        self.assertEqual(delta, 1)


class InventoryGateTests(unittest.TestCase):
    def test_loader_parses_real_nextest_json_shape(self):
        payload = {
            "rust-suites": {
                "nimbus-demo::bin/demo": {
                    "package-name": "nimbus-demo",
                    "binary-name": "demo",
                    "kind": "test",
                    "binary-path": "/t/demo",
                    "testcases": {
                        "mod_a::fast_case": {"ignored": False, "filter-match": {"status": "matches"}},
                        "mod_a::slow_ignored_case": {"ignored": True, "filter-match": {"status": "matches"}},
                    },
                }
            }
        }
        import json as _json
        tests = taxonomy.load_nextest_tests_from_json(_json.dumps(payload))
        self.assertEqual(len(tests), 2)
        by_name = {t.canonical_id: t for t in tests}
        self.assertFalse(by_name["mod_a::fast_case"].ignored)
        self.assertTrue(by_name["mod_a::slow_ignored_case"].ignored)
        self.assertEqual(by_name["mod_a::fast_case"].crate, "nimbus-demo")
        self.assertEqual(by_name["mod_a::fast_case"].binary_name, "demo")
        filter_row = row("test(/mod_a::fast_case/)", scope="filter")
        errors = taxonomy.check_taxonomy(
            exclusions=[filter_row],
            nextest_config_text=taxonomy.generate_nextest_section([filter_row]),
            tests=[],
            nextest_tests=tests,
            env_violations=[],
            today=dt.date(2026, 7, 5),
        )
        self.assertTrue(any("must cite a measured duration" in e for e in errors))
        self.assertFalse(any("matches zero" in e for e in errors))

    def test_filter_evidence_rejects_bare_words_accepts_measured(self):
        self.assertFalse(taxonomy.evidence_cites_duration_or_lane("duration TBD"))
        self.assertFalse(taxonomy.evidence_cites_duration_or_lane("lane"))
        self.assertTrue(taxonomy.evidence_cites_duration_or_lane("measured 20.2s in shard 2"))
        self.assertTrue(taxonomy.evidence_cites_duration_or_lane("runs in the ci-nightly lane"))

    def test_gating_requires_meta_platform_and_version(self):
        import tempfile, json as _json
        with tempfile.TemporaryDirectory() as tmp:
            inv = Path(tmp) / "nextest-list.json"
            meta = Path(tmp) / "nextest-list.meta.json"
            inv.write_text('{"rust-suites": {}}')
            orig_inv, orig_meta = taxonomy.NEXTEST_INVENTORY_PATH, taxonomy.INVENTORY_META_PATH
            try:
                taxonomy.NEXTEST_INVENTORY_PATH, taxonomy.INVENTORY_META_PATH = inv, meta
                tests, reason = taxonomy.load_inventory_for_gating()
                self.assertIsNone(tests)
                self.assertIn("meta absent", reason)
                meta.write_text(_json.dumps({"platform": "aarch64-apple-darwin", "nextest_version": "0.9.143"}))
                tests, reason = taxonomy.load_inventory_for_gating()
                self.assertIsNone(tests)
                self.assertIn("not canonical", reason)
                meta.write_text(_json.dumps({"platform": "x86_64-unknown-linux-gnu", "nextest_version": "0.9.135"}))
                tests, reason = taxonomy.load_inventory_for_gating()
                self.assertIsNone(tests)
                self.assertIn("not the required", reason)
                meta.write_text(_json.dumps({"platform": "x86_64-unknown-linux-gnu", "nextest_version": "0.9.143"}))
                tests, reason = taxonomy.load_inventory_for_gating()
                self.assertEqual(tests, [])
                self.assertEqual(reason, "canonical")
            finally:
                taxonomy.NEXTEST_INVENTORY_PATH, taxonomy.INVENTORY_META_PATH = orig_inv, orig_meta
