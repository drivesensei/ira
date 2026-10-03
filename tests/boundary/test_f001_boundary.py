"""Current F001 full-migration contract; historical starter checks retained in migration/historical.
Python3.13 + tomllib. README.md documents synthetic build environment and cache leases.
"""
from __future__ import annotations
import hashlib
import json
import os
import re
import subprocess
import tempfile
import tomllib
import unittest
from pathlib import Path
from rust_source import rust_code, module_paths
from git_provenance import git_at, candidate_errors, changed_root_paths, index_flags

CONTRACT = Path(__file__).resolve().parents[2]
ROOT = Path(os.environ.get("IRA_BOUNDARY_ROOT", CONTRACT)).resolve()
ORACLE = "1cad4ce43cc72d52d4cc4eef920e0da22cb69568"
MILESTONE = "e3462637c3072b304ab90f465cbea1e24d8f15a3"
HISTORICAL_HASH = "03da310a58cb76e8f367c2906ae0f8b21fcc112020de39da54c58384e78ad261"
ALLOWED_ROOT_FILES = {'src/services/transfer.rs', 'src/services/editor_staging_tests.rs', 'src/services/editor.rs', 'src/editor_safety_tests.rs', 'src/app.rs', 'src/services/transfer_safety_tests.rs'}
APPROVED_GRAPH_FILES = {'desktop/Cargo.toml', 'desktop/Cargo.lock', 'Cargo.lock', 'crates/core/Cargo.toml', 'Cargo.toml'}
FORBIDDEN = {"gpui", "ratatui", "ratatui-image", "crossterm"}
REQUIRED_MODULES = {"cursor", "domain", "services", "theme", "utils", "application", "model", "input", "observable", "editor"}


def read_toml(path):
    return tomllib.loads(path.read_text(encoding="utf-8"))


def dependencies(manifest):
    for table in [manifest, *manifest.get("target", {}).values()]:
        for section in ("dependencies", "dev-dependencies", "build-dependencies"):
            for name, config in table.get(section, {}).items():
                yield name, config


def package(name, config):
    return config.get("package", name) if isinstance(config, dict) else name



def imports(source, names):
    names = "|".join(re.escape(n.replace("-", "_")) for n in names)
    code = rust_code(source)
    return bool(re.search(r"\b(?:use|extern\s+crate)\s+(?:::)?(?:\w+\s*::\s*)*(?:" + names + r")\b|\b(?:" + names + r")\s*::", code))


