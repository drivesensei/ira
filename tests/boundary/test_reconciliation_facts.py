"""Source-backed surface/document reconciliation checks; no status promotion."""
import subprocess
import unittest
from test_f001_boundary import ROOT, ORACLE

class ReconciliationFacts(unittest.TestCase):
    def frozen(self,path):
        return subprocess.check_output(["git","show",ORACLE+":"+path],cwd=ROOT,text=True)

    def test_152_distinct_rows_and_no_unsupported_final_promotion(self):
        matrix=(ROOT/"migration/PARITY_MATRIX.md").read_text()
        rows=[[c.strip() for c in line.strip("|").split("|")] for line in matrix.splitlines() if line.startswith("| F-")]
        self.assertEqual(len(rows),152);self.assertEqual(len({r[0] for r in rows}),152)
        self.assertEqual({r[7] for r in rows},{"UNVERIFIED"},"this stage has no final native evidence")
        self.assertTrue(all(len(r)==11 and r[4]!="-" for r in rows))
        surfaces=[line for line in (ROOT/"migration/oracle/surface.txt").read_text().splitlines() if line and not line.startswith("#")]
        owned=[line.split("\t") for line in (ROOT/"migration/surface-ownership.tsv").read_text().splitlines()[1:]]
        self.assertEqual(set(surfaces),{r[0] for r in owned});self.assertEqual(len(owned),len({r[0] for r in owned}))
        self.assertTrue(all(surface in matrix for surface in surfaces))

    def test_right_opens_enter_renames_and_false_l_open_surface_removed(self):
        handler=self.frozen("src/handler.rs")
        self.assertIn("KeyCode::Right => app.enter_folder()",handler)
        self.assertIn("KeyCode::Enter => app.start_rename()",handler)
        self.assertIn("KeyCode::Char(c) if !c.is_digit(10)",handler)
        for path in ["migration/oracle/surface.txt","migration/oracle/surfaces/preview-external.txt","migration/surface-ownership.tsv","migration/PARITY_MATRIX.md"]:
            text=(ROOT/path).read_text();self.assertNotIn("key:normal:enter:open-default",text);self.assertNotIn("key:normal:l:open-default-or-directory",text)
        self.assertNotIn("KeyCode::Char('j') => app.",handler);self.assertNotIn("KeyCode::Char('k') => app.",handler)
        search=handler.split("if app.is_searching()",1)[1].split("// Copy Board controls",1)[0]
        self.assertIn("app.goto_top();\n                } else {\n                    app.prev_item();",search)
        self.assertIn("app.goto_bottom();\n                } else {\n                    app.next_item();",search)
        matrix=(ROOT/"migration/PARITY_MATRIX.md").read_text();self.assertIn("key:normal:enter",matrix);self.assertIn("key:normal:right:open-default-or-directory",matrix)

    def test_f152_worker_error_and_unchanged_generation_source_facts(self):
        source=self.frozen("src/app.rs")
        start=source.index("fn start_drive_poller(");end=source.index("/// Set running",start);worker=source[start:end]
        for marker in ["thread::spawn", "if let Ok(drives) = list_drives()", "if *guard != drives", "Duration::from_secs(2)"]:
            self.assertIn(marker,worker)
        start=source.index("pub fn refresh_drives(");end=source.index("pub fn list_files_from_selected_folder",start)
        self.assertIn("if gen == self.seen_drive_generation",source[start:end])
        row=next(line for line in (ROOT/"migration/PARITY_MATRIX.md").read_text().splitlines() if line.startswith("| F-152"))
        self.assertIn("UNVERIFIED",row);self.assertIn("runtime:drive-rescan-2s",row)
        self.assertIn("state:drive-refresh-error-retention",row);self.assertIn("state:drive-refresh-unchanged-suppression",row)

    def test_bookmark_shadow_and_filter_retention_remain_oracle_facts(self):
        handler=self.frozen("src/handler.rs");bookmarks=self.frozen("src/services/bookmarks.rs");app=self.frozen("src/app.rs")
        self.assertLess(handler.index("KeyCode::Char('n')"),handler.index("KeyCode::Char(c) if !c.is_digit(10)"))
        reserved=bookmarks.split("pub const RESERVED_KEYS:",1)[1].split(";",1)[0]
        self.assertNotIn("'n'",reserved)
        block=app.split("pub fn set_folder_from_bookmark",1)[1].split("pub fn enter_folder",1)[0]
        self.assertIn("self.search_query = None",block);self.assertNotIn("filter_query = None",block)
        docs=(ROOT/"migration/keymap-aliases.md").read_text();self.assertIn("G0015",docs);self.assertIn("G0017",docs)

if __name__=="__main__": unittest.main(verbosity=2)
