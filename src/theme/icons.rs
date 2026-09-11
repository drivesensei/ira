//! File-type icons: Nerd Font glyphs, a color-emoji set for terminals
//! without a Nerd Font, and a single-width Unicode fallback.

use crate::services::list_files::FEntry;

/// Which glyph set the UI is rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconSet {
    /// Nerd Fonts v3 Private-Use-Area glyphs (plus a trailing pad space
    /// at the call site so wide fallbacks do not shift the name).
    Nerd,
    /// Color emoji, restricted to codepoints Unicode classes as wide
    /// (`East_Asian_Width=W`): exactly two cells on every terminal that
    /// follows the standard, no variation selectors or ZWJ sequences.
    /// Rendered by the terminal's emoji fallback font (Segoe UI Emoji,
    /// Apple Color Emoji, Noto Color Emoji), so it needs nothing installed.
    Emoji,
    /// Built-in single-width geometric Unicode. Safe on every stock
    /// terminal font (DejaVu, Menlo, Consolas, Cascadia).
    Unicode,
}

/// Coarse kind shared by the icon, its color, the info-dialog label, and
/// (where it overlaps) preview classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileCategory {
    Folder,
    Text,
    Code,
    Image,
    Video,
    Audio,
    Archive,
    Document,
    Pdf,
    Spreadsheet,
    Executable,
    Data,
    DiskImage,
    Other,
}

/// Columns an icon occupies in a list row (Nerd = glyph + pad space,
/// Emoji = one wide glyph).
pub fn icon_cols(set: IconSet) -> usize {
    match set {
        IconSet::Nerd | IconSet::Emoji => 2,
        IconSet::Unicode => 1,
    }
}

/// Pads a Nerd glyph with a trailing space so a double-width fallback
/// cannot collide with the file name. Emoji are already two cells wide.
pub fn pad_icon(glyph: &str, set: IconSet) -> String {
    match set {
        IconSet::Nerd => format!("{glyph} "),
        IconSet::Emoji | IconSet::Unicode => glyph.to_string(),
    }
}

/// Icon + category for a directory listing entry.
pub fn icon_for(entry: &FEntry, set: IconSet) -> (&'static str, FileCategory) {
    icon_for_name(&entry.label, entry.is_dir, set)
}

/// Icon + category for a path/name. Lookup order: special directories,
/// exact (or prefix) filenames, then extension.
pub fn icon_for_name(name: &str, is_dir: bool, set: IconSet) -> (&'static str, FileCategory) {
    if is_dir {
        return (dir_icon(name, set), FileCategory::Folder);
    }
    if let Some((glyph, cat)) = special_file_icon(name, set) {
        return (glyph, cat);
    }
    let cat = file_category(false, name);
    (file_icon(cat, name, set), cat)
}

/// Coarse category for `name`. Directories are always [`FileCategory::Folder`].
pub fn file_category(is_dir: bool, name: &str) -> FileCategory {
    if is_dir {
        return FileCategory::Folder;
    }
    if is_compound_archive(name) {
        return FileCategory::Archive;
    }
    classify_ext(&extension(name)).0
}

/// Human label used by the info dialog (`Kind: …`). Keeps the existing
/// specific strings (`Rust source`, `Python source`, …).
pub fn kind_label(is_dir: bool, name: &str) -> &'static str {
    if is_dir {
        return "Folder";
    }
    if is_compound_archive(name) {
        return "Archive";
    }
    classify_ext(&extension(name)).1
}

/// Drive glyph for the drives panel.
pub fn drive_icon(set: IconSet) -> &'static str {
    match set {
        IconSet::Nerd => "\u{f0a0}", // nf-fa-hdd
        IconSet::Emoji => emoji::DRIVE,
        IconSet::Unicode => "▣",
    }
}

/// Bookmark glyph for the bookmarks panel.
pub fn bookmark_icon(set: IconSet) -> &'static str {
    match set {
        IconSet::Nerd => "\u{f02e}", // nf-fa-bookmark
        IconSet::Emoji => emoji::BOOKMARK,
        IconSet::Unicode => "▸",
    }
}

