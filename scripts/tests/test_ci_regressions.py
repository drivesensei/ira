"""Behavioral checks for the CI launcher; no Cargo/native execution."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest

path = Path(os.environ.get("IRA_CI_RUNNER_UNDER_TEST", "scripts/ci_regressions.py")).resolve()
spec = importlib.util.spec_from_file_location("ci_regressions", path)
launcher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(launcher)

class LauncherTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="ira-ci-launcher-review-"))
        # Retain this owned fixture; do not remove the user's app fixtures.
        self.original_popen = launcher.subprocess.Popen
        def record(event):
            with open(os.environ["IRA_CI_CHILD_REGISTRY"], "a") as output:
                output.write(json.dumps(event) + "\n")
                output.flush()
                os.fsync(output.fileno())
        def tracked_popen(*args, **kwargs):
            process = self.original_popen(*args, **kwargs)
            registry = os.environ.get("IRA_CI_CHILD_REGISTRY")
            if registry and kwargs.get("start_new_session"):
                identity = subprocess.run(
                    ["ps", "-p", str(process.pid), "-o", "pid=,pgid=,uid=,lstart="],
                    capture_output=True, text=True, timeout=1).stdout.strip()
                record({"event": "spawn", "pid": process.pid, "pgid": process.pid,
                        "identity": identity})
                original_wait = process.wait
                completed = False
                def tracked_wait(*args, **kwargs):
                    nonlocal completed
                    result = original_wait(*args, **kwargs)
                    if not completed:
                        record({"event": "reaped", "pid": process.pid, "exit": result})
                        completed = True
                    return result
                process.wait = tracked_wait
            return process
        launcher.subprocess.Popen = tracked_popen

    def tearDown(self):
        launcher.subprocess.Popen = self.original_popen

    def test_actual_child_receives_unique_synthetic_paths_and_scrubbed_overrides(self):
        parent = dict(os.environ, IRA_TEST_FFMPEG="/host/poison",
                      IRA_THUMBNAIL_CACHE_DIR="/host/poison", HOME="/host/poison",
                      CARGO_HOME=str(self.root / "toolchain/cargo"),
                      RUSTUP_HOME=str(self.root / "toolchain/rustup"))
        roots = [self.root / "one", self.root / "two"]
        for root in roots:
            env = launcher.child_environment(root, parent)
            probe = (
                "import os,pathlib; "
                "assert 'IRA_TEST_FFMPEG' not in os.environ; "
                "root=pathlib.Path(os.environ['HOME']).parent; "
                "keys=['HOME','CFFIXED_USER_HOME','USERPROFILE','APPDATA',"
                "'LOCALAPPDATA','XDG_CONFIG_HOME','XDG_CACHE_HOME',"
                "'XDG_DATA_HOME','TMPDIR','TEMP','TMP','IRA_THUMBNAIL_CACHE_DIR']; "
                "assert all(pathlib.Path(os.environ[k]).is_relative_to(root) for k in keys); "
                "pathlib.Path(os.environ['XDG_CONFIG_HOME'],'probe').write_text('owned')"
            )
            launcher.execute([sys.executable, "-c", probe], self.root, env, 2, True)
            self.assertEqual((root / "config/probe").read_text(), "owned")
            self.assertEqual(env["CARGO_HOME"], parent["CARGO_HOME"])
        self.assertNotEqual(launcher.child_environment(roots[0], parent)["TMPDIR"],
                            launcher.child_environment(roots[1], parent)["TMPDIR"])

    def test_artifact_selection_rejects_wrong_package_non_test_and_ambiguity(self):
        paths = [self.root / "one", self.root / "two"]
        for path in paths:
            path.write_bytes(b"fixture")
        def artifact(path, package="selected", test=True):
            return dict(reason="compiler-artifact", package_id=package,
                        target=dict(name="contract", kind=["test"]),
                        profile=dict(test=test), executable=str(path))
        messages = [artifact(paths[0], "other"), artifact(paths[1], test=False),
                    artifact(paths[0])]
        self.assertEqual(launcher.select_executable(messages, "selected", "contract", "test"),
                         paths[0])
        with self.assertRaises(RuntimeError):
            launcher.select_executable(messages + [artifact(paths[1])],
                                       "selected", "contract", "test")
        with self.assertRaises(RuntimeError):
            launcher.select_executable(messages[:2], "selected", "contract", "test")

    def test_actual_subprocess_failure_is_propagated(self):
        with self.assertRaisesRegex(RuntimeError, "Exit 7"):
            launcher.execute([sys.executable, "-c", "raise SystemExit(7)"],
                             self.root, os.environ, 2, True)

    def test_actual_timeout_terminates_owned_descendant_and_closes_pipes(self):
        marker = self.root / "child-started"
        child = (
            "import pathlib,time; pathlib.Path(" + repr(str(marker)) +
            ").write_text('started'); time.sleep(10)"
        )
        parent = (
            "import subprocess,sys,time; subprocess.Popen([sys.executable,'-c'," +
            repr(child) + "]); time.sleep(10)"
        )
        started = time.monotonic()
        with self.assertRaisesRegex(RuntimeError, "Timed out"):
            # test_child=False specifically exercises Cargo/metadata group policy.
            launcher.execute([sys.executable, "-c", parent], self.root,
                             os.environ, 0.4, False)
        self.assertTrue(marker.is_file(), "Owned descendant actually started")
        self.assertLess(time.monotonic() - started, 4)
        # Its inherited pipes had to close for bounded communicate() to return.

    def test_platform_allowlists_exclude_unix_bodies_on_windows(self):
        # Guards platform meaning; executable --list separately rejects cfg omissions.
        windows = launcher.selected_targets("harness", True)
        unix = launcher.selected_targets("harness", False)
        self.assertEqual(sum(len(names) for _, _, names in windows), 9)
        self.assertEqual(sum(len(names) for _, _, names in unix), 15)
        self.assertTrue(all(target != "f002_staging_isolation_review"
                            for _, target, _ in windows))

if __name__ == "__main__":
    unittest.main()
