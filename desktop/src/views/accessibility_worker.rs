//! Window-owned preparation mailbox. All projection, indexing, diff and retirement
//! occurs on this worker; callers use only try_lock and retain rejected ownership.
use crate::platform::accessibility::{
    Rejection,
    model::{
        AccessibilityModel, FrameKey, HostPresentationSnapshot, LayoutSnapshot, MaterializedNodes,
        NativeTextSnapshot, NodeId, PreparedFrame, PreparedSemantic, RequestKey, SemanticTree,
        Target, prepare_frame_cancellable,
    },
};
use ira_core::observable::Snapshot;
use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub struct SemanticRequest {
    pub sequence: u64,
    pub key: RequestKey,
    pub snapshot: Arc<Snapshot>,
    pub text: Option<NativeTextSnapshot>,
    pub focused: Option<NodeId>,
    pub focused_target: Option<Target>,
    pub presentation: HostPresentationSnapshot,
}
pub struct LayoutRequest {
    pub key: FrameKey,
    pub semantic: Arc<PreparedSemantic>,
    pub layout: LayoutSnapshot,
    pub materialized: Arc<MaterializedNodes>,
}
pub enum PreparedResult {
    Semantic {
        sequence: u64,
        value: Result<Arc<PreparedSemantic>, Rejection>,
        elapsed: Duration,
    },
    Frame {
        key: FrameKey,
        value: Result<Arc<PreparedFrame>, Rejection>,
        elapsed: Duration,
    },
}
type Retirement = Box<dyn Send>;
#[derive(Default)]
struct Mailbox {
    semantic: Option<Box<SemanticRequest>>,
    layout: Option<Box<LayoutRequest>>,
    result: Option<PreparedResult>,
    ack: Option<Arc<PreparedFrame>>,
    seed: Option<(Arc<SemanticTree>, RequestKey)>,
    retired: Vec<Retirement>,
}
const PHYSICAL_LIMIT: usize = 4;
static PHYSICAL_OWNERS: AtomicUsize = AtomicUsize::new(0);
#[derive(Debug)]
pub struct Pressure;
#[derive(Clone)]
pub struct CompletionTicket(Arc<AtomicBool>);
impl CompletionTicket {
    pub fn is_complete(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
struct PhysicalPermit(CompletionTicket);
impl Drop for PhysicalPermit {
    fn drop(&mut self) {
        self.0.0.store(true, Ordering::Release);
        PHYSICAL_OWNERS.fetch_sub(1, Ordering::AcqRel);
    }
}
struct Shared {
    mailbox: Mutex<Mailbox>,
    wake: Condvar,
    stopping: AtomicBool,
    latest: AtomicU64,
    _permit: PhysicalPermit,
}
/// One running preparation, one pending semantic request, one pending geometry
/// request and one completed result. At most32 ownership-retirement messages.
pub struct Worker {
    shared: Arc<Shared>,
    handed_off: bool,
}
impl Worker {
    pub fn has_capacity() -> bool {
        PHYSICAL_OWNERS.load(Ordering::Acquire) < PHYSICAL_LIMIT
    }
    pub fn reclamation_complete() -> bool {
        PHYSICAL_OWNERS.load(Ordering::Acquire) == 0
    }
    pub fn completion_ticket(&self) -> CompletionTicket {
        self.shared._permit.0.clone()
    }
    /// Admission reserves both live preparation and its future physical close.
    /// Failure creates no thread and captures no semantic ownership.
    pub fn try_new() -> Result<Self, Pressure> {
        let mut observed = PHYSICAL_OWNERS.load(Ordering::Acquire);
        loop {
            if observed >= PHYSICAL_LIMIT {
                return Err(Pressure);
            }
            match PHYSICAL_OWNERS.compare_exchange_weak(
                observed,
                observed + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(next) => observed = next,
            }
        }
        Ok(Self::from_reserved())
    }
    fn from_reserved() -> Self {
        let shared = Arc::new(Shared {
            mailbox: Mutex::new(Mailbox::default()),
            wake: Condvar::new(),
            stopping: AtomicBool::new(false),
            latest: AtomicU64::new(0),
            _permit: PhysicalPermit(CompletionTicket(Arc::new(AtomicBool::new(false)))),
        });
        let worker = shared.clone();
        thread::spawn(move || run(worker));
        Self {
            shared,
            handed_off: false,
        }
    }
    /// Failure returns the exact request: callers must retain its heavy ownership.
    pub fn request(&self, request: Box<SemanticRequest>) -> Result<(), Box<SemanticRequest>> {
        let Ok(mut state) = self.shared.mailbox.try_lock() else {
            return Err(request);
        };
        if self.shared.stopping.load(Ordering::Acquire) || state.retired.len() >= 32 {
            return Err(request);
        }
        self.shared
            .latest
            .store(request.sequence, Ordering::Release);
        if let Some(old) = state.semantic.replace(request) {
            state.retired.push(old);
        }
        self.shared.wake.notify_one();
        Ok(())
    }
    pub fn layout(&self, request: Box<LayoutRequest>) -> Result<(), Box<LayoutRequest>> {
        let Ok(mut state) = self.shared.mailbox.try_lock() else {
            return Err(request);
        };
        if self.shared.stopping.load(Ordering::Acquire) || state.retired.len() >= 32 {
            return Err(request);
        }
        if let Some(old) = state.layout.replace(request) {
            state.retired.push(old);
        }
        self.shared.wake.notify_one();
        Ok(())
    }
    /// Seed sequence0 from the ACTUALLY installed compatibility tree, then ACK only
    /// committed native installs (including installs whose notification delivery failed).
    /// O(1) foreground capture of the actual sink/cache tree; construction is off UI.
    /// Accepted seed is processed before any geometry request in the same mailbox.
    pub fn seed_actual_installed(
        &self,
        tree: Arc<SemanticTree>,
        key: RequestKey,
    ) -> Result<(), (Arc<SemanticTree>, RequestKey)> {
        let Ok(mut state) = self.shared.mailbox.try_lock() else {
            return Err((tree, key));
        };
        if self.shared.stopping.load(Ordering::Acquire) || state.seed.is_some() {
            return Err((tree, key));
        }
        state.seed = Some((tree, key));
        self.shared.wake.notify_one();
        Ok(())
    }
    pub fn acknowledge(&self, frame: Arc<PreparedFrame>) -> Result<(), Arc<PreparedFrame>> {
        let Ok(mut state) = self.shared.mailbox.try_lock() else {
            return Err(frame);
        };
        if self.shared.stopping.load(Ordering::Acquire) || state.ack.is_some() {
            return Err(frame);
        }
        state.ack = Some(frame);
        self.shared.wake.notify_one();
        Ok(())
    }
    pub fn take_result(&self) -> Option<PreparedResult> {
        let result = self.shared.mailbox.try_lock().ok()?.result.take();
        self.shared.wake.notify_one();
        result
    }
    /// Move ownership rather than dropping an old tree/index/snapshot on the UI.
    pub fn retire<T: Send + 'static>(&self, value: T) -> Result<(), T> {
        let Ok(mut state) = self.shared.mailbox.try_lock() else {
            return Err(value);
        };
        if state.retired.len() >= 32 {
            return Err(value);
        }
        state.retired.push(Box::new(value));
        self.shared.wake.notify_one();
        Ok(())
    }
    /// Stop computation immediately, retaining ownership for later native retirement.
    pub fn cancel(&self) {
        self.shared.stopping.store(true, Ordering::Release);
        self.shared.wake.notify_one();
    }
    /// Consuming close guarantees only one helper for this admitted window.
    /// Its permit survives native queue removal and every blocked destructor.
    pub fn close_with<T: Send + 'static>(mut self, pins: T) {
        self.cancel();
        self.handed_off = true;
        thread::spawn(move || {
            drop(pins);
            // run() owns all model/mailbox cleanup. Keep the final Shared pin on
            // this background helper until preparation physically exits.
            while Arc::strong_count(&self.shared) > 1 {
                thread::sleep(Duration::from_millis(1));
            }
            drop(self);
        });
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel();
        if !self.handed_off {
            let shared = self.shared.clone();
            thread::spawn(move || {
                while Arc::strong_count(&shared) > 1 {
                    thread::sleep(Duration::from_millis(1));
                }
                drop(shared);
            });
        }
    }
}

