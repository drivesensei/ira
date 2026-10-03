use super::*;

#[test]
fn opaque_seal_is_instance_bound_and_permanently_rejects_new_accepted_workers() {
    let root = std::env::temp_dir().join(format!(
        "ira-t066-sealed-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let protected = root.join("protected");
    std::fs::write(&protected, b"owned sealed bytes").unwrap();
    let mut app = App::default();
    app.state_path = Some(root.join("state"));
    let mut other = App::default();
    let seal = app.begin_exit_settlement();
    assert!(matches!(
        other.poll_exit_settlement(&seal, 1),
        ExitWorkPoll::Error(WorkSettlementError::ForeignSeal)
    ));
    let repeated = app.begin_exit_settlement();
    assert!(matches!(
        app.poll_exit_settlement(&repeated, 1),
        ExitWorkPoll::Settled
    ));
    app.confirming = Some(Confirm {
        action: ConfirmAction::Delete,
        policy: OverwritePolicy::AutoRename,
        label: "denied".into(),
        paths: vec![protected.to_string_lossy().into_owned()],
        dest_dir: None,
    });
    app.confirm_delete();
    assert!(app.deletion.is_none());
    assert!(app.exit_work.is_empty());
    assert_eq!(std::fs::read(protected).unwrap(), b"owned sealed bytes");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn checked_identity_overflow_denies_registration_before_issue() {
    let mut app = App::default();
    app.next_exit_work_id = u64::MAX;
    assert!(matches!(
        app.register_exit_work(JobControl::new()),
        Err(WorkSettlementError::IdentityExhausted)
    ));
    assert!(app.exit_work.is_empty());
}

#[test]
fn lost_receipt_waits_other_live_receipts_and_rotating_budget_reaches_every_owner() {
    let mut app = App::default();
    let lost = app.register_exit_work(JobControl::new()).unwrap();
    drop(lost);
    let live = app.register_exit_work(JobControl::new()).unwrap();
    let seal = app.begin_exit_settlement();
    assert!(matches!(
        app.poll_exit_settlement(&seal, 1),
        ExitWorkPoll::Pending
    ));
    assert!(matches!(
        app.poll_exit_settlement(&seal, 1),
        ExitWorkPoll::Pending
    ));
    live.acknowledge();
    assert!(matches!(
        app.poll_exit_settlement(&seal, 1),
        ExitWorkPoll::Error(WorkSettlementError::ReceiptLost { .. })
    ));
}

#[test]
fn actual_editor_ctrl_s_finishes_owned_write_synchronously_before_ctrl_c_quit() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let root = std::env::temp_dir().join(format!(
        "ira-t066-editor-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let file = root.join("document.txt");
    std::fs::write(&file, b"owned document\r\n").unwrap();
    let mut app = App::default();
    app.folders = None;
    app.drives = None;
    app.state_path = Some(root.join("state"));
    app.panes[0].preview_mode = PreviewMode::Column;
    app.panes[0].files = vec![FEntry {
        path: file.to_string_lossy().into_owned(),
        label: "document.txt".into(),
        is_dir: false,
        size: 16,
        modified: None,
    }];
    app.panes[0].state.select(Some(0));
    app.switch_pane();
    assert!(app.edit.is_some());
    assert!(app.edit_focus);
    crate::handler::handle_key_events(
        KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
        &mut app,
    )
    .unwrap();
    assert!(app.running, "plain q inserts in focused editor");
    assert_eq!(std::fs::read(&file).unwrap(), b"owned document\r\n");
    crate::handler::handle_key_events(
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        &mut app,
    )
    .unwrap();
    assert_eq!(std::fs::read(&file).unwrap(), b"qowned document\r\n");
    assert!(!app.edit.as_ref().unwrap().dirty);
    assert!(
        app.exit_work.is_empty(),
        "synchronous editor creates no asynchronous receipt obligation"
    );
    crate::handler::handle_key_events(
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        &mut app,
    )
    .unwrap();
    assert!(!app.running);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn actual_normal_and_board_q_and_modal_control_c_keep_root_exit_scope() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    for board in [false, true] {
        let mut app = App::default();
        app.board_focused = board;
        crate::handler::handle_key_events(
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
            &mut app,
        )
        .unwrap();
        assert!(!app.running);
    }
    let mut app = App::default();
    app.goto_prompt = Some("owned pending text".into());
    crate::handler::handle_key_events(
        KeyEvent::new(KeyCode::Char('C'), KeyModifiers::CONTROL),
        &mut app,
    )
    .unwrap();
    assert!(!app.running);
}