/// Common-folder glyph keyed by the folder's display label.
pub fn common_folder_icon(label: &str, set: IconSet) -> &'static str {
    if set == IconSet::Emoji {
        return emoji::common_folder(label);
    }
    match (label, set) {
        ("Home", IconSet::Nerd) => "\u{f015}",
        ("Desktop", IconSet::Nerd) => "\u{f108}",
        ("Documents", IconSet::Nerd) => "\u{f02d}",
        ("Downloads", IconSet::Nerd) => "\u{f019}",
        ("Music", IconSet::Nerd) => "\u{f001}",
        ("Videos", IconSet::Nerd) => "\u{f03d}",
        ("Public", IconSet::Nerd) => "\u{f0ac}",
        ("Home", IconSet::Unicode) => "⌂",
        ("Desktop", IconSet::Unicode) => "▢",
        ("Documents", IconSet::Unicode) => "▤",
        ("Downloads", IconSet::Unicode) => "↓",
        ("Music", IconSet::Unicode) => "♪",
        ("Videos", IconSet::Unicode) => "▶",
        ("Public", IconSet::Unicode) => "○",
        (_, IconSet::Nerd) => "\u{f07b}",
        (_, IconSet::Emoji) => emoji::FOLDER, // handled above; keeps the match total
        (_, IconSet::Unicode) => "□",
    }
}

fn dir_icon(name: &str, set: IconSet) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if set == IconSet::Emoji {
        return emoji::dir(&lower);
    }
    match (lower.as_str(), set) {
        (".git", IconSet::Nerd) => "\u{e702}",
        ("node_modules", IconSet::Nerd) => "\u{e718}",
        ("src", IconSet::Nerd) => "\u{f121}",
        ("target", IconSet::Nerd) => "\u{e7a8}",
        ("downloads", IconSet::Nerd) => "\u{f019}",
        ("documents", IconSet::Nerd) => "\u{f02d}",
        ("desktop", IconSet::Nerd) => "\u{f108}",
        ("music", IconSet::Nerd) => "\u{f001}",
        ("videos" | "movies", IconSet::Nerd) => "\u{f03d}",
        ("pictures" | "photos", IconSet::Nerd) => "\u{f1c5}",
        (_, IconSet::Nerd) => "\u{f07b}",
        (_, IconSet::Emoji) => emoji::FOLDER, // handled above; keeps the match total
        (_, IconSet::Unicode) => "□",
    }
}

fn special_file_icon(name: &str, set: IconSet) -> Option<(&'static str, FileCategory)> {
    let lower = name.to_ascii_lowercase();
    if set == IconSet::Emoji {
        return emoji::special_file(&lower);
    }
    let (glyph, cat) = match (lower.as_str(), set) {
        ("cargo.toml" | "cargo.lock", IconSet::Nerd) => ("\u{e7a8}", FileCategory::Code),
        ("cargo.toml" | "cargo.lock", IconSet::Unicode) => ("λ", FileCategory::Code),
        ("dockerfile" | "containerfile", IconSet::Nerd) => ("\u{f308}", FileCategory::Code),
        ("dockerfile" | "containerfile", IconSet::Unicode) => ("λ", FileCategory::Code),
        ("makefile" | "gnumakefile", IconSet::Nerd) => ("\u{f121}", FileCategory::Code),
        ("makefile" | "gnumakefile", IconSet::Unicode) => ("λ", FileCategory::Code),
        ("cmakelists.txt", IconSet::Nerd) => ("\u{f121}", FileCategory::Code),
        ("cmakelists.txt", IconSet::Unicode) => ("λ", FileCategory::Code),
        (".gitignore" | ".gitattributes" | ".gitmodules", IconSet::Nerd) => {
            ("\u{e702}", FileCategory::Code)
        }
        (".gitignore" | ".gitattributes" | ".gitmodules", IconSet::Unicode) => {
            ("λ", FileCategory::Code)
        }
        ("package.json" | "package-lock.json", IconSet::Nerd) => ("\u{e718}", FileCategory::Code),
        ("package.json" | "package-lock.json", IconSet::Unicode) => ("λ", FileCategory::Code),
        ("go.mod" | "go.sum", IconSet::Nerd) => ("\u{e724}", FileCategory::Code),
        ("go.mod" | "go.sum", IconSet::Unicode) => ("λ", FileCategory::Code),
        ("license" | "licence" | "copying", IconSet::Nerd) => ("\u{f15c}", FileCategory::Text),
        ("license" | "licence" | "copying", IconSet::Unicode) => ("≡", FileCategory::Text),
        _ if is_readme(&lower) => match set {
            IconSet::Nerd => ("\u{e73e}", FileCategory::Text),
            IconSet::Emoji => (emoji::README, FileCategory::Text), // handled above
            IconSet::Unicode => ("≡", FileCategory::Text),
        },
        _ => return None,
    };
    Some((glyph, cat))
}

