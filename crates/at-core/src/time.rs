//! Small time helpers without pulling in a date crate.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// RFC 3339 UTC timestamp, e.g. `2026-10-18T10:00:00Z`.
pub fn rfc3339_utc(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let (y, mo, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Local broken-down time: (year, month, day, hour, minute).
pub fn local_parts(ms: u64) -> (i64, u32, u32, u32, u32) {
    let t: libc::time_t = (ms / 1000) as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ok = unsafe { !libc::localtime_r(&t, &mut tm).is_null() };
    if !ok {
        let (y, m, d) = civil_from_days((ms / 1000 / 86_400) as i64);
        return (y, m, d, 0, 0);
    }
    (
        tm.tm_year as i64 + 1900,
        tm.tm_mon as u32 + 1,
        tm.tm_mday as u32,
        tm.tm_hour as u32,
        tm.tm_min as u32,
    )
}

/// `18.10.2026 10:00` for Russian, `2026-10-18 10:00` otherwise.
pub fn format_local(ms: u64, lang: &str) -> String {
    let (y, mo, d, h, mi) = local_parts(ms);
    if lang.starts_with("ru") {
        format!("{d:02}.{mo:02}.{y} {h:02}:{mi:02}")
    } else {
        format!("{y}-{mo:02}-{d:02} {h:02}:{mi:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339() {
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(1_792_317_600_000), "2026-10-18T10:00:00Z");
    }
}
