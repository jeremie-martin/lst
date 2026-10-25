"""Exercise nested/qualification process ownership with real process groups."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
import x11_nested  # noqa: E402


class ProcessCleanupTests(unittest.TestCase):
    def alive(self, pid):
        try:
            return Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()[0] != "Z"
        except FileNotFoundError:
            return False

    def test_exited_leader_does_not_leave_its_process_group_running(self):
        leader = subprocess.Popen(
            [sys.executable, "-c", "import subprocess; p = subprocess.Popen(['sleep', '60'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL); print(p.pid, flush=True)"],
            stdout=subprocess.PIPE, text=True, start_new_session=True,
        )
        try:
            descendant = int(leader.stdout.readline())
            self.assertEqual(leader.wait(timeout=5), 0)
            self.assertTrue(self.alive(descendant))
            x11_nested.terminate_process(leader)
            deadline = time.monotonic() + 3
            while self.alive(descendant) and time.monotonic() < deadline:
                time.sleep(.01)
            self.assertFalse(self.alive(descendant))
        finally:
            x11_nested.terminate_process(leader)
            leader.stdout.close()

    def test_termination_signals_unwind_both_command_runners(self):
        child_code = """
import json, os, pathlib, subprocess, sys, time
child = subprocess.Popen([sys.executable, '-c', 'import signal, time; signal.signal(signal.SIGTERM, signal.SIG_IGN); print("ready", flush=True); time.sleep(60)'], stdout=subprocess.PIPE, text=True)
assert child.stdout.readline().strip() == "ready"
ready = pathlib.Path(sys.argv[1])
ready.with_suffix(".tmp").write_text(json.dumps([os.getpid(), child.pid]))
ready.with_suffix(".tmp").rename(ready)
time.sleep(60)
"""
        worker_code = """
import os, pathlib, sys
from x11_nested import NestedDisplay, termination_signals
from qualify_x11_nested import run_logged
with termination_signals():
    command = [sys.executable, '-c', sys.argv[3], sys.argv[2]]
    if sys.argv[1] == 'nested':
        nested = NestedDisplay(host_display=':unused', host_xauthority=None, visible=True)
        nested.display = ':unused'
        try:
            nested.run(command)
        finally:
            nested.close()
    else:
        run_logged(command, cwd=pathlib.Path.cwd(), env=os.environ, log_path=pathlib.Path(sys.argv[2]).with_suffix('.log'))
"""
        for runner in ["nested", "logged"]:
            for sig in [signal.SIGTERM, signal.SIGHUP]:
                with self.subTest(runner=runner, signal=sig), tempfile.TemporaryDirectory() as directory:
                    ready = Path(directory) / "ready.json"
                    env = dict(os.environ, PYTHONPATH=str(SCRIPTS))
                    worker = subprocess.Popen([sys.executable, "-c", worker_code, runner, str(ready), child_code], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
                    pids = []
                    try:
                        deadline = time.monotonic() + 5
                        while time.monotonic() < deadline and not ready.exists() and worker.poll() is None:
                            time.sleep(.01)
                        self.assertTrue(ready.exists(), "worker did not start its subprocesses")
                        pids = json.loads(ready.read_text())
                        worker.send_signal(sig)
                        self.assertEqual(worker.wait(timeout=5), 128 + sig, worker.stderr.read())
                        deadline = time.monotonic() + 3
                        while any(self.alive(pid) for pid in pids) and time.monotonic() < deadline:
                            time.sleep(.01)
                        self.assertFalse(any(self.alive(pid) for pid in pids), "owned process survived termination")
                    finally:
                        if worker.poll() is None:
                            worker.kill()
                        worker.wait(timeout=5)
                        worker.stderr.close()
                        for pid in pids:
                            if self.alive(pid):
                                os.kill(pid, signal.SIGKILL)


if __name__ == "__main__":
    unittest.main()
