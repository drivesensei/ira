use super::*;
use std::sync::{
    mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    Mutex,
};
use std::thread::JoinHandle;

pub struct PreviewPool {
    hi: Option<SyncSender<PreviewRequest>>,
    lo: Option<SyncSender<PreviewRequest>>,
    pub events: Receiver<PreviewEvent>,
    shutdown: Cancellation,
    workers: Vec<JoinHandle<()>>,
    active: Arc<Mutex<Vec<Option<Cancellation>>>>,
}
impl PreviewPool {
    pub fn new(options: PreviewOptions) -> Self {
        let n = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2)
            .min(MAX_DECODE_THREADS);
        Self::with_workers(options, n)
    }
    pub fn with_workers(options: PreviewOptions, workers: usize) -> Self {
        let (hi_tx, hi_rx) = mpsc::sync_channel(JOB_QUEUE_HI_CAP);
        let (lo_tx, lo_rx) = mpsc::sync_channel(JOB_QUEUE_LO_CAP);
        let (tx, events) = mpsc::channel();
        let hi = Arc::new(Mutex::new(hi_rx));
        let lo = Arc::new(Mutex::new(lo_rx));
        let shutdown = Cancellation::default();
        let count = workers.clamp(1, MAX_DECODE_THREADS);
        let active = Arc::new(Mutex::new(vec![None; count]));
        let mut handles = Vec::new();
        for worker in 0..count {
            let hi = hi.clone();
            let lo = lo.clone();
            let tx = tx.clone();
            let options = options.clone();
            let shutdown = shutdown.clone();
            let active = active.clone();
            handles.push(std::thread::spawn(move || loop {
                if shutdown.cancelled() {
                    break;
                }
                let request = match take_next(&hi, &lo) {
                    Next::Request(r) => r,
                    Next::Idle => {
                        std::thread::sleep(JOB_POLL_INTERVAL);
                        continue;
                    }
                    Next::Closed => break,
                };
                if let Ok(mut current) = active.lock() {
                    current[worker] = Some(request.cancellation.clone());
                }
                if shutdown.cancelled() {
                    request.cancellation.cancel();
                }
                let result = load_preview(&request, &options);
                if let Ok(mut current) = active.lock() {
                    current[worker] = None;
                }
                if tx.send(PreviewEvent { request, result }).is_err() {
                    break;
                }
            }));
        }
        Self {
            hi: Some(hi_tx),
            lo: Some(lo_tx),
            events,
            shutdown,
            workers: handles,
            active,
        }
    }
    /// Full queues return the request for retry; no fake successful completion.
    pub fn submit(
        &self,
        request: PreviewRequest,
        visible: bool,
    ) -> Result<(), TrySendError<PreviewRequest>> {
        let sender = if visible { &self.hi } else { &self.lo };
        match sender {
            Some(tx) => tx.try_send(request),
            None => Err(TrySendError::Disconnected(request)),
        }
    }
    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }
    /// Signal shutdown without joining potentially blocked filesystem/decode work.
    pub fn cancel(&mut self) {
        self.shutdown.cancel();
        if let Ok(active) = self.active.lock() {
            for cancel in active.iter().flatten() {
                cancel.cancel();
            }
        }
        self.hi.take();
        self.lo.take();
    }
}
impl Drop for PreviewPool {
    fn drop(&mut self) {
        self.cancel();
    }
}
enum Next {
    Request(PreviewRequest),
    Idle,
    Closed,
}
fn take_next(hi: &Mutex<Receiver<PreviewRequest>>, lo: &Mutex<Receiver<PreviewRequest>>) -> Next {
    let high = match hi.lock() {
        Ok(rx) => rx.try_recv(),
        Err(_) => return Next::Closed,
    };
    match high {
        Ok(r) => Next::Request(r),
        Err(high_error) => {
            let low = match lo.lock() {
                Ok(rx) => rx.try_recv(),
                Err(_) => return Next::Closed,
            };
            match low {
                Ok(r) => Next::Request(r),
                Err(TryRecvError::Disconnected) if high_error == TryRecvError::Disconnected => {
                    Next::Closed
                }
                _ => Next::Idle,
            }
        }
    }
}
#[cfg(test)]
#[path = "pool_tests.rs"]
mod tests;
