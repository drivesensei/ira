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
