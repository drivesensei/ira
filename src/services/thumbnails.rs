//! Image preview pipeline: decode → thumbnail cache → terminal protocol.
//!
//! Runs entirely off the UI thread: a bounded pool of decode workers (see
//! [`spawn_workers`]) consumes queued [`ThumbRequest`]s and delivers
//! [`ThumbEvent`]s over a channel, picked up on the next tick, mirroring the
//! pane-listing and drive-poller patterns. The UI thread only ever renders a
//! finished [`ratatui_image::protocol::Protocol`].

use std::fs;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader};
use ratatui::layout::Size;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;
use ratatui_image::Resize;

/// Decode budget per image. Caps decompression-bomb allocation (the limit
/// applies to the decoded buffer): anything bigger fails cleanly and shows
/// the unsupported placeholder instead of spiking RSS.
pub const DECODE_MAX_ALLOC: u64 = 256 * 1024 * 1024;

/// Upper bound on decode workers. Scrolling a photo folder must never fork
/// the machine; `available_parallelism` is capped because each in-flight
/// decode can hold up to [`DECODE_MAX_ALLOC`] of pixel data. Six workers
/// keep up with scrolling photo folders while staying polite on 8-core
/// machines (the UI thread still gets a core).
pub const MAX_DECODE_THREADS: usize = 6;

/// Capacity of the high-priority queue (on-screen cells). `try_send` into a
/// full queue is dropped (retried on a later frame), so scrolling cannot
/// build an unbounded backlog.
pub const JOB_QUEUE_HI_CAP: usize = 32;

/// Capacity of the prefetch queue (screens adjacent to the viewport). Kept
/// separate from the visible queue so a big prefetch backlog — e.g. the
/// first fill of a 500-cell grid — can never delay what the user is
/// currently looking at.
pub const JOB_QUEUE_LO_CAP: usize = 256;

/// Idle poll interval for decode workers waiting on both empty queues
/// (std mpsc has no select). Wake latency for a new visible request.
pub const JOB_POLL_INTERVAL: Duration = Duration::from_millis(5);

/// Maximum number of files kept in the on-disk thumbnail cache; the oldest
/// (by mtime) are removed at startup.
pub const CACHE_MAX_FILES: usize = 512;

/// Longest side of a stored disk-cache thumbnail in pixels. Big enough to
/// stay sharp when re-fitted into a large preview column, small enough that
/// a cache hit decodes in microseconds.
pub const THUMB_MAX_PX: u32 = 512;

/// Wall-clock budget for one ffmpeg frame extraction. Long enough for a
/// seek into a large network-hosted video; short enough that a hung process
/// can't occupy a bounded-pool worker forever.
pub const FFMPEG_TIMEOUT: Duration = Duration::from_secs(10);

/// What a path can preview as, by extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    /// Decoded in-process by the `image` crate.
    Image,
    /// Video container — needs a frame extracted by an external `ffmpeg`
    /// binary (optional runtime dependency, never a build dependency).
    Video,
    /// HEIC/HEIF photo — needs HEVC; attempted via `ffmpeg` when available
    /// (many builds decode it, some don't).
    Heic,
    /// First page rasterized by the external `pdftoppm` binary (poppler,
    /// optional runtime dependency). There is no viable pure-Rust PDF
    /// rasterizer (pdfium/mupdf are C bindings; mupdf is AGPL).
    Pdf,
    /// Plain-text/code file — rendered natively as cells in the preview
    /// column (no protocol, no thumbnail).
    Text,
}

/// Classifies a path by extension. `None` = no preview at all.
pub fn preview_kind(path: &str) -> Option<PreviewKind> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp") => Some(PreviewKind::Image),
        Some("mp4" | "mov" | "m4v" | "webm" | "mkv" | "avi") => Some(PreviewKind::Video),
        Some("heic" | "heif") => Some(PreviewKind::Heic),
        Some("pdf") => Some(PreviewKind::Pdf),
        Some(
            "txt" | "md" | "markdown" | "rst" | "log" | "csv" | "tsv" | "json" | "toml" | "yaml"
            | "yml" | "xml" | "ini" | "conf" | "cfg" | "properties" | "env" | "sh" | "bash" | "zsh"
            | "fish" | "ps1" | "bat" | "py" | "rb" | "pl" | "js" | "ts" | "jsx" | "tsx" | "rs"
            | "go" | "c" | "h" | "cpp" | "hpp" | "cc" | "java" | "kt" | "swift" | "sql" | "html"
            | "htm" | "css" | "scss" | "less" | "lua" | "vim" | "service" | "desktop",
        ) => Some(PreviewKind::Text),
        _ => None,
    }
}

