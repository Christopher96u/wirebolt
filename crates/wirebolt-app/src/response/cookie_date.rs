//! The same ICU patterns, locale, timezone and two-digit-year window as the
//! original Foundation cookie adapter. ICU's C API is independent of Apple APIs.
use std::{
    ffi::{c_char, c_void},
    ptr::NonNull,
};

struct CookieDateFormat(NonNull<c_void>);
impl Drop for CookieDateFormat {
    fn drop(&mut self) {
        // SAFETY: This is the sole owner of an opened ICU date formatter.
        unsafe {
            udat_close(self.0.as_ptr());
        }
    }
}
pub(super) fn parse_cookie_date(value: &str) -> Option<f64> {
    let source: Vec<_> = value.encode_utf16().collect();
    let source_length = i32::try_from(source.len()).ok()?;
    for pattern in [
        "EEE, dd MMM yyyy HH:mm:ss zzz",
        "EEEE, dd-MMM-yy HH:mm:ss zzz",
    ] {
        let pattern: Vec<_> = pattern.encode_utf16().collect();
        let timezone = [71_u16, 77, 84];
        let mut status = 0;
        // SAFETY: ICU copies the locale, timezone and pattern during open. The
        // buffers have their declared lengths; -2 selects the explicit pattern.
        let raw = unsafe {
            udat_open(
                -2,
                -2,
                c"en_US_POSIX".as_ptr(),
                timezone.as_ptr(),
                3,
                pattern.as_ptr(),
                i32::try_from(pattern.len()).ok()?,
                &raw mut status,
            )
        };
        let Some(raw) = NonNull::new(raw) else {
            continue;
        };
        let format = CookieDateFormat(raw);
        if status > 0 {
            continue;
        }
        // SAFETY: The owned formatter is live and used synchronously. Foundation
        // uses strict parsing and a 1950-01-01 start for two-digit years.
        unsafe {
            udat_setLenient(format.0.as_ptr(), 0);
            udat_set2DigitYearStart(format.0.as_ptr(), -631_152_000_000.0, &raw mut status);
        }
        let mut position = 0;
        // SAFETY: Source text and both out parameters remain live for this call.
        let milliseconds = unsafe {
            udat_parse(
                format.0.as_ptr(),
                source.as_ptr(),
                source_length,
                &raw mut position,
                &raw mut status,
            )
        };
        if status <= 0 && position > 0 && milliseconds.is_finite() {
            return Some(milliseconds / 1000.0 - 978_307_200.0);
        }
    }
    None
}
#[link(name = "icucore")]
unsafe extern "C" {
    fn udat_open(
        time_style: i32,
        date_style: i32,
        locale: *const c_char,
        timezone: *const u16,
        timezone_length: i32,
        pattern: *const u16,
        pattern_length: i32,
        status: *mut i32,
    ) -> *mut c_void;
    fn udat_close(format: *mut c_void);
    fn udat_setLenient(format: *mut c_void, lenient: i8);
    fn udat_set2DigitYearStart(format: *mut c_void, start: f64, status: *mut i32);
    fn udat_parse(
        format: *const c_void,
        text: *const u16,
        length: i32,
        position: *mut i32,
        status: *mut i32,
    ) -> f64;
}
