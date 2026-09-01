use std::{
    fmt,
    io::{self, Write},
};

use crate::StreamControl;

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

/// Why the sink stopped accepting bytes. Read back after a decoder reports
/// an error to tell a caller stop or a hit limit apart from corrupt input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SinkRefusal {
    Stopped,
    TooLarge,
}

/// Hands every decoded piece to the caller's chunk callback as soon as a
/// decoder produces it and checks the response cap per piece, so decoded
/// bytes are never accumulated and a compression bomb fails at the first
/// piece past the cap instead of after it was allocated.
pub(crate) struct ChunkSink<F> {
    on_chunk: F,
    limit: u64,
    delivered: u64,
    refusal: Option<SinkRefusal>,
}

impl<F: FnMut(&[u8]) -> StreamControl> ChunkSink<F> {
    pub(crate) fn new(on_chunk: F, limit: Option<u64>) -> Self {
        Self {
            on_chunk,
            limit: limit.unwrap_or(u64::MAX),
            delivered: 0,
            refusal: None,
        }
    }

    /// Bytes handed to the callback so far.
    pub(crate) const fn delivered(&self) -> u64 {
        self.delivered
    }

    pub(crate) const fn refusal(&self) -> Option<SinkRefusal> {
        self.refusal
    }

    fn deliver(&mut self, bytes: &[u8]) -> io::Result<()> {
        // Decoders may retry or flush during drop; once refused, stay refused
        // so the callback never runs again after it asked to stop.
        if self.refusal.is_some() {
            return Err(refused());
        }
        if bytes.is_empty() {
            return Ok(());
        }
        let next = u64::try_from(bytes.len())
            .ok()
            .and_then(|added| self.delivered.checked_add(added))
            .filter(|next| *next <= self.limit);
        let Some(next) = next else {
            self.refusal = Some(SinkRefusal::TooLarge);
            return Err(refused());
        };
        if (self.on_chunk)(bytes) == StreamControl::Stop {
            self.refusal = Some(SinkRefusal::Stopped);
            return Err(refused());
        }
        self.delivered = next;
        Ok(())
    }
}

impl<F: FnMut(&[u8]) -> StreamControl> Write for ChunkSink<F> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.deliver(bytes)?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<F> fmt::Debug for ChunkSink<F> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChunkSink")
            .field("limit", &self.limit)
            .field("delivered", &self.delivered)
            .field("refusal", &self.refusal)
            .finish_non_exhaustive()
    }
}

fn refused() -> io::Error {
    io::Error::other("response sink refused further bytes")
}

/// Turns wire chunks into decoded pieces written to `W`. Each decoder keeps
/// only its own fixed working buffer, so memory stays bounded no matter how
/// far one wire chunk expands.
pub(crate) enum BodyStream<W: Write> {
    Identity(W),
    Gzip(flate2::write::MultiGzDecoder<W>),
    Deflate(Inflater<W>),
    Brotli(Box<brotli_decompressor::DecompressorWriter<W>>),
    Zstd(zstd::stream::zio::Writer<W, zstd::stream::raw::Decoder<'static>>),
}

const BROTLI_BUFFER_BYTES: usize = 64 * 1024;
const INFLATE_BUFFER_BYTES: usize = 32 * 1024;

impl<W: Write> BodyStream<W> {
    pub(crate) fn new(encoding: Option<ContentEncoding>, sink: W) -> io::Result<Self> {
        Ok(match encoding {
            None => Self::Identity(sink),
            Some(ContentEncoding::Gzip) => Self::Gzip(flate2::write::MultiGzDecoder::new(sink)),
            Some(ContentEncoding::Deflate) => Self::Deflate(Inflater::new(sink)),
            Some(ContentEncoding::Brotli) => Self::Brotli(Box::new(
                brotli_decompressor::DecompressorWriter::new(sink, BROTLI_BUFFER_BYTES),
            )),
            Some(ContentEncoding::Zstd) => Self::Zstd(zstd::stream::zio::Writer::new(
                sink,
                zstd::stream::raw::Decoder::new()?,
            )),
        })
    }

    /// Feeds one wire chunk; decoded pieces reach the sink before this returns.
    /// flate2 and zstd park output in their working buffer until the next
    /// write, so they are flushed to keep delivery (and the cap) current.
    pub(crate) fn write_chunk(&mut self, chunk: &[u8]) -> io::Result<()> {
        match self {
            Self::Identity(sink) => sink.write_all(chunk),
            Self::Gzip(decoder) => decoder.write_all(chunk).and_then(|()| decoder.flush()),
            Self::Deflate(inflater) => inflater.write_chunk(chunk),
            Self::Brotli(decoder) => decoder.write_all(chunk),
            Self::Zstd(decoder) => decoder.write_all(chunk).and_then(|()| decoder.flush()),
        }
    }

