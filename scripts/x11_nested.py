#!/usr/bin/env python3
"""Lifecycle and host-isolation support for nested lst X11 tests."""

from __future__ import annotations

import contextlib
import fcntl
import os
from pathlib import Path
import re
import select
import shutil
import signal
import subprocess
import tempfile
import time
from typing import Mapping, Sequence

try:
    from Xlib import X
    from Xlib import display as xdisplay
except ImportError:
    X = None
    xdisplay = None


REPO_ROOT = Path(__file__).resolve().parents[1]
LWM_FIXTURE = REPO_ROOT / "scripts" / "fixtures" / "lwm-x11.toml"
REQUIRED_COMMANDS = (
    "Xephyr",
    "lwm",
    "wmctrl",
    "xprop",
    "xwininfo",
    "xdpyinfo",
    "xclip",
)
REQUIRED_EXTENSIONS = ("DAMAGE", "XTEST", "XKEYBOARD")
DISPLAY_ALLOCATION_LOCK = Path(tempfile.gettempdir()) / "lst-x11-nested-display.lock"


class NestedDisplayError(RuntimeError):
    pass


@contextlib.contextmanager
def display_allocation_lock():
    """Serialize Xephyr's non-atomic `-displayfd` probe-and-bind window."""
    with DISPLAY_ALLOCATION_LOCK.open("a+b") as lock:
        fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(lock.fileno(), fcntl.LOCK_UN)


def command_path(name: str) -> str:
    path = shutil.which(name)
    if path is None:
        raise NestedDisplayError(f"required command is unavailable: {name}")
    return path


def run_text(
    command: Sequence[str],
    *,
    env: Mapping[str, str] | None = None,
    check: bool = True,
    timeout: float = 10,
) -> str:
    completed = subprocess.run(
        command,
        cwd=REPO_ROOT,
        env=None if env is None else dict(env),
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
    )
    if check and completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise NestedDisplayError(
            f"{' '.join(command)} failed ({completed.returncode}): {detail}"
        )
    return completed.stdout


def host_x11_env(display: str, xauthority: str | None) -> dict[str, str]:
    env = os.environ.copy()
    env["DISPLAY"] = display
    if xauthority:
        env["XAUTHORITY"] = xauthority
    else:
        env.pop("XAUTHORITY", None)
    return env


def parse_active_window(output: str) -> str | None:
    match = re.search(r"#\s*(0x[0-9a-fA-F]+)", output)
    if not match or int(match.group(1), 16) == 0:
        return None
    return match.group(1).lower()


def active_window(env: Mapping[str, str]) -> str | None:
    return parse_active_window(
        run_text(["xprop", "-root", "_NET_ACTIVE_WINDOW"], env=env)
    )


def parse_xwininfo(output: str) -> dict[str, int | str]:
    def number(label: str) -> int:
        match = re.search(rf"^\s*{re.escape(label)}:\s*(-?\d+)", output, re.MULTILINE)
        if not match:
            raise NestedDisplayError(f"xwininfo omitted {label}")
        return int(match.group(1))

    state = re.search(r"^\s*Map State:\s*(\S+)", output, re.MULTILINE)
    if not state:
        raise NestedDisplayError("xwininfo omitted Map State")
    return {
        "x": number("Absolute upper-left X"),
        "y": number("Absolute upper-left Y"),
        "width": number("Width"),
        "height": number("Height"),
        "map_state": state.group(1),
    }


def terminate_process(
    process: subprocess.Popen[object] | None, timeout: float = 3
) -> None:
    if process is None or process.poll() is not None:
        return
    with contextlib.suppress(ProcessLookupError):
        os.killpg(process.pid, signal.SIGTERM)
    try:
        process.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        with contextlib.suppress(ProcessLookupError):
            os.killpg(process.pid, signal.SIGKILL)
        process.wait(timeout=timeout)


