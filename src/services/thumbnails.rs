//! Image preview pipeline: decode → thumbnail cache → terminal protocol.
//!
//! Runs entirely off the UI thread (one worker per request, spawned from
//! [`spawn_thumbnail_job`]); results are delivered as [`ThumbEvent`]s over a
//! channel and picked up on the next tick, mirroring the pane-listing and
//! drive-poller patterns. The UI thread only ever renders a finished
//! [`ratatui_image::protocol::Protocol`].

use std::fs;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader};
use ratatui::layout::Size;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;
use ratatui_image::Resize;

/// Longest side of a stored disk-cache thumbnail in pixels. Big enough to
/// stay sharp when re-fitted into a large preview column, small enough that
/// a cache hit decodes in microseconds.
pub const THUMB_MAX_PX: u32 = 512;

/// Extensions the preview supports — exactly the formats `ira` compiles in
/// via the `image` crate's feature set. Anything else shows a placeholder.
pub fn is_previewable(path: &str) -> bool {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    matches!(
        ext.as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp")
    )
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
/// platform equivalent); `None` when no cache dir is available — previews
/// still work, they just re-decode every time.
fn cache_dir() -> Option<PathBuf> {
    dirs_next::cache_dir().map(|d| d.join("ira").join("thumbnails"))
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
    let img = decode_source(path)?;
    let thumb = img.thumbnail(THUMB_MAX_PX, THUMB_MAX_PX);
    if let Some(cached) = &cache_path {
        store_thumbnail(&thumb, cached);
    }
    Ok(thumb)
}

/// Decodes a source image with EXIF orientation applied (phone photos would
/// otherwise preview sideways).
fn decode_source(path: &str) -> Result<DynamicImage, image::ImageError> {
    let reader = ImageReader::open(path)?;
    decode_oriented(reader)
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
    /// In-memory cache key (disk cache uses [`cache_key`] — area must not
    /// invalidate the pixel cache, only the rendered protocol).
    pub fn mem_key(&self) -> String {
        format!(
            "{}\0{}\0{}\0{}",
            cache_key(&self.path, self.mtime, self.size),
            self.size,
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

/// Spawns a worker thread that resolves `req` and delivers the result on
/// `tx`. One thread per request: requests are rare (selection changes) and
/// `std::thread::spawn` beats building a pool for this cadence.
pub fn spawn_thumbnail_job(req: ThumbRequest, picker: Picker, tx: Sender<ThumbEvent>) {
    std::thread::spawn(move || {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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
    fn only_image_extensions_are_previewable() {
        for name in ["a.png", "a.JPG", "a.jpeg", "a.gif", "a.bmp", "a.webp"] {
            assert!(is_previewable(name), "{name} should preview");
        }
        for name in ["a.txt", "a.tar.gz", "a", "a.rs", "a.PDF"] {
            assert!(!is_previewable(name), "{name} must not preview");
        }
    }

    #[test]
    fn decodes_scales_and_caches() {
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
        let dir = std::env::temp_dir().join("ira_thumb_tests");
        fs::create_dir_all(&dir).unwrap();
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
