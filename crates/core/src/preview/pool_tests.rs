use super::*;
fn request(path: &str) -> PreviewRequest {
    PreviewRequest {
        session_id: 1,
        pane: 0,
        generation: 1,
        path: path.into(),
        mtime: None,
        size: 0,
        surface: PreviewSurface::Grid,
        cancellation: Cancellation::default(),
    }
}
#[test]
fn visible_requests_preempt_prefetch_and_closed_queues_terminate() {
    let (hi_tx, hi_rx) = mpsc::sync_channel(JOB_QUEUE_HI_CAP);
    let (lo_tx, lo_rx) = mpsc::sync_channel(JOB_QUEUE_LO_CAP);
    let hi = Mutex::new(hi_rx);
    let lo = Mutex::new(lo_rx);
    lo_tx.try_send(request("prefetch")).unwrap();
    hi_tx.try_send(request("visible")).unwrap();
    let Next::Request(first) = take_next(&hi, &lo) else {
        panic!("request")
    };
    assert_eq!(first.path, PathBuf::from("visible"));
    let Next::Request(next) = take_next(&hi, &lo) else {
        panic!("request")
    };
    assert_eq!(next.path, PathBuf::from("prefetch"));
    drop(hi_tx);
    drop(lo_tx);
    assert!(matches!(take_next(&hi, &lo), Next::Closed));
}
#[test]
fn queue_budgets_return_full_request_for_retry() {
    for cap in [JOB_QUEUE_HI_CAP, JOB_QUEUE_LO_CAP] {
        let (tx, rx) = mpsc::sync_channel(cap);
        for _ in 0..cap {
            tx.try_send(request("file")).unwrap();
        }
        assert!(
            matches!(tx.try_send(request("retry")),Err(TrySendError::Full(r)) if r.path==std::path::Path::new("retry"))
        );
        drop(rx);
    }
}