class NestedDisplay:
    def __init__(
        self,
        *,
        host_display: str,
        host_xauthority: str | None,
        visible: bool = False,
        session_dir: Path | None = None,
    ) -> None:
        self.host_env = host_x11_env(host_display, host_xauthority)
        self.visible = visible
        self._owned_session_dir = session_dir is None
        self.session_dir = session_dir or Path(
            tempfile.mkdtemp(prefix="lst-x11-nested-")
        )
        self.session_dir.mkdir(parents=True, exist_ok=True)
        self.display: str | None = None
        self.container_window: str | None = None
        self._host_connection: object | None = None
        self._host_window: object | None = None
        self.host_active_before: str | None = None
        self.xephyr: subprocess.Popen[object] | None = None
        self.wm: subprocess.Popen[object] | None = None
        self.capabilities: dict[str, object] = {}

    def __enter__(self) -> "NestedDisplay":
        for command in REQUIRED_COMMANDS:
            command_path(command)
        if not self.visible and (X is None or xdisplay is None):
            raise NestedDisplayError(
                "hidden nested runs require the python-xlib package"
            )
        if not LWM_FIXTURE.is_file():
            raise NestedDisplayError(f"missing nested lwm fixture: {LWM_FIXTURE}")

        self.host_active_before = active_window(self.host_env)
        try:
            if not self.visible:
                self._create_offscreen_container()
            # Xephyr's `-displayfd` allocation can race another Xephyr: both
            # processes may report the same free display before either binds
            # its socket. Keep allocation locked until the chosen server is
            # accepting connections, after which another run will choose a
            # different display.
            with display_allocation_lock():
                self._start_xephyr()
                self._wait_for_display()
            if not self.visible:
                self._restore_host_focus()
            if not self.visible:
                self._verify_offscreen_container()
            self._start_window_manager()
            self._record_capabilities()
            return self
        except BaseException:
            self.close()
            raise

    def __exit__(self, _type: object, _value: object, _traceback: object) -> None:
        self.close()

    def nested_env(self, extra: Mapping[str, str] | None = None) -> dict[str, str]:
        if self.display is None:
            raise NestedDisplayError("nested display has not started")
        env = os.environ.copy()
        env["DISPLAY"] = self.display
        env.pop("XAUTHORITY", None)
        if extra:
            env.update(extra)
        return env

    def run(
        self, command: Sequence[str], *, extra_env: Mapping[str, str] | None = None
    ) -> int:
        process = subprocess.Popen(
            command,
            cwd=REPO_ROOT,
            env=self.nested_env(extra_env),
            start_new_session=True,
        )
        try:
            return process.wait()
        except KeyboardInterrupt:
            terminate_process(process)
            raise

    def close(self) -> None:
        terminate_process(self.wm)
        terminate_process(self.xephyr)
        self.wm = None
        self.xephyr = None
        if self._host_window is not None:
            with contextlib.suppress(Exception):
                self._host_window.destroy()
                self._host_connection.sync()
        if self._host_connection is not None:
            with contextlib.suppress(Exception):
                self._host_connection.close()
        self._host_window = None
        self._host_connection = None
        if self._owned_session_dir:
            shutil.rmtree(self.session_dir, ignore_errors=True)

    def _create_offscreen_container(self) -> None:
        previous_display = os.environ.get("DISPLAY")
        previous_xauthority = os.environ.get("XAUTHORITY")
        os.environ["DISPLAY"] = self.host_env["DISPLAY"]
        if "XAUTHORITY" in self.host_env:
            os.environ["XAUTHORITY"] = self.host_env["XAUTHORITY"]
        else:
            os.environ.pop("XAUTHORITY", None)
        try:
            connection = xdisplay.Display(self.host_env["DISPLAY"])
        finally:
            if previous_display is None:
                os.environ.pop("DISPLAY", None)
            else:
                os.environ["DISPLAY"] = previous_display
            if previous_xauthority is None:
                os.environ.pop("XAUTHORITY", None)
            else:
                os.environ["XAUTHORITY"] = previous_xauthority

        screen = connection.screen()
        token = f"lst-x11-xephyr-{os.getpid()}"
        window = screen.root.create_window(
            -20000,
            0,
            1920,
            1080,
            0,
            screen.root_depth,
            X.InputOutput,
            X.CopyFromParent,
            background_pixel=screen.black_pixel,
            override_redirect=True,
        )
        window.set_wm_name(token)
        window.set_wm_class(token, token)
        window.map()
        connection.sync()
        self._host_connection = connection
        self._host_window = window
        self.container_window = f"0x{window.id:x}"

    def _restore_host_focus(self) -> None:
        if (
            self.host_active_before
            and active_window(self.host_env) != self.host_active_before
        ):
            run_text(["wmctrl", "-i", "-a", self.host_active_before], env=self.host_env)
        if (
            self.host_active_before
            and active_window(self.host_env) != self.host_active_before
        ):
            raise NestedDisplayError(
                "Xephyr stole host focus and it could not be restored"
            )

    def _start_xephyr(self) -> None:
        read_fd, write_fd = os.pipe()
        token = f"lst-x11-xephyr-{os.getpid()}"
        log = (self.session_dir / "xephyr.log").open("wb")
        command = [
            "Xephyr",
            "-displayfd",
            str(write_fd),
            "-screen",
            "1920x1080x24",
            "-dpi",
            "96",
            "-ac",
            "-noreset",
            "-sw-cursor",
            "-no-host-grab",
            "-name",
            token,
            "-title",
            token,
        ]
        if self.container_window:
            command.extend(["-parent", str(int(self.container_window, 16))])
        try:
            self.xephyr = subprocess.Popen(
                command,
                env=self.host_env,
                stdout=log,
                stderr=subprocess.STDOUT,
                pass_fds=(write_fd,),
                start_new_session=True,
            )
        finally:
            os.close(write_fd)
            log.close()
        ready, _, _ = select.select([read_fd], [], [], 10)
        if not ready:
            os.close(read_fd)
            raise NestedDisplayError(
                "Xephyr did not allocate a display within 10 seconds"
            )
        number = os.read(read_fd, 32).decode("ascii", errors="replace").strip()
        os.close(read_fd)
        if not number.isdigit():
            raise NestedDisplayError(
                f"Xephyr returned invalid display number: {number!r}"
            )
        self.display = f":{number}"

    def _wait_for_display(self) -> None:
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if self.xephyr is not None and self.xephyr.poll() is not None:
                raise NestedDisplayError(
                    f"Xephyr exited early with status {self.xephyr.returncode}"
                )
            output = run_text(
                ["xdpyinfo"], env=self.nested_env(), check=False, timeout=2
            )
            if "name of display" in output:
                return
            time.sleep(0.05)
        raise NestedDisplayError("nested X display was not ready within 10 seconds")

    def _verify_offscreen_container(self) -> None:
        if not self.container_window:
            raise NestedDisplayError("off-screen Xephyr container was not created")

        info = parse_xwininfo(
            run_text(["xwininfo", "-id", self.container_window], env=self.host_env)
        )
        if info["map_state"] != "IsViewable":
            raise NestedDisplayError(f"off-screen Xephyr is not mapped: {info}")
        root = parse_xwininfo(run_text(["xwininfo", "-root"], env=self.host_env))
        offscreen = (
            int(info["x"]) + int(info["width"]) <= 0
            or int(info["y"]) + int(info["height"]) <= 0
            or int(info["x"]) >= int(root["width"])
            or int(info["y"]) >= int(root["height"])
        )
        if not offscreen:
            raise NestedDisplayError(
                f"Xephyr container is visible on the host display: {info}"
            )

        host_active_after = active_window(self.host_env)
        if self.host_active_before and host_active_after != self.host_active_before:
            raise NestedDisplayError(
                "creating the hidden Xephyr container changed host focus "
                f"({self.host_active_before} -> {host_active_after})"
            )

        self.capabilities["parking"] = {
            "container_window": self.container_window,
            "mode": "override-redirect-parent",
            "geometry": info,
            "host_active_window": self.host_active_before,
        }

    def _start_window_manager(self) -> None:
        log = (self.session_dir / "lwm.log").open("wb")
        self.wm = subprocess.Popen(
            ["lwm", str(LWM_FIXTURE)],
            env=self.nested_env(),
            stdout=log,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )
        log.close()
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if self.wm.poll() is not None:
                raise NestedDisplayError(
                    f"nested lwm exited early with status {self.wm.returncode}"
                )
            output = run_text(
                ["xprop", "-root", "_NET_SUPPORTING_WM_CHECK"],
                env=self.nested_env(),
                check=False,
            )
            if "window id" in output:
                return
            time.sleep(0.05)
        raise NestedDisplayError(
            "nested lwm did not publish readiness within 10 seconds"
        )

    def _record_capabilities(self) -> None:
        output = run_text(["xdpyinfo"], env=self.nested_env())
        missing = [
            extension for extension in REQUIRED_EXTENSIONS if extension not in output
        ]
        if missing:
            raise NestedDisplayError(
                f"nested display lacks required extensions: {', '.join(missing)}"
            )
        dimensions = re.search(r"dimensions:\s*(\d+)x(\d+) pixels", output)
        resolution = re.search(r"resolution:\s*(\d+)x(\d+) dots per inch", output)
        self.capabilities.update(
            {
                "display": self.display,
                "dimensions": list(map(int, dimensions.groups()))
                if dimensions
                else None,
                "resolution": list(map(int, resolution.groups()))
                if resolution
                else None,
                "extensions": list(REQUIRED_EXTENSIONS),
                "window_manager": "lwm",
            }
        )
