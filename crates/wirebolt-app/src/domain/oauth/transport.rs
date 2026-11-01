//! OAuth transport uses the platform's complete, ordered system proxy decision.
use super::{
    DomainNetworkFailure, DomainOAuthError, DomainOAuthPlatform, DomainOAuthProxy, network_error,
};
use reqwest::{Client, Method, Response, StatusCode, cookie::Jar, header};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use url::Url;

pub(super) async fn exchange(
    mut url: Url,
    form: &str,
    platform: &dyn DomainOAuthPlatform,
) -> Result<Response, DomainOAuthError> {
    let cookies = Arc::new(Jar::default());
    let deadline = Instant::now() + Duration::from_hours(168);
    let mut method = Method::POST;
    let mut referer: Option<String> = None;
    for redirects in 0..=20 {
        let response = send(
            &url,
            &method,
            form,
            referer.as_deref(),
            platform,
            &cookies,
            deadline,
        )
        .await?;
        if !matches!(
            response.status(),
            StatusCode::MOVED_PERMANENTLY
                | StatusCode::FOUND
                | StatusCode::SEE_OTHER
                | StatusCode::TEMPORARY_REDIRECT
                | StatusCode::PERMANENT_REDIRECT
        ) {
            return Ok(response);
        }
        let Some(next) = response
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| url.join(value).ok())
        else {
            return Ok(response);
        };
        if redirects == 20 || !matches!(next.scheme(), "http" | "https") {
            return Err(DomainOAuthError::TransportFailure {
                kind: DomainNetworkFailure::ConnectionLost,
            });
        }
        if matches!(
            response.status(),
            StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND | StatusCode::SEE_OTHER
        ) {
            method = Method::GET;
        }
        referer = if url.scheme() == "https" && next.scheme() == "http" {
            None
        } else {
            let mut previous = url;
            let _ = previous.set_username("");
            let _ = previous.set_password(None);
            previous.set_fragment(None);
            Some(previous.into())
        };
        url = next;
    }
    Err(DomainOAuthError::TransportFailure {
        kind: DomainNetworkFailure::ConnectionLost,
    })
}

async fn send(
    url: &Url,
    method: &Method,
    form: &str,
    referer: Option<&str>,
    platform: &dyn DomainOAuthPlatform,
    cookies: &Arc<Jar>,
    deadline: Instant,
) -> Result<Response, DomainOAuthError> {
    let routes = platform.system_proxies(url.as_str());
    for (index, route) in routes.iter().enumerate() {
        let client = client(route, cookies.clone())?;
        let mut request = client
            .request(method.clone(), url.clone())
            .timeout(deadline.saturating_duration_since(Instant::now()));
        if *method == Method::POST {
            request = request
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(form.to_owned());
        }
        if let Some(referer) = referer {
            request = request.header(header::REFERER, referer);
        }
        match request.send().await {
            Ok(response) => return Ok(response),
            // Only failed connection establishment is replayable. A direct fallback
            // must be an explicit candidate from the platform's system settings.
            Err(error)
                if error.is_connect()
                    && index + 1 < routes.len()
                    && !matches!(
                        super::network_failure(&error),
                        DomainNetworkFailure::Certificate | DomainNetworkFailure::Tls
                    ) => {}
            Err(error) => return Err(network_error(&error)),
        }
    }
    Err(DomainOAuthError::InvalidTokenResponse)
}

fn client(route: &DomainOAuthProxy, cookies: Arc<Jar>) -> Result<Client, DomainOAuthError> {
    // Automatic discovery considers environment variables before system settings.
    let mut builder = Client::builder().no_proxy();
    if let DomainOAuthProxy::Proxy { endpoint } = route {
        builder = builder.proxy(
            reqwest::Proxy::all(endpoint).map_err(|_| DomainOAuthError::InvalidTokenResponse)?,
        );
    }
    builder
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .cookie_provider(cookies)
        .build()
        .map_err(|_| DomainOAuthError::InvalidTokenResponse)
}
