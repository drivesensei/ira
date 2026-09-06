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

/// Queued-request capacity between the UI and the worker pool. `try_send`
/// into a full queue is dropped (retried on a later frame), so scrolling
/// cannot build an unbounded backlog.
pub const JOB_QUEUE_CAP: usize = 64;

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

/// Decodes a source image with content sniffing (extension can lie) and EXIF
fn decode_source(path: &str) -> Result<DynamicImage, image::ImageError> {
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(DECODE_MAX_ALLOC);
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    reader.limits(limits);
    decode_oriented(reader)
}

/// Runs the decode worker pool: `n ≤ MAX_DECODE_THREADS` threads consuming
/// the shared `jobs` queue until it closes, delivering results on `tx`.
/// Workers own a `Picker` clone once, so protocol re-encoding per job is the
/// only per-request setup.
pub fn spawn_workers(
    picker: Picker,
    jobs: Arc<Mutex<Receiver<ThumbRequest>>>,
    tx: Sender<ThumbEvent>,
) {
    let n = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .min(MAX_DECODE_THREADS);
    for _ in 0..n {
        let picker = picker.clone();
        let jobs = jobs.clone();
        let tx = tx.clone();
        std::thread::spawn(move || loop {
            let Ok(req) = jobs.lock().expect("job queue poisoned").recv() else {
                break;
            };
            let event = match load_thumbnail(&req.path, req.mtime, req.size)
                .ok()
                .and_then(|img| build_protocol(&picker, img, req.cols, req.rows).ok())
            {
                Some(protocol) => ThumbEvent::Ready(req.clone(), protocol),
                None => ThumbEvent::Failed(req.clone()),
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
        for name in ["a.txt", "a.tar.gz", "a", "a.rs", "a.PDF"] {
            assert_eq!(preview_kind(name), None, "{name} must not preview");
        }
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
