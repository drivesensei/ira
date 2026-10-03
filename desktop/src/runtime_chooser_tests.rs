use super::*;
fn actor() -> (
    App,
    ActorChooser,
    mpsc::Sender<ChooserEvent>,
    mpsc::Receiver<ChooserEvent>,
) {
    let root = std::env::temp_dir().join(format!(
        "ira-chooser-actor-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let mut app = App::default();
    app.state_path = Some(root.join("state"));
    app.bookmarks_path = Some(root.join("bookmarks"));
    app.window_generation = 7;
    app.panes[0].listing_settled = true;
    let (tx, rx) = mpsc::channel();
    (app, ActorChooser::default(), tx, rx)
}
fn envelope(sequence: u64, command: Command) -> Envelope {
    Envelope {
        sequence,
        window_generation: 7,
        input_generation: None,
        command,
    }
}
#[test]
fn admitted_cancel_preserves_pane_filter_status_and_emits_captured_focus() {
    let (mut app, mut actor, tx, rx) = actor();
    app.panes[0].filter_query = Some("keep".into());
    actor
        .apply(
            &mut app,
            envelope(1, Command::Browse(ChooserKind::File)),
            &tx,
        )
        .unwrap();
    let ChooserEvent::Prompt(admitted) = rx.recv().unwrap() else {
        panic!()
    };
    let status = app.status.clone();
    let folder = app.panes[0].folder.clone();
    actor
        .apply(
            &mut app,
            envelope(
                2,
                Command::ChooserResult {
                    ticket: admitted.ticket,
                    outcome: Arc::new(ChooserOutcome::Canceled),
                },
            ),
            &tx,
        )
        .unwrap();
    assert_eq!(app.panes[0].folder, folder);
    assert_eq!(app.panes[0].filter_query.as_deref(), Some("keep"));
    assert_eq!(app.status, status);
    assert!(
        matches!(rx.recv().unwrap(),ChooserEvent::RestoreFocus(permit) if permit.window==7&&permit.pane==0)
    );
    assert!(actor.pending.is_none());
}
#[test]
fn stale_error_after_away_back_is_silent_and_pending_input_cannot_mutate() {
    let (mut app, mut actor, tx, rx) = actor();
    actor
        .apply(
            &mut app,
            envelope(1, Command::Browse(ChooserKind::Folder)),
            &tx,
        )
        .unwrap();
    let ChooserEvent::Prompt(admitted) = rx.recv().unwrap() else {
        panic!()
    };
    assert!(
        actor
            .apply(&mut app, envelope(2, Command::FocusPane(0)), &tx)
            .is_err()
    );
    assert!(
        actor
            .apply(
                &mut app,
                envelope(
                    3,
                    Command::Input(Input::Key(KeyEvent::new(
                        KeyCode::Char('n'),
                        KeyModifiers::NONE
                    )))
                ),
                &tx
            )
            .is_err()
    );
    assert!(app.new_entry.is_none());
    actor
        .apply(
            &mut app,
            envelope(
                4,
                Command::ChooserResult {
                    ticket: admitted.ticket,
                    outcome: Arc::new(ChooserOutcome::Error("must remain invisible".into())),
                },
            ),
            &tx,
        )
        .unwrap();
    assert!(app.status.is_none());
    assert!(rx.try_recv().is_err());
    assert!(actor.pending.is_none());
}
#[test]
fn unrelated_revision_does_not_cancel_admission_and_old_ticket_cannot_clear_new() {
    let (mut app, mut actor, tx, rx) = actor();
    actor
        .apply(
            &mut app,
            envelope(1, Command::Browse(ChooserKind::File)),
            &tx,
        )
        .unwrap();
    let ChooserEvent::Prompt(admitted) = rx.recv().unwrap() else {
        panic!()
    };
    app.revision += 10;
    assert!(actor.permit(&app).is_some());
    actor
        .apply(
            &mut app,
            envelope(
                2,
                Command::ChooserResult {
                    ticket: ChooserTicket {
                        request_id: 99,
                        kind: ChooserKind::File,
                    },
                    outcome: Arc::new(ChooserOutcome::Canceled),
                },
            ),
            &tx,
        )
        .unwrap();
    assert_eq!(actor.pending.as_ref().unwrap().ticket, admitted.ticket);
}

#[test]
fn chooser_input_gate_does_not_block_existing_control_c_shutdown() {
    let (mut app, mut actor, tx, _) = actor();
    actor
        .apply(
            &mut app,
            envelope(1, Command::Browse(ChooserKind::Folder)),
            &tx,
        )
        .unwrap();
    actor
        .apply(
            &mut app,
            envelope(
                2,
                Command::Input(Input::Key(KeyEvent::new(
                    KeyCode::Char('c'),
                    KeyModifiers::CONTROL,
                ))),
            ),
            &tx,
        )
        .unwrap();
    assert!(!app.running);
    assert!(actor.pending.is_some());
}
