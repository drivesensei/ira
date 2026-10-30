//! Native details presentation preserves the frozen source size/time semantics.
use ira_core::services::list_files::FEntry;
pub fn detail_size(entry: &FEntry) -> String {
    if entry.is_dir {
        "—".into()
    } else {
        ira_core::services::file_info::human(entry.size)
    }
}
pub fn modified_ago(modified: Option<i64>, now: i64) -> String {
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    let Some(epoch) = modified else {
        return "—".into();
    };
    let ago = (now - epoch).max(0);
    let unit = |n: i64, one: &str, many: &str| {
        if n == 1 {
            format!("1 {one} ago")
        } else {
            format!("{n} {many} ago")
        }
    };
    if ago < MINUTE {
        "just now".to_string()
    } else if ago < HOUR {
        unit(ago / MINUTE, "minute", "minutes")
    } else if ago < DAY {
        unit(ago / HOUR, "hour", "hours")
    } else {
        let days = ago / DAY;
        if days < 365 {
            unit(days, "day", "days")
        } else {
            // Past a year the day count stops being useful ("1403 days
            // ago"); one decimal in years keeps it compact ("3.8 years
            // ago"), integers from ten up, singular at exactly one.
            let years = (days as f64 / 365.25 * 10.0).round() / 10.0;
            if years >= 10.0 {
                format!("{years:.0} years ago")
            } else if years == 1.0 {
                "1 year ago".to_string()
            } else {
                format!("{years:.1} years ago")
            }
        }
    }
}

pub fn sort_mode_label(sort_mode: usize) -> &'static str {
    match sort_mode {
        1 => "size",
        2 => "modified",
        _ => "name",
    }
}

pub fn new_entry_kind_label(name: &str) -> String {
    if let Some((stem, ext)) = name.rsplit_once('.')
        && !stem.is_empty()
        && !ext.is_empty()
        && !name.contains('/')
    {
        format!("new file (.{ext})")
    } else if name.contains('/') {
        "new nested folder(s) + file".to_string()
    } else {
        "new folder".to_string()
    }
}

pub fn format_transfer_bytes(copied: u64, total: Option<u64>) -> (f32, u8, String) {
    let fraction = match total {
        Some(t) if t > 0 => (copied as f64 / t as f64).clamp(0.0, 1.0) as f32,
        _ => 0.0,
    };
    let percent = (fraction * 100.0).round() as u8;
    let bytes_text = format!(
        "{} / {}",
        ira_core::services::file_info::human(copied),
        total
            .map(ira_core::services::file_info::human)
            .unwrap_or_else(|| "?".to_string())
    );
    (fraction, percent, bytes_text)
}

pub fn format_multi_info(
    folders: usize,
    files: usize,
    aggregate: (bool, u64, u64, u64),
    spinner: &str,
) -> (String, String) {
    let selection = match (folders, files) {
        (f, 0) => format!("{f} folders selected"),
        (0, fl) => format!("{fl} files selected"),
        (f, fl) => format!("{f} folders / {fl} files selected"),
    };
    let (complete, data, items, on_disk) = aggregate;
    let size_line = if complete {
        format!(
            "Size: {} data / {} on disk ({items} items)",
            ira_core::services::file_info::human(data),
            ira_core::services::file_info::human(on_disk)
        )
    } else {
        format!(
            "Size: {spinner} {} data / {} on disk — calculating…",
            ira_core::services::file_info::human(data),
            ira_core::services::file_info::human(on_disk)
        )
    };
    (selection, size_line)
}