def structural_errors(root):
    errors = []
    core = root / "crates/core"
    manifests = [root / "Cargo.toml", root / "desktop/Cargo.toml", core / "Cargo.toml"]
    parsed = [read_toml(p) for p in manifests]
    for manifest, name, edition in zip(parsed, ["ira", "ira-desktop", "ira-core"], ["2021", "2024", "2021"]):
        if manifest["package"]["name"] != name or manifest["package"]["edition"] != edition:
            errors.append("package identity")
    for p, manifest in zip(manifests[:2], parsed[:2]):
        if "workspace" in manifest: errors.append("umbrella workspace")
        local = [(n,c) for n,c in dependencies(manifest) if package(n,c)=="ira-core"]
        if not local or any(not isinstance(c,dict) or "path" not in c or (p.parent/c["path"]).resolve()!=core.resolve() for _,c in local):
            errors.append("local core path")
    if (core/"build.rs").exists() or parsed[2]["package"].get("build"):
        errors.append("unreviewed core build script")
    for name, config in dependencies(parsed[2]):
        if name in FORBIDDEN or package(name,config) in FORBIDDEN: errors.append("forbidden core dependency")
    for directory, manifest, denied in [(root/"src",parsed[0],{"gpui","ira-desktop"}), (root/"desktop/src",parsed[1],{"ira"}), (core/"src",parsed[2],FORBIDDEN)]:
        aliases = {n for n,c in dependencies(manifest) if package(n,c) in denied}
        if directory != core/"src" and aliases: errors.append("host cross dependency")
        sources = list(directory.rglob("*.rs"))
        if not sources: errors.append("host/core source absent")
        for path in sources:
            source = path.read_text(encoding="utf-8"); code = rust_code(source)
            if imports(source, denied | aliases): errors.append("forbidden import " + str(path.relative_to(root)))
            if directory == core/"src":
                if re.search(r"\bunsafe\b",code): errors.append("unsafe core source")
                if re.search(r"\binclude\s*!",code): errors.append("unreviewed core include")
                for module_path in module_paths(source):
                    if not (path.parent/module_path).resolve().is_relative_to((core/"src").resolve()):
                        errors.append("core path escapes source boundary")
    core_source = (core/"src/lib.rs").read_text()
    if "#![forbid(unsafe_code)]" not in rust_code(core_source): errors.append("unsafe prohibition missing")
    for module in REQUIRED_MODULES:
        if not re.search(r"pub\s+mod\s+" + module + r"\s*;",rust_code(core_source)): errors.append("extracted module missing " + module)
    for lock in [root/"Cargo.lock",root/"desktop/Cargo.lock"]:
        if not lock.is_file() or not any(p["name"]=="ira-core" for p in read_toml(lock).get("package",[])):
            errors.append("independent core lock")
    if (root/"Cargo.lock").resolve()==(root/"desktop/Cargo.lock").resolve(): errors.append("shared host lock")
    desktop = parsed[1]
    gpui = [c for n,c in dependencies(desktop) if package(n,c)=="gpui"]
    if not gpui or any(not isinstance(c,dict) or c.get("version") not in {"0.2.2","=0.2.2"} for c in gpui): errors.append("GPUI pin")
    if not {"ratatui","ratatui-image"}.issubset({package(n,c) for n,c in dependencies(parsed[0])}): errors.append("TUI dependencies")
    shell = (root/"desktop/src/main.rs").read_text()
    for marker in ["app.on_reopen(open_main_window)","cx.open_window(","Runtime::start_with_fonts", "runtime.attach(generation)","Desktop::new(","actions::register(cx)","text_input::register(cx)","window.on_window_should_close", "this.close()"]:
        if marker not in rust_code(shell): errors.append("native lifecycle wiring " + marker)
    lib = rust_code((root/"desktop/src/lib.rs").read_text())
    for module in ["actions","components","runtime","views","platform"]:
        if not re.search(r"pub\s+mod\s+"+module+r"\s*;",lib): errors.append("desktop module missing "+module)
    return errors


def root_change_errors(changed, contents, approved):
    errors=[]
    if set(approved)!=ALLOWED_ROOT_FILES: errors.append("approval file identities changed")
    for name in set(approved)-set(changed): errors.append("approved root change missing "+name)
    for name in changed:
        record=approved.get(name)
        if not record: errors.append("unapproved root source " + name)
        elif name not in contents or hashlib.sha256(contents[name]).hexdigest()!=record["sha256"]:
            errors.append("unapproved root blob " + name)
    return errors


def graph_change_errors(contents, approved):
    errors=[]
    if set(approved)!=APPROVED_GRAPH_FILES: errors.append("approval graph identities changed")
    for name,record in approved.items():
        if name not in contents or hashlib.sha256(contents[name]).hexdigest()!=record["sha256"]:
            errors.append("unapproved graph blob "+name)
    return errors


def git(*args, root=ROOT):
    return git_at(root,*args)


def run_builds():
    """No skip flag: both locked builds and every root assertion remain required."""
    results=[]
    with tempfile.TemporaryDirectory(prefix="ira-f001-boundary-") as directory:
        env=os.environ.copy(); base=Path(directory)
        for key,suffix in [("HOME","home"),("XDG_CONFIG_HOME","config"),("XDG_DATA_HOME","data"),("XDG_CACHE_HOME","cache"),("TMPDIR","tmp")]:
            (base/suffix).mkdir(); env[key]=str(base/suffix)
        env.update(CARGO_HOME=os.environ.get("CARGO_HOME",str(Path.home()/".cargo")),GIT_CONFIG_NOSYSTEM="1",GIT_CONFIG_GLOBAL="/dev/null",GIT_CONFIG_COUNT="1",GIT_CONFIG_KEY_0="commit.gpgsign",GIT_CONFIG_VALUE_0="false")
        cargo=os.environ.get("IRA_BOUNDARY_CARGO","/opt/homebrew/bin/cargo")
        commands=[[cargo,"build","--locked","--offline"],[cargo,"build","--manifest-path","desktop/Cargo.toml","--locked","--offline","--features","gpui/runtime_shaders"],[cargo,"test","--locked","--offline","--no-fail-fast","--","--test-threads=1"]]
        for index,command in enumerate(commands):
            cache="IRA_BOUNDARY_DESKTOP_TARGET" if index==1 else "IRA_BOUNDARY_ROOT_TARGET"
            env["CARGO_TARGET_DIR"]=os.environ.get(cache,str(ROOT/("desktop/target" if index==1 else "target")))
            proc=subprocess.run(command,cwd=ROOT,env=env,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=1200,check=False)
            print("COMMAND",command,"EXIT",proc.returncode,"TMPDIR",env["TMPDIR"],flush=True);print(proc.stdout,flush=True)
            results.append(proc)
    return results


