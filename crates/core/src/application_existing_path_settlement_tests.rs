use super::*;
pub(super) fn app_fixture() -> (App, PathBuf) {
    let root = tests::fixture();
    let mut app = App::default();
    app.state_path = Some(root.join("state"));
    app.bookmarks_path = Some(root.join("bookmarks"));
    app.window_generation = 7;
    app.panes[0].listing_settled = true;
    (app, root)
}
// Wait for a real worker reply, then queue it before the real tick's peer drains.
pub(super) fn ready(app: &mut App, path: PathBuf, kind: ExistingPathKind) {
    let scope = app.existing_path_scope(0).unwrap();
    app.request_existing_path(0, path, kind, scope, 1).unwrap();
    let reply = app
        .existing_paths
        .rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    app.existing_paths.tx.send(reply).unwrap();
}
#[test]
fn real_tick_newer_delete_error_supersedes_success_and_own_error_receipts() {
    let _lane = TEST_LANE.lock().unwrap();
    for fail_own in [false, true] {
        let (mut app, root) = app_fixture();
        let selected = if fail_own {
            root.join("absent")
        } else {
            root.clone()
        };
        ready(&mut app, selected, ExistingPathKind::Folder);
        // cancelled avoids another filesystem listing; the real peer error still installs.
        app.job_tx
            .send(JobEvent::DeleteDone {
                cancelled: true,
                failed: vec![(
                    root.join("peer").to_str().unwrap().into(),
                    "newer failure".into(),
                )],
            })
            .unwrap();
        app.tick();
        let receipt = app.take_existing_path_receipts().pop().unwrap();
        assert_eq!(receipt.result.is_err(), fail_own);
        let stamp = receipt.focus_stamp.unwrap();
        assert!(!app.existing_path_focus_is_current(&stamp));
        assert!(app.status.as_ref().unwrap().text.contains("newer failure"));
    }
}
#[test]
fn same_value_error_direct_result_clear_and_expiry_retire_error_ownership() {
    let (mut app, _) = app_fixture();
    app.clock_override = Some(Instant::now());
    app.set_status("identical", true);
    let first = app.existing_path_focus_stamp().unwrap();
    let status = app.status.clone();
    app.set_status("identical", true);
    assert_eq!(status, app.status);
    assert!(!app.existing_path_focus_is_current(&first));
    let second = app.existing_path_focus_stamp().unwrap();
    let before = app.operation_state();
    let source = std::array::from_fn(|i| {
        (
            before.panes[i].folder.as_ref().map(|f| f.path.clone()),
            before.panes[i].listing_generation,
        )
    });
    app.apply_operation_result(OperationResult {
        epoch: app.operation_epoch.load(Ordering::Acquire),
        navigation_ticket: None,
        source,
        state: before.clone(),
        before,
        status,
        listings: vec![],
        host_requests: vec![],
    });
    assert!(!app.existing_path_focus_is_current(&second));
    let third = app.existing_path_focus_stamp().unwrap();
    app.clear_status();
    assert!(app.existing_paths.error_owner.is_none());
    assert!(!app.existing_path_focus_is_current(&third));
    app.set_status("expiring", true);
    let expiring = app.existing_path_focus_stamp().unwrap();
    app.clock_override = Some(app.now() + STATUS_TTL);
    app.expire_status();
    assert!(app.existing_paths.error_owner.is_none());
    assert!(!app.existing_path_focus_is_current(&expiring));
}
#[test]
fn own_error_focus_is_observed_before_tick_and_unrelated_job_progress_keeps_success() {
    let _lane = TEST_LANE.lock().unwrap();
    let (mut app, root) = app_fixture();
    ready(&mut app, root.join("missing"), ExistingPathKind::Folder);
    app.tick();
    let receipt = app.take_existing_path_receipts().pop().unwrap();
    assert!(receipt.result.is_err());
    assert!(app.existing_path_focus_is_current(&receipt.focus_stamp.unwrap()));
    assert_eq!(app.focus_generation, 1);
    app.clear_status();
    ready(&mut app, root, ExistingPathKind::Folder);
    app.tick();
    let stamp = app
        .take_existing_path_receipts()
        .pop()
        .unwrap()
        .focus_stamp
        .unwrap();
    app.jobs.push(Job {
        id: 7,
        kind: JobKind::Copy,
        overwrite: OverwritePolicy::AutoRename,
        paths: vec![],
        dest_dir: String::new(),
        label: "progress".into(),
        total_bytes: Some(100),
        copied_bytes: 0,
        current: String::new(),
        status: JobStatus::Running,
        started_at: Instant::now(),
        control: JobControl::new(),
    });
    let revision = app.revision;
    app.job_tx
        .send(JobEvent::Progress {
            id: 7,
            copied_bytes: 10,
            current: "progress".into(),
        })
        .unwrap();
    app.tick();
    assert!(app.revision > revision);
    assert!(app.existing_path_focus_is_current(&stamp));
    app.set_status("ordinary notice", false);
    assert!(app.existing_path_focus_is_current(&stamp));
    app.document_generation += 1;
    assert!(!app.existing_path_focus_is_current(&stamp));
}
