"""Executable structural and build-contract checks for the F-001 boundary.

Run from the repository root with:
    python3 -m unittest discover -s tests/boundary -p 'test_*.py' -v
"""

from __future__ import annotations

import re
import subprocess
import tomllib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CORE = ROOT / "crates" / "core"
CORE_MANIFEST = CORE / "Cargo.toml"


def read_toml(path: Path) -> dict:
    return tomllib.loads(path.read_text(encoding="utf-8"))


def all_dependencies(manifest: dict) -> dict[str, object]:
    result: dict[str, object] = {}
    for section in ("dependencies", "dev-dependencies", "build-dependencies"):
        result.update(manifest.get(section, {}))
    for target in manifest.get("target", {}).values():
        for section in ("dependencies", "dev-dependencies", "build-dependencies"):
            result.update(target.get(section, {}))
    return result


def run(command: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=1200,
        check=False,
    )


class F001BoundaryTests(unittest.TestCase):
    def test_s1_root_tui_package_identity_and_build_contract(self) -> None:
        manifest = read_toml(ROOT / "Cargo.toml")
        self.assertEqual(manifest["package"]["name"], "ira")
        self.assertEqual(manifest["package"]["edition"], "2021")
        self.assertTrue((ROOT / "src" / "main.rs").is_file())
        deps = all_dependencies(manifest)
        self.assertIn("ratatui", deps)
        self.assertIn("ratatui-image", deps)
        proc = run(["cargo", "build", "--locked"])
        self.assertEqual(proc.returncode, 0, proc.stdout)

    def test_s2_desktop_shell_identity_and_build_contract(self) -> None:
        manifest = read_toml(ROOT / "desktop" / "Cargo.toml")
        self.assertEqual(manifest["package"]["name"], "ira-desktop")
        self.assertEqual(manifest["package"]["edition"], "2024")
        deps = all_dependencies(manifest)
        gpui = deps["gpui"]
        self.assertEqual(gpui["version"], "0.2.2")

        source = (ROOT / "desktop" / "src" / "main.rs").read_text(encoding="utf-8")
        for marker in (
            "app.on_reopen(open_main_window)",
            "cx.open_window(",
            '"hello-button"',
            '"Click me"',
            "this.button_was_clicked = true",
        ):
            self.assertIn(marker, source, f"desktop shell contract missing: {marker}")
        proc = run(["cargo", "build", "--manifest-path", "desktop/Cargo.toml", "--locked"])
        self.assertEqual(proc.returncode, 0, proc.stdout)

    # GAP-RESOLVED(G-F001-ADV-03) sev=high kind=missing-feature feature=F-001
    #   what: A minimal ira-core library must exist and exclude host UI dependencies/imports.
    #   tui-ref: AGENTS.md §7 invariant 2
    #   oracle: N/A (structural contract)
    #   repro: Run this unittest against the pre-F-001 tree.
    #   expected: core library exists with no gpui/ratatui/crossterm dependency or source import;
    #             neither host imports the other host's crate types.
    #   actual: crates/core is absent.
    #   cover: test_s3_core_is_ui_neutral_and_hosts_are_isolated
    #   fixed-by: 296c8927b57e526b2144f431397d2343e3aeb414
    #   verified-by: adversarial-reviewer 2026-10-01
    def test_s3_core_is_ui_neutral_and_hosts_are_isolated(self) -> None:
        self.assertTrue(CORE_MANIFEST.is_file(), "crates/core/Cargo.toml is required")
        core_manifest = read_toml(CORE_MANIFEST)
        self.assertEqual(core_manifest["package"]["name"], "ira-core")
        self.assertTrue((CORE / "src" / "lib.rs").is_file())

        forbidden = {"gpui", "ratatui", "crossterm"}
        core_deps = all_dependencies(core_manifest)
        forbidden_dependencies = {
            dep_name
            for dep_name, config in core_deps.items()
            if dep_name in forbidden
            or (isinstance(config, dict) and config.get("package") in forbidden)
        }
        self.assertFalse(
            forbidden_dependencies,
            f"forbidden core dependencies (including renamed dependencies): {forbidden_dependencies}",
        )
        core_sources = list((CORE / "src").rglob("*.rs"))
        self.assertTrue(core_sources, "core library source is required")
        import_pattern = re.compile(
            r"\b(?:use|extern\s+crate)\s+(?:::)?(?:\w+\s*::\s*)*(gpui|ratatui|crossterm)\b"
            r"|(?:::)?\b(gpui|ratatui|crossterm)\s*::"
        )
        for path in core_sources:
            found = import_pattern.findall(path.read_text(encoding="utf-8"))
            self.assertFalse(found, f"forbidden host import(s) in {path.relative_to(ROOT)}: {found}")

        root_sources = list((ROOT / "src").rglob("*.rs"))
        self.assertNotIn("ira-desktop", all_dependencies(read_toml(ROOT / "Cargo.toml")))
        self.assertFalse(
            [(p, m.group(0)) for p in root_sources for m in re.finditer(r"\b(?:gpui|ira_desktop)\s*::", p.read_text(encoding="utf-8"))],
            "TUI must not import GPUI or desktop host types",
        )
        desktop_sources = list((ROOT / "desktop" / "src").rglob("*.rs"))
        self.assertNotIn("ira", all_dependencies(read_toml(ROOT / "desktop" / "Cargo.toml")))
        self.assertFalse(
            [(p, m.group(0)) for p in desktop_sources for m in re.finditer(r"\bira\s*::", p.read_text(encoding="utf-8"))],
            "desktop must not import root TUI types",
        )

    # GAP-RESOLVED(G-F001-ADV-05) sev=low kind=test-gap feature=F-001
    #   what: The original S3 dependency/import checks missed renamed dependencies and absolute crate paths.
    #   tui-ref: AGENTS.md §7 invariant 2
    #   oracle: N/A (structural contract)
    #   repro: Alias a forbidden package as `ui` in crates/core/Cargo.toml, or use `use ::gpui::...`.
    #   expected: S3 rejects forbidden package identities and imports regardless of alias/path spelling.
    #   actual: original checks only rejected the literal dependency key and relative imports.
    #   cover: test_s3_ui_neutrality_rejects_renamed_dependencies_and_absolute_imports (mutation checks below)
    #   fixed-by: adversarial-review F-001
    #   verified-by: adversarial-reviewer 2026-10-01
    def test_s3_ui_neutrality_rejects_renamed_dependencies_and_absolute_imports(self) -> None:
        forbidden = {"gpui", "ratatui", "crossterm"}
        manifest = read_toml(CORE_MANIFEST)
        dependencies = all_dependencies(manifest)
        for name, config in dependencies.items():
            package = config.get("package", name) if isinstance(config, dict) else name
            self.assertNotIn(package, forbidden)

        import_pattern = re.compile(
            r"\b(?:use|extern\s+crate)\s+(?:::)?(?:\w+\s*::\s*)*(gpui|ratatui|crossterm)\b"
            r"|(?:::)?\b(gpui|ratatui|crossterm)\s*::"
        )
        for path in (CORE / "src").rglob("*.rs"):
            self.assertFalse(import_pattern.search(path.read_text(encoding="utf-8")))

    # GAP-RESOLVED(G-F001-ADV-04) sev=high kind=missing-feature feature=F-001
    #   what: Both hosts need the same local core crate while retaining independent lockfiles and no umbrella workspace.
    #   tui-ref: N/A (package graph contract)
    #   oracle: migration/specs/F-001.md S4
    #   repro: Run this unittest against the pre-F-001 tree.
    #   expected: both manifests use a local path dependency resolving to crates/core; root and desktop locks differ;
    #             neither manifest introduces a cross-app workspace.
    #   actual: no ira-core path dependency exists in either host manifest.
    #   cover: test_s4_local_core_and_independent_cargo_graphs
    #   fixed-by: 296c8927b57e526b2144f431397d2343e3aeb414
    #   verified-by: adversarial-reviewer 2026-10-01
    def test_s4_local_core_and_independent_cargo_graphs(self) -> None:
        manifests = [ROOT / "Cargo.toml", ROOT / "desktop" / "Cargo.toml"]
        canonical_core = CORE.resolve()
        for path in manifests:
            manifest = read_toml(path)
            self.assertNotIn("workspace", manifest, f"unexpected umbrella workspace in {path.relative_to(ROOT)}")
            dep = all_dependencies(manifest).get("ira-core")
            self.assertIsInstance(dep, dict, f"ira-core must be a path dependency in {path.relative_to(ROOT)}")
            self.assertIn("path", dep, f"ira-core dependency lacks path in {path.relative_to(ROOT)}")
            resolved = (path.parent / dep["path"]).resolve()
            self.assertEqual(resolved, canonical_core)

        root_lock = ROOT / "Cargo.lock"
        desktop_lock = ROOT / "desktop" / "Cargo.lock"
        self.assertTrue(root_lock.is_file())
        self.assertTrue(desktop_lock.is_file())
        self.assertNotEqual(root_lock.resolve(), desktop_lock.resolve())
        self.assertIn('name = "ira-core"', root_lock.read_text(encoding="utf-8"))
        self.assertIn('name = "ira-core"', desktop_lock.read_text(encoding="utf-8"))

    def test_s5_both_packages_build_and_root_tests_pass(self) -> None:
        commands = (
            ["cargo", "build", "--locked"],
            ["cargo", "build", "--manifest-path", "desktop/Cargo.toml", "--locked"],
            ["bash", "-lc", 'mkdir -p target/tmp && TMPDIR="$PWD/target/tmp" cargo test --locked -- --test-threads=1'],
        )
        for command in commands:
            with self.subTest(command=command):
                proc = run(command)
                self.assertEqual(proc.returncode, 0, f"command {command!r} failed:\n{proc.stdout}")

    def test_s6_scope_is_limited_to_the_minimal_boundary(self) -> None:
        proc = run(["git", "diff", "--quiet", "tui-oracle-baseline", "--", "src"])
        self.assertEqual(proc.returncode, 0, "root TUI source changed from the frozen oracle baseline")
        if CORE.exists():
            core_rs = list((CORE / "src").rglob("*.rs"))
            self.assertEqual([p.relative_to(CORE / "src").as_posix() for p in core_rs], ["lib.rs"],
                             "F-001 must not move feature behavior into core")
        desktop_rs = list((ROOT / "desktop" / "src").rglob("*.rs"))
        self.assertEqual([p.relative_to(ROOT / "desktop" / "src").as_posix() for p in desktop_rs], ["main.rs"],
                         "F-001 desktop changes are limited to the existing starter adapter")


if __name__ == "__main__":
    unittest.main(verbosity=2)
