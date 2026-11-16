use super::*;
use crate::app::{Confirm, ConfirmAction, Status, WorkSettlementError};
use crate::services::persistence::PersistenceError;
use crate::services::transfer::{JobKind, OverwritePolicy, WorkerTestHooks};
use std::fs;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

impl ExitApp for crate::app::App {
    type Seal = crate::app::ExitWorkSeal;
    type SaveError = crate::services::persistence::PersistenceError;
    fn begin_exit_work(&mut self) -> Self::Seal {
        self.begin_exit_settlement()
    }
    fn poll_exit_work(&mut self, seal: &Self::Seal, budget: usize) -> AppResult<bool> {
        match self.poll_exit_settlement(seal, budget) {
            crate::app::ExitWorkPoll::Pending => Ok(false),
            crate::app::ExitWorkPoll::Settled => Ok(true),
            crate::app::ExitWorkPoll::Error(error) => Err(error.into()),
        }
    }
    fn save_exit_state(&self) -> Result<(), PersistenceError> {
        self.try_persist_state()
    }
}

const WAIT: Duration = Duration::from_secs(5);
const HELD: Duration = Duration::from_millis(100);
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "ira-t066-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("dest")).unwrap();
        fs::write(root.join("source"), b"positive owned bytes").unwrap();
        fs::write(root.join("delete"), b"owned deletion bytes").unwrap();
        fs::write(root.join("state"), b"old state sentinel").unwrap();
        Self { root }
    }
}
// Retain ALL unique owned fixture paths. Neither helper nor ACK under attack
// authenticates the detached worker's physical completion for deletion.
struct ReleaseGuard {
    sender: Option<mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl ReleaseGuard {
    fn release(&mut self) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(());
        }
    }
    fn join(&mut self) {
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
impl Drop for ReleaseGuard {
    fn drop(&mut self) {
        self.release();
        if self.thread.as_ref().is_some_and(|t| t.is_finished()) {
            self.join();
        }
    }
}
#[derive(Debug)]
enum Outcome {
    Saved,
    SaveFailed(PersistenceError),
    SettlementFailed(WorkSettlementError),
    CleanupFailed(String),
}
struct Observation {
    outcome: Outcome,
    expected_save_error: Option<PersistenceError>,
    hint: usize,
    status: bool,
}
#[derive(Clone, Copy)]
enum Worker {
    Transfer,
    Delete,
}
#[derive(Clone, Copy)]
enum Prior {
    None,
    Settled,
    Lost,
}

fn exercise(
    worker: Worker,
    save_fails: bool,
    cleanup_fails: bool,
    lose_receipt: bool,
    prior: Prior,
) {
    let prior_lost = matches!(prior, Prior::Lost);
    let f = Fixture::new();
    if save_fails {
        fs::write(f.root.join("blocked"), b"old parent").unwrap();
    }
    let root = f.root.clone();
    let cleanup_count = Arc::new(AtomicUsize::new(0));
    let count = cleanup_count.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let (business_entered_tx, business_entered_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let mut app = crate::app::App::default();
        app.folders = None;
        app.drives = None;
        app.bookmarks = Some(Vec::new());
        app.state_path = Some(root.join("expected-state"));
        app.try_persist_state().unwrap();
        let expected_state = fs::read(root.join("expected-state")).unwrap();
        app.state_path = Some(if save_fails {
            root.join("blocked/state")
        } else {
            root.join("state")
        });
        let expected_save_error = if save_fails {
            Some(app.try_persist_state().unwrap_err())
        } else {
            None
        };
        if !matches!(prior, Prior::None) {
            app.exit_worker_test = Some(WorkerTestHooks {
                lose_receipt: prior_lost,
                ..Default::default()
            });
            app.confirming = Some(Confirm {
                action: ConfirmAction::Delete,
                policy: OverwritePolicy::AutoRename,
                label: "prior".into(),
                paths: vec![root.join("delete").to_string_lossy().into_owned()],
                dest_dir: None,
            });
            app.confirm_delete();
            let deadline = Instant::now() + WAIT;
            loop {
                if matches!(
                    app_test_event(&mut app),
                    Some(crate::services::transfer::JobEvent::DeleteDone { .. })
                ) {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "prior real deletion never finished"
                );
                std::thread::yield_now();
            }
        }
        let gate = Arc::new(move || {
            entered_tx.send(()).unwrap();
            business_entered_tx.send(()).unwrap();
            release_rx.lock().unwrap().recv().unwrap();
        });
        let hooks = match worker {
            Worker::Transfer => {
                let public = root.join("dest/source");
                WorkerTestHooks {
                    provider: Some(Arc::new(move |src, dst| {
                        let result = crate::services::transfer::test_real_no_replace(src, dst);
                        if result.is_ok() && dst == public {
                            gate();
                        }
                        result
                    })),
                    lose_receipt,
                    ..Default::default()
                }
            }
            Worker::Delete => WorkerTestHooks {
                after_remove: Some(Arc::new(move |_| gate())),
                lose_receipt,
                ..Default::default()
            },
        };
        app.exit_worker_test = Some(hooks);
        app.confirming = Some(Confirm {
            action: match worker {
                Worker::Transfer => ConfirmAction::Copy,
                Worker::Delete => ConfirmAction::Delete,
            },
            policy: OverwritePolicy::AutoRename,
            label: "live".into(),
            paths: vec![root
                .join(match worker {
                    Worker::Transfer => "source",
                    Worker::Delete => "delete",
                })
                .to_string_lossy()
                .into_owned()],
            dest_dir: matches!(worker, Worker::Transfer)
                .then(|| root.join("dest").to_string_lossy().into_owned()),
        });
        app.confirm_pending();
        business_entered_rx
            .recv_timeout(WAIT)
            .expect("real mutation must enter before quit cancellation");
        app.jobs.clear();
        app.deletion = None;
        app.deletion_box_hidden = true;
        app.hint_offset = 77;
        app.status = Some(Status {
            text: "expired witness".into(),
            is_error: false,
            raised: Instant::now() - Duration::from_secs(20),
        });
        crate::handler::handle_key_events(
            ratatui::crossterm::event::KeyEvent::new(
                ratatui::crossterm::event::KeyCode::Char('c'),
                ratatui::crossterm::event::KeyModifiers::CONTROL,
            ),
            &mut app,
        )
        .unwrap();
        assert!(!app.running);
        ready_tx.send(()).unwrap();
        let result = finish_exit(&mut app, |_| {
            count.fetch_add(1, Ordering::SeqCst);
            if save_fails || lose_receipt || prior_lost {
                assert_eq!(fs::read(root.join("state")).unwrap(), b"old state sentinel");
            } else {
                assert_eq!(fs::read(root.join("state")).unwrap(), expected_state);
            }
            if cleanup_fails {
                Err(io::Error::other("owned cleanup failure").into())
            } else {
                Ok(())
            }
        });
        let outcome = match result {
            Ok(()) => Outcome::Saved,
            Err(error) => {
                if let Some(e) = error.downcast_ref::<PersistenceError>() {
                    Outcome::SaveFailed(e.clone())
                } else if let Some(e) = error.downcast_ref::<WorkSettlementError>() {
                    Outcome::SettlementFailed(e.clone())
                } else {
                    Outcome::CleanupFailed(error.to_string())
                }
            }
        };
        let observation = Observation {
            outcome,
            expected_save_error,
            hint: app.hint_offset,
            status: app.status.is_some(),
        };
        result_tx.send(observation).unwrap();
    });
    let mut guard = ReleaseGuard {
        sender: Some(release_tx),
        thread: Some(thread),
    };
    entered_rx
        .recv_timeout(WAIT)
        .expect("actual owned business I/O did not enter gate");
    ready_rx.recv_timeout(WAIT).unwrap();
    assert!(
        matches!(
            result_rx.recv_timeout(HELD),
            Err(mpsc::RecvTimeoutError::Timeout)
        ),
        "finish_exit returned while actual worker was held"
    );
    assert_eq!(
        fs::read(f.root.join("state")).unwrap(),
        b"old state sentinel"
    );
    assert_eq!(cleanup_count.load(Ordering::SeqCst), 0);
    match worker {
        Worker::Transfer => assert_eq!(
            fs::read(f.root.join("dest/source")).unwrap(),
            b"positive owned bytes"
        ),
        Worker::Delete => assert!(!f.root.join("delete").exists()),
    }
    guard.release();
    let observation = result_rx
        .recv_timeout(WAIT)
        .expect("exit did not settle after release");
    guard.join();
    assert_eq!(cleanup_count.load(Ordering::SeqCst), 1);
    assert_eq!(
        observation.hint, 77,
        "settlement must never call ordinary tick"
    );
    assert!(
        observation.status,
        "ordinary tick must not expire status while waiting"
    );
    match observation.outcome {
        Outcome::SettlementFailed(WorkSettlementError::ReceiptLost { .. })
            if lose_receipt || prior_lost => {}
        Outcome::SaveFailed(error) if save_fails && !lose_receipt && !prior_lost => {
            assert_eq!(Some(error), observation.expected_save_error)
        }
        Outcome::CleanupFailed(error)
            if cleanup_fails && !save_fails && !lose_receipt && !prior_lost =>
        {
            assert_eq!(error, "owned cleanup failure")
        }
        Outcome::Saved if !save_fails && !cleanup_fails && !lose_receipt && !prior_lost => {}
        outcome => panic!("wrong error priority/outcome: {outcome:?}"),
    }
    assert_eq!(
        fs::read(f.root.join("source")).unwrap(),
        b"positive owned bytes"
    );
    if matches!(worker, Worker::Transfer) {
        assert_eq!(
            fs::read(f.root.join("dest/source")).unwrap(),
            b"positive owned bytes"
        );
        assert_eq!(
            fs::read_dir(f.root.join("dest")).unwrap().count(),
            1,
            "staged suffix must finish before ACK"
        );
    }
}

fn app_test_event(app: &mut crate::app::App) -> Option<crate::services::transfer::JobEvent> {
    app.exit_test_event()
}
#[test]
fn actual_finish_exit_waits_real_transfer_before_four_case_save_cleanup_matrix() {
    for save_fails in [false, true] {
        for cleanup_fails in [false, true] {
            exercise(
                Worker::Transfer,
                save_fails,
                cleanup_fails,
                false,
                Prior::None,
            );
        }
    }
}
#[test]
fn actual_finish_exit_waits_hidden_real_delete_and_never_ticks() {
    exercise(Worker::Delete, false, false, false, Prior::None);
}
#[test]
fn lost_real_worker_receipt_skips_save_and_preserves_settlement_error_over_cleanup() {
    exercise(Worker::Transfer, false, true, true, Prior::None);
}
#[test]
fn prior_lost_receipt_cannot_release_later_live_hidden_worker() {
    exercise(Worker::Transfer, false, false, false, Prior::Lost);
}

#[test]
fn completed_prior_real_batch_does_not_discharge_live_hidden_batch() {
    exercise(Worker::Transfer, false, false, false, Prior::Settled);
}
