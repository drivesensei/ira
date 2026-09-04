//! Measures a folder with the same walker the `?` dialog uses.
//! Usage: cargo run --release --example dirsize -- /path/to/folder
use std::path::Path;
use std::time::Instant;

use ira::services::file_info::{dir_size, WalkHandle};
fn main() {
    let path = std::env::args().nth(1).expect("folder path required");
    let handle = WalkHandle::new();
    let start = Instant::now();
    let mut last_report = Instant::now();
    let mut on_progress = |bytes: u64, items: u64, on_disk: u64| {
        if last_report.elapsed().as_secs() >= 10 {
            last_report = Instant::now();
            eprintln!(
                "[{:>5}s] progress: {:.2} GiB data / {:.2} GiB on disk, {} items",
                start.elapsed().as_secs(),
                bytes as f64 / 1073741824.0,
                on_disk as f64 / 1073741824.0,
                items
            );
        }
    };
    let size = dir_size(Path::new(&path), &handle, &mut on_progress);
    println!(
        "done in {:.1}s: {:.2} GiB data, {:.2} GiB on disk, {} items",
        start.elapsed().as_secs_f32(),
        size.bytes as f64 / 1073741824.0,
        size.on_disk as f64 / 1073741824.0,
        size.items
    );
}