fn run(shared: Arc<Shared>) {
    let mut model = AccessibilityModel::default();
    let mut baseline: Option<Arc<PreparedFrame>> = None;
    let mut semantic_pin: Option<Arc<PreparedSemantic>> = None;
    let mut frame_pin: Option<Arc<PreparedFrame>> = None;
    let mut publication = 0u64;
    loop {
        let (semantic, layout, retired, seed) = {
            let Ok(mut state) = shared.mailbox.lock() else {
                break;
            };
            if shared.stopping.load(Ordering::Acquire) {
                break;
            }
            if state.semantic.is_none()
                && (state.layout.is_none() || state.result.is_some())
                && state.ack.is_none()
                && state.seed.is_none()
                && state.retired.is_empty()
            {
                let Ok(next) = shared.wake.wait_timeout(state, Duration::from_millis(100)) else {
                    break;
                };
                state = next.0;
                if shared.stopping.load(Ordering::Acquire) {
                    break;
                }
            }
            if let Some(ack) = state.ack.take()
                && baseline
                    .as_ref()
                    .is_none_or(|old| ack.publication_seq >= old.publication_seq)
            {
                publication = publication.max(ack.publication_seq);
                baseline = Some(ack);
            }
            let semantic = state.semantic.take();
            let layout = if semantic.is_none() && state.result.is_none() {
                state.layout.take()
            } else {
                None
            };
            (
                semantic,
                layout,
                std::mem::take(&mut state.retired),
                state.seed.take(),
            )
        };
        drop(retired);
        if let Some((tree, key)) = seed {
            match PreparedFrame::compatibility_baseline(tree, key) {
                Ok(installed) => {
                    publication = installed.publication_seq;
                    baseline = Some(Arc::new(installed));
                }
                Err(_) => {
                    continue;
                }
            }
        }
        let result = if let Some(request) = semantic {
            let start = Instant::now();
            let cancelled = || {
                shared.stopping.load(Ordering::Acquire)
                    || shared.latest.load(Ordering::Acquire) != request.sequence
            };
            let value = model
                .prepare_with_presentation_cancellable(
                    &request.snapshot,
                    request.text.as_ref(),
                    request.key,
                    request.focused,
                    &request.presentation,
                    &cancelled,
                )
                .and_then(|prepared| {
                    // Newly rendered controls can receive native focus before they existed
                    // in the previous index. Resolve against this fresh worker projection,
                    // then reuse provider policy for enabled/modal-scope validation.
                    let focused = request
                        .focused_target
                        .as_ref()
                        .and_then(|target| prepared.index.lookup(target, None, None));
                    if focused.is_some() && focused != request.focused {
                        model.prepare_with_presentation_cancellable(
                            &request.snapshot,
                            request.text.as_ref(),
                            request.key,
                            focused,
                            &request.presentation,
                            &cancelled,
                        )
                    } else {
                        Ok(prepared)
                    }
                })
                .map(Arc::new);
            if let Ok(value) = &value {
                semantic_pin = Some(value.clone());
            }
            Some(PreparedResult::Semantic {
                sequence: request.sequence,
                value,
                elapsed: start.elapsed(),
            })
        } else if let Some(request) = layout {
            let start = Instant::now();
            publication = publication.wrapping_add(1).max(1);
            let cancelled = || {
                shared.stopping.load(Ordering::Acquire)
                    || shared.latest.load(Ordering::Acquire) != request.key.request_seq
            };
            let value = prepare_frame_cancellable(
                baseline.as_deref(),
                request.semantic,
                request.key,
                request.layout,
                publication,
                &cancelled,
            )
            .and_then(|mut frame| {
                request.materialized.prepare_notifications(&mut frame)?;
                Ok(Arc::new(frame))
            });
            if let Ok(value) = &value {
                frame_pin = Some(value.clone());
            }
            Some(PreparedResult::Frame {
                key: request.key,
                value,
                elapsed: start.elapsed(),
            })
        } else {
            None
        };
        if let Some(result) = result {
            let Ok(mut state) = shared.mailbox.lock() else {
                break;
            };
            // Overwritten results and all their last ownership drop on THIS worker.
            state.result = Some(result);
        }
    }
    drop((semantic_pin, frame_pin, baseline, model));
    // Drain any queued/result/ACK ownership while this remains the final worker owner.
    if let Ok(mut state) = shared.mailbox.lock() {
        *state = Mailbox::default();
    }
}

