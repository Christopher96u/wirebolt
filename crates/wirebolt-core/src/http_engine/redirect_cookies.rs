//! Cookies belong to one run, never to a pooled HTTP client or another workspace.
use http::{HeaderMap, HeaderValue, header::SET_COOKIE};
use reqwest::cookie::{CookieStore, Jar};
use std::{
    future::Future,
    sync::{Arc, Mutex},
};
use url::Url;

type Update = (Url, HeaderMap);
#[derive(Default)]
struct RunCookies {
    jar: Jar,
    updates: Mutex<Vec<Update>>,
}
tokio::task_local! { static ACTIVE: Arc<RunCookies>; }

pub(super) struct ScopedCookies;
impl CookieStore for ScopedCookies {
    fn set_cookies(&self, cookies: &mut dyn Iterator<Item = &HeaderValue>, url: &Url) {
        let _ = ACTIVE.try_with(|state| {
            let values: Vec<_> = cookies.cloned().collect();
            state.jar.set_cookies(&mut values.iter(), url);
            if !values.is_empty() {
                let mut headers = HeaderMap::new();
                for value in values {
                    headers.append(SET_COOKIE, value);
                }
                state
                    .updates
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push((url.clone(), headers));
            }
        });
    }
    fn cookies(&self, url: &Url) -> Option<HeaderValue> {
        ACTIVE
            .try_with(|state| state.jar.cookies(url))
            .ok()
            .flatten()
    }
}

pub(super) async fn capture<T>(future: impl Future<Output = T>) -> (T, Vec<Update>) {
    let state = Arc::new(RunCookies::default());
    let result = ACTIVE.scope(Arc::clone(&state), future).await;
    let updates = std::mem::take(
        &mut *state
            .updates
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    (result, updates)
}
