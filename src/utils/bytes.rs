/// Formats a byte count with binary (1024-based) units, e.g. `12.3G`.
pub fn format_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["K", "M", "G", "T", "P"];
    if n < 1024 {
        return format!("{n}B");
    }
    let mut v = n as f64 / 1024.0;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    // Round first, then pick the unit/precision, so 1023.96K shows as 1.0M
    // (not "1024K") and 99.96K as "100K" (not "100.0K").
    let mut rounded = (v * 10.0).round() / 10.0;
    if rounded >= 1024.0 && i < UNITS.len() - 1 {
        v = rounded / 1024.0;
        i += 1;
        rounded = (v * 10.0).round() / 10.0;
    }
    if rounded >= 100.0 {
        format!("{:.0}{}", v, UNITS[i])
    } else {
        format!("{:.1}{}", v, UNITS[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_boundaries() {
        assert_eq!(format_bytes(0), "0B");
        assert_eq!(format_bytes(1023), "1023B");
        assert_eq!(format_bytes(1024), "1.0K");
        assert_eq!(format_bytes(1536 * 1024 * 1024), "1.5G");
        assert_eq!(format_bytes(2 * 1024u64.pow(4)), "2.0T");
        assert_eq!(format_bytes(500 * 1024u64.pow(3)), "500G");
    }

    #[test]
    fn rounds_up_across_unit_boundaries() {
        // 1023.96K -> 1.0M, not "1024K".
        assert_eq!(format_bytes(1024 * 1024 - 41), "1.0M");
        assert_eq!(format_bytes(1024u64.pow(3) - 1), "1.0G");
        // 99.96K -> "100K", not "100.0K".
        assert_eq!(format_bytes(102_359), "100K");
        // Largest unit never overflows past P.
        assert_eq!(format_bytes(u64::MAX), "16384P");
    }
}