#[cfg(test)]
mod tests {
    fn test_worker() -> super::Worker {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Ok(worker) = super::Worker::try_new() {
                return worker;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "test admission exceeded physical cleanup deadline"
            );
            std::thread::yield_now();
        }
    }

    use super::*;
    use crate::platform::accessibility::model::{Role, Target};
    use ira_core::{application::App, services::list_files::FEntry};
    fn request(sequence: u64, footer: &str) -> Box<SemanticRequest> {
        let mut app = App::default();
        app.window_generation = 7;
        app.panes[0].files = (0..1000)
            .map(|i| FEntry {
                path: format!("/fixture/{i}"),
                label: format!("file{i}"),
                is_dir: false,
                size: 1,
                modified: None,
            })
            .collect();
        app.panes[0].selected = vec![false; 1000];
        let snapshot = Arc::new(app.snapshot());
        Box::new(SemanticRequest {
            sequence,
            key: RequestKey {
                window_generation: 7,
                semantic_revision: snapshot.revision,
                document_generation: snapshot.document_generation,
                focus_generation: snapshot.focus_generation,
                native_text_revision: 0,
                host_focus_revision: 0,
                host_presentation_revision: sequence,
            },
            snapshot,
            text: None,
            focused: None,
            focused_target: None,
            presentation: HostPresentationSnapshot {
                revision: sequence,
                footer: footer.into(),
            },
        })
    }
    fn result(worker: &Worker) -> PreparedResult {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(result) = worker.take_result() {
                return result;
            }
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
    }
    #[test]
    fn footer_projection_uses_host_revision_without_core_mutation() {
        let _fixture_scope = crate::test_support::enter();
        let worker = test_worker();
        let first = request(1, "Enter: rename · Right: open · /: search · Space: select");
        let core_revision = first.snapshot.revision;
        assert!(worker.request(first).is_ok());
        let PreparedResult::Semantic {
            value: Ok(first), ..
        } = result(&worker)
        else {
            panic!("semantic")
        };
        let id = first
            .index
            .lookup(&Target::Window, Some(Role::Status), None)
            .unwrap();
        assert_eq!(
            first.tree.nodes[&id].value.as_deref(),
            Some("Enter: rename · Right: open · /: search · Space: select")
        );
        let second = request(2, "Native feedback");
        assert_eq!(second.snapshot.revision, core_revision);
        assert!(worker.request(second).is_ok());
        let PreparedResult::Semantic {
            value: Ok(second), ..
        } = result(&worker)
        else {
            panic!("semantic")
        };
        assert_eq!(
            second
                .index
                .lookup(&Target::Window, Some(Role::Status), None),
            Some(id)
        );
        assert_eq!(
            second.tree.nodes[&id].value.as_deref(),
            Some("Native feedback")
        );
        assert_eq!(second.key.semantic_revision, first.key.semantic_revision);
        assert_ne!(
            second.key.host_presentation_revision,
            first.key.host_presentation_revision
        );
        worker.close_with((first, second));
    }
    fn semantic(worker: &Worker, sequence: u64, footer: &str) -> Arc<PreparedSemantic> {
        assert!(worker.request(request(sequence, footer)).is_ok());
        let PreparedResult::Semantic {
            value: Ok(value), ..
        } = result(worker)
        else {
            panic!("semantic")
        };
        value
    }
    fn frame(
        worker: &Worker,
        semantic: Arc<PreparedSemantic>,
        sequence: u64,
        registry: Arc<MaterializedNodes>,
    ) -> Arc<PreparedFrame> {
        let key = FrameKey {
            request: semantic.key,
            request_seq: sequence,
            layout_revision: sequence,
        };
        let layout = LayoutSnapshot {
            window_generation: 7,
            semantic_revision: semantic.key.semantic_revision,
            revision: sequence,
            nodes: Default::default(),
        };
        assert!(
            worker
                .layout(Box::new(LayoutRequest {
                    key,
                    semantic,
                    layout,
                    materialized: registry
                }))
                .is_ok()
        );
        let PreparedResult::Frame {
            value: Ok(value), ..
        } = result(worker)
        else {
            panic!("frame")
        };
        value
    }
    #[test]
    fn discarded_preparation_is_not_notification_baseline() {
        let _fixture_scope = crate::test_support::enter();
        let worker = test_worker();
        let installed = semantic(&worker, 1, "installed");
        let id = installed
            .index
            .lookup(&Target::Window, Some(Role::Status), None)
            .unwrap();
        let baseline = Arc::new(
            PreparedFrame::compatibility_baseline(installed.tree.clone(), installed.key).unwrap(),
        );
        assert!(worker.acknowledge(baseline.clone()).is_ok());
        let registry = Arc::new(MaterializedNodes::default());
        registry.record(id).unwrap();
        let discarded = semantic(&worker, 2, "computed-but-never-installed");
        let discarded_frame = frame(&worker, discarded.clone(), 2, registry.clone());
        assert_eq!(discarded_frame.base_publication_seq, 0);
        // Deliberately no ACK for this computed result.
        let newest = semantic(&worker, 3, "newest");
        let latest = frame(&worker, newest.clone(), 3, registry.clone());
        assert_eq!(latest.base_publication_seq, 0);
        assert_eq!(
            latest.notifications.values[&id],
            (Some("installed".into()), Some("newest".into()))
        );
        worker.close_with((
            installed,
            baseline,
            registry,
            discarded,
            discarded_frame,
            newest,
            latest,
        ));
    }
    #[test]
    fn layout_pressure_cannot_overwrite_unconsumed_new_semantics() {
        let _fixture_scope = crate::test_support::enter();
        let worker = test_worker();
        let old = semantic(&worker, 1, "old");
        let registry = Arc::new(MaterializedNodes::default());
        let held = worker.shared.mailbox.lock().unwrap();
        // Queue directly under the test lock so the adverse ordering is deterministic.
        drop(held);
        assert!(worker.request(request(2, "newest")).is_ok());
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let ready = worker.shared.mailbox.lock().unwrap().result.is_some();
            if ready {
                break;
            }
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        let key = FrameKey {
            request: old.key,
            request_seq: 1,
            layout_revision: 1,
        };
        assert!(
            worker
                .layout(Box::new(LayoutRequest {
                    key,
                    semantic: old.clone(),
                    layout: LayoutSnapshot {
                        window_generation: 7,
                        semantic_revision: old.key.semantic_revision,
                        revision: 1,
                        nodes: Default::default()
                    },
                    materialized: registry.clone()
                }))
                .is_ok()
        );
        // Wait for a worker turn while deliberately leaving semantic completion occupied.
        thread::sleep(Duration::from_millis(20));
        let PreparedResult::Semantic {
            sequence: 2,
            value: Ok(newest),
            ..
        } = result(&worker)
        else {
            panic!("geometry overwrote semantic completion")
        };
        worker.close_with((old, newest, registry));
    }

    #[test]
    fn full_pending_mailbox_never_discards_caller_ownership() {
        let _fixture_scope = crate::test_support::enter();
        let worker = test_worker();
        let shared = worker.shared.clone();
        let lock = shared.mailbox.lock().unwrap();
        let input = request(3, "pending");
        let pointer = Arc::as_ptr(&input.snapshot);
        let returned = match worker.request(input) {
            Err(value) => value,
            Ok(()) => panic!("try-lock must refuse"),
        };
        assert_eq!(Arc::as_ptr(&returned.snapshot), pointer);
        assert_eq!(returned.sequence, 3);
        drop(lock);
        worker.close_with(returned);
    }
    #[test]
    fn retire_and_close_drop_heavy_ownership_off_calling_thread() {
        let _fixture_scope = crate::test_support::enter();
        struct Probe {
            thread: thread::ThreadId,
            tx: std::sync::mpsc::Sender<bool>,
        }
        impl Drop for Probe {
            fn drop(&mut self) {
                let _ = self.tx.send(thread::current().id() != self.thread);
            }
        }
        let worker = test_worker();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut probe = Probe {
            thread: thread::current().id(),
            tx: tx.clone(),
        };
        loop {
            match worker.retire(probe) {
                Ok(()) => break,
                Err(value) => {
                    probe = value;
                    thread::yield_now();
                }
            }
        }
        assert!(rx.recv_timeout(Duration::from_secs(2)).unwrap());
        worker.close_with(Probe {
            thread: thread::current().id(),
            tx,
        });
        assert!(rx.recv_timeout(Duration::from_secs(2)).unwrap());
    }
}

