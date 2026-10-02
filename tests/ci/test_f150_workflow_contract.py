"""Executable contract checks for the F-150 migration CI workflow.

These checks deliberately inspect workflow text rather than loading YAML: they
need to run with Python's standard library before any CI parser dependency is
introduced. Assertions target stable command/step tokens and tolerate normal
YAML layout changes.
"""

from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "migration-ci.yml"
BASELINE_SHA = "1cad4ce43cc72d52d4cc4eef920e0da22cb69568"
MANIFESTS = ("Cargo.toml", "desktop/Cargo.toml", "tools/ira-parity/Cargo.toml")


class F150WorkflowContract(unittest.TestCase):
    def workflow_text(self) -> str:
        self.assertTrue(
            WORKFLOW.is_file(),
            "F-150 requires .github/workflows/migration-ci.yml",
        )
        return WORKFLOW.read_text(encoding="utf-8")

    def test_explicit_three_os_runners_and_runner_image_evidence(self):
        # GAP(G-F150-ADV-01) sev=high kind=missing-feature feature=F-150
        #   what:     Migration CI must run on the three approved explicit runner labels.
        #   tui-ref:  migration/specs/F-150.md Requirement 1
        #   oracle:   migration/reports/F-150/platform-review-1.md R1
        #   repro:    Read .github/workflows/migration-ci.yml.
        #   expected: ubuntu-24.04, macos-15, windows-2022 and ImageOS/ImageVersion on each job.
        #   actual:   No migration workflow exists on the reviewed commit.
        #   cover:    test_explicit_three_os_runners_and_runner_image_evidence
        text = self.workflow_text()
        jobs = re.findall(r"(?m)^\s{2}[\w-]+:\s*(?:#.*)?$", text.split("jobs:", 1)[-1])
        self.assertGreaterEqual(len(jobs), 3, "define separate CI jobs for Linux, macOS, Windows")
        for label in ("ubuntu-24.04", "macos-15", "windows-2022"):
            self.assertIn(label, text, f"missing explicit runner label {label}")
        self.assertGreaterEqual(text.count("ImageOS"), 3)
        self.assertGreaterEqual(text.count("ImageVersion"), 3)

    def test_triggers_pin_toolchain_and_cache_keys_cover_lockfiles(self):
        # GAP(G-F150-ADV-08) sev=medium kind=test-gap feature=F-150
        #   what:     CI inputs and cache keys must be stable and traceable.
        #   tui-ref:  migration/specs/F-150.md Requirements 1, 2, and 4
        #   oracle:   migration/reports/F-150/platform-review-1.md R1
        #   repro:    Inspect workflow triggers, Rust setup, cache key, and checkout.
        #   expected: PR/push/manual triggers; exact Rust pin; OS/target/toolchain/lockfile-keyed cache; clean checkout.
        #   actual:   No migration workflow exists on the reviewed commit.
        #   cover:    test_triggers_pin_toolchain_and_cache_keys_cover_lockfiles
        text = self.workflow_text()
        for trigger in ("pull_request", "push:", "workflow_dispatch"):
            self.assertIn(trigger, text)
        self.assertRegex(text, r"(?i)(toolchain|rust-version):\s*['\"]?\d+\.\d+\.\d+")
        self.assertRegex(text, r"(?i)rustc\s+-Vv")
        self.assertRegex(text, r"(?i)cargo\s+-V")
        for cache_dimension in ("runner.os", "target", "toolchain"):
            self.assertIn(cache_dimension, text)
        for lockfile in ("Cargo.lock", "desktop/Cargo.lock", "tools/ira-parity/Cargo.lock"):
            self.assertIn(lockfile, text)
        self.assertRegex(text, r"(?i)(fetch-depth:\s*0|checkout.{0,100}clean)")

    def test_each_independent_package_runs_locked_build_test_clippy_and_fmt(self):
        # GAP(G-F150-ADV-02) sev=high kind=test-gap feature=F-150
        #   what:     All three independent manifests need their explicit locked build/test/lint/fmt checks.
        #   tui-ref:  migration/specs/F-150.md Requirement 2
        #   oracle:   migration/reports/F-150/architecture-review-1.md R1
        #   repro:    Search the workflow for each manifest-scoped cargo command.
        #   expected: Every manifest has build, all-target test, all-target clippy -D warnings, and fmt check.
        #   actual:   No migration workflow exists on the reviewed commit.
        #   cover:    test_each_independent_package_runs_locked_build_test_clippy_and_fmt
        text = re.sub(r"\s+", " ", self.workflow_text())
        for manifest in MANIFESTS:
            escaped = re.escape(manifest)
            for verb, tail in (
                ("build", r"--locked"),
                ("test", r"--all-targets --locked"),
                ("clippy", r"--all-targets --locked -- -D warnings"),
                ("fmt", r"--manifest-path " + escaped + r" -- --check"),
            ):
                pattern = rf"cargo {verb} --manifest-path {escaped} {tail}"
                self.assertRegex(text, pattern, f"missing `{verb}` command for {manifest}")

    def test_selects_the_named_live_replay_without_blanket_ignored(self):
        # GAP(G-F150-ADV-03) sev=high kind=test-gap feature=F-150
        #   what:     Each native OS must execute exactly the named F-002 live scenario.
        #   tui-ref:  migration/specs/F-150.md Requirement 3
        #   oracle:   migration/reports/F-150/architecture-review-1.md R2
        #   repro:    Inspect live replay test invocation and its selection flags.
        #   expected: Exact named test selected; no blanket --ignored; one pass plus scenario/SHA/cleanup evidence.
        #   actual:   No migration workflow exists on the reviewed commit.
        #   cover:    test_selects_the_named_live_replay_without_blanket_ignored
        text = self.workflow_text()
        self.assertIn("tagged_oracle_trace_replays_on_linux_macos_windows", text)
        self.assertGreaterEqual(text.count("tagged_oracle_trace_replays_on_linux_macos_windows"), 3)
        self.assertNotIn("--ignored", text)
        self.assertIn("--exact", text)
        self.assertIn("--test f002_contract", text)
        for evidence in ("scenario", "baseline SHA", "cleanup"):
            self.assertRegex(text, rf"(?i){re.escape(evidence)}")
        self.assertRegex(text, r"(?i)(exactly one|1 test passed|test result: ok\. 1 passed)")

    def test_fetches_and_verifies_the_immutable_oracle_baseline(self):
        # GAP(G-F150-ADV-04) sev=high kind=test-gap feature=F-150
        #   what:     CI must make the frozen tag available and assert/log its immutable peeled SHA.
        #   tui-ref:  migration/specs/F-150.md Requirement 3
        #   oracle:   migration/reports/F-150/architecture-review-1.md R3
        #   repro:    Inspect checkout/fetch and baseline verification steps.
        #   expected: tui-oracle-baseline and exact peeled SHA are fetched/verified and emitted as evidence.
        #   actual:   No migration workflow exists on the reviewed commit.
        #   cover:    test_fetches_and_verifies_the_immutable_oracle_baseline
        text = self.workflow_text()
        self.assertIn("tui-oracle-baseline", text)
        self.assertIn(BASELINE_SHA, text)
        self.assertRegex(text, r"(?i)(rev-parse|verify|assert).{0,160}" + BASELINE_SHA)
        self.assertRegex(text, r"(?i)(fetch-depth:\s*0|git fetch.{0,100}tui-oracle-baseline)")

    def test_audits_the_pinned_full_parity_lockfile_closure(self):
        # GAP(G-F150-ADV-05) sev=high kind=security feature=F-150
        #   what:     A pinned license tool must audit the parity package lockfile and preserve an inventory/policy.
        #   tui-ref:  migration/specs/F-150.md Requirement 5
        #   oracle:   migration/reports/F-150/platform-review-1.md R3
        #   repro:    Inspect license command, policy, and inventory references.
        #   expected: Pinned cargo-deny + parity manifest/lock + committed policy/inventory + unknown denial.
        #   actual:   No migration workflow exists on the reviewed commit.
        #   cover:    test_audits_the_pinned_full_parity_lockfile_closure
        text = self.workflow_text()
        self.assertRegex(text, r"cargo-deny(?:\s|@|=|:).{0,50}\bv?\d+\.\d+\.\d+")
        self.assertIn("tools/ira-parity/Cargo.toml", text)
        self.assertIn("tools/ira-parity/Cargo.lock", text)
        self.assertRegex(text, r"(?i)(deny|unknown).{0,100}(license|spdx)|(license|spdx).{0,100}(deny|unknown)")
        self.assertRegex(text, r"(?i)(inventory|license-report).{0,100}(tools/ira-parity|cargo-deny)")
        self.assertRegex(text, r"(?i)(all targets|target profiles|--all-features|cargo deny check)")

    def test_failures_are_not_suppressed_and_evidence_has_finite_retention(self):
        # GAP(G-F150-ADV-09) sev=medium kind=test-gap feature=F-150
        #   what:     Failed checks must fail the job, while diagnostic evidence has finite retention.
        #   tui-ref:  migration/specs/F-150.md Error handling and determinism; Requirement 7
        #   oracle:   migration/reports/F-150/architecture-review-1.md R5
        #   repro:    Inspect workflow error handling, deadlines, and artifact retention.
        #   expected: No continue-on-error/hidden skips; bounded execution and finite evidence retention.
        #   actual:   No migration workflow exists on the reviewed commit.
        #   cover:    test_failures_are_not_suppressed_and_evidence_has_finite_retention
        text = self.workflow_text()
        self.assertNotIn("continue-on-error", text)
        self.assertNotRegex(text, r"(?m)\|\|\s*true\b")
        self.assertRegex(text, r"(?i)(timeout-minutes|timeout|deadline)")
        self.assertRegex(text, r"(?i)(retention-days|retention).{0,100}(artifact|log|report)")

    def test_ci_is_read_only_and_never_publishes_release_assets(self):
        # GAP(G-F150-ADV-06) sev=high kind=security feature=F-150
        #   what:     Migration CI must not publish releases or alter repository contents.
        #   tui-ref:  migration/specs/F-150.md Requirements 1 and 7
        #   oracle:   migration/reports/F-150/architecture-review-1.md R6
        #   repro:    Inspect workflow permission grants and release/publishing actions.
        #   expected: contents: read only; no release creation or release asset upload.
        #   actual:   No migration workflow exists on the reviewed commit.
        #   cover:    test_ci_is_read_only_and_never_publishes_release_assets
        text = self.workflow_text()
        self.assertRegex(text, r"(?s)permissions:\s*\n\s*contents:\s*read\b")
        self.assertNotRegex(text, r"(?i)(softprops/action-gh-release|gh release create|upload_url|release assets)")
        self.assertNotRegex(text, r"(?i)contents:\s*(write|admin)")

    def test_native_gui_smoke_is_real_or_explicitly_unresolved_experiment(self):
        # GAP(G-F150-ADV-07) sev=high kind=platform feature=F-150
        #   what:     CI must prove a native visible-window/close smoke or preserve an explicit unresolved experiment gate.
        #   tui-ref:  migration/specs/F-150.md Requirement 6
        #   oracle:   migration/reports/F-150/platform-review-1.md R4
        #   repro:    Inspect macOS and Windows GUI smoke steps and recorded outcomes.
        #   expected: Native window visibility + requested close + bounded clean shutdown, or explicit unresolved gate.
        #   actual:   No migration workflow exists on the reviewed commit.
        #   cover:    test_native_gui_smoke_is_real_or_explicitly_unresolved_experiment
        text = self.workflow_text()
        unresolved = re.search(r"(?is)GUI_SMOKE_EXPERIMENT:\s*unresolved.{0,500}(hosted|interactive|self-hosted).{0,200}(runner|GUI|window)", text)
        native_smoke = all(
            re.search(pattern, text, re.IGNORECASE)
            for pattern in (
                r"(native|OS-native).{0,100}(window|GUI)",
                r"(visible|enumerat).{0,100}window",
                r"(close|quit).{0,100}(window|process)",
                r"(deadline|timeout).{0,100}(shutdown|exit|process)",
            )
        )
        self.assertTrue(unresolved or native_smoke, "must implement native smoke or declare explicit experiment gate")
        if native_smoke:
            for label in ("macos-15", "windows-2022"):
                self.assertIn(label, text)


if __name__ == "__main__":
    unittest.main(verbosity=2)
