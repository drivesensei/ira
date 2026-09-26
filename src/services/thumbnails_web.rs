//! Browser implementation of IRA's preview transport.
//!
//! The native module encodes graphics for Kitty/iTerm2/Sixel. Browsers paint
//! previews independently, so the Ratatui renderer keeps the same request
//! types while this transport remains deliberately inert.

use std::sync::{
    mpsc::{Receiver, Sender},
    Arc, Mutex,
};

use image::DynamicImage;
use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

use super::blocks::BlockImage;

pub const JOB_QUEUE_HI_CAP: usize = 32;
pub const JOB_QUEUE_LO_CAP: usize = 256;

#[derive(Debug, Clone, Default)]
pub struct Picker;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreviewSurface {
    Grid,
    Column,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    Image,
    Video,
    Heic,
    Pdf,
    Text,
}

pub fn preview_kind(path: &str) -> Option<PreviewKind> {
    let p = std::path::Path::new(path);
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp") => Some(PreviewKind::Image),
        Some("mp4" | "mov" | "m4v" | "webm" | "mkv" | "avi") => Some(PreviewKind::Video),
        Some("heic" | "heif") => Some(PreviewKind::Heic),
        Some("pdf") => Some(PreviewKind::Pdf),
        Some(
            "txt" | "md" | "markdown" | "rst" | "log" | "csv" | "tsv" | "json" | "toml" | "yaml"
            | "yml" | "xml" | "ini" | "conf" | "cfg" | "env" | "sh" | "js" | "ts" | "jsx" | "tsx"
            | "rs" | "go" | "c" | "h" | "cpp" | "hpp" | "java" | "kt" | "swift" | "sql" | "html"
            | "css" | "py" | "rb",
        ) => Some(PreviewKind::Text),
        None if !p.file_name()?.to_str()?.is_empty() => Some(PreviewKind::Text),
        _ if p.file_name()?.to_str()?.starts_with('.') => Some(PreviewKind::Text),
        _ => None,
    }
}

fn cache_key(path: &str, mtime: Option<i64>, size: u64) -> String {
    format!("{path}\0{}\0{size}", mtime.unwrap_or(-1))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ThumbRequest {
    pub path: String,
    pub mtime: Option<i64>,
    pub size: u64,
    pub cols: u16,
    pub rows: u16,
    pub surface: PreviewSurface,
}

impl ThumbRequest {
    pub fn mem_key(&self) -> String {
        format!(
            "{}\0{}\0{}",
            cache_key(&self.path, self.mtime, self.size),
            self.cols,
            self.rows
        )
    }
}

pub enum Rendered {
    Blocks(BlockImage),
    Pixels {
        img: DynamicImage,
        fitted: Option<(u32, u32, DynamicImage)>,
        fallback: BlockImage,
    },
}

impl Rendered {
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        match self {
            Self::Blocks(blocks)
            | Self::Pixels {
                fallback: blocks, ..
            } => blocks.render(area, buf),
        }
    }
    pub fn pixels(&self) -> Option<&DynamicImage> {
        match self {
            Self::Pixels { img, .. } => Some(img),
            _ => None,
        }
    }
    pub fn fitted_at(&self, w: u32, h: u32) -> Option<&DynamicImage> {
        match self {
            Self::Pixels {
                fitted: Some((fw, fh, img)),
                ..
            } if *fw == w && *fh == h => Some(img),
            _ => None,
        }
    }
    pub fn fitted_bytes(&self) -> Option<usize> {
        match self {
            Self::Pixels {
                fitted: Some((w, h, img)),
                ..
            } => Some(*w as usize * *h as usize * img.color().bytes_per_pixel() as usize),
            _ => None,
        }
    }
    pub fn ensure_fitted(&mut self, w: u32, h: u32) -> Option<&DynamicImage> {
        if w == 0 || h == 0 {
            return None;
        }
        match self {
            Self::Pixels { img, fitted, .. } => {
                if !matches!(fitted.as_ref(), Some((fw, fh, _)) if *fw == w && *fh == h) {
                    *fitted = Some((w, h, crate::services::overlay::fit_pixels(img, w, h)));
                }
                fitted.as_ref().map(|(_, _, image)| image)
            }
            _ => None,
        }
    }
    pub fn clear_fitted(&mut self) {
        if let Self::Pixels { fitted, .. } = self {
            *fitted = None;
        }
    }
}

pub enum ThumbEvent {
    Ready(ThumbRequest, Rendered),
    Failed(ThumbRequest),
    Text {
        req: ThumbRequest,
        content: String,
        binary: bool,
        truncated: bool,
    },
}

pub struct WorkerQueues {
    pub hi: Arc<Mutex<Receiver<ThumbRequest>>>,
    pub lo: Arc<Mutex<Receiver<ThumbRequest>>>,
}

pub fn spawn_workers(_: Picker, _: bool, _: WorkerQueues, _: Sender<ThumbEvent>) {}
pub fn prune_cache() {}
