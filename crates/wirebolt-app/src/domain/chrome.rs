//! Workspace/window/tab labels and timeout presentation shared by native interfaces.
#[must_use]
pub fn workspace_name(name: &str) -> String {
    if name.trim().is_empty() {
        "Wirebolt".into()
    } else {
        name.into()
    }
}
#[must_use]
pub fn window_title(workspace: &str, request: Option<&str>) -> String {
    let w = workspace_name(workspace);
    request
        .filter(|r| !r.is_empty())
        .map_or_else(|| w.clone(), |r| format!("{w} — {r}"))
}
#[must_use]
pub fn tab_description(method: &str, ws: bool, dirty: bool, preview: bool) -> String {
    let mut p = vec![if ws { "WebSocket" } else { method }];
    if dirty {
        p.push("Edited");
    }
    if preview {
        p.push("Preview");
    }
    p.join(", ")
}
#[derive(Clone, Debug, PartialEq)]
pub struct TimeoutPresentation {
    pub text: Option<String>,
    pub seconds: Option<f64>,
    pub suffix: String,
}
#[must_use]
pub fn timeout_presentation(ms: u64) -> TimeoutPresentation {
    let (text, seconds, suffix) = if ms == 0 {
        (Some("No limit".into()), None, "")
    } else if ms < 1000 {
        (Some(format!("{ms} ms")), None, "")
    } else if ms.is_multiple_of(60_000) {
        (Some(format!("{} min", ms / 60_000)), None, "")
    } else {
        (None, Some(milliseconds_as_seconds(ms)), " s")
    };
    TimeoutPresentation {
        text,
        seconds,
        suffix: suffix.into(),
    }
}
/// Uses the same floating-point representation as native timeout formatting.
#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "Timeouts are rendered as floating-point seconds, matching the existing native formatter."
)]
pub fn milliseconds_as_seconds(ms: u64) -> f64 {
    ms as f64 / 1000.0
}
