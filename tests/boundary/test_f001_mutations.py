"""Adverse fixtures exercise the same current-contract functions, never live source."""
import contextlib
import hashlib
import json
import os
import subprocess
import shutil
import tempfile
import unittest
from pathlib import Path
from test_f001_boundary import ROOT, ALLOWED_ROOT_FILES, structural_errors, root_change_errors, rust_code, changed_root_paths

@contextlib.contextmanager
def fixture():
    with tempfile.TemporaryDirectory(prefix="ira-f001-mutation-") as temp:
        root=Path(temp)
        for name in ["src","desktop/src","crates/core/src"]:
            shutil.copytree(ROOT/name,root/name)
        for name in ["Cargo.toml","Cargo.lock","desktop/Cargo.toml","desktop/Cargo.lock","crates/core/Cargo.toml"]:
            dest=root/name; dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(ROOT/name,dest)
        yield root

class F001MutationTests(unittest.TestCase):
    def test_alias_target_dependency_is_rejected(self):
        with fixture() as root:
            p=root/"crates/core/Cargo.toml";p.write_text(p.read_text()+ '\n[target.\"cfg(any())\".build-dependencies]\nhidden_ui = { package="gpui", version="0.2.2" }\n')
            self.assertIn("forbidden core dependency",structural_errors(root))

    def test_core_absolute_ui_import_and_unsafe_are_rejected(self):
        with fixture() as root:
            p=root/"crates/core/src/lib.rs";p.write_text(p.read_text()+"\nuse ::gpui::App as Hidden;\nfn host_leak(){ unsafe { } }\n")
            errors=structural_errors(root)
            self.assertTrue(any("forbidden import" in e for e in errors));self.assertIn("unsafe core source",errors)

    def test_core_external_path_and_include_escape_are_rejected(self):
        with fixture() as root:
            p=root/"crates/core/src/lib.rs";p.write_text(p.read_text()+'\n#[path="../../../src/app.rs"] mod leak;\ninclude!("../../../src/app.rs");\n')
            errors=structural_errors(root)
            self.assertIn("core path escapes source boundary",errors);self.assertIn("unreviewed core include",errors)

    def test_host_dependency_alias_and_absolute_import_are_rejected(self):
        with fixture() as root:
            p=root/"desktop/Cargo.toml";p.write_text(p.read_text()+'\n[target.\"cfg(any())\".dependencies]\nterminal = {package="ira",path=".."}\n')
            p=root/"desktop/src/lib.rs";p.write_text(p.read_text()+"\nuse ::terminal::App;\n")
            errors=structural_errors(root);self.assertIn("host cross dependency",errors);self.assertTrue(any("forbidden import" in e for e in errors))

    def test_root_host_absolute_desktop_import_is_rejected(self):
        with fixture() as root:
            p=root/"src/main.rs";p.write_text(p.read_text()+"\nuse ::ira_desktop::runtime::Runtime;\n")
            self.assertTrue(any("forbidden import" in e for e in structural_errors(root)))

    def test_local_core_path_and_workspace_are_rejected(self):
        with fixture() as root:
            p=root/"desktop/Cargo.toml";p.write_text(p.read_text().replace('../crates/core','../other-core')+'\n[workspace]\n')
            errors=structural_errors(root);self.assertIn("local core path",errors);self.assertIn("umbrella workspace",errors)

    def test_missing_extracted_module_and_unsafe_prohibition_are_rejected(self):
        with fixture() as root:
            p=root/"crates/core/src/lib.rs";p.write_text(p.read_text().replace('pub mod application;','').replace('#![forbid(unsafe_code)]',''))
            errors=structural_errors(root);self.assertIn("extracted module missing application",errors);self.assertIn("unsafe prohibition missing",errors)

    def test_missing_shell_close_and_actor_attachment_are_rejected(self):
        with fixture() as root:
            p=root/"desktop/src/main.rs";p.write_text(p.read_text().replace('window.on_window_should_close','window.on_close_fake').replace('runtime.attach(generation)','runtime.attach_fake(generation)'))
            errors=structural_errors(root);self.assertIn("native lifecycle wiring window.on_window_should_close",errors);self.assertIn("native lifecycle wiring runtime.attach(generation)",errors)

    def test_lock_core_removal_and_gpui_version_drift_are_rejected(self):
        with fixture() as root:
            p=root/"desktop/Cargo.lock";p.write_text(p.read_text().replace('name = "ira-core"','name = "other-core"'))
            p=root/"desktop/Cargo.toml";p.write_text(p.read_text().replace('version = "0.2.2"','version = "0.3.0"'))
            errors=structural_errors(root);self.assertIn("independent core lock",errors);self.assertIn("GPUI pin",errors)

    def test_unknown_root_files_modified_approved_blob_and_deletion_fail(self):
        approved=json.loads((ROOT/"tests/boundary/fixtures/approved-root-changes.json").read_text())["files"]
        data={name:(ROOT/name).read_bytes() for name in approved}
        self.assertEqual(root_change_errors(set(data),data,approved),[])
        self.assertIn("unapproved root source src/handler.rs",root_change_errors(set(data)|{"src/handler.rs"},data,approved))
        name="src/services/transfer.rs";data[name]+=b"\n// unrelated edit\n"
        self.assertIn("unapproved root blob "+name,root_change_errors(set(data),data,approved))
        del data[name]
        self.assertIn("unapproved root blob "+name,root_change_errors(ALLOWED_ROOT_FILES,data,approved))
        widened=dict(approved);widened["src/handler.rs"]={"sha256":hashlib.sha256(b"anything").hexdigest()}
        self.assertIn("approval file identities changed",root_change_errors(set(),{},widened))

    def test_comment_and_string_markers_cannot_satisfy_lifecycle_contract(self):
        with fixture() as root:
            p=root/"desktop/src/main.rs";source=p.read_text().replace('window.on_window_should_close','window.on_close_fake');p.write_text(source+'\n// window.on_window_should_close\nconst FAKE:&str="window.on_window_should_close";\n')
            self.assertIn("native lifecycle wiring window.on_window_should_close",structural_errors(root))

    def test_actual_git_inspector_includes_untracked_and_ignored_root_source(self):
        with tempfile.TemporaryDirectory(prefix="ira-f001-git-") as temp:
            root=Path(temp);(root/"src").mkdir();(root/"src/main.rs").write_text("fn main(){}")
            env=os.environ.copy();env.update(HOME=temp,GIT_CONFIG_NOSYSTEM="1",GIT_CONFIG_GLOBAL="/dev/null")
            def command(*args):
                subprocess.run(["git","-c","commit.gpgsign=false","-c","user.name=T030","-c","user.email=t030@example.invalid",*args],cwd=root,env=env,check=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
            command("init");command("add","src");command("commit","-m","synthetic baseline");command("tag","tui-oracle-baseline")
            (root/".gitignore").write_text("src/ignored.rs\n")
            (root/"src/unknown.rs").write_text("pub fn leak(){}")
            (root/"src/ignored.rs").write_text("pub fn hidden(){}")
            self.assertEqual(changed_root_paths(root),{"src/unknown.rs","src/ignored.rs"})

    def test_nested_comments_raw_strings_and_lifetimes_do_not_fake_host_imports(self):
        source='/* outer /* use gpui::App; */ unsafe {} */ fn valid<\'a>(s:&\'a str){let x=r###"use ::gpui::App; unsafe {}"###;}'
        code=rust_code(source);self.assertNotIn("gpui",code);self.assertNotIn("unsafe",code);self.assertIn("fn valid<'a>",code)

if __name__=="__main__": unittest.main(verbosity=2)
