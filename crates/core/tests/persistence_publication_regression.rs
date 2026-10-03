use ira_core::services::state::{save_state_to, SessionState};
use std::{
    fs,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Barrier,
    },
    thread,
};
#[test]
fn legacy_save_publishes_only_complete_old_or_new_snapshots() {
    let directory = std::env::temp_dir().join(format!(
        "ira-atomic-publication-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    let path = directory.join("state");
    let a = SessionState {
        theme: Some("a".repeat(4 * 1024 * 1024)),
        ..SessionState::default()
    };
    let b = SessionState {
        theme: Some("b".repeat(4 * 1024 * 1024)),
        ..SessionState::default()
    };
    save_state_to(&path, &a);
    let old = fs::read(&path).unwrap();
    save_state_to(&directory.join("golden"), &b);
    let new = fs::read(directory.join("golden")).unwrap();
    let start = Arc::new(Barrier::new(2));
    let stop = Arc::new(AtomicBool::new(false));
    let reader_start = start.clone();
    let reader_stop = stop.clone();
    let reader_path = path.clone();
    let reader = thread::spawn(move || {
        reader_start.wait();
        let mut invalid = 0;
        let mut reads = 0;
        while !reader_stop.load(Ordering::Acquire) {
            let bytes = fs::read(&reader_path).unwrap();
            reads += 1;
            if bytes != old && bytes != new {
                invalid += 1;
            }
        }
        (invalid, reads)
    });
    start.wait();
    for index in 0..24 {
        save_state_to(&path, if index % 2 == 0 { &b } else { &a });
    }
    stop.store(true, Ordering::Release);
    let (invalid, reads) = reader.join().unwrap();
    fs::remove_dir_all(&directory).unwrap();
    assert!(reads > 0);
    assert_eq!(
        invalid, 0,
        "readers observed {invalid} truncated/partial snapshots across {reads} reads"
    );
}