fn is_readme(lower: &str) -> bool {
    lower == "readme" || lower.starts_with("readme.")
}

fn file_icon(cat: FileCategory, name: &str, set: IconSet) -> &'static str {
    match set {
        IconSet::Nerd => return nerd_file_icon(cat, name),
        IconSet::Emoji => return emoji::file(cat, name),
        IconSet::Unicode => {}
    }
    match cat {
        FileCategory::Folder => "□",
        FileCategory::Archive => "▣",
        FileCategory::Video => "▶",
        FileCategory::Audio => "♪",
        FileCategory::Text => "≡",
        FileCategory::Code => "λ",
        FileCategory::Image => "▦",
        FileCategory::Document | FileCategory::Pdf => "▤",
        FileCategory::Spreadsheet => "▥",
        FileCategory::Executable => "▸",
        FileCategory::Data => "◈",
        FileCategory::DiskImage => "◉",
        FileCategory::Other => "·",
    }
}

fn nerd_file_icon(cat: FileCategory, name: &str) -> &'static str {
    let ext = extension(name);
    match ext.as_str() {
        "rs" => return "\u{e7a8}",
        "py" => return "\u{e73c}",
        "js" | "mjs" | "cjs" => return "\u{e74e}",
        "ts" | "tsx" | "jsx" => return "\u{e628}",
        "html" | "htm" => return "\u{e736}",
        "css" | "scss" | "less" => return "\u{e749}",
        "go" => return "\u{e724}",
        "java" => return "\u{e738}",
        "rb" => return "\u{e739}",
        "c" | "h" => return "\u{e61e}",
        "cpp" | "hpp" | "cc" => return "\u{e61d}",
        "md" | "markdown" => return "\u{e73e}",
        "json" => return "\u{e60b}",
        "toml" | "yml" | "yaml" => return "\u{f013}",
        _ => {}
    }
    match cat {
        FileCategory::Folder => "\u{f07b}",
        FileCategory::Text => "\u{f15c}",
        FileCategory::Code => "\u{f1c9}",
        FileCategory::Image => "\u{f1c5}",
        FileCategory::Video => "\u{f1c8}",
        FileCategory::Audio => "\u{f1c7}",
        FileCategory::Archive => "\u{f1c6}",
        FileCategory::Pdf => "\u{f1c1}",
        FileCategory::Document => "\u{f15b}",
        FileCategory::Spreadsheet => "\u{f1c3}",
        FileCategory::Executable => "\u{f489}",
        FileCategory::Data => "\u{f1c0}",
        FileCategory::DiskImage => "\u{f0a0}",
        FileCategory::Other => "\u{f15b}",
    }
}

/// The emoji set. Every glyph is one scalar with `East_Asian_Width=W`
/// (asserted by `emoji_glyphs_are_single_scalars_two_cells_wide`), so it
/// occupies exactly two cells and needs no variation selector. Glyphs in
/// the ambiguous/neutral class (⚙ 🖼 🗄 🖥 …) are deliberately absent:
/// terminals disagree on their width.
mod emoji {
    use super::{extension, FileCategory};

    pub const FOLDER: &str = "📁"; // U+1F4C1
    pub const DRIVE: &str = "💾"; // U+1F4BE
    pub const BOOKMARK: &str = "🔖"; // U+1F516
    pub const README: &str = "📖"; // U+1F4D6

    const TEXT: &str = "📄"; // U+1F4C4
    const CODE: &str = "🧩"; // U+1F9E9
    const CONFIG: &str = "🔧"; // U+1F527
    const PACKAGE: &str = "📦"; // U+1F4E6
    const GIT: &str = "🌿"; // U+1F33F
    const RUST: &str = "🦀"; // U+1F980
    const GO: &str = "🐹"; // U+1F439
    const WEB: &str = "🌐"; // U+1F310
    const MUSIC: &str = "🎵"; // U+1F3B5
    const VIDEO: &str = "🎬"; // U+1F3AC
    const PICTURES: &str = "🌄"; // U+1F304
    const DOCUMENTS: &str = "📚"; // U+1F4DA
    const DOWNLOADS: &str = "📥"; // U+1F4E5

