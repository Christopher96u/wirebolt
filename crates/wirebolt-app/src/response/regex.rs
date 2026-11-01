//! ICU's portable Unicode regular-expression C API. The macOS build links the
//! system ICU library so patterns retain Foundation's dialect and Unicode data.
//! No Foundation or other Apple API participates in searching.
use super::{
    ResponseCancellation, ResponseError, ResponseResult, ResponseTextRange, SearchQuery, count,
};
use std::{
    ffi::{CStr, c_char, c_void},
    ptr::{self, NonNull},
};

const MULTILINE: u32 = 8;
const CASE_INSENSITIVE: u32 = 2;

// Opaque ICU handle. Pattern/text buffers and callback context remain borrowed
// until the expression is dropped; the handle never leaves this synchronous call.
struct Expression(NonNull<c_void>);
impl Drop for Expression {
    fn drop(&mut self) {
        // SAFETY: This is the sole owner of the successfully opened ICU handle.
        unsafe {
            uregex_close(self.0.as_ptr());
        }
    }
}
pub(super) fn matches(
    query: &SearchQuery,
    source: &str,
    scope: Option<ResponseTextRange>,
    cancellation: &ResponseCancellation,
) -> ResponseResult<Vec<ResponseTextRange>> {
    cancellation.check()?;
    if query.text.is_empty() {
        return Ok(vec![]);
    }
    let pattern = if query.regular_expression {
        query.text.clone()
    } else {
        format!(r"\Q{}\E", query.text.replace(r"\E", r"\E\\E\Q"))
    };
    let pattern = if query.whole_word {
        format!(r"(?<![\p{{L}}\p{{N}}_])(?:{pattern})(?![\p{{L}}\p{{N}}_])")
    } else {
        pattern
    };
    let pattern: Vec<_> = pattern.encode_utf16().collect();
    let units: Vec<_> = source.encode_utf16().collect();
    let pattern_length =
        i32::try_from(pattern.len()).map_err(|_| invalid_pattern("Pattern too long"))?;
    let length = i32::try_from(units.len()).map_err(|_| invalid_pattern("Text too long"))?;
    let mut status = 0;
    // SAFETY: ICU copies the pattern during open. A null parse-error pointer is
    // explicitly supported. Both buffers contain their declared UTF-16 lengths.
    let raw = unsafe {
        uregex_open(
            pattern.as_ptr(),
            pattern_length,
            MULTILINE
                | if query.match_case {
                    0
                } else {
                    CASE_INSENSITIVE
                },
            ptr::null_mut(),
            &raw mut status,
        )
    };
    let expression = NonNull::new(raw)
        .map(Expression)
        .ok_or_else(|| status_error(status))?;
    check_status(status)?;
    let region = scope.unwrap_or(ResponseTextRange {
        location: 0,
        length: count(units.len()),
    });
    let start = i32::try_from(region.location.min(count(units.len()))).unwrap_or(length);
    let end = i32::try_from(region.end().min(count(units.len())))
        .unwrap_or(length)
        .max(start);
    // SAFETY: Text and cancellation context outlive the expression. ICU invokes
    // progress callbacks synchronously; it does not retain them after close.
    unsafe {
        uregex_setText(
            expression.0.as_ptr(),
            units.as_ptr(),
            length,
            &raw mut status,
        );
        uregex_setRegion(expression.0.as_ptr(), start, end, &raw mut status);
        uregex_setMatchCallback(
            expression.0.as_ptr(),
            Some(match_progress),
            ptr::from_ref(cancellation).cast(),
            &raw mut status,
        );
        uregex_setFindProgressCallback(
            expression.0.as_ptr(),
            Some(find_progress),
            ptr::from_ref(cancellation).cast(),
            &raw mut status,
        );
    }
    check_status(status)?;
    let mut result = Vec::new();
    loop {
        cancellation.check()?;
        // SAFETY: The handle, text and callback context are live for the call.
        let found = unsafe { uregex_findNext(expression.0.as_ptr(), &raw mut status) };
        cancellation.check()?;
        check_status(status)?;
        if found == 0 {
            break;
        }
        // SAFETY: A successful match has capture group zero and live bounds.
        let (start, end) = unsafe {
            (
                uregex_start64(expression.0.as_ptr(), 0, &raw mut status),
                uregex_end64(expression.0.as_ptr(), 0, &raw mut status),
            )
        };
        check_status(status)?;
        let start = u64::try_from(start).map_err(|_| invalid_pattern("Invalid match start"))?;
        let end = u64::try_from(end).map_err(|_| invalid_pattern("Invalid match end"))?;
        result.push(ResponseTextRange {
            location: start,
            length: end.saturating_sub(start),
        });
        if result.len() == SearchQuery::MAXIMUM_MATCHES {
            break;
        }
    }
    Ok(result)
}
fn invalid_pattern(reason: &str) -> ResponseError {
    ResponseError::InvalidPattern {
        reason: reason.into(),
    }
}
fn status_error(status: i32) -> ResponseError {
    // SAFETY: ICU returns a static, NUL-terminated name for every status code.
    let reason = unsafe { CStr::from_ptr(u_errorName(status)) }.to_string_lossy();
    invalid_pattern(&reason)
}
fn check_status(status: i32) -> ResponseResult<()> {
    if status > 0 {
        Err(status_error(status))
    } else {
        Ok(())
    }
}
unsafe extern "C" fn match_progress(context: *const c_void, _steps: i32) -> i8 {
    // SAFETY: `matches` installs a live borrowed cancellation token as context.
    i8::from(
        unsafe { &*context.cast::<ResponseCancellation>() }
            .check()
            .is_ok(),
    )
}
unsafe extern "C" fn find_progress(context: *const c_void, _offset: i64) -> i8 {
    // SAFETY: The callback has the same live context as `match_progress`.
    unsafe { match_progress(context, 0) }
}
#[link(name = "icucore")]
unsafe extern "C" {
    fn uregex_open(
        pattern: *const u16,
        length: i32,
        flags: u32,
        parse_error: *mut c_void,
        status: *mut i32,
    ) -> *mut c_void;
    fn uregex_close(expression: *mut c_void);
    fn uregex_setText(expression: *mut c_void, text: *const u16, length: i32, status: *mut i32);
    fn uregex_setRegion(expression: *mut c_void, start: i32, end: i32, status: *mut i32);
    fn uregex_findNext(expression: *mut c_void, status: *mut i32) -> i8;
    fn uregex_start64(expression: *mut c_void, group: i32, status: *mut i32) -> i64;
    fn uregex_end64(expression: *mut c_void, group: i32, status: *mut i32) -> i64;
    fn uregex_setMatchCallback(
        expression: *mut c_void,
        callback: Option<unsafe extern "C" fn(*const c_void, i32) -> i8>,
        context: *const c_void,
        status: *mut i32,
    );
    fn uregex_setFindProgressCallback(
        expression: *mut c_void,
        callback: Option<unsafe extern "C" fn(*const c_void, i64) -> i8>,
        context: *const c_void,
        status: *mut i32,
    );
    fn u_errorName(status: i32) -> *const c_char;
}
