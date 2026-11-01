//! Bounded PEM file input shared by identity and CA selection.
use std::{fs::File, io::Read, path::Path};

pub const MAXIMUM_PEM_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PemFileError {
    pub name: String,
}
impl std::fmt::Display for PemFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "“{}” isn’t a PEM certificate file. Convert DER or PKCS #12 (.p12) files to PEM first.",
            self.name
        )
    }
}
impl std::error::Error for PemFileError {}
/// # Errors
/// Returns the PEM diagnostic for oversized, unreadable or non-UTF-8 input.
pub fn read(path: &Path) -> Result<String, PemFileError> {
    let error = || PemFileError {
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
    };
    let file = File::open(path).map_err(|_| error())?;
    if file.metadata().map_err(|_| error())?.len() > MAXIMUM_PEM_BYTES {
        return Err(error());
    }
    let mut bytes = Vec::new();
    file.take(MAXIMUM_PEM_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| error())?;
    if bytes.len() as u64 > MAXIMUM_PEM_BYTES {
        return Err(error());
    }
    let text = String::from_utf8(bytes).map_err(|_| error())?;
    // String(contentsOf:encoding:.utf8) consumes a UTF-8 BOM.
    Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned())
}
/// # Errors
/// Returns the same file diagnostic when a CA source lacks the certificate marker.
pub fn validate_ca(path: &Path) -> Result<(), PemFileError> {
    if read(path)?.contains("-----BEGIN CERTIFICATE-----") {
        Ok(())
    } else {
        Err(PemFileError {
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn limits_utf8_bom_and_existing_ca_marker_policy() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ca.pem");
        std::fs::write(&path, "\u{feff}-----BEGIN CERTIFICATE-----\nfixture").unwrap();
        assert!(read(&path).unwrap().starts_with("-----BEGIN"));
        assert!(validate_ca(&path).is_ok());
        std::fs::write(
            &path,
            vec![b' '; usize::try_from(MAXIMUM_PEM_BYTES).unwrap()],
        )
        .unwrap();
        assert!(read(&path).is_ok());
        assert!(validate_ca(&path).is_err());
        std::fs::write(
            &path,
            vec![b' '; usize::try_from(MAXIMUM_PEM_BYTES).unwrap() + 1],
        )
        .unwrap();
        assert!(read(&path).is_err());
        std::fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(read(&path).is_err());
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            read(&path).unwrap_err().to_string(),
            "“ca.pem” isn’t a PEM certificate file. Convert DER or PKCS #12 (.p12) files to PEM first."
        );
    }
}
