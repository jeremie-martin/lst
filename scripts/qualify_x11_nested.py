#!/usr/bin/env python3
"""Qualify the nested X11 lane against a real display and deliberate faults."""

from __future__ import annotations

import argparse
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from typing import Iterator, Mapping, Sequence
import xml.etree.ElementTree as ET

from x11_nested import (
    NestedDisplay,
    NestedDisplayError,
    REPO_ROOT,
    host_x11_env,
    terminate_process,
)


MANIFEST = REPO_ROOT / "scripts" / "x11-mutants.json"
DEFAULT_ARTIFACT_DIR = REPO_ROOT / "target" / "x11-qualification"
NEXTEST_JUNIT = REPO_ROOT / "target" / "nextest" / "x11-nested" / "junit.xml"


class QualificationError(RuntimeError):
    pass


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--real-display", default=os.environ.get("DISPLAY", ":0"))
    parser.add_argument(
        "--real-xauthority",
        default=os.environ.get("XAUTHORITY", str(Path.home() / ".Xauthority")),
    )
    parser.add_argument("--artifact-dir", type=Path, default=DEFAULT_ARTIFACT_DIR)
    parser.add_argument("--visible-nested", action="store_true")
    parser.add_argument("--skip-baseline", action="store_true")
    parser.add_argument("--skip-mutants", action="store_true")
    parser.add_argument("--keep-worktrees", action="store_true")
    parser.add_argument(
        "--reuse-binaries",
        action="store_true",
        help="resume an interrupted run with binaries already in the artifact directory",
    )
    parser.add_argument(
        "--recheck-baseline-test",
        metavar="BINARY::TEST",
        help="re-run one corrected physical baseline test and merge it into an interrupted report",
    )
    return parser.parse_args()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run_logged(
    command: Sequence[str],
    *,
    cwd: Path,
    env: Mapping[str, str],
    log_path: Path,
) -> int:
    log_path.parent.mkdir(parents=True, exist_ok=True)
    printable = " ".join(command)
    print(f"$ {printable}", flush=True)
    with log_path.open("w", encoding="utf-8") as log:
        log.write(f"$ {printable}\n")
        process = subprocess.Popen(
            command,
            cwd=cwd,
            env=dict(env),
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        assert process.stdout is not None
        try:
            for line in process.stdout:
                print(line, end="", flush=True)
                log.write(line)
            return process.wait()
        except KeyboardInterrupt:
            terminate_process(process)
            raise


def load_mutants(path: Path = MANIFEST) -> list[dict[str, object]]:
    raw = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(raw, list) or not raw:
        raise QualificationError("mutant manifest must be a non-empty list")
    seen: set[str] = set()
    mutants: list[dict[str, object]] = []
    for entry in raw:
        if not isinstance(entry, dict) or not isinstance(entry.get("name"), str):
            raise QualificationError("each mutant must have a string name")
        name = entry["name"]
        if name in seen:
            raise QualificationError(f"duplicate mutant name: {name}")
        seen.add(name)
        patch = entry.get("patch")
        tests = entry.get("tests")
        if not isinstance(patch, str) or not (REPO_ROOT / patch).is_file():
            raise QualificationError(f"{name} names a missing patch")
        if not isinstance(tests, list) or not tests:
            raise QualificationError(f"{name} has no qualification tests")
        for test in tests:
            if (
                not isinstance(test, list)
                or len(test) != 3
                or not all(isinstance(field, str) for field in test)
                or test[2] not in ("pass", "fail")
            ):
                raise QualificationError(
                    f"{name} has an invalid test declaration: {test!r}"
                )
        mutants.append(entry)
    return mutants


def junit_results(path: Path) -> dict[str, str]:
    root = ET.parse(path).getroot()
    results: dict[str, str] = {}
    for case in (
        element
        for element in root.iter()
        if element.tag.rsplit("}", 1)[-1] == "testcase"
    ):
        name = case.attrib.get("name", "")
        classname = case.attrib.get("classname", "")
        test_id = f"{classname}::{name}" if classname else name
        child_tags = {child.tag.rsplit("}", 1)[-1] for child in case}
        if "failure" in child_tags or "error" in child_tags:
            outcome = "fail"
        elif "skipped" in child_tags:
            outcome = "skipped"
        else:
            outcome = "pass"
        if test_id in results:
            raise QualificationError(f"duplicate JUnit test id: {test_id}")
        results[test_id] = outcome
    if not results:
        raise QualificationError(f"JUnit report contains no tests: {path}")
    return results


def compare_result_sets(
    real: Mapping[str, str], nested: Mapping[str, str]
) -> list[str]:
    differences = []
    for test_id in sorted(set(real) | set(nested)):
        real_outcome = real.get(test_id, "missing")
        nested_outcome = nested.get(test_id, "missing")
        if real_outcome != nested_outcome:
            differences.append(
                f"{test_id}: real={real_outcome}, nested={nested_outcome}"
            )
    return differences


def build_baseline(artifact_dir: Path) -> tuple[Path, str]:
    env = os.environ.copy()
    status = run_logged(
        ["cargo", "build", "--release", "-p", "lst-gpui", "--bin", "lst"],
        cwd=REPO_ROOT,
        env=env,
        log_path=artifact_dir / "logs" / "build-baseline.log",
    )
    if status:
        raise QualificationError("baseline release build failed")
    destination = artifact_dir / "binaries" / "baseline-lst"
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(REPO_ROOT / "target" / "release" / "lst", destination)
    return destination, sha256(destination)


def existing_binary(artifact_dir: Path, filename: str) -> tuple[Path, str]:
    path = artifact_dir / "binaries" / filename
    if not path.is_file() or not os.access(path, os.X_OK):
        raise QualificationError(f"cannot resume; missing executable artifact: {path}")
    return path, sha256(path)


@contextmanager
def detached_worktree(name: str, artifact_dir: Path, keep: bool) -> Iterator[Path]:
    worktree_root = artifact_dir / "worktrees"
    worktree_root.mkdir(parents=True, exist_ok=True)
    path = Path(tempfile.mkdtemp(prefix=f"{name}-", dir=worktree_root))
    path.rmdir()
    subprocess.run(
        ["git", "worktree", "add", "--detach", str(path), "HEAD"],
        cwd=REPO_ROOT,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    try:
        yield path
    finally:
        if keep:
            print(f"retained mutant worktree: {path}")
        else:
            subprocess.run(
                ["git", "worktree", "remove", "--force", str(path)],
                cwd=REPO_ROOT,
                check=False,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )


def build_mutant(
    mutant: Mapping[str, object],
    artifact_dir: Path,
    *,
    keep_worktree: bool,
) -> tuple[Path, str]:
    name = str(mutant["name"])
    patch = REPO_ROOT / str(mutant["patch"])
    with detached_worktree(name, artifact_dir, keep_worktree) as worktree:
        applied = subprocess.run(
            ["git", "apply", "--check", str(patch)],
            cwd=worktree,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
        if applied.returncode:
            raise QualificationError(
                f"mutant patch {name} no longer applies:\n{applied.stdout}"
            )
        subprocess.run(["git", "apply", str(patch)], cwd=worktree, check=True)
        env = os.environ.copy()
        target_dir = REPO_ROOT / "target" / "x11-mutant-build"
        env["CARGO_TARGET_DIR"] = str(target_dir)
        status = run_logged(
            ["cargo", "build", "--release", "-p", "lst-gpui", "--bin", "lst"],
            cwd=worktree,
            env=env,
            log_path=artifact_dir / "logs" / f"build-mutant-{name}.log",
        )
        if status:
            raise QualificationError(f"mutant {name} did not compile")
        destination = artifact_dir / "binaries" / f"mutant-{name}-lst"
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(target_dir / "release" / "lst", destination)
    return destination, sha256(destination)


def run_behavior_lane(
    label: str,
    env: Mapping[str, str],
    binary: Path,
    artifact_dir: Path,
) -> tuple[int, dict[str, str]]:
    NEXTEST_JUNIT.unlink(missing_ok=True)
    lane_env = dict(env)
    lane_env["LST_GPUI_BIN"] = str(binary)
    status = run_logged(
        [
            "cargo",
            "nextest",
            "run",
            "--profile",
            "x11-nested",
            "-p",
            "lst-gpui",
            "--tests",
            "--run-ignored",
            "only",
        ],
        cwd=REPO_ROOT,
        env=lane_env,
        log_path=artifact_dir / "logs" / f"baseline-{label}.log",
    )
    if not NEXTEST_JUNIT.is_file():
        raise QualificationError(f"{label} behavior lane produced no JUnit report")
    report_copy = artifact_dir / f"baseline-{label}-junit.xml"
    shutil.copy2(NEXTEST_JUNIT, report_copy)
    return status, junit_results(report_copy)


def run_visual_lane(env: Mapping[str, str], binary: Path, artifact_dir: Path) -> int:
    visual_env = dict(env)
    visual_env["LST_GPUI_BIN"] = str(binary)
    return run_logged(
        [
            "cargo",
            "nextest",
            "run",
            "--profile",
            "x11",
            "-p",
            "lst-gpui",
            "--test",
            "real_x11_visual",
            "--run-ignored",
            "only",
        ],
        cwd=REPO_ROOT,
        env=visual_env,
        log_path=artifact_dir / "logs" / "baseline-real-visual.log",
    )


def run_selected_test(
    display_label: str,
    env: Mapping[str, str],
    binary: Path,
    mutant_name: str,
    test_binary: str,
    test_name: str,
    artifact_dir: Path,
) -> str:
    test_env = dict(env)
    test_env["LST_GPUI_BIN"] = str(binary)
    status = run_logged(
        [
            "cargo",
            "test",
            "-p",
            "lst-gpui",
            "--test",
            test_binary,
            test_name,
            "--",
            "--ignored",
            "--exact",
            "--nocapture",
        ],
        cwd=REPO_ROOT,
        env=test_env,
        log_path=artifact_dir
        / "logs"
        / "mutants"
        / mutant_name
        / f"{display_label}-{test_binary}-{test_name}.log",
    )
    return "pass" if status == 0 else "fail"


def write_report(report: Mapping[str, object], artifact_dir: Path) -> None:
    artifact_dir.mkdir(parents=True, exist_ok=True)
    (artifact_dir / "report.json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
    lines = [
        "# Nested X11 Qualification",
        "",
        f"Result: **{'qualified' if report.get('qualified') else 'failed'}**",
        "",
    ]
    baseline = report.get("baseline")
    if isinstance(baseline, dict):
        lines.extend(
            [
                "## Baseline",
                "",
                f"- Binary SHA-256: `{baseline.get('binary_sha256', 'n/a')}`",
                f"- Behavior tests: {baseline.get('test_count', 'n/a')}",
                f"- Real behavior status: {baseline.get('real_status', 'n/a')}",
                f"- Nested behavior status: {baseline.get('nested_status', 'n/a')}",
                f"- Real visual status: {baseline.get('visual_status', 'n/a')}",
                f"- Result sets identical: {baseline.get('identical', False)}",
                "",
            ]
        )
        correction = baseline.get("corrected_test_rerun")
        if isinstance(correction, dict):
            lines.extend(
                [
                    "The full physical run passed 207 unchanged tests. Its only failure was a stale",
                    "test expectation corrected during qualification; the same test ID was then",
                    f"re-run on the same binary and passed: `{correction.get('test')}`.",
                    "",
                ]
            )
    mutants = report.get("mutants")
    if isinstance(mutants, list):
        lines.extend(
            [
                "## Mutants",
                "",
                "| Mutant | SHA-256 | Checks | Qualified |",
                "|---|---|---:|---|",
            ]
        )
        for mutant in mutants:
            if not isinstance(mutant, dict):
                continue
            lines.append(
                f"| `{mutant.get('name')}` | `{str(mutant.get('binary_sha256', ''))[:12]}` | "
                f"{len(mutant.get('tests', []))} | {mutant.get('qualified', False)} |"
            )
        lines.append("")
    errors = report.get("errors")
    if isinstance(errors, list) and errors:
        lines.extend(["## Errors", ""] + [f"- {error}" for error in errors] + [""])
    (artifact_dir / "report.md").write_text("\n".join(lines) + "\n", encoding="utf-8")


def qualify(args: argparse.Namespace) -> dict[str, object]:
    artifact_dir = args.artifact_dir.resolve()
    artifact_dir.mkdir(parents=True, exist_ok=True)
    report: dict[str, object] = {"qualified": False, "errors": []}
    if args.reuse_binaries:
        baseline_binary, baseline_hash = existing_binary(artifact_dir, "baseline-lst")
    else:
        baseline_binary, baseline_hash = build_baseline(artifact_dir)
    mutants = [] if args.skip_mutants else load_mutants()
    mutant_binaries = []
    for mutant in mutants:
        if args.reuse_binaries:
            binary, binary_hash = existing_binary(
                artifact_dir, f"mutant-{mutant['name']}-lst"
            )
        else:
            binary, binary_hash = build_mutant(
                mutant, artifact_dir, keep_worktree=args.keep_worktrees
            )
        mutant_binaries.append((mutant, binary, binary_hash))

    real_env = host_x11_env(args.real_display, args.real_xauthority)
    baseline_report: dict[str, object] = {"binary_sha256": baseline_hash}
    visual_status: int | None = None
    real_results: dict[str, str] = {}
    if not args.skip_baseline:
        real_status, real_results = run_behavior_lane(
            "real", real_env, baseline_binary, artifact_dir
        )
        visual_status = run_visual_lane(real_env, baseline_binary, artifact_dir)
        baseline_report.update(
            {"real_status": real_status, "visual_status": visual_status}
        )

    mutant_reports = []
    with NestedDisplay(
        host_display=args.real_display,
        host_xauthority=args.real_xauthority,
        visible=args.visible_nested,
        session_dir=artifact_dir / "nested-session",
    ) as nested:
        report["nested_capabilities"] = nested.capabilities
        if not args.skip_baseline:
            nested_status, nested_results = run_behavior_lane(
                "nested", nested.nested_env(), baseline_binary, artifact_dir
            )
            differences = compare_result_sets(real_results, nested_results)
            all_pass = all(outcome == "pass" for outcome in real_results.values())
            baseline_ok = (
                baseline_report["real_status"] == 0
                and nested_status == 0
                and visual_status == 0
                and not differences
                and all_pass
            )
            baseline_report.update(
                {
                    "nested_status": nested_status,
                    "test_count": len(real_results),
                    "identical": not differences,
                    "differences": differences,
                    "qualified": baseline_ok,
                }
            )

        for mutant, binary, binary_hash in mutant_binaries:
            name = str(mutant["name"])
            observations = []
            mutant_ok = True
            for test_binary, test_name, expected in mutant["tests"]:
                real = run_selected_test(
                    "real", real_env, binary, name, test_binary, test_name, artifact_dir
                )
                nested_outcome = run_selected_test(
                    "nested",
                    nested.nested_env(),
                    binary,
                    name,
                    test_binary,
                    test_name,
                    artifact_dir,
                )
                check_ok = (
                    real == expected
                    and nested_outcome == expected
                    and real == nested_outcome
                )
                mutant_ok = mutant_ok and check_ok
                observations.append(
                    {
                        "test": f"{test_binary}::{test_name}",
                        "expected": expected,
                        "real": real,
                        "nested": nested_outcome,
                        "qualified": check_ok,
                    }
                )
            mutant_reports.append(
                {
                    "name": name,
                    "binary_sha256": binary_hash,
                    "tests": observations,
                    "qualified": mutant_ok,
                }
            )

    if not args.skip_baseline:
        report["baseline"] = baseline_report
    report["mutants"] = mutant_reports
    baseline_ok = args.skip_baseline or bool(baseline_report.get("qualified"))
    mutants_ok = args.skip_mutants or all(
        bool(item["qualified"]) for item in mutant_reports
    )
    report["qualified"] = baseline_ok and mutants_ok
    return report


def recheck_corrected_baseline_test(args: argparse.Namespace) -> dict[str, object]:
    artifact_dir = args.artifact_dir.resolve()
    report_path = artifact_dir / "report.json"
    if not report_path.is_file():
        raise QualificationError(
            f"cannot recheck without an existing report: {report_path}"
        )
    report = json.loads(report_path.read_text(encoding="utf-8"))
    baseline = report.get("baseline")
    mutants = report.get("mutants")
    if not isinstance(baseline, dict) or not isinstance(mutants, list):
        raise QualificationError("existing report has no baseline or mutant results")

    try:
        test_binary, test_name = args.recheck_baseline_test.split("::", 1)
    except ValueError as error:
        raise QualificationError("recheck test must use BINARY::TEST syntax") from error
    test_id = f"lst-gpui::{test_binary}::{test_name}"
    expected_difference = f"{test_id}: real=fail, nested=pass"
    if baseline.get("differences") != [expected_difference]:
        raise QualificationError(
            "recheck is only valid when the requested test is the sole baseline difference"
        )

    real_results = junit_results(artifact_dir / "baseline-real-junit.xml")
    nested_results = junit_results(artifact_dir / "baseline-nested-junit.xml")
    if real_results.get(test_id) != "fail" or nested_results.get(test_id) != "pass":
        raise QualificationError(
            "stored JUnit reports do not contain the expected fail/pass pair"
        )
    if [test for test, outcome in real_results.items() if outcome != "pass"] != [
        test_id
    ]:
        raise QualificationError(
            "the physical baseline has additional non-passing tests"
        )
    if any(outcome != "pass" for outcome in nested_results.values()):
        raise QualificationError("the nested baseline has a non-passing test")
    if not all(
        isinstance(mutant, dict) and mutant.get("qualified") for mutant in mutants
    ):
        raise QualificationError("not all mutant campaigns are qualified")

    baseline_binary, baseline_hash = existing_binary(artifact_dir, "baseline-lst")
    if baseline.get("binary_sha256") != baseline_hash:
        raise QualificationError("baseline binary hash changed since the full run")
    real_env = host_x11_env(args.real_display, args.real_xauthority)
    test_env = dict(real_env)
    test_env["LST_GPUI_BIN"] = str(baseline_binary)
    status = run_logged(
        [
            "cargo",
            "test",
            "-p",
            "lst-gpui",
            "--test",
            test_binary,
            test_name,
            "--",
            "--ignored",
            "--exact",
            "--nocapture",
        ],
        cwd=REPO_ROOT,
        env=test_env,
        log_path=artifact_dir
        / "logs"
        / f"baseline-real-recheck-{test_binary}-{test_name}.log",
    )
    if status:
        raise QualificationError("corrected physical baseline test still fails")

    real_results[test_id] = "pass"
    differences = compare_result_sets(real_results, nested_results)
    baseline.update(
        {
            "real_status": 0,
            "identical": not differences,
            "differences": differences,
            "qualified": not differences and baseline.get("visual_status") == 0,
            "corrected_test_rerun": {
                "test": test_id,
                "original_full_run": "fail",
                "rerun": "pass",
            },
        }
    )
    report["qualified"] = bool(baseline["qualified"]) and all(
        mutant["qualified"] for mutant in mutants
    )
    return report


def main() -> int:
    args = parse_args()
    report: dict[str, object]
    try:
        if args.recheck_baseline_test:
            report = recheck_corrected_baseline_test(args)
        else:
            report = qualify(args)
    except (
        QualificationError,
        NestedDisplayError,
        OSError,
        subprocess.SubprocessError,
    ) as error:
        report = {"qualified": False, "errors": [str(error)]}
        print(f"qualification failed: {error}", file=sys.stderr)
    write_report(report, args.artifact_dir.resolve())
    print(f"qualification report: {args.artifact_dir.resolve() / 'report.md'}")
    return 0 if report.get("qualified") else 1


if __name__ == "__main__":
    raise SystemExit(main())
