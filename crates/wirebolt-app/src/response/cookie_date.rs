//! Cookie dates accepted by the `en_US_POSIX` `EEE, dd MMM yyyy HH:mm:ss zzz`
//! and `EEEE, dd-MMM-yy HH:mm:ss zzz` formatter patterns, in that order.
use regex::Regex;
use std::sync::OnceLock;

pub(super) fn parse_cookie_date(value: &str) -> Option<f64> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let fields = PATTERN.get_or_init(|| Regex::new(concat!(
        r"(?ix)\A\s*(?:mon(?:day)?|tue(?:sday)?|wed(?:nesday)?|thu(?:rsday)?|fri(?:day)?|sat(?:urday)?|sun(?:day)?)",
        r"\s*(?:,\s+|[./-][\s./-]*,?\s*|\s+)",
        r"(?P<day>[0-9]+)(?P<day_sep>\s*(?:-\s*|[.,/][\s,./-]*)?)",
        r"(?P<month>jan(?:uary)?|feb(?:ruary)?|mar(?:ch)?|apr(?:il)?|may|jun(?:e)?|jul(?:y)?|aug(?:ust)?|sep(?:tember)?|oct(?:ober)?|nov(?:ember)?|dec(?:ember)?)",
        r"(?P<month_sep>\s*(?:-\s*|[.,/][\s,./-]*)|\s+)(?P<year>[0-9]+)\s+",
        r"(?P<hour>[0-9]+)(?:\s*[:.-]\s*|\s+)(?P<minute>[0-9]+)(?:\s*[:.-]\s*|\s+)(?P<second>[0-9]+)\s+",
        r"(?P<zone>\S+)\x20*\z"
    )).expect("cookie date pattern")).captures(value)?;
    let day: i32 = fields["day"].parse().ok()?;
    let month = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ]
    .iter()
    .position(|name| fields["month"][..3].eq_ignore_ascii_case(name))?;
    let month = i32::try_from(month).ok()? + 1;
    let year_text = &fields["year"];
    let mut year: i32 = year_text.parse().ok()?;
    // The first pattern reads a literal year, even with fewer than four digits.
    // Only the second pattern applies the 1950..2049 window to exactly two digits.
    let first_pattern = !fields["day_sep"].is_empty()
        && fields["day_sep"].chars().all(char::is_whitespace)
        && fields["month_sep"].chars().all(char::is_whitespace);
    if (!first_pattern || year == 0) && year_text.len() == 2 {
        year += if year < 50 { 2000 } else { 1900 };
    }
    let hour: i32 = fields["hour"].parse().ok()?;
    let minute: i32 = fields["minute"].parse().ok()?;
    let second: i32 = fields["second"].parse().ok()?;
    let gregorian = (year, month, day) >= (1582, 10, 15);
    let leap = year % 4 == 0 && (!gregorian || year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if !(1..=144_683).contains(&year)
        || !(1..=days).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let clock = hour * 3600 + minute * 60 + second;
    Some(
        f64::from(days_from_civil(year, month, day, gregorian)) * 86_400.0
            + f64::from(clock - timezone_offset(&fields["zone"], (year, month, day, clock))?),
    )
}

// Foundation's Gregorian calendar uses Julian dates before 1582-10-15.
fn days_from_civil(year: i32, month: i32, day: i32, gregorian: bool) -> i32 {
    let adjustment = (14 - month) / 12;
    let year = year + 4800 - adjustment;
    let month = month + 12 * adjustment - 3;
    let days = day + (153 * month + 2) / 5 + 365 * year + year / 4;
    let julian_day = if gregorian {
        days - year / 100 + year / 400 - 32_045
    } else {
        days - 32_083
    };
    julian_day - 2_440_588
}

fn timezone_offset(zone: &str, date: (i32, i32, i32, i32)) -> Option<i32> {
    let uppercase = zone.to_ascii_uppercase();
    for prefix in ["GMT", "UTC", "UT"] {
        if let Some(tail) = uppercase.strip_prefix(prefix) {
            return if tail.is_empty() {
                Some(0)
            } else {
                signed_offset(
                    tail,
                    if prefix == "GMT" {
                        OffsetStyle::Gmt
                    } else {
                        OffsetStyle::Universal
                    },
                )
            };
        }
    }
    if uppercase.starts_with(['+', '-']) {
        return signed_offset(&uppercase, OffsetStyle::Bare);
    }
    // The formatter resolves US abbreviations through their named zones, then
    // applies the abbreviation's standard/daylight flag, even out of season.
    let daylight = uppercase.ends_with("DT");
    let continental = [
        (["EST", "EDT"], -17_762, -18_000),
        (["CST", "CDT"], -21_036, -21_600),
        (["MST", "MDT"], -25_196, -25_200),
        (["PST", "PDT"], -28_378, -28_800),
    ]
    .into_iter()
    .find(|(names, _, _)| names.contains(&uppercase.as_str()));
    let standard = if let Some((_, local_mean, standard)) = continental {
        if date < (1883, 11, 18, 43_200) {
            local_mean
        } else {
            standard
        }
    } else {
        match uppercase.as_str() {
            "Z" => 0,
            "AST" | "ADT" => {
                if date < (1902, 6, 15, 864) {
                    -15_264
                } else {
                    -14_400
                }
            }
            "AKST" | "AKDT" => match date {
                d if d < (1867, 10, 18, 56_012) => 54_139,
                d if d < (1900, 8, 20, 46_661) => -32_261,
                d if d < (1983, 10, 30, if daylight { 7200 } else { 0 }) => -28_800,
                _ => -32_400,
            },
            "HST" => match date {
                d if d < (1896, 1, 13, 43_286) => -37_886,
                d if d < (1947, 6, 8, 9000) => -37_800,
                _ => -36_000,
            },
            _ => return None,
        }
    };
    Some(standard + if daylight { 3600 } else { 0 })
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum OffsetStyle {
    Gmt,
    Universal,
    Bare,
}
fn signed_offset(source: &str, style: OffsetStyle) -> Option<i32> {
    let sign = match source.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let source = &source[1..];
    if !source.bytes().all(|b| b.is_ascii_digit() || b == b':') {
        return None;
    }
    let (hour, minute, second) = if source.contains(':') {
        let fields: Vec<_> = source.split(':').collect();
        if !(2..=3).contains(&fields.len())
            || fields[0].is_empty()
            || fields[0].len() > 2
            || style == OffsetStyle::Bare && fields[0].len() != 2
            || fields[1..].iter().any(|f| f.len() != 2)
        {
            return None;
        }
        (fields[0], fields[1], fields.get(2).copied().unwrap_or("0"))
    } else if style != OffsetStyle::Gmt && (1..=6).contains(&source.len()) {
        let hour_length = if source.len().is_multiple_of(2) { 2 } else { 1 };
        (
            &source[..hour_length],
            source.get(hour_length..hour_length + 2).unwrap_or("0"),
            source
                .get(hour_length + 2..)
                .filter(|s| !s.is_empty())
                .unwrap_or("0"),
        )
    } else if style == OffsetStyle::Gmt && (1..=2).contains(&source.len()) {
        (source, "0", "0")
    } else {
        return None;
    };
    let hour: i32 = hour.parse().ok()?;
    let minute: i32 = minute.parse().ok()?;
    let second: i32 = second.parse().ok()?;
    (hour <= 23 && minute <= 59 && second <= 59)
        .then_some((hour * 3600 + minute * 60 + second) * sign)
}
