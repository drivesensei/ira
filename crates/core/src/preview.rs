//! UI-neutral preview pixels, text, cache and bounded workers.
//! Filesystem/process calls run only on caller background workers.
#[path = "preview/classify.rs"]
mod classify;
pub use classify::{cache_key, preview_kind, PreviewKind};
#[path = "preview/cache.rs"]
mod cache;
#[path = "preview/pool.rs"]
mod pool;
#[path = "preview/process.rs"]
mod process;
pub use pool::PreviewPool;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

pub const DECODE_MAX_ALLOC: u64 = 256 * 1024 * 1024;
pub const MAX_DECODE_THREADS: usize = 6;
pub const JOB_QUEUE_HI_CAP: usize = 32;
pub const JOB_QUEUE_LO_CAP: usize = 256;
pub const JOB_POLL_INTERVAL: Duration = Duration::from_millis(5);
pub const CACHE_MAX_FILES: usize = 512;
pub const THUMB_MAX_PX: u32 = 768;
pub const TEXT_PREVIEW_CAP: u64 = 256 * 1024;
pub const FFMPEG_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreviewSurface {
    Grid,
    Column,
}
#[derive(Debug, Clone)]
pub struct PreviewRequest {
    pub session_id: u64,
    pub pane: usize,
    pub generation: u64,
    pub path: PathBuf,
    pub mtime: Option<i64>,
    pub size: u64,
    pub surface: PreviewSurface,
    pub cancellation: Cancellation,
}
impl PreviewRequest {
    /// Caller rejects completions whose transport identity no longer matches.
    pub fn matches(&self, session_id: u64, pane: usize, generation: u64, path: &Path) -> bool {
        self.session_id == session_id
            && self.pane == pane
            && self.generation == generation
            && self.path == path
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPreview {
    pub content: String,
    pub binary: bool,
    pub truncated: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewContent {
    Pixels(Pixels),
    Text(TextPreview),
}
#[derive(Debug, Clone)]
pub struct PreviewEvent {
    pub request: PreviewRequest,
    pub result: Result<PreviewContent, PreviewError>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewError {
    Cancelled,
    Failed(String),
}
impl std::fmt::Display for PreviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str("preview cancelled"),
            Self::Failed(s) => f.write_str(s),
        }
    }
}
impl std::error::Error for PreviewError {}
impl From<std::io::Error> for PreviewError {
    fn from(e: std::io::Error) -> Self {
        Self::Failed(e.to_string())
    }
}
impl From<image::ImageError> for PreviewError {
    fn from(e: image::ImageError) -> Self {
        Self::Failed(e.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct PreviewOptions {
    pub cache_dir: Option<PathBuf>,
    pub temp_dir: PathBuf,
    pub ffmpeg: PathBuf,
    pub pdftoppm: PathBuf,
    /// ffmpeg deadline; PDF retains the source watchdog defect G0009 and may
    /// wait beyond this advertised timeout until the external child exits.
    pub process_timeout: Duration,
}
impl PreviewOptions {
    /// Read configuration on the worker/actor at startup, never during UI painting.
    pub fn from_env() -> Self {
        let cache_dir = std::env::var_os("IRA_THUMBNAIL_CACHE_DIR")
            .map(PathBuf::from)
            .or_else(|| dirs_next::cache_dir().map(|d| d.join("ira/thumbnails")));
        Self {
            cache_dir,
            temp_dir: std::env::temp_dir(),
            ffmpeg: "ffmpeg".into(),
            pdftoppm: "pdftoppm".into(),
            process_timeout: FFMPEG_TIMEOUT,
        }
    }
}
pub fn read_text_preview(path: &Path) -> std::io::Result<TextPreview> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let cap = TEXT_PREVIEW_CAP.min(len) as usize;
    let mut bytes = vec![0; cap];
    file.read_exact(&mut bytes)?;
    Ok(TextPreview {
        binary: bytes.contains(&0),
        content: String::from_utf8_lossy(&bytes).into_owned(),
        truncated: len > cap as u64,
    })
}
/// Preserve legacy UTF-8 cache bytes while avoiding identity collisions for new native paths.
pub fn cache_key_path(path: &Path, mtime: Option<i64>, size: u64) -> String {
    if let Some(path) = path.to_str() {
        return cache_key(path, mtime, size);
    }
    let mut bytes = b"native-path\0".to_vec();
    bytes.extend(path.as_os_str().as_encoded_bytes());
    bytes.extend(format!("\0{}\0{size}", mtime.unwrap_or(-1)).as_bytes());
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for b in bytes {
        hash = (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}
pub fn prune_cache(options: &PreviewOptions) {
    cache::prune_cache(options);
}
pub fn load_preview(
    request: &PreviewRequest,
    options: &PreviewOptions,
) -> Result<PreviewContent, PreviewError> {
    if request.cancellation.cancelled() {
        return Err(PreviewError::Cancelled);
    }
    let content = if preview_kind(&request.path.to_string_lossy()) == Some(PreviewKind::Text) {
        PreviewContent::Text(read_text_preview(&request.path)?)
    } else {
        let img = cache::load_thumbnail(request, options)?;
        let rgba = img.into_rgba8();
        let (width, height) = rgba.dimensions();
        PreviewContent::Pixels(Pixels {
            width,
            height,
            rgba: rgba.into_raw().into(),
        })
    };
    if request.cancellation.cancelled() {
        Err(PreviewError::Cancelled)
    } else {
        Ok(content)
    }
}
