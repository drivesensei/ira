//! Benchmarks the real batch copy worker: N small files + one large file.
use ira::services::transfer::{spawn_job, Job, JobControl, JobEvent, JobKind, OverwritePolicy};
use std::sync::mpsc;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "/tmp/ira_bench/src".into());
    let dst = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "/tmp/ira_bench/dst".into());

    let mut paths: Vec<String> = std::fs::read_dir(&src)
        .unwrap()
        .flatten()
        .map(|e| e.path().to_string_lossy().into_owned())
        .collect();
    paths.sort();

    let job = Job {
        id: 1,
        kind: JobKind::Copy,
        overwrite: OverwritePolicy::AutoRename,
        paths: paths.clone(),
        dest_dir: dst.clone(),
        label: "bench".into(),
        total_bytes: None,
        copied_bytes: 0,
        current: String::new(),
        status: ira::services::transfer::JobStatus::Running,
        started_at: Instant::now(),
        control: JobControl::new(),
    };
    let (tx, rx) = mpsc::channel();
    let t0 = Instant::now();
    let t_prescan = t0;
    spawn_job(&job, tx);
    let mut events = 0usize;
    loop {
        match rx.recv_timeout(std::time::Duration::from_secs(60)) {
            Ok(ev) => {
                events += 1;
                if matches!(ev, JobEvent::Done { .. } | JobEvent::Failed { .. }) {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    println!(
        "batch copy: {} files in {:.2?} ({} events, prescan included)",
        paths.len(),
        t0.elapsed(),
        events
    );
    let _ = (t_prescan, dst);
}
