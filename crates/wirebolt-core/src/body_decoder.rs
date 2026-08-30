use std::{
    fmt,
    io::{self, Write},
};

/// Content encodings the engine can decode while streaming.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentEncoding {
    Gzip,
    Deflate,
    Brotli,
    Zstd,
}

impl ContentEncoding {
    /// The `Accept-Encoding` value advertised when the caller sets none.
    pub const ACCEPT: &'static str = "gzip, deflate, br, zstd";

    /// Recognizes a single-token `Content-Encoding` value. Stacked encodings
    /// and unknown tokens return `None`, and the body is delivered verbatim.
    #[must_use]
    pub fn parse(header: &str) -> Option<Self> {
        let token = header.trim();
        if token.eq_ignore_ascii_case("gzip") || token.eq_ignore_ascii_case("x-gzip") {
            Some(Self::Gzip)
        } else if token.eq_ignore_ascii_case("deflate") {
            Some(Self::Deflate)
        } else if token.eq_ignore_ascii_case("br") {
            Some(Self::Brotli)
        } else if token.eq_ignore_ascii_case("zstd") {
            Some(Self::Zstd)
        } else {
            None
        }
    }

    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Gzip => "gzip",
            Self::Deflate => "deflate",
            Self::Brotli => "br",
            Self::Zstd => "zstd",
        }
    }
}

impl fmt::Display for ContentEncoding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.token())
    }
}

/// Decodes wire chunks incrementally. Each decoder writes into an owned
/// buffer that is handed back borrowed, so decoded bytes are never copied a
/// second time before reaching the caller.
pub(crate) enum BodyDecoder {
    Gzip(flate2::write::MultiGzDecoder<Vec<u8>>),
    Deflate(flate2::write::ZlibDecoder<Vec<u8>>),
    Brotli(Box<brotli_decompressor::DecompressorWriter<Vec<u8>>>),
    Zstd(zstd::stream::write::Decoder<'static, Vec<u8>>),
}

const BROTLI_BUFFER_BYTES: usize = 64 * 1024;

impl BodyDecoder {
    pub(crate) fn new(encoding: ContentEncoding) -> io::Result<Self> {
        Ok(match encoding {
            ContentEncoding::Gzip => Self::Gzip(flate2::write::MultiGzDecoder::new(Vec::new())),
            ContentEncoding::Deflate => Self::Deflate(flate2::write::ZlibDecoder::new(Vec::new())),
            ContentEncoding::Brotli => Self::Brotli(Box::new(
                brotli_decompressor::DecompressorWriter::new(Vec::new(), BROTLI_BUFFER_BYTES),
            )),
            ContentEncoding::Zstd => Self::Zstd(zstd::stream::write::Decoder::new(Vec::new())?),
        })
    }

    /// Feeds one wire chunk and returns whatever it decoded.
    pub(crate) fn decode(&mut self, chunk: &[u8]) -> io::Result<&[u8]> {
        self.sink().clear();
        self.write_all(chunk)?;
        Ok(self.sink())
    }

    /// Flushes trailing output once the wire body ends.
    pub(crate) fn finish(&mut self) -> io::Result<&[u8]> {
        self.sink().clear();
        match self {
            Self::Gzip(decoder) => decoder.try_finish()?,
            Self::Deflate(decoder) => decoder.try_finish()?,
            Self::Brotli(decoder) => decoder.flush()?,
            Self::Zstd(decoder) => decoder.flush()?,
        }
        Ok(self.sink())
    }

    fn write_all(&mut self, chunk: &[u8]) -> io::Result<()> {
        match self {
            Self::Gzip(decoder) => decoder.write_all(chunk),
            Self::Deflate(decoder) => decoder.write_all(chunk),
            Self::Brotli(decoder) => decoder.write_all(chunk),
            Self::Zstd(decoder) => decoder.write_all(chunk),
        }
    }

    fn sink(&mut self) -> &mut Vec<u8> {
        match self {
            Self::Gzip(decoder) => decoder.get_mut(),
            Self::Deflate(decoder) => decoder.get_mut(),
            Self::Brotli(decoder) => decoder.get_mut(),
            Self::Zstd(decoder) => decoder.get_mut(),
        }
    }
}

impl fmt::Debug for BodyDecoder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let encoding = match self {
            Self::Gzip(_) => ContentEncoding::Gzip,
            Self::Deflate(_) => ContentEncoding::Deflate,
            Self::Brotli(_) => ContentEncoding::Brotli,
            Self::Zstd(_) => ContentEncoding::Zstd,
        };
        formatter
            .debug_struct("BodyDecoder")
            .field("encoding", &encoding)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_in_pieces(encoding: ContentEncoding, wire: &[u8]) -> Vec<u8> {
        let mut decoder = BodyDecoder::new(encoding).expect("decoder");
        let mut output = Vec::new();
        for piece in wire.chunks(7) {
            output.extend_from_slice(decoder.decode(piece).expect("decode piece"));
        }
        output.extend_from_slice(decoder.finish().expect("finish"));
        output
    }

    #[test]
    fn decodes_gzip_across_arbitrary_chunk_boundaries() {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"wirebolt gzip body").expect("encode");
        let wire = encoder.finish().expect("finish encoding");

        assert_eq!(
            decode_in_pieces(ContentEncoding::Gzip, &wire),
            b"wirebolt gzip body"
        );
    }

    #[test]
    fn decodes_deflate_and_zstd() {
        let mut zlib = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        zlib.write_all(b"deflate body").expect("encode");
        let zlib = zlib.finish().expect("finish encoding");
        assert_eq!(
            decode_in_pieces(ContentEncoding::Deflate, &zlib),
            b"deflate body"
        );

        let zstd = zstd::encode_all(&b"zstd body"[..], 3).expect("zstd encode");
        assert_eq!(decode_in_pieces(ContentEncoding::Zstd, &zstd), b"zstd body");
    }

    #[test]
    fn parses_encoding_tokens_case_insensitively() {
        assert_eq!(
            ContentEncoding::parse(" GZip "),
            Some(ContentEncoding::Gzip)
        );
        assert_eq!(
            ContentEncoding::parse("x-gzip"),
            Some(ContentEncoding::Gzip)
        );
        assert_eq!(ContentEncoding::parse("br"), Some(ContentEncoding::Brotli));
        assert_eq!(ContentEncoding::parse("zstd"), Some(ContentEncoding::Zstd));
        assert_eq!(ContentEncoding::parse("identity"), None);
        assert_eq!(ContentEncoding::parse("gzip, br"), None);
    }

    #[test]
    fn corrupt_input_is_reported_not_swallowed() {
        let mut decoder = BodyDecoder::new(ContentEncoding::Gzip).expect("decoder");
        assert!(decoder.decode(b"definitely not gzip data").is_err());
    }
}
