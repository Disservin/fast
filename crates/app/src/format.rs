//! Numbers, sizes and times written the way the app shows them everywhere.

/// A count with thousands separators: "1,234,567".
pub fn format_count(count: u64) -> String {
    let digits = count.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// A byte count as a player reads it: "820 B", "14.2 MB", "1.3 GB".
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1000.;
    let mut unit = 0;
    // 999.95 and up would round to "1000.0", so it moves to the next unit.
    while value >= 999.95 && unit + 1 < UNITS.len() {
        value /= 1000.;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// A length of time in whole seconds: "42s", "3m 05s", "2h 07m".
pub fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h {:02}m", secs / 3600, secs / 60 % 60)
    }
}

/// Seconds since the Unix epoch, or 0 if the clock is before it.
pub fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_have_thousands_separators() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1_000), "1,000");
        assert_eq!(format_count(1_234_567), "1,234,567");
    }

    #[test]
    fn byte_counts_are_short_and_never_show_1000() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(1_000), "1.0 KB");
        assert_eq!(format_bytes(14_230_000), "14.2 MB");
        assert_eq!(format_bytes(999_960), "1.0 MB");
        assert_eq!(format_bytes(1_300_000), "1.3 MB");
        assert_eq!(format_bytes(1_300_000_000), "1.3 GB");
    }

    #[test]
    fn durations_use_the_two_largest_units() {
        assert_eq!(format_duration(42), "42s");
        assert_eq!(format_duration(185), "3m 05s");
        assert_eq!(format_duration(7_620), "2h 07m");
    }
}