pub fn split_info_line(line: &str) -> Option<(&str, &str, bool)> {
    let (label, value) = line.split_once(": ")?;
    let is_partial =
        label == "Size" && (value.contains("calculating") || value.contains("cancelled"));
    Some((label, value, is_partial))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frozen_relative_time_boundaries_and_clock_skew() {
        let now = 2_000_000_000;
        for (ago, text) in [
            (0, "just now"),
            (59, "just now"),
            (60, "1 minute ago"),
            (120, "2 minutes ago"),
            (3600, "1 hour ago"),
            (86400, "1 day ago"),
            (86400 * 3, "3 days ago"),
            (86400 * 365, "1 year ago"),
            (86400 * 1403, "3.8 years ago"),
        ] {
            assert_eq!(modified_ago(Some(now - ago), now), text);
        }
        assert_eq!(modified_ago(None, now), "—");
        assert_eq!(modified_ago(Some(now + 10), now), "just now");
    }
    #[test]
    fn directories_never_show_recursive_size_in_details() {
        let entry = FEntry {
            path: "/fixture/dir".into(),
            label: "dir".into(),
            is_dir: true,
            size: u64::MAX,
            modified: None,
        };
        assert_eq!(detail_size(&entry), "—");
    }
    #[test]
    fn sort_mode_labels_cover_all_modes_and_fallback() {
        assert_eq!(sort_mode_label(0), "name");
        assert_eq!(sort_mode_label(1), "size");
        assert_eq!(sort_mode_label(2), "modified");
        assert_eq!(sort_mode_label(99), "name");
    }
    #[test]
    fn new_entry_kind_matches_tui_rules() {
        assert_eq!(new_entry_kind_label("notes.md"), "new file (.md)");
        assert_eq!(new_entry_kind_label("archive.tar.gz"), "new file (.gz)");
        assert_eq!(new_entry_kind_label(".gitignore"), "new folder");
        assert_eq!(new_entry_kind_label("trailing."), "new folder");
        assert_eq!(new_entry_kind_label("folder"), "new folder");
        assert_eq!(
            new_entry_kind_label("a/b/file.rs"),
            "new nested folder(s) + file"
        );
        assert_eq!(new_entry_kind_label("a/b"), "new nested folder(s) + file");
    }
    #[test]
    fn transfer_bytes_formatting_handles_known_unknown_and_clamped_totals() {
        let (frac, pct, text) = format_transfer_bytes(512, Some(1024));
        assert!((frac - 0.5).abs() < 1e-6);
        assert_eq!(pct, 50);
        assert_eq!(text, "512 B / 1.0 KiB");

        let (frac_none, pct_none, text_none) = format_transfer_bytes(2048, None);
        assert_eq!(frac_none, 0.0);
        assert_eq!(pct_none, 0);
        assert_eq!(text_none, "2.0 KiB / ?");

        let (frac_zero, pct_zero, text_zero) = format_transfer_bytes(100, Some(0));
        assert_eq!(frac_zero, 0.0);
        assert_eq!(pct_zero, 0);
        assert_eq!(text_zero, "100 B / 0 B");

        let (frac_over, pct_over, _) = format_transfer_bytes(2048, Some(1024));
        assert_eq!(frac_over, 1.0);
        assert_eq!(pct_over, 100);
    }
    #[test]
    fn multi_info_formats_selection_and_aggregate_state() {
        let (sel, size) = format_multi_info(2, 0, (true, 1024, 5, 4096), "⠋");
        assert_eq!(sel, "2 folders selected");
        assert_eq!(size, "Size: 1.0 KiB data / 4.0 KiB on disk (5 items)");

        let (sel_files, size_calc) = format_multi_info(0, 3, (false, 512, 3, 4096), "⠙");
        assert_eq!(sel_files, "3 files selected");
        assert_eq!(
            size_calc,
            "Size: ⠙ 512 B data / 4.0 KiB on disk — calculating…"
        );

        let (sel_both, _) = format_multi_info(2, 4, (true, 0, 6, 0), "⠋");
        assert_eq!(sel_both, "2 folders / 4 files selected");
    }
    #[test]
    fn split_info_line_parses_kv_and_flags_partial_sizes() {
        assert_eq!(
            split_info_line("Name: report.pdf"),
            Some(("Name", "report.pdf", false))
        );
        assert_eq!(
            split_info_line("Size: 1.0 KiB data / 4.0 KiB on disk (5 items)"),
            Some(("Size", "1.0 KiB data / 4.0 KiB on disk (5 items)", false))
        );
        assert_eq!(
            split_info_line("Size: ⠋ 512 B — calculating…"),
            Some(("Size", "⠋ 512 B — calculating…", true))
        );
        assert_eq!(
            split_info_line("Size: 512 B (cancelled)"),
            Some(("Size", "512 B (cancelled)", true))
        );
        assert_eq!(
            split_info_line("Note: calculating something else"),
            Some(("Note", "calculating something else", false))
        );
        assert_eq!(split_info_line("Plain line without separator"), None);
    }
}