    /// Flushes trailing output once the wire body ends and fails when the
    /// compressed stream stopped short of its end marker.
    pub(crate) fn finish(&mut self) -> io::Result<()> {
        match self {
            Self::Identity(_) => Ok(()),
            Self::Gzip(decoder) => decoder.try_finish(),
            Self::Deflate(inflater) => inflater.finish(),
            Self::Brotli(decoder) => decoder.close(),
            Self::Zstd(decoder) => decoder.finish(),
        }
    }

    pub(crate) fn sink(&mut self) -> &mut W {
        match self {
            Self::Identity(sink) => sink,
            Self::Gzip(decoder) => decoder.get_mut(),
            Self::Deflate(inflater) => &mut inflater.sink,
            Self::Brotli(decoder) => decoder.get_mut(),
            Self::Zstd(decoder) => decoder.writer_mut(),
        }
    }

    const fn encoding(&self) -> Option<ContentEncoding> {
        match self {
            Self::Identity(_) => None,
            Self::Gzip(_) => Some(ContentEncoding::Gzip),
            Self::Deflate(_) => Some(ContentEncoding::Deflate),
            Self::Brotli(_) => Some(ContentEncoding::Brotli),
            Self::Zstd(_) => Some(ContentEncoding::Zstd),
        }
    }
}

impl<W: Write> fmt::Debug for BodyStream<W> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BodyStream")
            .field("encoding", &self.encoding())
            .finish()
    }
}

/// Streaming zlib inflater driven through fixed buffers. flate2's writer
/// adapter cannot report whether the stream reached its end marker, so this
/// drives the raw state machine to reject truncated bodies.
pub(crate) struct Inflater<W: Write> {
    state: Box<flate2::Decompress>,
    output: Box<[u8]>,
    sink: W,
    ended: bool,
}

impl<W: Write> Inflater<W> {
    fn new(sink: W) -> Self {
        Self {
            state: Box::new(flate2::Decompress::new(true)),
            output: vec![0; INFLATE_BUFFER_BYTES].into_boxed_slice(),
            sink,
            ended: false,
        }
    }