class F001BoundaryTests(unittest.TestCase):
    def test_s1_root_tui_package_identity_and_build_contract(self):
        self.assertEqual(read_toml(ROOT/"Cargo.toml")["package"]["name"],"ira")
        self.assertTrue((ROOT/"src/main.rs").is_file())
        self.assertFalse(structural_errors(ROOT))

    def test_s2_desktop_shell_identity_and_build_contract(self):
        self.assertFalse(structural_errors(ROOT))

    # Historical GAP-RESOLVED notes and exact seven milestone tests preserved byte-for-byte at migration/historical/F-001-e346263.
    def test_s3_core_is_ui_neutral_and_hosts_are_isolated(self):
        self.assertFalse(structural_errors(ROOT))

    def test_s3_ui_neutrality_rejects_renamed_dependencies_and_absolute_imports(self):
        self.assertFalse(structural_errors(ROOT))

    def test_s4_local_core_and_independent_cargo_graphs(self):
        self.assertFalse(structural_errors(ROOT))

    def test_s5_both_packages_build_and_root_tests_pass(self):
        for result in run_builds():
            with self.subTest(command=result.args): self.assertEqual(result.returncode,0,"See preceding full command output: "+str(result.args))

    def test_s6_current_extraction_has_only_exact_approved_root_changes(self):
        fixture=json.loads((CONTRACT/"tests/boundary/fixtures/approved-root-changes.json").read_text())
        self.assertEqual(git("rev-parse","tui-oracle-baseline^{}"),ORACLE)
        self.assertEqual(fixture["oracle"],ORACLE)
        head=git("rev-parse","HEAD")
        print("CANDIDATE_ROOT",str(ROOT),"HEAD",head,"CONTRACT",str(CONTRACT),flush=True)
        if ROOT!=CONTRACT:
            self.assertFalse(candidate_errors(ROOT,os.environ.get("IRA_BOUNDARY_EXPECTED_HEAD",fixture["snapshot"])))
        graph_contents={p:(ROOT/p).read_bytes() for p in APPROVED_GRAPH_FILES if (ROOT/p).is_file() and not (ROOT/p).is_symlink()}
        self.assertFalse(graph_change_errors(graph_contents,fixture["graphs"]))
        self.assertFalse(index_flags(ROOT),"hidden Git index flags")
        changed=changed_root_paths(ROOT)
        contents={p:(ROOT/p).read_bytes() for p in changed if (ROOT/p).is_file() and not (ROOT/p).is_symlink()}
        self.assertFalse(root_change_errors(changed,contents,fixture["files"]))
        for name,record in {**fixture["files"],**fixture["graphs"]}.items():
            self.assertEqual(git("rev-parse",record["commit"]+":"+name),record["blob"])
            data=subprocess.check_output(["git","show",record["commit"]+":"+name],cwd=ROOT)
            self.assertEqual(hashlib.sha256(data).hexdigest(),record["sha256"])
            self.assertTrue(record["regressions"])
        self.assertFalse(structural_errors(ROOT))

    def test_historical_milestone_is_preserved_and_oracle_is_unchanged(self):
        historical=CONTRACT/"migration/historical/F-001-e346263/test_f001_boundary.py"
        data=subprocess.check_output(["git","show",MILESTONE+":tests/boundary/test_f001_boundary.py"],cwd=ROOT)
        self.assertEqual(historical.read_bytes(),data)
        self.assertEqual(hashlib.sha256(data).hexdigest(),HISTORICAL_HASH)
        self.assertEqual(git("rev-parse","tui-oracle-baseline^{}"),ORACLE)

if __name__=="__main__": unittest.main(verbosity=2)
