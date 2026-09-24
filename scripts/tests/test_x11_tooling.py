import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

import qualify_x11_nested  # noqa: E402
import x11_nested  # noqa: E402


class NestedX11ToolingTests(unittest.TestCase):
    def test_active_window_treats_x11_none_as_absent(self) -> None:
        self.assertIsNone(
            x11_nested.parse_active_window(
                "_NET_ACTIVE_WINDOW(WINDOW): window id # 0x0\n"
            )
        )

    def test_xwininfo_parser_preserves_offscreen_geometry(self) -> None:
        output = """
          Absolute upper-left X:  -20000
          Absolute upper-left Y:  48
          Width: 1920
          Height: 1080
          Map State: IsViewable
        """

        self.assertEqual(
            x11_nested.parse_xwininfo(output),
            {
                "x": -20000,
                "y": 48,
                "width": 1920,
                "height": 1080,
                "map_state": "IsViewable",
            },
        )

    def test_junit_parser_records_pass_fail_and_skip(self) -> None:
        xml = """<?xml version="1.0"?>
        <testsuites xmlns="urn:test">
          <testsuite>
            <testcase classname="suite" name="passes" />
            <testcase classname="suite" name="fails"><failure /></testcase>
            <testcase classname="suite" name="skips"><skipped /></testcase>
          </testsuite>
        </testsuites>
        """
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "junit.xml"
            path.write_text(xml, encoding="utf-8")
            self.assertEqual(
                qualify_x11_nested.junit_results(path),
                {
                    "suite::passes": "pass",
                    "suite::fails": "fail",
                    "suite::skips": "skipped",
                },
            )

    def test_result_comparison_reports_missing_and_changed_tests(self) -> None:
        self.assertEqual(
            qualify_x11_nested.compare_result_sets(
                {"same": "pass", "changed": "pass", "real-only": "pass"},
                {"same": "pass", "changed": "fail", "nested-only": "pass"},
            ),
            [
                "changed: real=pass, nested=fail",
                "nested-only: real=missing, nested=pass",
                "real-only: real=pass, nested=missing",
            ],
        )

    def test_committed_mutants_apply_and_name_existing_tests(self) -> None:
        for mutant in qualify_x11_nested.load_mutants():
            with self.subTest(mutant=mutant["name"]):
                check = subprocess.run(
                    ["git", "apply", "--check", mutant["patch"]],
                    cwd=qualify_x11_nested.REPO_ROOT,
                    capture_output=True,
                    text=True,
                )
                self.assertEqual(check.returncode, 0, check.stderr)

    def test_manifest_rejects_a_renamed_x11_test(self) -> None:
        manifest = [
            {
                "name": "stale",
                "patch": "scripts/mutants/backspace-noop.patch",
                "tests": [
                    ["real_x11_text_input", "no_such_test", "fail"],
                    ["real_x11_mouse", "click_below_last_line_moves_caret_to_document_end", "pass"],
                ],
            }
        ]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "mutants.json"
            path.write_text(json.dumps(manifest), encoding="utf-8")
            with self.assertRaisesRegex(
                qualify_x11_nested.QualificationError, "no_such_test"
            ):
                qualify_x11_nested.load_mutants(path)


if __name__ == "__main__":
    unittest.main()