/// FNV-1a 64-bit — a stable, dependency-free hash for cache file names.
/// Stability matters more than cryptographic strength: the name only needs to
/// change when the source file changes (mtime/size are folded in).
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Disk-cache key for a source file: identity (path) plus staleness guards
/// (mtime, size), so an edited image re-generates instead of showing stale
/// pixels. `mtime=None` hashes as -1 (stat failed; still cached per session).
pub fn cache_key(path: &str, mtime: Option<i64>, size: u64) -> String {
    format!(
        "{:016x}",
        fnv1a64(format!("{path}\0{}\0{size}", mtime.unwrap_or(-1)).as_bytes())
    )
}

/// Thumbnail cache directory (`$XDG_CACHE_HOME/ira/thumbnails` or the
/// platform equivalent), overridable with `IRA_THUMBNAIL_CACHE_DIR` (used by
/// tests to keep their fixtures out of the user's real cache). `None` when no
/// cache dir is available — previews still work, they just re-decode every
/// time.
fn cache_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("IRA_THUMBNAIL_CACHE_DIR") {
        return Some(PathBuf::from(dir));
    }
    dirs_next::cache_dir().map(|d| d.join("ira").join("thumbnails"))
}

/// Caps the on-disk cache at [`CACHE_MAX_FILES`], deleting the oldest entries
/// (by mtime). Called once at startup on a background thread; failures are
/// ignored — pruning is an optimization, not a correctness requirement.
pub fn prune_cache() {
    let Some(dir) = cache_dir() else {
        return;
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "png"))
        .filter_map(|e| {
            let md = e.metadata().ok()?;
            Some((md.modified().ok()?, e.path()))
        })
        .collect();
    if files.len() <= CACHE_MAX_FILES {
        return;
    }
    files.sort_by_key(|(t, _)| *t);
    let excess = files.len() - CACHE_MAX_FILES;
    for (_, path) in files.into_iter().take(excess) {
        let _ = fs::remove_file(path);
    }
}

/// Loads a thumbnail-sized [`DynamicImage`] for `path`: from the disk cache
/// when a fresh entry exists, otherwise by decoding the original (EXIF
/// orientation applied), downscaling, and writing the cache entry.
pub fn load_thumbnail(
    path: &str,
    mtime: Option<i64>,
    size: u64,
) -> Result<DynamicImage, image::ImageError> {
    let key = cache_key(path, mtime, size);
    let cache_path = cache_dir().map(|d| d.join(format!("{key}.png")));
    if let Some(cached) = &cache_path {
        // A decodable cache entry is by construction thumbnail-sized and
        // already oriented — return it without touching the source file.
        // A missing or corrupt entry falls through and regenerates
        // (self-healing cache), so no `?` may escape this block.
        if let Ok(reader) = ImageReader::open(cached) {
            if let Ok(img) = decode_oriented(reader) {
                return Ok(img);
            }
        }
    }
    let img = match preview_kind(path) {
        Some(PreviewKind::Image) | None => decode_source(path)?,
        Some(PreviewKind::Video) | Some(PreviewKind::Heic) => decode_video_frame(path)?,
        Some(PreviewKind::Pdf) => decode_pdf_first_page(path)?,
        Some(PreviewKind::Text) => {
            return Err(std::io::Error::other("text previews are cell-native").into())
        }
    };
    // The ffmpeg path already returns ≤512 px frames; don't upscale those.
    let thumb = if img.width() <= THUMB_MAX_PX && img.height() <= THUMB_MAX_PX {
        img
    } else {
        img.thumbnail(THUMB_MAX_PX, THUMB_MAX_PX)
    };
    if let Some(cached) = &cache_path {
        store_thumbnail(&thumb, cached);
    }
    Ok(thumb)
}