    pub fn common_folder(label: &str) -> &'static str {
        match label {
            "Home" => "🏠",    // U+1F3E0
            "Desktop" => "💻", // U+1F4BB
            "Documents" => DOCUMENTS,
            "Downloads" => DOWNLOADS,
            "Music" => MUSIC,
            "Videos" => VIDEO,
            "Public" => WEB,
            _ => FOLDER,
        }
    }

    pub fn dir(lower: &str) -> &'static str {
        match lower {
            ".git" => GIT,
            "node_modules" => PACKAGE,
            "src" => CODE,
            "target" => "🎯", // U+1F3AF
            "downloads" => DOWNLOADS,
            "documents" => DOCUMENTS,
            "desktop" => "💻",
            "music" => MUSIC,
            "videos" | "movies" => VIDEO,
            "pictures" | "photos" => PICTURES,
            _ => FOLDER,
        }
    }

    pub fn special_file(lower: &str) -> Option<(&'static str, FileCategory)> {
        Some(match lower {
            "cargo.toml" | "cargo.lock" => (RUST, FileCategory::Code),
            "dockerfile" | "containerfile" => ("🐳", FileCategory::Code), // U+1F433
            "makefile" | "gnumakefile" | "cmakelists.txt" => (CONFIG, FileCategory::Code),
            ".gitignore" | ".gitattributes" | ".gitmodules" => (GIT, FileCategory::Code),
            "package.json" | "package-lock.json" => (PACKAGE, FileCategory::Code),
            "go.mod" | "go.sum" => (GO, FileCategory::Code),
            "license" | "licence" | "copying" => ("📜", FileCategory::Text), // U+1F4DC
            _ if super::is_readme(lower) => (README, FileCategory::Text),
            _ => return None,
        })
    }

    pub fn file(cat: FileCategory, name: &str) -> &'static str {
        match extension(name).as_str() {
            "rs" => return RUST,
            "py" => return "🐍",                 // U+1F40D
            "js" | "mjs" | "cjs" => return "🟨", // U+1F7E8
            "ts" | "tsx" | "jsx" => return "🟦", // U+1F7E6
            "html" | "htm" => return WEB,
            "css" | "scss" | "less" => return "🎨", // U+1F3A8
            "go" => return GO,
            "java" => return "☕",                           // U+2615 (wide)
            "rb" => return "💎",                             // U+1F48E
            "c" | "h" | "cpp" | "hpp" | "cc" => return "🔩", // U+1F529
            "md" | "markdown" => return "📝",                // U+1F4DD
            "json" => return "🧾",                           // U+1F9FE
            "toml" | "yml" | "yaml" => return CONFIG,
            _ => {}
        }
        match cat {
            FileCategory::Folder => FOLDER,
            FileCategory::Text => TEXT,
            FileCategory::Code => CODE,
            FileCategory::Image => "📷", // U+1F4F7
            FileCategory::Video => VIDEO,
            FileCategory::Audio => MUSIC,
            FileCategory::Archive => PACKAGE,
            FileCategory::Pdf => "📕",         // U+1F4D5
            FileCategory::Document => "📘",    // U+1F4D8
            FileCategory::Spreadsheet => "📊", // U+1F4CA
            FileCategory::Executable => "🚀",  // U+1F680
            FileCategory::Data => "📇",        // U+1F4C7
            FileCategory::DiskImage => "💿",   // U+1F4BF
            FileCategory::Other => "📃",       // U+1F4C3
        }
    }

    /// Every glyph this module can emit (for the width test).
    #[cfg(test)]
    pub fn all() -> Vec<&'static str> {
        let mut v = vec![FOLDER, DRIVE, BOOKMARK, README];
        for label in [
            "Home",
            "Desktop",
            "Documents",
            "Downloads",
            "Music",
            "Videos",
            "Public",
            "x",
        ] {
            v.push(common_folder(label));
        }
        for d in [
            ".git",
            "node_modules",
            "src",
            "target",
            "downloads",
            "documents",
            "desktop",
            "music",
            "videos",
            "pictures",
            "x",
        ] {
            v.push(dir(d));
        }
        for f in [
            "cargo.toml",
            "dockerfile",
            "makefile",
            ".gitignore",
            "package.json",
            "go.mod",
            "license",
            "readme.md",
        ] {
            v.push(special_file(f).unwrap().0);
        }
        for f in [
            "a.rs", "a.py", "a.js", "a.ts", "a.html", "a.css", "a.go", "a.java", "a.rb", "a.c",
            "a.md", "a.json", "a.toml",
        ] {
            v.push(file(FileCategory::Other, f));
        }
        for cat in [
            FileCategory::Folder,
            FileCategory::Text,
            FileCategory::Code,
            FileCategory::Image,
            FileCategory::Video,
            FileCategory::Audio,
            FileCategory::Archive,
            FileCategory::Pdf,
            FileCategory::Document,
            FileCategory::Spreadsheet,
            FileCategory::Executable,
            FileCategory::Data,
            FileCategory::DiskImage,
            FileCategory::Other,
        ] {
            v.push(file(cat, "noext"));
        }
        v
    }
}