#[cfg(test)]
mod physical_reclamation_tests {
    use super::*;
    static EXCLUSIVE_FOUR_SLOT_PROBE: Mutex<()> = Mutex::new(());
    fn reserve_four_atomically(deadline: Instant) -> Vec<Worker> {
        loop {
            if PHYSICAL_OWNERS
                .compare_exchange(0, 4, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return (0..4).map(|_| Worker::from_reserved()).collect();
            }
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
    }

    #[test]
    fn actual_close_handoffs_keep_four_permits_until_blocked_pins_finish() {
        let _fixture_scope = crate::test_support::enter();
        let _exclusive = EXCLUSIVE_FOUR_SLOT_PROBE.lock().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let workers = reserve_four_atomically(deadline);
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        struct Release(Arc<(Mutex<bool>, Condvar)>);
        impl Drop for Release {
            fn drop(&mut self) {
                *self.0.0.lock().unwrap() = true;
                self.0.1.notify_all();
            }
        }
        struct Hold {
            gate: Arc<(Mutex<bool>, Condvar)>,
            entered: std::sync::mpsc::Sender<thread::ThreadId>,
        }
        impl Drop for Hold {
            fn drop(&mut self) {
                self.entered.send(thread::current().id()).unwrap();
                let mut open = self.gate.0.lock().unwrap();
                while !*open {
                    open = self.gate.1.wait(open).unwrap();
                }
            }
        }
        let release = Release(gate.clone());
        let tickets: Vec<_> = workers.iter().map(Worker::completion_ticket).collect();
        let (entered, receiver) = std::sync::mpsc::channel();
        for worker in workers {
            worker.close_with(Hold {
                gate: gate.clone(),
                entered: entered.clone(),
            });
        }
        for _ in 0..4 {
            assert_ne!(
                receiver.recv_timeout(Duration::from_secs(3)).unwrap(),
                thread::current().id()
            );
        }
        for _ in 0..8 {
            assert!(
                Worker::try_new().is_err(),
                "unfinished physical ownership must reject admission"
            );
        }
        assert_eq!(PHYSICAL_OWNERS.load(Ordering::Acquire), 4);
        assert!(!Worker::has_capacity());
        assert!(!crate::views::accessibility_retirement::Retirement::default().can_open());
        let mut deferred = crate::views::accessibility::Host::headless();
        assert!(!deferred.can_accept_snapshot());
        let snapshot = Arc::new(ira_core::application::App::default().snapshot());
        deferred.begin_prepared(snapshot.clone(), None, "deferred".into());
        assert_eq!(
            Arc::strong_count(&snapshot),
            1,
            "pressure must not capture semantic ownership"
        );
        deferred.close();
        assert!(tickets.iter().all(|ticket| !ticket.is_complete()));
        drop(release);
        while !tickets.iter().all(CompletionTicket::is_complete) {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
    }
    #[test]
    fn actual_preparation_cleanup_keeps_four_permits_until_retired_rows_finish() {
        let _fixture_scope = crate::test_support::enter();
        let _exclusive = EXCLUSIVE_FOUR_SLOT_PROBE.lock().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let workers = reserve_four_atomically(deadline);
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        struct Release(Arc<(Mutex<bool>, Condvar)>);
        impl Drop for Release {
            fn drop(&mut self) {
                *self.0.0.lock().unwrap() = true;
                self.0.1.notify_all();
            }
        }
        struct Hold {
            gate: Arc<(Mutex<bool>, Condvar)>,
            entered: std::sync::mpsc::Sender<thread::ThreadId>,
        }
        impl Drop for Hold {
            fn drop(&mut self) {
                self.entered.send(thread::current().id()).unwrap();
                let mut open = self.gate.0.lock().unwrap();
                while !*open {
                    open = self.gate.1.wait(open).unwrap();
                }
            }
        }
        let release = Release(gate.clone());
        let tickets: Vec<_> = workers.iter().map(Worker::completion_ticket).collect();
        let (entered, receiver) = std::sync::mpsc::channel();
        for worker in workers {
            let mut payload = Hold {
                gate: gate.clone(),
                entered: entered.clone(),
            };
            loop {
                match worker.retire(payload) {
                    Ok(()) => break,
                    Err(returned) => payload = returned,
                }
                assert!(Instant::now() < deadline);
                thread::yield_now();
            }
            worker.close_with(());
        }
        for _ in 0..4 {
            assert_ne!(
                receiver.recv_timeout(Duration::from_secs(3)).unwrap(),
                thread::current().id()
            );
        }
        for _ in 0..8 {
            assert!(
                Worker::try_new().is_err(),
                "unfinished physical ownership must reject admission"
            );
        }
        assert_eq!(PHYSICAL_OWNERS.load(Ordering::Acquire), 4);
        assert!(!Worker::has_capacity());
        assert!(!crate::views::accessibility_retirement::Retirement::default().can_open());
        let mut deferred = crate::views::accessibility::Host::headless();
        assert!(!deferred.can_accept_snapshot());
        let snapshot = Arc::new(ira_core::application::App::default().snapshot());
        deferred.begin_prepared(snapshot.clone(), None, "deferred".into());
        assert_eq!(
            Arc::strong_count(&snapshot),
            1,
            "pressure must not capture semantic ownership"
        );
        deferred.close();
        assert!(tickets.iter().all(|ticket| !ticket.is_complete()));
        drop(release);
        while !tickets.iter().all(CompletionTicket::is_complete) {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
    }
}