/// Extracts a representative frame from a video (or HEIC photo) with the
/// system `ffmpeg` binary and decodes it to a `DynamicImage`. Pure Rust
/// cannot demux MP4 or decode H.264/HEVC, so this is the sanctioned
/// runtime-shell-out tier: `ffmpeg` is looked up on `PATH` per call, never
/// linked or required at build time. A watchdog kills the child after
/// [`FFMPEG_TIMEOUT`] so a hung process can't occupy a pool worker forever.
fn decode_video_frame(path: &str) -> Result<DynamicImage, image::ImageError> {
    use std::io::{Cursor, Read};
    use std::process::{Command, Stdio};

    let mut child = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-i",
            path,
            "-frames:v",
            "1",
            "-vf",
            "scale='min(iw,512)':-2",
            "-f",
            "image2pipe",
            "-vcodec",
            "png",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(image::ImageError::IoError)?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("no ffmpeg stdout"))?;

    let shared: Arc<Mutex<Option<std::process::Child>>> = Arc::new(Mutex::new(Some(child)));
    let watchdog = shared.clone();
    std::thread::spawn(move || {
        for _ in 0..(FFMPEG_TIMEOUT.as_millis() / 200) {
            std::thread::sleep(Duration::from_millis(200));
            if watchdog.lock().expect("watchdog poisoned").is_none() {
                return;
            }
        }
        if let Some(mut c) = watchdog.lock().expect("watchdog poisoned").take() {
            let _ = c.kill();
        }
    });

    let mut bytes = Vec::new();
    stdout
        .read_to_end(&mut bytes)
        .map_err(image::ImageError::IoError)?;
    if let Some(mut c) = shared.lock().expect("ffmpeg child poisoned").take() {
        let _ = c.wait();
    }
    if bytes.is_empty() {
        return Err(std::io::Error::other("ffmpeg produced no frame").into());
    }
    ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()?
        .decode()
}

