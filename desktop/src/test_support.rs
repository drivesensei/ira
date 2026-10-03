//! Test-only fixture lifetime; never changes production admission or environment.
use std::{
    cell::RefCell,
    path::PathBuf,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
static FIXTURE_LIFETIME: Mutex<()> = Mutex::new(());
static NEXT: AtomicU64 = AtomicU64::new(1);
thread_local! { static CURRENT: RefCell<Option<Arc<Record>>> = const { RefCell::new(None) }; }
pub(crate) struct Record {
    pub directory: PathBuf,
    actors: Mutex<Vec<(Arc<AtomicBool>, Arc<AtomicBool>)>>,
}
impl Record {
    pub fn register(&self, stop: Arc<AtomicBool>, acknowledgment: Arc<AtomicBool>) {
        self.actors.lock().unwrap().push((stop, acknowledgment));
    }
}
pub(crate) fn current() -> Option<Arc<Record>> {
    CURRENT.with(|slot| slot.borrow().clone())
}
pub(crate) struct FixtureScope {
    _exclusive: MutexGuard<'static, ()>,
    record: Arc<Record>,
}
pub(crate) fn enter() -> FixtureScope {
    let exclusive = FIXTURE_LIFETIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !crate::views::accessibility_worker::Worker::reclamation_complete() {
        assert!(
            Instant::now() < deadline,
            "previous fixture's physical ownership must complete before new TTL/admission fixture starts"
        );
        std::thread::yield_now();
    }
    let directory = std::env::temp_dir().join(format!(
        "ira-headless-scope-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory).unwrap();
    std::fs::create_dir(directory.join("preview-temp")).unwrap();
    let record = Arc::new(Record {
        directory,
        actors: Mutex::new(Vec::new()),
    });
    CURRENT.with(|slot| *slot.borrow_mut() = Some(record.clone()));
    FixtureScope {
        _exclusive: exclusive,
        record,
    }
}
impl Drop for FixtureScope {
    fn drop(&mut self) {
        CURRENT.with(|slot| slot.borrow_mut().take());
        let actors = self.record.actors.lock().unwrap().clone();
        for (stop, _) in &actors {
            stop.store(true, Ordering::Release);
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while actors
            .iter()
            .any(|(_, acknowledgment)| !acknowledgment.load(Ordering::Acquire))
        {
            if Instant::now() >= deadline {
                break;
            }
            std::thread::yield_now();
        }
        // Keep these task-owned directories for evidence: preview/other workers may
        // still retain paths, and the legacy drain is not a Result-bearing write ACK.
    }
}
