#!/usr/bin/env python3
"""Run lst's real X11 behavior suite inside an off-screen Xephyr display."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import sys

from x11_nested import NestedDisplay, NestedDisplayError, REPO_ROOT


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host-display", default=os.environ.get("DISPLAY", ":0"))
    parser.add_argument(
        "--host-xauthority",
        default=os.environ.get("XAUTHORITY", str(Path.home() / ".Xauthority")),
    )
    parser.add_argument(
        "--visible",
        action="store_true",
        help="leave the Xephyr host window visible for diagnostics",
    )
    parser.add_argument(
        "--probe",
        action="store_true",
        help="run two focused capability tests instead of the full lane",
    )
    parser.add_argument(
        "--keep-session",
        action="store_true",
        help="retain Xephyr/lwm logs under target/x11-nested",
    )
    parser.add_argument(
        "nextest_args",
        nargs=argparse.REMAINDER,
        help="arguments after -- replace the default command",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    session_dir = REPO_ROOT / "target" / "x11-nested" / f"session-{os.getpid()}"
    commands = []
    if args.nextest_args:
        command = (
            args.nextest_args[1:] if args.nextest_args[0] == "--" else args.nextest_args
        )
        commands.append(command)
    elif args.probe:
        commands.extend(
            [
                [
                    "cargo",
                    "test",
                    "-p",
                    "lst-gpui",
                    "--test",
                    "real_x11_daily_driver",
                    "standard_alt_shift_up_duplicates_line_above",
                    "--",
                    "--ignored",
                    "--exact",
                    "--nocapture",
                ],
                [
                    "cargo",
                    "test",
                    "-p",
                    "lst-gpui",
                    "--test",
                    "real_x11_daily_driver",
                    "app_menu_does_not_dim_the_editor",
                    "--",
                    "--ignored",
                    "--exact",
                    "--nocapture",
                ],
            ]
        )
    else:
        commands.append(
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
            ]
        )

    try:
        with NestedDisplay(
            host_display=args.host_display,
            host_xauthority=args.host_xauthority,
            visible=args.visible,
            session_dir=session_dir,
        ) as nested:
            print(json.dumps(nested.capabilities, indent=2), flush=True)
            for command in commands:
                status = nested.run(command)
                if status != 0:
                    print(
                        f"nested command failed with status {status}; logs: {session_dir}",
                        file=sys.stderr,
                    )
                    return status
    except NestedDisplayError as error:
        print(f"nested X11 setup failed: {error}; logs: {session_dir}", file=sys.stderr)
        return 2

    if not args.keep_session:
        import shutil

        shutil.rmtree(session_dir, ignore_errors=True)
    else:
        print(f"nested session logs: {session_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
