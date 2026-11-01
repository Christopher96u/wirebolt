//! Retains typed native certificate dates when the system verifier erases its error code.
use rustls::{
    CertificateError, DigitallySignedStruct, Error, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use security_framework::{
    certificate::SecCertificate, policy::SecPolicy, secure_transport::SslProtocolSide,
    trust::SecTrust,
};
use std::sync::Arc;

pub(super) fn verifier() -> Option<Arc<dyn ServerCertVerifier>> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let inner = rustls_platform_verifier::Verifier::new(provider).ok()?;
    Some(Arc::new(NativeCertificateVerifier(inner)))
}
#[derive(Debug)]
struct NativeCertificateVerifier(rustls_platform_verifier::Verifier);
impl ServerCertVerifier for NativeCertificateVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        self.0
            .verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
            .map_err(|error| {
                if matches!(error, Error::InvalidCertificate(_)) {
                    native_date_error(end_entity, intermediates, server_name, ocsp_response)
                        .map_or(error, Error::InvalidCertificate)
                } else {
                    error
                }
            })
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.0.verify_tls12_signature(message, cert, signature)
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.0.verify_tls13_signature(message, cert, signature)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_verify_schemes()
    }
}
fn native_date_error(
    end_entity: &CertificateDer<'_>,
    intermediates: &[CertificateDer<'_>],
    server_name: &ServerName<'_>,
    ocsp: &[u8],
) -> Option<CertificateError> {
    let certificates = std::iter::once(end_entity)
        .chain(intermediates)
        .map(|certificate| SecCertificate::from_der(certificate.as_ref()))
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let policy = SecPolicy::create_ssl(SslProtocolSide::SERVER, Some(&server_name.to_str()));
    let mut trust = SecTrust::create_with_certificates(&certificates, &[policy]).ok()?;
    if !ocsp.is_empty() {
        trust.set_trust_ocsp_response(std::iter::once(ocsp)).ok()?;
    }
    let error = trust.evaluate_with_error().err()?;
    certificate_date_error(error.code())
}
fn certificate_date_error(status: isize) -> Option<CertificateError> {
    // SecBase.h's typed SecTrust evaluation statuses.
    match status {
        -67818 => Some(CertificateError::Expired),
        -67819 => Some(CertificateError::NotValidYet),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_certificate_date_statuses_remain_typed() {
        assert_eq!(
            certificate_date_error(-67818),
            Some(CertificateError::Expired)
        );
        assert_eq!(
            certificate_date_error(-67819),
            Some(CertificateError::NotValidYet)
        );
        assert_eq!(certificate_date_error(-67843), None);
    }
}
