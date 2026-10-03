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
}
impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}
impl Host {
    pub fn new() -> Self {
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
            while !stop.load(Ordering::Acquire) {
                if let Ok(work) = commands.recv_timeout(Duration::from_millis(5)) {
                    let request = PreviewRequest {
                        session_id: work.key.window,
                        pane: work.key.pane,
                        generation: work.ticket,
                        path: work.key.path.clone(),
                        mtime: work.key.mtime,
                        size: work.key.size,
                        surface: PreviewSurface::Column,
                        cancellation: work.cancellation,
                    };
                    keys.insert(work.ticket, work.key);
                    if let Err(error) = pool.submit(request, true) {
                        let request = match error {
                            mpsc::TrySendError::Full(request)
                            | mpsc::TrySendError::Disconnected(request) => request,
                        };
                        if let Some(key) = keys.remove(&request.generation) {
                            let _ = events.send(Event {
                                key,
                                ticket: request.generation,
                                content: Content::Error(
                                    "Preview queue is full; change selection to retry".into(),
                                ),
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
        }
    }
    pub fn key(snapshot: &Snapshot, pane: usize) -> Option<Key> {
        let p = snapshot.panes.get(pane)?;
        if p.preview_mode != PreviewMode::Column {
            return None;
        }
        let row = p.rows.get(p.cursor?)?;
        if row.entry.is_dir {
            return None;
        }
        Some(Key {
            window: snapshot.window_generation,
            pane,
            listing: p.listing_generation,
            path: PathBuf::from(&row.entry.path),
            mtime: row.entry.modified,
            size: row.entry.size,
        })
    }
    pub fn request(&mut self, key: Key) {
        if self.items.contains_key(&key) {
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
        let current: Vec<_> = snapshot
            .into_iter()
            .flat_map(|s| (0..if s.split { 2 } else { 1 }).filter_map(|pane| Self::key(s, pane)))
            .collect();
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
