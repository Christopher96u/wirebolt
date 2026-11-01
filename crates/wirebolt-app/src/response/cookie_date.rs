//! RFC 6265 date tokens, with the legacy formatter's 1950 two-digit-year window
//! and named/numeric timezone extensions. All results are Unix seconds.

pub(super) fn parse_cookie_date(value: &str) -> Option<f64> {
    let mut time = None;
    let mut timezone = "";
    let mut day = None;
    let mut month = None;
    let mut year = None;
    let tokens: Vec<_> = value
        .split(|c: char| {
            c.is_whitespace() || matches!(c, ' '..='/' | ';'..='@' | '['..='`' | '{'..='~')
        })
        .filter(|t| !t.is_empty())
        .collect();
    for token in &tokens {
        if time.is_none()
            && let Some(parsed) = parse_time(token)
        {
            let start = value.find(token)?;
            timezone = &value[start + parsed.length..];
            time = Some(parsed);
            continue;
        }
        if day.is_none()
            && let Some(number) = number_prefix(token, 1, 2)
        {
            day = Some(number);
            continue;
        }
        if month.is_none()
            && let Some(index) = [
                "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
            ]
            .iter()
            .position(|m| token.get(..3).is_some_and(|s| s.eq_ignore_ascii_case(m)))
        {
            month = Some(i32::try_from(index).ok()? + 1);
            continue;
        }
        if year.is_none() {
            year = number_prefix(token, 2, 4);
        }
    }
    let CookieTime {
        hour,
        minute,
        second,
        ..
    } = time?;
    let (day, month, year) = (day?, month?, year?);
    // Preserve the original Foundation adapter's explicit 1950 window.
    let year = match year {
        0..=49 => year + 2000,
        50..=99 => year + 1900,
        _ => year,
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
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
    if year < 1601 || !(1..=days).contains(&day) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(
        f64::from(days) * 86_400.0
            + f64::from(hour * 3600 + minute * 60 + second - timezone_offset(timezone)?),
    )
}
fn number_prefix(token: &str, min: usize, max: usize) -> Option<i32> {
    let length = token.bytes().take_while(u8::is_ascii_digit).count();
    (min..=max)
        .contains(&length)
        .then(|| token[..length].parse().ok())
        .flatten()
}
struct CookieTime {
    hour: i32,
    minute: i32,
    second: i32,
    length: usize,
}
fn parse_time(token: &str) -> Option<CookieTime> {
    let mut fields = token.splitn(3, ':');
    let hour = fields.next()?;
    let minute = fields.next()?;
    let second = fields.next()?;
    if !hour.bytes().all(|b| b.is_ascii_digit()) || !minute.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(CookieTime {
        hour: number_prefix(hour, 1, 2)?,
        minute: number_prefix(minute, 1, 2)?,
        second: number_prefix(second, 1, 2)?,
        length: hour.len()
            + minute.len()
            + 2
            + second.bytes().take_while(u8::is_ascii_digit).count(),
    })
}
// Gregorian civil date to days since 1970-01-01; all operations fit for 1601..9999.
fn days_from_civil(mut year: i32, month: i32, day: i32) -> i32 {
    year -= i32::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month + 2) / 5 + day - 1;
    era * 146_097 + year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year - 719_468
}
// Only the timezone immediately following the recognized clock participates.
// The legacy formatter accepts a consumed prefix and ignores trailing comments.
fn timezone_offset(tail: &str) -> Option<i32> {
    let tail = tail.trim_start();
    let uppercase = tail.to_ascii_uppercase();
    for prefix in ["GMT", "UTC", "UT"] {
        if let Some(offset) = uppercase.strip_prefix(prefix) {
            return Some(
                signed_offset(
                    offset,
                    if prefix == "GMT" {
                        OffsetStyle::Gmt
                    } else {
                        OffsetStyle::Utc
                    },
                )
                .unwrap_or(0),
            );
        }
    }
    if tail.starts_with(['+', '-']) {
        return signed_offset(tail, OffsetStyle::Bare);
    }
    for (zone, hours) in [
        ("EST", -5),
        ("CDT", -5),
        ("EDT", -4),
        ("AST", -4),
        ("CST", -6),
        ("MDT", -6),
        ("MST", -7),
        ("PDT", -7),
        ("PST", -8),
        ("AKDT", -8),
        ("AKST", -9),
        ("HST", -10),
        ("CET", 1),
        ("CEST", 2),
        ("EET", 2),
        ("EEST", 3),
        ("JST", 9),
        ("AEST", 10),
        ("AEDT", 11),
    ] {
        if uppercase.starts_with(zone) {
            return Some(hours * 3600);
        }
    }
    // RFC 6265 ignores other date tokens and interprets the clock in UTC.
    Some(0)
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum OffsetStyle {
    Gmt,
    Utc,
    Bare,
}
fn signed_offset(source: &str, style: OffsetStyle) -> Option<i32> {
    let sign = match source.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let source = &source[1..];
    let hour_digits = source.bytes().take_while(u8::is_ascii_digit).count();
    if hour_digits == 0 {
        return None;
    }
    if source.as_bytes().get(hour_digits) == Some(&b':') {
        if style != OffsetStyle::Bare || hour_digits == 2 {
            return colon_offset(source, hour_digits).map(|offset| offset * sign);
        }
        // The old bare-offset fallback reads an hour prefix across punctuation.
        let digits: String = source
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == ':')
            .filter(char::is_ascii_digit)
            .collect();
        return hour_prefix(&digits).map(|offset| offset * 3600 * sign);
    }
    if style == OffsetStyle::Gmt {
        return hour_prefix(&source[..hour_digits]).map(|offset| offset * 3600 * sign);
    }
    // Bare compact offsets accept H, HH, HMM, HHMM, HMMSS, or HHMMSS.
    // Like ICU, use the longest valid consumed prefix rather than reject suffixes.
    for length in (1..=hour_digits.min(6)).rev() {
        let digits = &source[..length];
        let hour_length = if length % 2 == 0 { 2 } else { 1 };
        let hour: i32 = digits[..hour_length].parse().ok()?;
        let minute = digits
            .get(hour_length..hour_length + 2)
            .map_or(Some(0), |s| s.parse().ok())?;
        let second = digits
            .get(hour_length + 2..hour_length + 4)
            .map_or(Some(0), |s| s.parse().ok())?;
        if hour <= 23 && minute <= 59 && second <= 59 {
            return Some((hour * 3600 + minute * 60 + second) * sign);
        }
    }
    None
}
fn hour_prefix(digits: &str) -> Option<i32> {
    for length in (1..=digits.len().min(2)).rev() {
        let hour = digits[..length].parse::<i32>().ok()?;
        if hour <= 23 {
            return Some(hour);
        }
    }
    None
}
fn colon_offset(source: &str, hour_length: usize) -> Option<i32> {
    let hour = hour_prefix(&source[..hour_length])?;
    // Invalid minute/second suffixes leave the already consumed prefix intact.
    let mut offset = hour * 3600;
    if hour_length > 2 {
        return Some(offset);
    }
    let tail = &source[hour_length + 1..];
    let minute = tail.get(..2).and_then(|s| s.parse::<i32>().ok());
    if let Some(minute) = minute.filter(|&m| m <= 59) {
        offset += minute * 60;
        if tail.as_bytes().get(2) == Some(&b':') {
            let second = tail.get(3..5).and_then(|s| s.parse::<i32>().ok());
            if let Some(second) = second.filter(|&s| s <= 59) {
                offset += second;
            }
        }
    }
    Some(offset)
}