fn is_compound_archive(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".tar.gz")
        || lower.ends_with(".tar.bz2")
        || lower.ends_with(".tar.xz")
        || lower.ends_with(".tar.zst")
        || lower.ends_with(".tar.zstd")
}

/// Last `.ext` of `name`, lowercased. Empty when there is no extension.
fn extension(name: &str) -> String {
    let mut dot: Option<usize> = None;
    for (i, c) in name.char_indices() {
        if c == '.' {
            dot = Some(i);
        }
    }
    match dot {
        Some(i) if i + 1 < name.len() => name[i + 1..].to_ascii_lowercase(),
        _ => String::new(),
    }
}

fn classify_ext(ext: &str) -> (FileCategory, &'static str) {
    match ext {
        "txt" | "md" | "markdown" | "rst" | "log" | "conf" | "ini" | "yml" | "yaml" => {
            (FileCategory::Text, "Text file")
        }
        "toml" => (FileCategory::Text, "Text file"),
        "rs" => (FileCategory::Code, "Rust source"),
        "py" => (FileCategory::Code, "Python source"),
        "js" | "ts" | "mjs" | "cjs" | "jsx" | "tsx" => (FileCategory::Code, "JavaScript/TS source"),
        "json" => (FileCategory::Data, "JSON document"),
        "html" | "htm" | "css" | "scss" | "less" => (FileCategory::Code, "Web document"),
        "go" | "c" | "h" | "cpp" | "hpp" | "cc" | "java" | "kt" | "swift" | "rb" | "pl" | "lua"
        | "vim" | "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" => {
            if matches!(ext, "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat") {
                (FileCategory::Executable, "Executable")
            } else {
                (FileCategory::Code, "File")
            }
        }
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "avif" | "ico" | "heic"
        | "heif" => (FileCategory::Image, "Image"),
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "m4v" => (FileCategory::Video, "Video"),
        "mp3" | "flac" | "ogg" | "wav" | "m4a" | "aac" => (FileCategory::Audio, "Audio"),
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "zst" => {
            (FileCategory::Archive, "Archive")
        }
        "pdf" => (FileCategory::Pdf, "PDF document"),
        "doc" | "docx" | "odt" | "rtf" => (FileCategory::Document, "Word document"),
        "xls" | "xlsx" | "ods" | "csv" | "tsv" => (FileCategory::Spreadsheet, "Spreadsheet"),
        "iso" | "img" => (FileCategory::DiskImage, "Disk image"),
        "exe" | "msi" | "bin" | "dmg" => (FileCategory::Executable, "Executable"),
        "sqlite" | "db" | "sqlite3" => (FileCategory::Data, "Database"),
        "git" => (FileCategory::Data, "Git repository"),
        "xml" | "cfg" | "properties" | "env" | "sql" | "service" | "desktop" => {
            (FileCategory::Text, "Text file")
        }
        _ => (FileCategory::Other, "File"),
    }
}

