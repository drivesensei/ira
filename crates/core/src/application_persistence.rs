//! Persistence-only receipts and sealed retries; no business commands are retained.
use super::*;
use crate::services::persistence::PersistenceError;
use crate::services::{
    bookmarks::{try_write_bookmarks, try_write_bookmarks_to},
    state::{try_save_state, try_save_state_to},
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistenceReceipt {
    pub epoch: u64,
}
#[derive(Clone, Debug)]
pub struct PersistenceFailure {
    pub epoch: u64,
    pub errors: Vec<PersistenceError>,
    pub retry: PersistenceRetry,
}
#[derive(Clone)]
pub struct PersistenceRetry {
    shared: Arc<Mutex<Ledger>>,
    sealed: Arc<Vec<FailedWrite>>,
    epoch: u64,
    busy: Arc<AtomicBool>,
}
impl std::fmt::Debug for PersistenceRetry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersistenceRetry")
            .field("epoch", &self.epoch)
            .field("payloads", &self.sealed.len())
            .finish()
    }
}
#[derive(Clone)]
enum Payload {
    State(Option<PathBuf>, SessionState),
    Bookmarks(Option<PathBuf>, Vec<Folder>),
}
impl Payload {
    fn key(&self) -> (bool, Option<PathBuf>) {
        match self {
            Self::State(p, _) => (false, p.clone()),
            Self::Bookmarks(p, _) => (true, p.clone()),
        }
    }
    fn write(&self) -> Result<(), PersistenceError> {
        match self {
            Self::State(Some(p), s) => try_save_state_to(p, s),
            Self::State(None, s) => try_save_state(s),
            Self::Bookmarks(Some(p), b) => try_write_bookmarks_to(p, b),
            Self::Bookmarks(None, b) => try_write_bookmarks(b),
        }
    }
}
#[derive(Clone)]
struct FailedWrite {
    epoch: u64,
    payload: Payload,
    error: PersistenceError,
}
#[derive(Default)]
struct Ledger {
    epoch: u64,
    latest: HashMap<(bool, Option<PathBuf>), u64>,
    failed: Vec<FailedWrite>,
    busy: Arc<AtomicBool>,
}
impl Ledger {
    fn write(&mut self, payload: Payload) {
        self.epoch += 1;
        let key = payload.key();
        self.latest.insert(key.clone(), self.epoch);
        self.failed.retain(|f| f.payload.key() != key);
        if let Err(error) = payload.write() {
            self.failed.push(FailedWrite {
                epoch: self.epoch,
                payload,
                error,
            });
        }
    }
    fn outcome(&self, shared: &Arc<Mutex<Self>>) -> Result<PersistenceReceipt, PersistenceFailure> {
        if self.failed.is_empty() {
            Ok(PersistenceReceipt { epoch: self.epoch })
        } else {
            Err(PersistenceFailure {
                epoch: self.epoch,
                errors: self.failed.iter().map(|f| f.error.clone()).collect(),
                retry: PersistenceRetry {
                    shared: shared.clone(),
                    sealed: Arc::new(self.failed.clone()),
                    epoch: self.epoch,
                    busy: self.busy.clone(),
                },
            })
        }
    }
}
impl PersistenceRetry {
    /// Starts at most one retry for this sealed receipt at a time. A dropped
    /// receiver does not cancel publication. All filesystem work is off-thread.
    pub fn retry(&self) -> mpsc::Receiver<Result<PersistenceReceipt, PersistenceFailure>> {
        let (tx, rx) = mpsc::channel();
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            let _ = tx.send(Err(PersistenceFailure {
                epoch: self.epoch,
                errors: vec![PersistenceError {
                    path: None,
                    stage: "retry",
                    message: "Retry is already pending".into(),
                }],
                retry: self.clone(),
            }));
            return rx;
        }
        let retry = self.clone();
        let failed_retry = self.clone();
        let failed_reply = tx.clone();
        let spawned = thread::Builder::new()
            .name("ira-persistence-retry".into())
            .spawn(move || {
                let mut ledger = retry.shared.lock().unwrap_or_else(|e| e.into_inner());
                for failed in retry.sealed.iter() {
                    let key = failed.payload.key();
                    // Never overwrite a newer accepted state with an older retry.
                    if ledger.latest.get(&key) != Some(&failed.epoch) {
                        continue;
                    }
                    if !ledger.failed.iter().any(|f| f.epoch == failed.epoch) {
                        continue;
                    }
                    match failed.payload.write() {
                        Ok(()) => ledger.failed.retain(|f| f.epoch != failed.epoch),
                        Err(error) => {
                            if let Some(f) =
                                ledger.failed.iter_mut().find(|f| f.epoch == failed.epoch)
                            {
                                f.error = error;
                            }
                        }
                    }
                }
                let outcome = ledger.outcome(&retry.shared);
                retry.busy.store(false, Ordering::Release);
                let _ = tx.send(outcome);
            });
        if let Err(error) = spawned {
            failed_retry.busy.store(false, Ordering::Release);
            let _ = failed_reply.send(Err(PersistenceFailure {
                epoch: failed_retry.epoch,
                errors: vec![PersistenceError {
                    path: None,
                    stage: "retry spawn",
                    message: error.to_string(),
                }],
                retry: failed_retry,
            }));
        }
        rx
    }
}
pub(super) fn worker(rx: mpsc::Receiver<PersistenceRequest>) {
    let shared = Arc::new(Mutex::new(Ledger::default()));
    worker_with_ledger(rx, shared);
}
fn worker_with_ledger(rx: mpsc::Receiver<PersistenceRequest>, shared: Arc<Mutex<Ledger>>) {
    while let Ok(request) = rx.recv() {
        let mut ledger = shared.lock().unwrap_or_else(|e| e.into_inner());
        match request {
            PersistenceRequest::State(p, s) => ledger.write(Payload::State(p, s)),
            PersistenceRequest::Bookmarks(p, b) => ledger.write(Payload::Bookmarks(p, b)),
            PersistenceRequest::Barrier(tx) => {
                let _ = tx.send(());
            }
            PersistenceRequest::CheckedBarrier(tx) => {
                let _ = tx.send(ledger.outcome(&shared));
            }
        }
    }
}

#[cfg(test)]
#[path = "application_persistence_worker_tests.rs"]
mod tests;
