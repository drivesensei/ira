use super::*;
struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "ira-editor-{}-{name}-{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.0.join(name);
        fs::write(&p, bytes).unwrap();
        p
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn snapshot(doc: &EditorDocument, revision: u64, content: &str) -> SaveSnapshot {
    SaveSnapshot {
        document_id: doc.id,
        base_document: doc.clone(),
        mtime: doc.mtime,
        edit_revision: revision,
        content: content.into(),
    }
}
#[test]
fn crlf_normalizes_restores_and_preserves_trailing_line_and_permissions() {
    let f = Fixture::new("crlf");
    let p = f.file("file.txt", b"one\r\ntwo\r\n");
    let doc = open_document(1, &p).unwrap();
    assert_eq!(doc.content, "one\ntwo\n");
    assert!(doc.crlf);
    let completion = save_document(&snapshot(&doc, 7, "edited\ntwo\n")).unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"edited\r\ntwo\r\n");
    assert_eq!(completion.new_size, 13);
    assert_eq!(completion.invalidate_path, p);
    assert!(completion.may_clear_dirty(1, 7));
    assert!(!completion.may_clear_dirty(1, 8));
    assert!(!completion.may_clear_dirty(2, 7));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&doc.canonical_path)
                .unwrap()
                .permissions()
                .mode(),
            doc.permissions.mode()
        );
    }
}
#[test]
fn empty_binary_non_utf8_and_five_mebibyte_boundary_match_oracle_messages() {
    let f = Fixture::new("limits");
    assert_eq!(open_document(1, &f.file("empty", b"")).unwrap().content, "");
    assert_eq!(
        open_document(1, &f.file("binary", b"a\0b")).unwrap_err().0,
        "binary file — not editable"
    );
    assert_eq!(
        open_document(1, &f.file("badutf8", &[255])).unwrap_err().0,
        "non-UTF-8 file — read-only preview only"
    );
    let p = f.file("limit", &vec![b'x'; EDIT_MAX_BYTES as usize]);
    assert_eq!(
        open_document(1, &p).unwrap().content.len(),
        EDIT_MAX_BYTES as usize
    );
    fs::write(&p, vec![b'x'; EDIT_MAX_BYTES as usize + 1]).unwrap();
    assert_eq!(
        open_document(1, &p).unwrap_err().0,
        "file too large to edit (> 5 MB)"
    );
}
#[test]
fn readonly_refuses_save_without_touching_bytes() {
    let f = Fixture::new("readonly");
    let p = f.file("file", b"original");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&p, Permissions::from_mode(0o444)).unwrap();
    }
    #[cfg(not(unix))]
    {
        let mut permissions = fs::metadata(&p).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&p, permissions).unwrap();
    }
    let doc = open_document(2, &p).unwrap();
    assert!(doc.read_only);
    assert_eq!(
        save_document(&snapshot(&doc, 1, "new")).unwrap_err().0,
        "file is read-only"
    );
    assert_eq!(fs::read(&p).unwrap(), b"original");
    #[cfg(not(unix))]
    {
        let mut permissions = fs::metadata(&p).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(&p, permissions).unwrap();
    }
}
#[test]
fn full_precision_external_mtime_change_refuses_clobber_and_unknown_skips_guard() {
    let f = Fixture::new("mtime");
    let p = f.file("file", b"old");
    let doc = open_document(3, &p).unwrap();
    let altered = doc.mtime.unwrap() + std::time::Duration::from_nanos(1);
    fs::File::options()
        .write(true)
        .open(&p)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(altered))
        .unwrap();
    assert_eq!(
        save_document(&snapshot(&doc, 1, "clobber")).unwrap_err().0,
        "file changed on disk — press Esc and reopen"
    );
    assert_eq!(fs::read(&p).unwrap(), b"old");
    let mut request = snapshot(&doc, 2, "allowed");
    request.mtime = None;
    save_document(&request).unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"allowed");
}
#[cfg(unix)]
#[test]
fn symlink_save_preserves_link_and_retarget_refuses_both_targets() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new("symlink");
    let a = f.file("a", b"first");
    let b = f.file("b", b"second");
    let link = f.0.join("link");
    symlink(&a, &link).unwrap();
    let doc = open_document(4, &link).unwrap();
    save_document(&snapshot(&doc, 1, "changed")).unwrap();
    assert!(fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read(&a).unwrap(), b"changed");
    let doc = open_document(4, &link).unwrap();
    fs::remove_file(&link).unwrap();
    symlink(&b, &link).unwrap();
    assert_eq!(
        save_document(&snapshot(&doc, 2, "wrong")).unwrap_err().0,
        "file path changed on disk — press Esc and reopen"
    );
    assert_eq!(fs::read(&a).unwrap(), b"changed");
    assert_eq!(fs::read(&b).unwrap(), b"second");
}
#[test]
fn queued_saves_rebase_success_and_reject_old_revision_or_wrong_document() {
    let f = Fixture::new("queue");
    let p = f.file("file", b"initial");
    let doc = open_document(5, &p).unwrap();
    let session = EditorSession::new(doc.clone());
    session.save(&snapshot(&doc, 1, "first")).unwrap();
    session.save(&snapshot(&doc, 2, "second")).unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"second");
    assert!(session.save(&snapshot(&doc, 1, "old")).is_err());
    let mut wrong = snapshot(&doc, 3, "wrong");
    wrong.document_id = 6;
    assert!(session.save(&wrong).is_err());
    assert_eq!(fs::read(&p).unwrap(), b"second");
}
#[test]
fn concurrent_sessions_for_one_file_do_not_share_the_temporary_write() {
    use std::sync::{Arc, Barrier};
    let f = Fixture::new("concurrent");
    let p = f.file("file", b"original");
    fs::File::options()
        .write(true)
        .open(&p)
        .unwrap()
        .set_times(
            fs::FileTimes::new()
                .set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(123)),
        )
        .unwrap();
    let doc = open_document(8, &p).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let mut threads = Vec::new();
    for content in ["first", "second"] {
        let b = barrier.clone();
        let req = snapshot(&doc, 1, content);
        threads.push(std::thread::spawn(move || {
            b.wait();
            save_document(&req)
        }));
    }
    let successes = threads
        .into_iter()
        .map(|t| t.join().unwrap())
        .filter(Result::is_ok)
        .count();
    assert_eq!(successes, 1);
    assert!(matches!(
        fs::read(&p).unwrap().as_slice(),
        b"first" | b"second"
    ));
    assert!(!f.0.join("file.ira-tmp").exists());
}
#[cfg(unix)]
#[test]
fn characterize_inherited_editor_tmp_symlink_write_hazard_only_on_temp_fixture() {
    // Frozen App::save_edit (src/app.rs1400): fs::write follows an existing
    // sibling .ira-tmp symlink, then rename installs the link over the document.
    // Keep the frozen pre-fix oracle explicit alongside the new safety regression.
    // These operations are the old source block, executed only on owned fixtures.
    use std::os::unix::fs::symlink;
    let f = Fixture::new("tmp-symlink-oracle");
    let file = f.file("file", b"document");
    let victim = f.file("victim", b"unrelated");
    let tmp = f.0.join("file.ira-tmp");
    symlink(&victim, &tmp).unwrap();
    let doc = open_document(9, &file).unwrap();
    fs::write(&tmp, b"replacement").unwrap();
    fs::set_permissions(&tmp, doc.permissions.clone()).unwrap();
    fs::rename(&tmp, &doc.canonical_path).unwrap();
    assert_eq!(fs::read(&victim).unwrap(), b"replacement");
    assert!(fs::symlink_metadata(&file)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[cfg(unix)]
#[test]
fn save_ignores_legacy_temp_symlink_and_preserves_victim() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new("safe-temp-symlink");
    let file = f.file("file", b"document");
    let victim = f.file("victim", b"unrelated");
    let tmp = f.0.join("file.ira-tmp");
    symlink(&victim, &tmp).unwrap();
    let doc = open_document(10, &file).unwrap();
    let result = save_document(&snapshot(&doc, 1, "replacement"));
    assert_eq!(
        fs::read(&victim).unwrap(),
        b"unrelated",
        "victim must survive"
    );
    assert_eq!(fs::read(&file).unwrap(), b"replacement");
    assert!(!fs::symlink_metadata(&file)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read_link(&tmp).unwrap(), victim);
    assert!(result.is_ok());
}

#[test]
fn save_ignores_legacy_regular_temp_and_keeps_its_bytes() {
    let f = Fixture::new("safe-temp-file");
    let file = f.file("file", b"document");
    let tmp = f.file("file.ira-tmp", b"unowned");
    let doc = open_document(11, &file).unwrap();
    let result = save_document(&snapshot(&doc, 1, "replacement"));
    assert_eq!(fs::read(&file).unwrap(), b"replacement");
    assert_eq!(fs::read(&tmp).unwrap(), b"unowned");
    assert!(result.is_ok());
}

#[test]
fn failed_rename_removes_only_exclusively_created_staging() {
    let f = Fixture::new("failed-rename");
    let file = f.file("file", b"document");
    let mut doc = open_document(12, &file).unwrap();
    doc.mtime = None;
    fs::remove_file(&file).unwrap();
    fs::create_dir(&file).unwrap();
    assert!(save_document(&snapshot(&doc, 1, "replacement")).is_err());
    assert!(file.is_dir());
    assert!(!f.0.join("file.ira-tmp").exists());
}