/// Every Unicode-set glyph used by this module (for width tests).
#[cfg(test)]
fn unicode_glyphs() -> &'static [&'static str] {
    &[
        "□", "▣", "▶", "♪", "≡", "λ", "▦", "▤", "▥", "▸", "◈", "◉", "·", "⌂", "▢", "↓", "○",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    fn entry(name: &str, is_dir: bool) -> FEntry {
        FEntry {
            path: format!("/x/{name}"),
            label: name.to_string(),
            is_dir,
            size: 0,
            modified: None,
        }
    }

    #[test]
    fn unicode_glyphs_are_single_width() {
        for g in unicode_glyphs() {
            assert_eq!(UnicodeWidthStr::width(*g), 1, "glyph {g:?} must be width 1");
        }
    }

    #[test]
    fn emoji_glyphs_are_single_scalars_two_cells_wide() {
        for g in emoji::all() {
            assert_eq!(
                g.chars().count(),
                1,
                "emoji {g:?} must be one scalar (no VS16 / ZWJ sequences)"
            );
            assert_eq!(
                UnicodeWidthStr::width(g),
                2,
                "emoji {g:?} must be East_Asian_Width=W (two cells)"
            );
        }
        assert_eq!(icon_cols(IconSet::Emoji), 2);
        assert_eq!(pad_icon(emoji::FOLDER, IconSet::Emoji), emoji::FOLDER);
    }

    #[test]
    fn emoji_set_distinguishes_languages_like_the_nerd_set() {
        let rs = icon_for(&entry("main.rs", false), IconSet::Emoji).0;
        let py = icon_for(&entry("app.py", false), IconSet::Emoji).0;
        let txt = icon_for(&entry("notes.txt", false), IconSet::Emoji).0;
        assert_ne!(rs, py);
        assert_ne!(rs, txt);
        // Special names win over the extension, as in the other sets.
        assert_eq!(
            icon_for(&entry("Cargo.toml", false), IconSet::Emoji).0,
            rs,
            "Cargo.toml shares the Rust glyph"
        );
        assert_eq!(
            icon_for(&entry("config.toml", false), IconSet::Emoji).1,
            FileCategory::Text
        );
    }

    #[test]
    fn both_sets_cover_the_same_categories() {
        let samples = [
            ("src", true),
            (".git", true),
            ("notes.txt", false),
            ("main.rs", false),
            ("shot.png", false),
            ("clip.mp4", false),
            ("song.mp3", false),
            ("pack.zip", false),
            ("doc.pdf", false),
            ("data.csv", false),
            ("app.exe", false),
            ("db.sqlite", false),
            ("disk.iso", false),
            ("unknown.bin.xyz", false),
            ("Cargo.toml", false),
            ("README.md", false),
        ];
        for (name, is_dir) in samples {
            let (n_glyph, n_cat) = icon_for(&entry(name, is_dir), IconSet::Nerd);
            let (e_glyph, e_cat) = icon_for(&entry(name, is_dir), IconSet::Emoji);
            let (u_glyph, u_cat) = icon_for(&entry(name, is_dir), IconSet::Unicode);
            assert!(!n_glyph.is_empty(), "nerd icon missing for {name}");
            assert!(!e_glyph.is_empty(), "emoji icon missing for {name}");
            assert!(!u_glyph.is_empty(), "unicode icon missing for {name}");
            assert_eq!(n_cat, u_cat, "categories must agree for {name}");
            assert_eq!(n_cat, e_cat, "categories must agree for {name}");
        }
    }

    #[test]
    fn special_filenames_beat_extensions() {
        let (glyph, cat) = icon_for(&entry("Cargo.toml", false), IconSet::Unicode);
        assert_eq!(cat, FileCategory::Code);
        assert_eq!(glyph, "λ");
        // A random toml file stays a text document.
        let (_, cat) = icon_for(&entry("config.toml", false), IconSet::Unicode);
        assert_eq!(cat, FileCategory::Text);
        let (_, cat) = icon_for(&entry("README.md", false), IconSet::Unicode);
        assert_eq!(cat, FileCategory::Text);
    }

    #[test]
    fn kind_label_matches_legacy_info_strings() {
        assert_eq!(kind_label(true, "src"), "Folder");
        assert_eq!(kind_label(false, "main.rs"), "Rust source");
        assert_eq!(kind_label(false, "app.py"), "Python source");
        assert_eq!(kind_label(false, "notes.txt"), "Text file");
        assert_eq!(kind_label(false, "shot.png"), "Image");
        assert_eq!(kind_label(false, "clip.mp4"), "Video");
        assert_eq!(kind_label(false, "pack.zip"), "Archive");
        assert_eq!(kind_label(false, "doc.pdf"), "PDF document");
        assert_eq!(kind_label(false, "weird"), "File");
    }

    #[test]
    fn folder_and_file_defaults_stay_the_legacy_glyphs() {
        let (g, _) = icon_for(&entry("src", true), IconSet::Unicode);
        assert_eq!(g, "□");
        let (g, _) = icon_for(&entry("r.txt", false), IconSet::Unicode);
        assert_eq!(g, "≡"); // text, not the generic dot
        let (g, _) = icon_for(&entry("noext", false), IconSet::Unicode);
        assert_eq!(g, "·");
    }
}
