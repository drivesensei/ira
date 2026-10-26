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
}