    fn write_chunk(&mut self, mut input: &[u8]) -> io::Result<()> {
        while !input.is_empty() {
            if self.ended {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "data after the end of the deflate stream",
                ));
            }
            let (consumed, status) = self.step(input, flate2::FlushDecompress::None)?;
            input = &input[consumed..];
            if status == flate2::Status::StreamEnd {
                self.ended = true;
            } else if consumed == 0 {
                // A fresh output buffer is offered every step, so no progress
                // on pending input means the stream itself is stuck.
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "deflate stream made no progress",
                ));
            }
        }
        Ok(())
    }

    fn finish(&mut self) -> io::Result<()> {
        loop {
            if self.ended {
                return Ok(());
            }
            let (_, status) = self.step(&[], flate2::FlushDecompress::Finish)?;
            match status {
                flate2::Status::StreamEnd => self.ended = true,
                flate2::Status::Ok => {}
                flate2::Status::BufError => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "deflate stream ended before its end marker",
                    ));
                }
            }
        }
    }

    /// Runs one inflate step into the working buffer and forwards whatever it
    /// produced. Returns the input bytes consumed and the stream status.
    fn step(
        &mut self,
        input: &[u8],
        flush: flate2::FlushDecompress,
    ) -> io::Result<(usize, flate2::Status)> {
        let before_in = self.state.total_in();
        let before_out = self.state.total_out();
        let status = self
            .state
            .decompress(input, &mut self.output, flush)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let consumed = usize::try_from(self.state.total_in() - before_in).unwrap_or(usize::MAX);
        let produced =
            usize::try_from(self.state.total_out() - before_out).unwrap_or(self.output.len());
        self.sink.write_all(&self.output[..produced])?;
        Ok((consumed, status))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// Feeds `wire` in tiny pieces and collects what the sink receives.
    fn decode_in_pieces(
        encoding: ContentEncoding,
        wire: &[u8],
        limit: Option<u64>,
    ) -> Result<Vec<u8>, (io::Error, Option<SinkRefusal>)> {
        let output = RefCell::new(Vec::new());
        let sink = ChunkSink::new(
            |piece: &[u8]| {
                output.borrow_mut().extend_from_slice(piece);
                StreamControl::Continue
            },
            limit,
        );
        let mut stream = BodyStream::new(Some(encoding), sink).expect("decoder");
        let result = wire
            .chunks(7)
            .try_for_each(|piece| stream.write_chunk(piece))
            .and_then(|()| stream.finish());
        let refusal = stream.sink().refusal();
        drop(stream);
        match result {
            Ok(()) => Ok(output.into_inner()),
            Err(error) => Err((error, refusal)),
        }
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).expect("encode");
        encoder.finish().expect("finish encoding")
    }

    fn zlib(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(bytes).expect("encode");
        encoder.finish().expect("finish encoding")
    }

    fn brotli(bytes: &[u8]) -> Vec<u8> {
        // brotli-decompressor ships no encoder; this is `brotli -c` output for
        // the fixed sample below, checked in as bytes.
        assert_eq!(bytes, b"wirebolt brotli body");
        vec![
            0x8f, 0x09, 0x80, 0x77, 0x69, 0x72, 0x65, 0x62, 0x6f, 0x6c, 0x74, 0x20, 0x62, 0x72,
            0x6f, 0x74, 0x6c, 0x69, 0x20, 0x62, 0x6f, 0x64, 0x79, 0x03,
        ]
    }

    #[test]
    fn decodes_gzip_across_arbitrary_chunk_boundaries() {
        let wire = gzip(b"wirebolt gzip body");
        assert_eq!(
            decode_in_pieces(ContentEncoding::Gzip, &wire, None).expect("decoded"),
            b"wirebolt gzip body"
        );
    }

    #[test]
    fn decodes_deflate_and_zstd() {
        let wire = zlib(b"deflate body");
        assert_eq!(
            decode_in_pieces(ContentEncoding::Deflate, &wire, None).expect("decoded"),
            b"deflate body"
        );

        let wire = zstd::encode_all(&b"zstd body"[..], 3).expect("zstd encode");
        assert_eq!(
            decode_in_pieces(ContentEncoding::Zstd, &wire, None).expect("decoded"),
            b"zstd body"
        );
    }

    #[test]
    fn decodes_brotli_across_arbitrary_chunk_boundaries() {
        let wire = brotli(b"wirebolt brotli body");
        assert_eq!(
            decode_in_pieces(ContentEncoding::Brotli, &wire, None).expect("decoded"),
            b"wirebolt brotli body"
        );
    }

    #[test]
    fn truncated_streams_fail_at_finish_for_every_encoding() {
        let zstd = zstd::encode_all(&b"zstd body that is long enough"[..], 3).expect("encode");
        for (encoding, wire) in [
            (
                ContentEncoding::Gzip,
                gzip(b"gzip body that is long enough"),
            ),
            (
                ContentEncoding::Deflate,
                zlib(b"deflate body that is long enough"),
            ),
            (ContentEncoding::Brotli, brotli(b"wirebolt brotli body")),
            (ContentEncoding::Zstd, zstd),
        ] {
            let truncated = &wire[..wire.len() - 3];
            let (error, refusal) =
                decode_in_pieces(encoding, truncated, None).expect_err("truncated must fail");
            assert_eq!(refusal, None, "{encoding}: {error}");
        }
    }

    #[test]
    fn decoded_bytes_are_capped_before_they_accumulate() {
        let mut plain = Vec::new();
        for index in 0..4_096_u32 {
            plain.extend_from_slice(format!("row {index}\n").as_bytes());
        }
        let wire = zlib(&plain);
        let delivered = RefCell::new(0_usize);
        let sink = ChunkSink::new(
            |piece: &[u8]| {
                *delivered.borrow_mut() += piece.len();
                StreamControl::Continue
            },
            Some(1_000),
        );
        let mut stream = BodyStream::new(Some(ContentEncoding::Deflate), sink).expect("decoder");

        let error = stream
            .write_chunk(&wire)
            .expect_err("decoded size must exceed the limit");

        assert_eq!(
            stream.sink().refusal(),
            Some(SinkRefusal::TooLarge),
            "{error}"
        );
        assert!(
            *delivered.borrow() <= 1_000,
            "delivered {} bytes past the limit",
            delivered.borrow()
        );
    }

    #[test]
    fn a_stop_from_the_callback_is_reported_and_final() {
        let calls = RefCell::new(0_u32);
        let sink = ChunkSink::new(
            |_: &[u8]| {
                *calls.borrow_mut() += 1;
                StreamControl::Stop
            },
            None,
        );
        let mut stream = BodyStream::new(Some(ContentEncoding::Gzip), sink).expect("decoder");

        assert!(stream.write_chunk(&gzip(b"first")).is_err());
        assert_eq!(stream.sink().refusal(), Some(SinkRefusal::Stopped));
        assert!(stream.write_chunk(&gzip(b"second")).is_err());
        assert!(stream.finish().is_err());
        assert_eq!(*calls.borrow(), 1, "callback ran again after it stopped");
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
        let sink = ChunkSink::new(|_: &[u8]| StreamControl::Continue, None);
        let mut stream = BodyStream::new(Some(ContentEncoding::Gzip), sink).expect("decoder");
        assert!(stream.write_chunk(b"definitely not gzip data").is_err());
        assert_eq!(stream.sink().refusal(), None);
    }
}
