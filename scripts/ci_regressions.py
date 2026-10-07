#!/usr/bin/env python3
"""Run an explicit regression allowlist under synthetic application paths.

Cargo compilation uses the installed toolchain environment. Only the selected
test executables receive synthetic application configuration. This is environment
isolation, not an OS sandbox or proof of native/whole-suite parity.
Windows dirs-next uses native known folders; HOME/APPDATA do not redirect
them. Selected persistence bodies explicitly supply state/bookmark fixture
paths. Broad tests and native quit/retry require separate validation plans.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time

CORE = [
    "application::persistence_tests::checked_barrier_retains_write_failure_across_later_barriers_and_actor_stop_retry",
    "application::persistence_tests::later_success_does_not_let_stale_retry_overwrite_newer_snapshot",
    "application::persistence_tests::checked_receipt_aggregates_state_and_bookmarks_and_legacy_barrier_is_only_processing",
]
ROOT = ["main_exit_settlement_tests::" + name for name in [
    "actual_finish_exit_waits_real_transfer_before_four_case_save_cleanup_matrix",
    "actual_finish_exit_waits_hidden_real_delete_and_never_ticks",
    "lost_real_worker_receipt_skips_save_and_preserves_settlement_error_over_cleanup",
    "prior_lost_receipt_cannot_release_later_live_hidden_worker",
    "completed_prior_real_batch_does_not_discharge_live_hidden_batch",
]]
DESKTOP = [
    "latest_snapshot_coalesces_only_snapshot_values",
    "native_color_preserves_rgb_named_indexed_and_reset_resolution",
    "geometry_rejects_corrupt_nonfinite_and_unbounded_values",
]
LOGIC = [
    "process_observation_assertions_are_compared",
    "normalizer_preserves_text_after_csi_tilde_final",
    "repeated_expected_observations_are_all_checked",
    "readiness_uses_the_declared_process_observation",
    "golden_candidate_contains_captured_observations_and_input_evidence",
]
CAPTURE = [
    "s13_capture_uses_actual_observation_not_trace_expectation",
    "s13_byte_observations_round_trip_nul_and_invalid_utf8",
    "s13_paths_round_trip_without_unicode_normalization_or_loss",
    "s13_bundle_order_is_stable_and_manifest_is_published_last",
]
STAGING = [
    "metadata_symlink_must_not_write_outside_staging",
    "staging_parent_and_payload_directory_symlinks_are_rejected",
    "preexisting_metadata_manifest_and_pending_artifacts_are_immutable",
    "legacy_trace_staging_rejects_metadata_symlink_without_external_writes",
    "concurrent_capture_has_exactly_one_exclusive_owner",
]


def selected_targets(scope, windows):
    if scope == "headless":
        return [("crates/core/Cargo.toml", None, CORE), ("Cargo.toml", None, ROOT)]
    if scope == "desktop":
        return [("desktop/Cargo.toml", "runtime_contract", DESKTOP)]
    logic = LOGIC + ([] if windows else ["symlink_parent_escape_has_no_external_side_effect"])
    targets = [("tools/ira-parity/Cargo.toml", "f002_logic", logic),
               ("tools/ira-parity/Cargo.toml", "f002_capture_bundle_contract", CAPTURE)]
    if not windows:
        targets.append(("tools/ira-parity/Cargo.toml", "f002_staging_isolation_review", STAGING))
    return targets


def child_environment(root, parent):
    root = root.resolve()
    env = {k: v for k, v in parent.items()
           if not k.startswith("IRA_") and k not in ("FFMPEG", "PDFTOPPM")}
    home = root / "home"
    locations = {
        "HOME": home, "CFFIXED_USER_HOME": home, "USERPROFILE": home,
        "XDG_CONFIG_HOME": root / "config", "XDG_CACHE_HOME": root / "cache",
        "XDG_DATA_HOME": root / "data", "APPDATA": root / "roaming",
        "LOCALAPPDATA": root / "local", "TMPDIR": root / "tmp",
        "TEMP": root / "tmp", "TMP": root / "tmp",
        "IRA_THUMBNAIL_CACHE_DIR": root / "thumbnails",
    }
    for key, path in locations.items():
        path.mkdir(parents=True, exist_ok=True)
        env[key] = str(path)
    (home / "Library/Application Support/ira").mkdir(parents=True, exist_ok=True)
    (home / "Library/Caches").mkdir(parents=True, exist_ok=True)
    # rustc version queries in the harness still need the installed toolchain.
    for key, leaf in (("CARGO_HOME", ".cargo"), ("RUSTUP_HOME", ".rustup")):
        value = parent.get(key)
        if not value:
            original_home = parent.get("HOME") or parent.get("USERPROFILE")
            if not original_home:
                raise RuntimeError("Cannot resolve installed toolchain home")
            value = str(Path(original_home) / leaf)
        env[key] = str(Path(value).resolve())
    return env


def unix_child():
    import resource
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    soft, hard = resource.getrlimit(resource.RLIMIT_CORE)
    os.write(2, f"ci child RLIMIT_CORE={soft},{hard}\n".encode())


def execute(argv, cwd, env, timeout, test_child=False):
    # Compilation and metadata own process groups too: killing only Cargo can
    # leave compiler/build-script descendants holding pipes or writing output.
    options = ({"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP}
               if os.name == "nt" else {"start_new_session": True})
    if test_child and os.name != "nt":
        options["preexec_fn"] = unix_child
    process = subprocess.Popen(argv, cwd=cwd, env=env, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, text=True, **options)
    try:
        output, errors = process.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        termination_errors = []
        if os.name == "nt":
            try:
                killed = subprocess.run(
                    ["taskkill", "/PID", str(process.pid), "/T", "/F"],
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                    check=False, timeout=3)
                if killed.returncode and process.poll() is None:
                    termination_errors.append("taskkill did not confirm termination")
            except (OSError, subprocess.TimeoutExpired) as error:
                termination_errors.append(str(error))
            if process.poll() is None:
                try:
                    process.kill()
                except OSError as error:
                    termination_errors.append(str(error))
        else:
            import signal
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass  # The owned group already disappeared.
            except OSError as error:
                termination_errors.append(str(error))
                try:
                    process.kill()
                except OSError as error:
                    termination_errors.append(str(error))
        try:
            output, errors = process.communicate(timeout=3)
        except subprocess.TimeoutExpired as error:
            # Never block indefinitely on descendant-held pipes after timeout.
            process.stdout.close()
            process.stderr.close()
            raise RuntimeError(
                f"Timed out; termination/reap NOT_VERIFIED for PID {process.pid}: {argv}"
            ) from error
        print(output, end="")
        print(errors, end="", file=sys.stderr)
        detail = "; ".join(termination_errors)
        raise RuntimeError(f"Timed out: {argv}; {detail}")
    print(errors, end="", file=sys.stderr)
    if process.returncode:
        print(output, end="")
        raise RuntimeError(f"Exit {process.returncode}: {argv}")
    return output


def select_executable(messages, package_id, target_name, kind):
    matches = []
    for message in messages:
        if (message.get("reason") == "compiler-artifact"
                and message.get("package_id") == package_id
                and message.get("target", {}).get("name") == target_name
                and kind in message.get("target", {}).get("kind", [])
                and message.get("profile", {}).get("test") is True
                and message.get("executable")):
            matches.append(Path(message["executable"]).resolve())
    matches = list(dict.fromkeys(matches))
    if len(matches) != 1 or not matches[0].is_file():
        raise RuntimeError(f"Expected exactly one existing test executable: {matches}")
    return matches[0]


def run(scope, repo):
    if os.name == "nt":
        import ctypes
        ctypes.windll.kernel32.SetErrorMode(0x0001 | 0x0002 | 0x8000)
        print("Windows error dialogs disabled; WER dump policy is not verified.")
    compile_env = dict(os.environ, CARGO_TARGET_DIR=str(repo / "target"))
    fixture_parent = repo / "target/fixtures/ci"
    fixture_parent.mkdir(parents=True, exist_ok=True)
    invocation = Path(tempfile.mkdtemp(prefix="regressions-", dir=fixture_parent))
    total = 0
    for relative, target, names in selected_targets(scope, os.name == "nt"):
        manifest = (repo / relative).resolve()
        metadata = json.loads(execute(["cargo", "metadata", "--no-deps", "--locked",
                                      "--format-version=1", "--manifest-path", str(manifest)],
                                     repo, compile_env, 120))
        packages = [p for p in metadata["packages"]
                    if Path(p["manifest_path"]).resolve() == manifest]
        if len(packages) != 1:
            raise RuntimeError("Manifest package identity is ambiguous")
        package = packages[0]
        kind = "test" if target else "lib"
        if not target:
            libraries = [t["name"] for t in package["targets"] if "lib" in t["kind"]]
            if len(libraries) != 1:
                raise RuntimeError("Library target identity is ambiguous")
            target_name = libraries[0]
        else:
            target_name = target
        flags = ["--test", target] if target else ["--lib"]
        output = execute(["cargo", "test", "--locked", "--no-run", "--message-format=json",
                          "--manifest-path", str(manifest), *flags], repo, compile_env, 1200)
        messages = [json.loads(line) for line in output.splitlines() if line.startswith("{")]
        for message in messages:
            rendered = message.get("message", {}).get("rendered")
            if rendered:
                print(rendered, end="")
        executable = select_executable(messages, package["id"], target_name, kind)
        cwd = Path(package["manifest_path"]).parent
        for name in names:
            fixture = invocation / str(total)
            env = child_environment(fixture, os.environ)
            listing = execute([str(executable), name, "--exact", "--list", "--format=terse"],
                              cwd, env, 30, True)
            if listing.splitlines().count(name + ": test") != 1:
                raise RuntimeError(f"Selected test missing or omitted by cfg: {name}\n{listing}")
            started = time.monotonic()
            output = execute([str(executable), name, "--exact", "--test-threads=1"],
                             cwd, env, 30, True)
            print(output, end="")
            if not re.search(r"test result: ok\. 1 passed; 0 failed; 0 ignored;", output):
                raise RuntimeError(f"One actual passing body required: {name}")
            print(json.dumps({"test": name, "fixture": str(fixture),
                              "elapsed_seconds": time.monotonic() - started}))
            total += 1
    print(f"Selected regressions passed: {total}; fixtures retained: {invocation}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scope", choices=["headless", "desktop", "harness"])
    args = parser.parse_args()
    run(args.scope, Path(__file__).resolve().parents[1])
