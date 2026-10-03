//! Background preview decoding and raw BGRA transport. Painting never reads files.
use gpui::RenderImage;
use ira_core::{
    model::PreviewMode,
    observable::Snapshot,
    preview::{
        Cancellation, PreviewContent, PreviewOptions, PreviewPool, PreviewRequest, PreviewSurface,
    },
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Key {
    window: u64,
    pane: usize,
    listing: u64,
    path: PathBuf,
    mtime: Option<i64>,
    size: u64,
    grid: bool,
}
pub enum Content {
    Image(Arc<RenderImage>),
    Text(String),
    Error(String),
}
struct Item {
    ticket: u64,
    cancellation: Cancellation,
    content: Option<Content>,
}
struct Event {
    key: Key,
    ticket: u64,
    content: Content,
}
struct Work {
    key: Key,
    ticket: u64,
    cancellation: Cancellation,
}
pub struct Host {
    tx: mpsc::SyncSender<Work>,
    rx: mpsc::Receiver<Event>,
    items: BTreeMap<Key, Item>,
    next: u64,
    stopped: Arc<AtomicBool>,
    visible: BTreeMap<usize, Vec<Key>>,
}
impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}
impl Host {
    pub fn new() -> Self {
        #[cfg(test)]
        {
            let fixture = crate::test_support::current();
            Self::start(move || {
                let mut options = PreviewOptions::from_env();
                options.cache_dir = fixture.as_ref().map(|f| f.directory.join("preview-cache"));
                if let Some(fixture) = fixture {
                    options.temp_dir = fixture.directory.join("preview-temp");
                }
                options
            })
        }
        #[cfg(not(test))]
        Self::start(PreviewOptions::from_env)
    }
    fn start(options: impl FnOnce() -> PreviewOptions + Send + 'static) -> Self {
        let (tx, commands) = mpsc::sync_channel::<Work>(64);
        let (events, rx) = mpsc::sync_channel(64);
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        std::thread::spawn(move || {
            let mut pool = PreviewPool::new(options());
            let mut keys = BTreeMap::new();
            let mut pending: Option<(Key, PreviewRequest)> = None;
            while !stop.load(Ordering::Acquire) {
                if pending.is_none()
                    && let Ok(work) = commands.recv_timeout(Duration::from_millis(5))
                {
                    let request = PreviewRequest {
                        session_id: work.key.window,
                        pane: work.key.pane,
                        generation: work.ticket,
                        path: work.key.path.clone(),
                        mtime: work.key.mtime,
                        size: work.key.size,
                        surface: if work.key.grid {
                            PreviewSurface::Grid
                        } else {
                            PreviewSurface::Column
                        },
                        cancellation: work.cancellation,
                    };
                    pending = Some((work.key, request));
                }
                if let Some((key, request)) = pending.take()
                    && !request.cancellation.cancelled()
                {
                    let ticket = request.generation;
                    match pool.submit(request, true) {
                        Ok(()) => {
                            keys.insert(ticket, key);
                        }
                        Err(mpsc::TrySendError::Full(request)) => {
                            pending = Some((key, request));
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(mpsc::TrySendError::Disconnected(request)) => {
                            let _ = events.send(Event {
                                key,
                                ticket: request.generation,
                                content: Content::Error("Preview workers stopped".into()),
                            });
                        }
                    }
                }
                while let Ok(event) = pool.events.try_recv() {
                    let Some(key) = keys.remove(&event.request.generation) else {
                        continue;
                    };
                    if event.request.cancellation.cancelled() {
                        continue;
                    }
                    let content = match event.result {
                        Ok(PreviewContent::Pixels(pixels)) => raw_image(pixels)
                            .map(Content::Image)
                            .unwrap_or_else(Content::Error),
                        Ok(PreviewContent::Text(text)) => Content::Text(if text.binary {
                            "Binary file".into()
                        } else {
                            format!(
                                "{}{}",
                                text.content,
                                if text.truncated {
                                    "\n[preview truncated]"
                                } else {
                                    ""
                                }
                            )
                        }),
                        Err(error) => Content::Error(error.to_string()),
                    };
                    if events
                        .send(Event {
                            key,
                            ticket: event.request.generation,
                            content,
                        })
                        .is_err()
                    {
                        pool.cancel();
                        return;
                    }
                }
            }
            pool.cancel();
        });
        Self {
            tx,
            rx,
            items: BTreeMap::new(),
            next: 0,
            stopped,
            visible: BTreeMap::new(),
        }
    }
    pub fn key(snapshot: &Snapshot, pane: usize) -> Option<Key> {
        let p = snapshot.panes.get(pane)?;
        if p.preview_mode != PreviewMode::Column {
            return None;
        }
        Self::row_key(snapshot, pane, p.cursor?, false)
    }
    pub fn grid_key(snapshot: &Snapshot, pane: usize, row: usize) -> Option<Key> {
        let p = snapshot.panes.get(pane)?;
        if p.preview_mode != PreviewMode::Grid {
            return None;
        }
        Self::row_key(snapshot, pane, row, true)
    }
    fn row_key(snapshot: &Snapshot, pane: usize, row: usize, grid: bool) -> Option<Key> {
        let p = snapshot.panes.get(pane)?;
        let row = p.rows.get(row)?;
        if row.entry.is_dir {
            return None;
        }
        if grid
            && matches!(
                ira_core::preview::preview_kind(&row.entry.path),
                None | Some(ira_core::preview::PreviewKind::Text)
            )
        {
            return None;
        }
        Some(Key {
            window: snapshot.window_generation,
            pane,
            listing: p.listing_generation,
            path: PathBuf::from(&row.entry.path),
            mtime: row.entry.modified,
            size: row.entry.size,
            grid,
        })
    }
    /// Only the real virtualized viewport supplies tile work. No full-folder traversal.
    pub fn visible_grid(
        &mut self,
        snapshot: &Snapshot,
        pane: usize,
        range: std::ops::Range<usize>,
    ) {
        let keys: Vec<_> = range
            .take(128)
            .filter_map(|row| Self::grid_key(snapshot, pane, row))
            .collect();
        self.visible.insert(pane, keys);
    }
    pub fn request(&mut self, key: Key) {
        if self.items.contains_key(&key) || self.items.len() >= 256 {
            return;
        }
        self.next = self.next.wrapping_add(1);
        let cancellation = Cancellation::default();
        let work = Work {
            key: key.clone(),
            ticket: self.next,
            cancellation: cancellation.clone(),
        };
        if self.tx.try_send(work).is_ok() {
            self.items.insert(
                key,
                Item {
                    ticket: self.next,
                    cancellation,
                    content: None,
                },
            );
        }
    }
    pub fn poll(&mut self, snapshot: Option<&Snapshot>) -> bool {
        if self.stopped.load(Ordering::Acquire) {
            return false;
        }
        let mut current: Vec<_> = snapshot
            .into_iter()
            .flat_map(|s| (0..if s.split { 2 } else { 1 }).filter_map(|pane| Self::key(s, pane)))
            .collect();
        if let Some(snapshot) = snapshot {
            for (pane, keys) in &self.visible {
                if snapshot
                    .panes
                    .get(*pane)
                    .is_some_and(|p| p.preview_mode == PreviewMode::Grid)
                    && (*pane == 0 || snapshot.split)
                {
                    current.extend(
                        keys.iter()
                            .filter(|key| {
                                key.window == snapshot.window_generation
                                    && key.listing == snapshot.panes[*pane].listing_generation
                            })
                            .cloned(),
                    );
                }
            }
        }
        self.items.retain(|key, item| {
            let keep = current.contains(key);
            if !keep {
                item.cancellation.cancel();
            }
            keep
        });
        for key in current {
            self.request(key);
        }
        let mut changed = false;
        for _ in 0..64 {
            let Ok(event) = self.rx.try_recv() else { break };
            if let Some(item) = self.items.get_mut(&event.key)
                && item.ticket == event.ticket
            {
                item.content = Some(event.content);
                changed = true;
            }
        }
        changed
    }
    pub fn content(&self, key: &Key) -> Option<&Content> {
        self.items.get(key)?.content.as_ref()
    }
    pub fn invalidate(&mut self, path: &std::path::Path) {
        self.items.retain(|key, item| {
            if key.path == path {
                item.cancellation.cancel();
                false
            } else {
                true
            }
        });
    }
    pub fn close(&mut self) {
        self.stopped.store(true, Ordering::Release);
        for item in self.items.values() {
            item.cancellation.cancel();
        }
        self.items.clear();
        self.visible.clear();
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        self.close();
    }
}
fn raw_image(pixels: ira_core::preview::Pixels) -> Result<Arc<RenderImage>, String> {
    let expected = u64::from(pixels.width)
        .checked_mul(u64::from(pixels.height))
        .and_then(|n| n.checked_mul(4))
        .ok_or("Preview dimensions overflow")?;
    if expected != pixels.rgba.len() as u64 {
        return Err("Preview pixel length does not match dimensions".into());
    }
    let mut bgra = pixels.rgba.to_vec();
    for pixel in bgra.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(pixels.width, pixels.height, bgra)
        .ok_or("Invalid preview image")?;
    Ok(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocked_decoder_startup_keeps_viewport_work_bounded_and_nonblocking() {
        use ira_core::{application::App, services::list_files::FEntry};
        let (release, gate) = mpsc::channel();
        let mut host = Host::start(move || {
            gate.recv().unwrap();
            PreviewOptions {
                cache_dir: None,
                temp_dir: std::env::temp_dir(),
                ffmpeg: "ffmpeg".into(),
                pdftoppm: "pdftoppm".into(),
                process_timeout: Duration::from_secs(1),
            }
        });
        let mut app = App::default();
        app.window_generation = 9;
        app.panes[0].preview_mode = PreviewMode::Grid;
        app.panes[0].files = (0..1000)
            .map(|i| FEntry {
                path: format!("/tmp/ira-queue-fixture-{i}.png"),
                label: format!("{i}.png"),
                is_dir: false,
                size: 10,
                modified: None,
            })
            .collect();
        app.panes[0].selected = vec![false; 1000];
        let snapshot = app.snapshot();
        host.visible_grid(&snapshot, 0, 0..1000);
        assert_eq!(host.visible[&0].len(), 128);
        let begin = std::time::Instant::now();
        host.poll(Some(&snapshot));
        eprintln!(
            "blocked_preview_enqueue_us={} pending_items={}",
            begin.elapsed().as_micros(),
            host.items.len()
        );
        assert_eq!(
            host.items.len(),
            64,
            "foreground enqueue respects the bounded channel even while startup has no receiver"
        );
        host.close();
        release.send(()).unwrap();
        assert!(!host.poll(Some(&snapshot)));
    }
    #[test]
    fn raw_rgba_transport_swizzles_without_encoding() {
        let image = raw_image(ira_core::preview::Pixels {
            width: 1,
            height: 1,
            rgba: Arc::from([255, 20, 30, 128]),
        })
        .unwrap();
        assert_eq!(image.as_bytes(0).unwrap(), [30, 20, 255, 128]);
    }
    #[test]
    fn background_text_and_invalidation_reject_old_ticket() {
        use ira_core::{application::App, services::list_files::FEntry};
        let fixture =
            std::env::temp_dir().join(format!("ira-native-preview-{}", std::process::id()));
        std::fs::create_dir_all(&fixture).unwrap();
        let path = fixture.join("text.txt");
        std::fs::write(&path, b"old preview").unwrap();
        let options = PreviewOptions {
            cache_dir: None,
            temp_dir: fixture.clone(),
            ffmpeg: "ffmpeg".into(),
            pdftoppm: "pdftoppm".into(),
            process_timeout: Duration::from_secs(1),
        };
        let (release, gate) = mpsc::channel();
        let mut host = Host::start(move || {
            gate.recv().unwrap();
            options
        });
        let mut app = App::default();
        app.window_generation = 77;
        app.panes[0].preview_mode = PreviewMode::Column;
        app.panes[0].files = vec![FEntry {
            path: path.to_string_lossy().into_owned(),
            label: "text.txt".into(),
            is_dir: false,
            size: 11,
            modified: None,
        }];
        app.panes[0].selected = vec![false];
        app.panes[0].state.select(Some(0));
        let snapshot = app.snapshot();
        let key = Host::key(&snapshot, 0).unwrap();
        host.poll(Some(&snapshot));
        let old = host.items[&key].ticket;
        host.invalidate(&path);
        assert!(!host.items.contains_key(&key));
        std::fs::write(&path, b"new preview").unwrap();
        host.poll(Some(&snapshot));
        assert!(host.items[&key].ticket > old);
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            host.poll(Some(&snapshot));
            if let Some(Content::Text(text)) = host.content(&key) {
                assert_eq!(text, "new preview");
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        host.close();
        assert!(!host.poll(Some(&snapshot)));
        assert!(host.items.is_empty());
        drop(host);
        std::fs::remove_dir_all(fixture).unwrap();
    }
    #[test]
    fn malformed_pixel_transport_is_rejected() {
        assert!(
            raw_image(ira_core::preview::Pixels {
                width: 2,
                height: 1,
                rgba: Arc::from([1, 2, 3, 4])
            })
            .is_err()
        );
    }
}
