use std::fmt::Write;

pub(super) fn format(source: &str) -> Option<String> {
    if !source.contains(['.', 'e', 'E']) {
        if let Ok(value) = source.parse::<i64>() {
            return Some(value.to_string());
        }
        if let Ok(value) = source.parse::<u64>() {
            return Some(value.to_string());
        }
    }
    let unsigned = source.trim_start_matches('-');
    let (mantissa, exponent) = unsigned.split_once(['e', 'E']).unwrap_or((unsigned, "0"));
    let digit_count = mantissa
        .trim_start_matches('0')
        .bytes()
        .filter(u8::is_ascii_digit)
        .count();
    if digit_count > 17 {
        return decimal(mantissa, exponent, source.starts_with('-'));
    }
    let value: f64 = source.parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    if value == 0.0 {
        return Some(if value.is_sign_negative() { "-0" } else { "0" }.into());
    }
    // Foundation writes NSNumber doubles with %.17g. Rust's decimal formatter
    // uses the same correctly rounded 17 significant digits, with C's thresholds.
    let scientific = format!("{value:.16e}");
    let (mantissa, exponent) = scientific.split_once('e')?;
    let exponent: i32 = exponent.parse().ok()?;
    let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
    if !(-4..17).contains(&exponent) {
        return Some(format!("{mantissa}e{exponent:+03}"));
    }
    let digits = mantissa.trim_start_matches('-').replace('.', "");
    decimal_digits(&digits, exponent + 1, value.is_sign_negative())
}

// NSDecimalNumber retains a 128-bit unsigned mantissa and a signed 8-bit
// decimal exponent. Overflow discards low digits, matching the old formatter.
fn decimal(mantissa: &str, exponent: &str, negative: bool) -> Option<String> {
    let fraction = mantissa
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    let mut exponent = exponent
        .parse::<i32>()
        .ok()?
        .checked_sub(i32::try_from(fraction).ok()?)?;
    let mut value = 0_u128;
    let mut overflowing = false;
    for digit in mantissa.bytes().filter(u8::is_ascii_digit) {
        if !overflowing
            && let Some(next) = value
                .checked_mul(10)
                .and_then(|v| v.checked_add(u128::from(digit - b'0')))
        {
            value = next;
        } else {
            overflowing = true;
            exponent = exponent.checked_add(1)?;
        }
    }
    if value == 0 {
        return Some("0".into());
    }
    while value.is_multiple_of(10) && exponent < 127 {
        value /= 10;
        exponent = exponent.checked_add(1)?;
    }
    if !(-128..=127).contains(&exponent) {
        return None;
    }
    let digits = value.to_string();
    let position = i32::try_from(digits.len()).ok()?.checked_add(exponent)?;
    decimal_digits(&digits, position, negative)
}
fn decimal_digits(digits: &str, position: i32, negative: bool) -> Option<String> {
    let mut output = if negative {
        "-".to_owned()
    } else {
        String::new()
    };
    if position <= 0 {
        output.push_str("0.");
        for _ in position..0 {
            output.push('0');
        }
        output.push_str(digits);
    } else {
        let position = usize::try_from(position).ok()?;
        if position >= digits.len() {
            output.push_str(digits);
            for _ in digits.len()..position {
                output.push('0');
            }
        } else {
            let _ = write!(output, "{}.{}", &digits[..position], &digits[position..]);
        }
    }
    Some(output)
}