/// Rasterizes page 1 of a PDF with the system `pdftoppm` (poppler) into a
/// temp PNG, then decodes it. Same optional-runtime-dependency tier as the
/// ffmpeg path; a watchdog bounds hung processes.
fn decode_pdf_first_page(path: &str) -> Result<DynamicImage, image::ImageError> {
    use std::io::{Cursor, Read};
    use std::process::{Command, Stdio};
    use std::sync::atomic::AtomicU64;

    /// Unique temp-file root per extraction: PDF workers run concurrently.
    static PDF_SEQ: AtomicU64 = AtomicU64::new(0);

    let root = std::env::temp_dir().join(format!(
        "ira_pdf_{}_{}",
        std::process::id(),
        PDF_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let child = Command::new("pdftoppm")
        .args([
            "-png",
            "-f",
            "1",
            "-l",
            "1",
            "-scale-to",
            "512",
            "-singlefile",
            path,
        ])
        .arg(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(image::ImageError::IoError)?;

    let shared: Arc<Mutex<Option<std::process::Child>>> = Arc::new(Mutex::new(Some(child)));
    let watchdog = shared.clone();
    std::thread::spawn(move || {
        for _ in 0..(FFMPEG_TIMEOUT.as_millis() / 200) {
            std::thread::sleep(Duration::from_millis(200));
            if watchdog.lock().expect("watchdog poisoned").is_none() {
                return;
            }
        }
        if let Some(mut c) = watchdog.lock().expect("watchdog poisoned").take() {
            let _ = c.kill();
        }
    });

    let png = root.with_extension("png");
    let result = (|| -> Result<DynamicImage, image::ImageError> {
        if let Some(mut c) = shared.lock().expect("pdf child poisoned").take() {
            let status = c.wait();
            if !status.map(|s| s.success()).unwrap_or(false) {
                return Err(std::io::Error::other("pdftoppm failed").into());
            }
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&png)?.read_to_end(&mut bytes)?;
        if bytes.is_empty() {
            return Err(std::io::Error::other("pdftoppm produced no output").into());
        }
        ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()?
            .decode()
    })();
    let _ = std::fs::remove_file(&png);
    result
}

/// Reads the head of a text file for the column preview: capped, NUL-sniffed
/// (binary → `binary`), lossy UTF-8. Returns `(content, binary, truncated)`.
pub fn read_text_preview(path: &str) -> Result<(String, bool, bool), std::io::Error> {
    use std::io::Read;

    const CAP: u64 = 256 * 1024;
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let cap = CAP.min(len) as usize;
    let mut buf = vec![0u8; cap];
    file.read_exact(&mut buf)?;
    let truncated = len > cap as u64;
    let binary = buf.contains(&0);
    let content = String::from_utf8_lossy(&buf).into_owned();
    Ok((content, binary, truncated))
}

/// Decodes a source image with content sniffing (extension can lie) and EXIF
fn decode_source(path: &str) -> Result<DynamicImage, image::ImageError> {
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(DECODE_MAX_ALLOC);
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    reader.limits(limits);
    decode_oriented(reader)
}

/// The two job queues workers consume. Visible (on-screen) requests always
/// jump ahead of prefetch requests, so scrolling stays responsive even when
/// hundreds of prefetch jobs are backed up.
pub struct WorkerQueues {
    pub hi: Arc<Mutex<Receiver<ThumbRequest>>>,
    pub lo: Arc<Mutex<Receiver<ThumbRequest>>>,
}

/// Runs the decode worker pool: `n ≤ MAX_DECODE_THREADS` threads consuming
/// the shared job queues until they close, delivering results on `tx`.
/// Workers own a `Picker` clone once, so protocol re-encoding per job is the
/// only per-request setup.
pub fn spawn_workers(picker: Picker, queues: WorkerQueues, tx: Sender<ThumbEvent>) {
    let n = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .min(MAX_DECODE_THREADS);
    for _ in 0..n {
        let picker = picker.clone();
        let hi = queues.hi.clone();
        let lo = queues.lo.clone();
        let tx = tx.clone();
        std::thread::spawn(move || loop {
            // On-screen requests always jump ahead of prefetch. std mpsc has
            // no select(), so idle workers poll both queues on a short
            // interval — two try_recv calls per worker per 5 ms is
            // negligible, and a hi request is picked up within one tick.
            let next = {
                let hi = hi.lock().expect("hi queue poisoned");
                match hi.try_recv() {
                    Ok(req) => Some(req),
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        drop(hi);
                        let lo = lo.lock().expect("lo queue poisoned");
                        match lo.try_recv() {
                            Ok(req) => Some(req),
                            Err(_) => {
                                drop(lo);
                                std::thread::sleep(JOB_POLL_INTERVAL);
                                None
                            }
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        // Hi closed (App dropped): drain lo, then exit.
                        drop(hi);
                        lo.lock().expect("lo queue poisoned").recv().ok()
                    }
                }
            };
            let Some(req) = next else {
                continue;
            };
            let event = match preview_kind(&req.path) {
                Some(PreviewKind::Text) => match read_text_preview(&req.path) {
                    Ok((content, binary, truncated)) => ThumbEvent::Text {
                        req: req.clone(),
                        content,
                        binary,
                        truncated,
                    },
                    Err(_) => ThumbEvent::Failed(req.clone()),
                },
                _ => {
                    match load_thumbnail(&req.path, req.mtime, req.size)
                        .ok()
                        .and_then(|img| build_protocol(&picker, img, req.cols, req.rows).ok())
                    {
                        Some(protocol) => ThumbEvent::Ready(req.clone(), protocol),
                        None => ThumbEvent::Failed(req.clone()),
                    }
                }
            };
            let _ = tx.send(event);
        });
    }
}

fn decode_oriented(
    reader: ImageReader<BufReader<std::fs::File>>,
) -> Result<DynamicImage, image::ImageError> {
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder)?;
    img.apply_orientation(orientation);
    Ok(img)
}

/// Writes the cache entry atomically (temp file + rename) so a crash can
/// never leave a torn PNG behind; failures are silently ignored — the cache
/// is an optimization, never a requirement.
fn store_thumbnail(img: &DynamicImage, dest: &std::path::Path) {
    let Some(parent) = dest.parent() else { return };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let tmp = dest.with_extension("tmp");
    if img.save_with_format(&tmp, image::ImageFormat::Png).is_ok() {
        let _ = fs::rename(&tmp, dest);
    }
    let _ = fs::remove_file(&tmp);
}

/// A preview request as seen by both ends of the worker channel.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ThumbRequest {
    pub path: String,
    pub mtime: Option<i64>,
    pub size: u64,
    /// Preview area in terminal cells the protocol must fit.
    pub cols: u16,
    pub rows: u16,
}

impl ThumbRequest {
    /// In-memory cache key: content identity (path+mtime+size via
    /// [`cache_key`]) plus the preview area, which the protocol encoding
    /// depends on. The disk cache uses [`cache_key`] alone — area must not
    /// invalidate the pixel cache.
    pub fn mem_key(&self) -> String {
        format!(
            "{}\0{}\0{}",
            cache_key(&self.path, self.mtime, self.size),
            self.cols,
            self.rows
        )
    }
}

/// Result of a background thumbnail job.
pub enum ThumbEvent {
    /// Ready to render; the protocol is terminal-agnostic until drawn.
    Ready(ThumbRequest, Protocol),
    /// Undecodable/unreadable file — the UI shows a placeholder.
    Failed(ThumbRequest),
    /// Head of a text file (`read_text_preview`), rendered natively as cells.
    Text {
        req: ThumbRequest,
        content: String,
        binary: bool,
        truncated: bool,
    },
}

/// Builds the terminal-protocol representation for a preview area of
/// `cols × rows` cells. This is the expensive encoding step (PNG re-encode
/// for iTerm2, escape-sequence payload for kitty) and must stay off the UI
/// thread.
pub fn build_protocol(
    picker: &Picker,
    img: DynamicImage,
    cols: u16,
    rows: u16,
) -> Result<Protocol, ratatui_image::errors::Errors> {
    picker.new_protocol(img, Size::new(cols, rows), Resize::Fit(None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Redirects the disk cache to a temp dir so tests never touch the
    /// user's real `~/.cache/ira/thumbnails`. Process-global: tests calling
    /// this stay isolated from the user's data even if they share a dir.
    fn use_test_cache() {
        std::env::set_var(
            "IRA_THUMBNAIL_CACHE_DIR",
            std::env::temp_dir().join("ira_thumb_cache_tests"),
        );
    }
    /// Writes a solid-color PNG and returns its path.
    fn temp_png(name: &str, w: u32, h: u32) -> PathBuf {
        let dir = std::env::temp_dir().join("ira_thumb_tests");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_fn(w, h, |x, _| {
            image::Rgb([x as u8, 0, 0])
        }));
        img.save_with_format(&path, image::ImageFormat::Png)
            .unwrap();
        path
    }

    #[test]
    fn cache_key_changes_when_file_changes() {
        let k1 = cache_key("/a.png", Some(100), 5);
        let k2 = cache_key("/a.png", Some(200), 5);
        let k3 = cache_key("/a.png", Some(100), 6);
        let k4 = cache_key("/b.png", Some(100), 5);
        assert_ne!(k1, k2);
        assert_ne!(k1, k3);
        assert_ne!(k1, k4);
        assert_eq!(k1, cache_key("/a.png", Some(100), 5));
    }

    #[test]
    fn preview_kind_classifies_extensions() {
        for name in ["a.png", "a.JPG", "a.jpeg", "a.gif", "a.bmp", "a.webp"] {
            assert_eq!(preview_kind(name), Some(PreviewKind::Image), "{name}");
        }
        for name in ["a.mp4", "a.MOV", "a.m4v", "a.webm", "a.mkv", "a.avi"] {
            assert_eq!(preview_kind(name), Some(PreviewKind::Video), "{name}");
        }
        for name in ["a.heic", "a.heif"] {
            assert_eq!(preview_kind(name), Some(PreviewKind::Heic), "{name}");
        }
        assert_eq!(preview_kind("a.pdf"), Some(PreviewKind::Pdf));
        for name in [
            "a.txt", "a.md", "a.json", "a.toml", "a.rs", "a.py", "a.sh", "a.csv", "a.yaml",
        ] {
            assert_eq!(preview_kind(name), Some(PreviewKind::Text), "{name}");
        }
        for name in ["a.tar.gz", "a", "a.pdfx"] {
            assert_eq!(preview_kind(name), None, "{name} must not preview");
        }
    }

    #[test]
    fn text_head_is_capped_and_sniffed() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("ira_thumb_tests");
        fs::create_dir_all(&dir).unwrap();

        let text = dir.join("sample.txt");
        fs::write(&text, "hello\nworld\n").unwrap();
        let (content, binary, truncated) = read_text_preview(text.to_str().unwrap()).unwrap();
        assert_eq!(content, "hello\nworld\n");
        assert!(!binary && !truncated);

        let big = dir.join("big.txt");
        let mut f = fs::File::create(&big).unwrap();
        let line = "x".repeat(100);
        for _ in 0..4000 {
            f.write_all(line.as_bytes()).unwrap();
            f.write_all(b"\n").unwrap();
        }
        drop(f);
        let (content, binary, truncated) = read_text_preview(big.to_str().unwrap()).unwrap();
        assert!(truncated && !binary);
        assert_eq!(content.len(), 256 * 1024);

        let bin = dir.join("blob.dat.txt");
        let mut f = fs::File::create(&bin).unwrap();
        f.write_all(b"ok\0binary").unwrap();
        drop(f);
        let (_, binary, _) = read_text_preview(bin.to_str().unwrap()).unwrap();
        assert!(binary, "NUL byte must mark the file binary");
    }

    #[test]
    fn pdf_first_page_rasterizes_through_poppler() {
        use std::process::{Command, Stdio};
        if Command::new("pdftoppm")
            .arg("-v")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err()
        {
            eprintln!("skipping: pdftoppm not on PATH");
            return;
        }
        use_test_cache();
        let dir = std::env::temp_dir().join("ira_thumb_tests");
        let pdf = dir.join("sample.pdf");
        // Minimal one-page PDF with visible text; hand-written is fine.
        let pdf_body = "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 100]/Contents 4 0 R/Resources<</Font<</F1 5 0 R>>>>>>endobj\n4 0 obj<</Length 60>>stream\nBT /F1 24 Tf 20 50 Td (IRA pdf preview) Tj ET\nendstream endobj\n5 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>endobj\ntrailer<</Root 1 0 R/Size 6>>\n%%EOF\n";
        fs::write(&pdf, pdf_body).unwrap();
        let size = fs::metadata(&pdf).unwrap().len();

        let thumb = load_thumbnail(pdf.to_str().unwrap(), Some(1), size).unwrap();
        assert!(thumb.width() <= THUMB_MAX_PX && thumb.height() <= THUMB_MAX_PX);
        assert!(thumb.width() > 0 && thumb.height() > 0);
    }

    #[test]
    fn video_frame_extracts_through_ffmpeg() {
        use std::process::{Command, Stdio};
        // Needs the optional runtime dependency; skipped where absent.
        if Command::new("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err()
        {
            eprintln!("skipping: ffmpeg not on PATH");
            return;
        }
        use_test_cache();
        let dir = std::env::temp_dir().join("ira_thumb_tests");
        fs::create_dir_all(&dir).unwrap();
        let video = dir.join("clip.mp4");
        let ok = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=0.5:size=1280x720:rate=10",
                "-pix_fmt",
                "yuv420p",
                video.to_str().unwrap(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("ffmpeg status")
            .success();
        assert!(ok, "ffmpeg could not render the test clip");
        let size = fs::metadata(&video).unwrap().len();

        let thumb = load_thumbnail(video.to_str().unwrap(), Some(1), size).unwrap();
        // ffmpeg downscales to ≤512 on extract; no upscale from our side.
        assert!(thumb.width() <= THUMB_MAX_PX && thumb.height() <= THUMB_MAX_PX);
        assert_eq!(thumb.width(), 512, "16:9 video frame keeps aspect ratio");
    }

    #[test]
    fn decodes_scales_and_caches() {
        use_test_cache();
        let src = temp_png("src.png", 1024, 512);
        let mtime: Option<i64> = fs::metadata(&src).ok().and_then(|m| {
            m.modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
        });
        let size = fs::metadata(&src).unwrap().len();

        let thumb = load_thumbnail(src.to_str().unwrap(), mtime, size).unwrap();
        assert!(thumb.width() <= THUMB_MAX_PX);
        assert!(thumb.height() <= THUMB_MAX_PX);
        assert_eq!((thumb.width(), thumb.height()), (512, 256)); // 2:1 source keeps aspect

        // Second load must come from the disk cache: delete the source and
        // verify the thumbnail still resolves.
        fs::remove_file(&src).unwrap();
        let cached = load_thumbnail(src.to_str().unwrap(), mtime, size).unwrap();
        assert_eq!(
            (cached.width(), cached.height()),
            (thumb.width(), thumb.height())
        );
    }

    #[test]
    fn undecodable_file_fails_cleanly() {
        use_test_cache();
        let dir = std::env::temp_dir().join("ira_thumb_tests");
        let path = dir.join("not_an_image.png");
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(b"definitely not a png").unwrap();
        drop(f);
        assert!(load_thumbnail(path.to_str().unwrap(), Some(1), 19).is_err());
    }
    #[test]
    fn builds_halfblocks_protocol_for_area() {
        let picker = Picker::halfblocks();
        assert!(matches!(
            picker.protocol_type(),
            ratatui_image::picker::ProtocolType::Halfblocks
        ));
        let img = DynamicImage::ImageRgb8(image::RgbImage::new(100, 100));
        let protocol = build_protocol(&picker, img, 10, 5).unwrap();
        // End-to-end: the protocol must render into a 12×7 grid without
        // panicking (halfblocks draws as plain cells).
        let backend = ratatui::backend::TestBackend::new(12, 7);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| f.render_widget(ratatui_image::Image::new(&protocol), f.area()))
            .unwrap();
    }
}
