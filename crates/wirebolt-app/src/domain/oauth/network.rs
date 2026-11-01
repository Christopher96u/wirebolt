//! Typed transport causes retain the native OAuth diagnostic categories.
use super::{DomainNetworkFailure, DomainOAuthError};
use std::{error::Error, fmt, io};

#[derive(Debug)]
pub(super) struct DnsFailure(io::Error);
impl fmt::Display for DnsFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}
impl Error for DnsFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.0)
    }
}
#[derive(Debug)]
pub(super) struct SystemResolver;
impl reqwest::dns::Resolve for SystemResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let addresses = tokio::net::lookup_host((host, 0))
                .await
                .map_err(|error| Box::new(DnsFailure(error)) as Box<dyn Error + Send + Sync>)?;
            Ok(Box::new(addresses) as reqwest::dns::Addrs)
        })
    }
}
pub(super) fn network_error(error: &reqwest::Error) -> DomainOAuthError {
    DomainOAuthError::TransportFailure {
        kind: network_failure(error),
        host: error.url().and_then(url::Url::host_str).map(str::to_owned),
    }
}
#[must_use]
pub fn network_failure(error: &reqwest::Error) -> DomainNetworkFailure {
    if error.is_timeout() {
        return DomainNetworkFailure::Timeout;
    }
    if error.is_decode() {
        return DomainNetworkFailure::Decode;
    }
    if error.is_redirect() {
        return DomainNetworkFailure::TooManyRedirects;
    }
    let mut cause = error.source();
    while let Some(source) = cause {
        if let Some(kind) = classify_cause(source) {
            return kind;
        }
        cause = source.source();
    }
    if error.is_connect() {
        DomainNetworkFailure::CannotConnect
    } else {
        DomainNetworkFailure::Unknown
    }
}
fn classify_cause(error: &(dyn Error + 'static)) -> Option<DomainNetworkFailure> {
    use DomainNetworkFailure as F;
    if let Some(dns) = error.downcast_ref::<DnsFailure>() {
        return Some(classify_cause(&dns.0).unwrap_or(F::HostNotFound));
    }
    if let Some(tls) = error.downcast_ref::<rustls::Error>() {
        return Some(match tls {
            rustls::Error::InvalidCertificate(
                rustls::CertificateError::Expired | rustls::CertificateError::ExpiredContext { .. },
            ) => F::CertificateExpired,
            rustls::Error::InvalidCertificate(
                rustls::CertificateError::NotValidYet
                | rustls::CertificateError::NotValidYetContext { .. },
            ) => F::CertificateNotYetValid,
            rustls::Error::InvalidCertificate(_) | rustls::Error::NoCertificatesPresented => {
                F::Certificate
            }
            _ => F::Tls,
        });
    }
    if let Some(http) = error.downcast_ref::<hyper::Error>() {
        if http.is_parse() {
            return Some(F::CannotParseResponse);
        }
        if http.is_incomplete_message() || http.is_closed() {
            return Some(F::ConnectionLost);
        }
        if http.is_canceled() {
            return Some(F::Cancelled);
        }
    }
    if let Some(io) = error.downcast_ref::<io::Error>() {
        if let Some(inner) = io.get_ref()
            && let Some(kind) = classify_cause(inner)
        {
            return Some(kind);
        }
        return match io.kind() {
            io::ErrorKind::NetworkUnreachable
            | io::ErrorKind::HostUnreachable
            | io::ErrorKind::NotConnected => Some(F::Offline),
            io::ErrorKind::ConnectionRefused | io::ErrorKind::AddrNotAvailable => {
                Some(F::CannotConnect)
            }
            io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::UnexpectedEof => Some(F::ConnectionLost),
            io::ErrorKind::TimedOut => Some(F::Timeout),
            _ => None,
        };
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn certificate_failures_never_permit_proxy_fallback() {
        for cause in [
            rustls::CertificateError::Expired,
            rustls::CertificateError::NotValidYet,
            rustls::CertificateError::UnknownIssuer,
        ] {
            let kind = classify_cause(&io::Error::new(
                io::ErrorKind::InvalidData,
                rustls::Error::InvalidCertificate(cause),
            ))
            .unwrap();
            assert!(
                kind.is_tls(),
                "certificate failure must stop the ordered proxy attempt"
            );
        }
        assert!(!DomainNetworkFailure::CannotConnect.is_tls());
    }
    #[test]
    fn typed_causes_distinguish_network_tls_and_unknown_failures() {
        use DomainNetworkFailure as F;
        for (kind, expected) in [
            (io::ErrorKind::NetworkUnreachable, F::Offline),
            (io::ErrorKind::ConnectionRefused, F::CannotConnect),
            (io::ErrorKind::ConnectionReset, F::ConnectionLost),
            (io::ErrorKind::TimedOut, F::Timeout),
        ] {
            assert_eq!(classify_cause(&io::Error::from(kind)), Some(expected));
        }
        assert_eq!(
            classify_cause(&DnsFailure(io::Error::other("opaque"))),
            Some(F::HostNotFound)
        );
        assert_eq!(
            classify_cause(&rustls::Error::InvalidCertificate(
                rustls::CertificateError::Expired
            )),
            Some(F::CertificateExpired)
        );
        assert_eq!(
            classify_cause(&rustls::Error::InvalidCertificate(
                rustls::CertificateError::NotValidYet
            )),
            Some(F::CertificateNotYetValid)
        );
        assert_eq!(
            classify_cause(&rustls::Error::InvalidCertificate(
                rustls::CertificateError::UnknownIssuer
            )),
            Some(F::Certificate)
        );
        assert_eq!(
            classify_cause(&rustls::Error::General("opaque".into())),
            Some(F::Tls)
        );
        assert_eq!(
            classify_cause(&io::Error::new(
                io::ErrorKind::InvalidData,
                rustls::Error::InvalidCertificate(rustls::CertificateError::Expired)
            )),
            Some(F::CertificateExpired)
        );
        assert_eq!(
            classify_cause(&DnsFailure(io::Error::from(
                io::ErrorKind::NetworkUnreachable
            ))),
            Some(F::Offline)
        );
        assert_eq!(
            classify_cause(&io::Error::other("certificate TLS DNS lookup")),
            None
        );
    }
}
