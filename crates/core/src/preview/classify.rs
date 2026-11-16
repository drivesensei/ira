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

/// Classifies a path by extension, then by name. `None` = no preview at all.
///
/// Extensionless files and dotfiles (`.env`, `.env.local`, `.gitignore`,
/// `Makefile`, `LICENSE`, …) count as text: the reader NUL-sniffs the head and
/// shows a "binary file" placeholder, so a mislabelled binary degrades
/// gracefully instead of being unopenable.
pub fn preview_kind(path: &str) -> Option<PreviewKind> {
    let p = std::path::Path::new(path);
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    if let Some(kind) = ext.as_deref().and_then(kind_from_ext) {
        return Some(kind);
    }
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if is_text_like_name(name) {
        return Some(PreviewKind::Text);
    }
    None
}

/// Preview kind for a known file extension.
fn kind_from_ext(ext: &str) -> Option<PreviewKind> {
    match ext {
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" => Some(PreviewKind::Image),
        "mp4" | "mov" | "m4v" | "webm" | "mkv" | "avi" => Some(PreviewKind::Video),
        "heic" | "heif" => Some(PreviewKind::Heic),
        "pdf" => Some(PreviewKind::Pdf),
        "txt" | "md" | "markdown" | "rst" | "log" | "csv" | "tsv" | "json" | "toml" | "yaml"
        | "yml" | "xml" | "ini" | "conf" | "cfg" | "properties" | "env" | "sh" | "bash" | "zsh"
        | "fish" | "ps1" | "bat" | "py" | "rb" | "pl" | "js" | "ts" | "jsx" | "tsx" | "rs"
        | "go" | "c" | "h" | "cpp" | "hpp" | "cc" | "java" | "kt" | "swift" | "sql" | "html"
        | "htm" | "css" | "scss" | "less" | "lua" | "vim" | "service" | "desktop" => {
            Some(PreviewKind::Text)
        }
        _ => None,
    }
}

/// Extensionless or dotfile names that are text: well-known build/doc files
/// (by full name or stem), every dotfile (`.env`, `.env.local`, `.bashrc`,
/// `.gitignore`, …), and anything with no dot at all (`Makefile`, `LICENSE`,
/// `configure`, an extensionless script).
fn is_text_like_name(name: &str) -> bool {
    const NAMES: &[&str] = &[
        "makefile",
        "dockerfile",
        "containerfile",
        "justfile",
        "rakefile",
        "gemfile",
        "procfile",
        "vagrantfile",
        "cmakelists.txt",
        "license",
        "licence",
        "copying",
        "readme",
        "changelog",
        "authors",
        "notice",
        "install",
        "todo",
    ];
    let lower = name.to_ascii_lowercase();
    if NAMES.contains(&lower.as_str()) {
        return true;
    }
    // `Makefile.am`, `Dockerfile.dev`, `README.old`: a known stem whose
    // trailing part the extension table did not recognise still reads as text.
    let stem = lower.split('.').next().unwrap_or_default();
    if !stem.is_empty() && NAMES.contains(&stem) {
        return true;
    }
    // Dotfiles — including ones whose trailing part looks like an extension
    // (`.env.local`, `.env.production`).
    if lower.starts_with('.') {
        return true;
    }
    // No dot at all: `Makefile`, `LICENSE`, `configure`. Binaries mislabelled
    // here render as the "binary file" placeholder, not as garbage.
    !lower.contains('.')
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
