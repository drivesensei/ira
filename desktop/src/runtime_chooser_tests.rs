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

// Each case waits for its actual physical worker receipt; all paths stay task-owned.
#[test]
fn settled_receipt_cannot_recapture_newer_error_and_own_success_error_restore_once() {
    let (mut app, mut actor, tx, rx) = actor();
    let root = app
        .state_path
        .as_ref()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let mut sequence = 1;
    for (failure, peer) in [(false, false), (true, false), (false, true), (true, true)] {
        app.clear_status();
        actor
            .apply(
                &mut app,
                envelope(sequence, Command::Browse(ChooserKind::Folder)),
                &tx,
            )
            .unwrap();
        sequence += 1;
        let ChooserEvent::Prompt(admitted) = rx.recv().unwrap() else {
            panic!()
        };
        actor
            .apply(
                &mut app,
                envelope(
                    sequence,
                    Command::ChooserResult {
                        ticket: admitted.ticket,
                        outcome: Arc::new(ChooserOutcome::Selected(if failure {
                            root.join("missing")
                        } else {
                            root.clone()
                        })),
                    },
                ),
                &tx,
            )
            .unwrap();
        sequence += 1;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            app.tick();
            let completed = if failure {
                app.status.as_ref().is_some_and(|s| s.is_error)
            } else {
                app.panes[0]
                    .folder
                    .as_ref()
                    .is_some_and(|f| f.path == root.to_str().unwrap())
                    && app.panes[0].listing_generation != admitted.context.listing
            };
            if completed {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        if peer {
            // The matching receipt already exists, but authority now belongs to a peer.
            let text = app
                .status
                .as_ref()
                .map(|s| s.text.clone())
                .unwrap_or_else(|| "peer failure".into());
            app.set_status(text, true);
        }
        actor.drain(&mut app, &tx);
        if peer {
            assert!(rx.try_recv().is_err());
        } else {
            let ChooserEvent::RestoreFocus(stamp) = rx.recv().unwrap() else {
                panic!()
            };
            assert!(app.existing_path_focus_is_current(&stamp));
            app.set_status("later unrelated error", true);
            assert!(!app.existing_path_focus_is_current(&stamp));
        }
        actor.drain(&mut app, &tx);
        assert!(rx.try_recv().is_err());
        assert!(actor.pending.is_none());
    }
}
#[test]
fn immediate_helper_error_owns_one_current_stamp_cancel_keeps_job_revision_permission() {
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
    actor
        .apply(
            &mut app,
            envelope(
                2,
                Command::ChooserResult {
                    ticket: admitted.ticket,
                    outcome: Arc::new(ChooserOutcome::Error("own native error".into())),
                },
            ),
            &tx,
        )
        .unwrap();
    let ChooserEvent::RestoreFocus(stamp) = rx.recv().unwrap() else {
        panic!()
    };
    assert!(app.existing_path_focus_is_current(&stamp));
    app.set_status("own native error", true);
    assert!(!app.existing_path_focus_is_current(&stamp));
    app.clear_status();
    actor
        .apply(
            &mut app,
            envelope(3, Command::Browse(ChooserKind::File)),
            &tx,
        )
        .unwrap();
    let ChooserEvent::Prompt(admitted) = rx.recv().unwrap() else {
        panic!()
    };
    app.revision += 10;
    actor
        .apply(
            &mut app,
            envelope(
                4,
                Command::ChooserResult {
                    ticket: admitted.ticket,
                    outcome: Arc::new(ChooserOutcome::Canceled),
                },
            ),
            &tx,
        )
        .unwrap();
    let ChooserEvent::RestoreFocus(stamp) = rx.recv().unwrap() else {
        panic!()
    };
    assert!(app.existing_path_focus_is_current(&stamp));
    app.invalidate_existing_path_requests();
    assert!(!app.existing_path_focus_is_current(&stamp));
}
