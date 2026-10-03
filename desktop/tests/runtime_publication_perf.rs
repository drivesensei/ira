//! Reproducible actor publication measurement; synthetic in-memory source rows, no filesystem writes.
use ira_core::{
    application::App,
    domain::data::Folder,
    input::{Input, KeyCode, KeyEvent, KeyModifiers},
    services::list_files::FEntry,
};
use ira_desktop::runtime::{Command, Publication, Runtime};
use std::{
    collections::BTreeSet,
    sync::Arc,
    time::{Duration, Instant},
};
fn publication(runtime: &Runtime, sequence: u64) -> Publication {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(value) = runtime.try_snapshot()
            && value.snapshot.ack_sequence >= sequence
        {
            return value;
        }
        assert!(
            Instant::now() < deadline,
            "actor publication did not arrive"
        );
        std::thread::yield_now();
    }
}
#[test]
fn measure_actual_actor_100k_cursor_publications_and_retained_rows() {
    let mut app = App::default();
    app.window_generation = 7;
    app.panes[0].folder = Some(Folder::new(
        "Synthetic performance fixture".into(),
        "/tmp/ira-in-memory-publication-fixture".into(),
        '#',
    ));
    app.panes[0].files = (0..100_000)
        .map(|index| FEntry {
            path: format!("/tmp/ira-in-memory-publication-fixture/file-{index:06}.txt"),
            label: format!("file-{index:06}.txt"),
            is_dir: false,
            size: 7,
            modified: None,
        })
        .collect();
    app.panes[0].selected = vec![false; 100_000];
    app.panes[0].listing_settled = true;
    app.panes[0].listing_generation = 3;
    app.panes[0].state.select(Some(0));
    let start = Instant::now();
    let mut runtime = Runtime::with_factory(7, move || app);
    let first = publication(&runtime, 0);
    let cold = start.elapsed();
    assert_eq!(first.snapshot.panes[0].rows.len(), 100_000);
    let mut retained = vec![first.snapshot];
    let mut latency = Vec::new();
    let mut previous_cursor = 0;
    for index in 1..=25 {
        let start = Instant::now();
        let sequence = runtime.enqueue(
            Command::Input(Input::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))),
            None,
        );
        let value = publication(&runtime, sequence);
        // Frozen handler accelerates closely repeated Down keys; assert actual forward progress.
        let cursor = value.snapshot.panes[0].cursor.unwrap();
        assert!(cursor > previous_cursor && cursor < 100_000);
        previous_cursor = cursor;
        if index > 5 {
            latency.push(start.elapsed().as_micros());
        }
        retained.push(value.snapshot);
    }
    assert_eq!(
        retained[0].panes[0].cursor,
        Some(0),
        "retained publication must remain immutable"
    );
    let identities: BTreeSet<_> = retained
        .iter()
        .map(|s| s.panes[0].rows.as_ptr() as usize)
        .collect();
    let rows = &retained[0].panes[0].rows;
    let one_payload = rows.len() * std::mem::size_of::<ira_core::observable::Row>()
        + rows
            .iter()
            .map(|r| r.entry.path.capacity() + r.entry.label.capacity())
            .sum::<usize>();
    latency.sort_unstable();
    let mean = latency.iter().sum::<u128>() / latency.len() as u128;
    println!(
        "actor_100k cold_us={} samples={} cursor_us_min={} cursor_us_p50={} cursor_us_p95={} cursor_us_max={} cursor_us_mean={} retained_snapshots={} distinct_row_allocations={} estimated_retained_row_payload_bytes={}",
        cold.as_micros(),
        latency.len(),
        latency[0],
        latency[latency.len() / 2],
        latency[latency.len() * 95 / 100],
        latency[latency.len() - 1],
        mean,
        retained.len(),
        identities.len(),
        one_payload * identities.len()
    );
    runtime.stop(&[]);
    drop(retained);
    drop(runtime);
    // Arc itself is the native foreground snapshot contract; no window or native provider was attached.
    let _ = std::mem::size_of::<Arc<ira_core::observable::Snapshot>>();
}
