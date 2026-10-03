"""Compiler-backed Rust metadata probes exercise the actual boundary checker."""
import subprocess
import unittest
from pathlib import Path
from test_f001_mutations import fixture
from test_f001_boundary import structural_errors


class RustPathIsolationTests(unittest.TestCase):
    def compile_path(self, root, metadata, filename="outside.rs"):
        target=root/"crates"/filename
        target.write_text("pub fn seven()->u8 {7}\n")
        source=root/"crates/core/src/path_probe.rs"
        source.write_text(metadata+' mod escaped;\nfn main(){assert_eq!(escaped::seven(),7);}\n')
        command=["/opt/homebrew/bin/rustc","--edition=2021",str(source),"-o",str(root/"probe")]
        result=subprocess.run(command,text=True,capture_output=True)
        print("RUSTC",command,"EXIT",result.returncode,result.stderr,flush=True)
        self.assertEqual(result.returncode,0,result.stderr)
        result=subprocess.run([str(root/"probe")],capture_output=True,text=True)
        self.assertEqual(result.returncode,0,result.stderr)

    def test_valid_raw_escaped_and_conditional_external_paths_are_rejected(self):
        forms=[
            '#[path = "../../outside.rs"]',
            '#[path = r"../../outside.rs"]',
            '#[path = r###"../../outside.rs"###]',
            '#[path = r'+('#'*255)+'"../../outside.rs"'+('#'*255)+']',
            r'#[path = "\x2e\x2e/../outside.rs"]',
            r'#[path = "\u{2e}\u{2e}/../outside.rs"]',
            '#[path = "../\\\n    ../outside.rs"]',
            '#[cfg_attr(all(), path = r#"../../outside.rs"#)]',
            '#[cfg_attr(all(), cfg_attr(all(), path = r"../../outside.rs"))]',
            '#[path = r###"../../out"side.rs"###]',
        ]
        for index,metadata in enumerate(forms):
            with self.subTest(metadata=metadata),fixture() as root:
                self.compile_path(root,metadata,'out"side.rs' if index==9 else 'outside.rs')
                p=root/"crates/core/src/lib.rs"
                p.write_text(p.read_text()+"\n"+metadata+" pub mod escaped_oracle;\n")
                self.assertIn("core path escapes source boundary",structural_errors(root))

    def test_valid_internal_raw_and_escaped_paths_are_allowed(self):
        forms=['#[path = r#"safe.rs"#]',r'#[path = "s\u{61}fe.rs"]','#[cfg_attr(all(), path = r"safe.rs")]']
        for metadata in forms:
            with self.subTest(metadata=metadata),fixture() as root:
                (root/"crates/core/src/safe.rs").write_text("pub fn seven()->u8 {7}\n")
                source=root/"crates/core/src/path_probe.rs";source.write_text(metadata+' mod safe;\nfn main(){assert_eq!(safe::seven(),7);}\n')
                result=subprocess.run(["/opt/homebrew/bin/rustc","--edition=2021",str(source),"-o",str(root/"probe")],capture_output=True,text=True)
                self.assertEqual(result.returncode,0,result.stderr)
                p=root/"crates/core/src/lib.rs";p.write_text(p.read_text()+"\n"+metadata+" mod safe;\n")
                self.assertEqual(structural_errors(root),[])

    def test_comment_and_string_metadata_decoys_are_allowed(self):
        with fixture() as root:
            p=root/"crates/core/src/lib.rs"
            p.write_text(p.read_text()+ '\n/* #[path = r"../../outside.rs"] mod fake; */\nconst DECOY:&str = r####"#[cfg_attr(all(), path = r#"../../outside.rs"#)]"####;\n')
            self.assertEqual(structural_errors(root),[])


if __name__=="__main__": unittest.main(verbosity=2)
