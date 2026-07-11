from pathlib import Path
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

    def test_committed_mutant_manifest_is_valid_and_complete(self) -> None:
        mutants = qualify_x11_nested.load_mutants()
        self.assertEqual(
            [mutant["name"] for mutant in mutants],
            [
                "duplicate_line_binding",
                "backspace_noop",
                "mouse_below_document",
                "app_menu_backdrop",
                "replace_all_noop",
                "ordinary_autosave",
            ],
        )
        self.assertTrue(
            all(
                any(test[2] == "fail" for test in mutant["tests"]) for mutant in mutants
            )
        )
        self.assertTrue(
            all(
                any(test[2] == "pass" for test in mutant["tests"]) for mutant in mutants
            )
        )


if __name__ == "__main__":
    unittest.main()
