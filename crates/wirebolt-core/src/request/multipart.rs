use std::{
    fs::File,
    io::{self, Read},
    path::PathBuf,
};

use bytes::Bytes;
use futures_util::{Stream, stream};
use tokio::io::{AsyncReadExt, Take};

use super::PreparedBodySource;

pub(crate) const UPLOAD_FRAME_BYTES: usize = 64 * 1024;

#[derive(Clone)]
pub(crate) enum MultipartSegment {
    Bytes(Vec<u8>),
    File { path: PathBuf, byte_count: u64 },
}

#[derive(Clone)]
pub(crate) struct MultipartBody {
    segments: Vec<MultipartSegment>,
    byte_count: u64,
}

impl MultipartBody {
    pub(crate) fn new(segments: Vec<MultipartSegment>) -> Option<Self> {
        let byte_count = segments.iter().try_fold(0_u64, |total, segment| {
            total.checked_add(match segment {
                MultipartSegment::Bytes(bytes) => u64::try_from(bytes.len()).ok()?,
                MultipartSegment::File { byte_count, .. } => *byte_count,
            })
        })?;
        Some(Self {
            segments,
            byte_count,
        })
    }

    pub(crate) const fn byte_count(&self) -> u64 {
        self.byte_count
    }

    /// Small complete bodies remain reusable across redirects. Reads are capped
    /// even if a file grows between metadata lookup and preparation.
    pub(crate) fn into_prepared(self) -> io::Result<PreparedBodySource> {
        let has_files = self
            .segments
            .iter()
            .any(|s| matches!(s, MultipartSegment::File { .. }));
        if has_files && self.byte_count > UPLOAD_FRAME_BYTES as u64 {
            return Ok(PreparedBodySource::Multipart(self));
        }
        let mut output = Vec::new();
        for segment in self.segments {
            match segment {
                MultipartSegment::Bytes(bytes) => output.extend(bytes),
                MultipartSegment::File { path, byte_count } => {
                    let start = output.len();
                    File::open(path)?
                        .take(byte_count + 1)
                        .read_to_end(&mut output)?;
                    if (output.len() - start) as u64 != byte_count {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Upload file size changed",
                        ));
                    }
                }
            }
        }
        Ok(PreparedBodySource::Bytes(output))
    }

    pub(crate) fn into_stream(self) -> impl Stream<Item = io::Result<Bytes>> + Send {
        let state = MultipartUpload {
            segments: self.segments.into_iter(),
            bytes: Bytes::new(),
            file: None,
        };
        stream::try_unfold(state, |mut state| async move {
            loop {
                if !state.bytes.is_empty() {
                    let count = state.bytes.len().min(UPLOAD_FRAME_BYTES);
                    return Ok(Some((state.bytes.split_to(count), state)));
                }
                if let Some(file) = &mut state.file {
                    if file.limit() == 0 {
                        state.file = None;
                        continue;
                    }
                    let mut buffer = vec![0; UPLOAD_FRAME_BYTES];
                    let count = file.read(&mut buffer).await?;
                    if count == 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "Upload file was truncated",
                        ));
                    }
                    buffer.truncate(count);
                    return Ok(Some((Bytes::from(buffer), state)));
                }
                match state.segments.next() {
                    Some(MultipartSegment::Bytes(bytes)) => state.bytes = Bytes::from(bytes),
                    Some(MultipartSegment::File { path, byte_count }) => {
                        state.file = Some(tokio::fs::File::open(path).await?.take(byte_count));
                    }
                    None => return Ok(None),
                }
            }
        })
    }
}

struct MultipartUpload {
    segments: std::vec::IntoIter<MultipartSegment>,
    bytes: Bytes,
    file: Option<Take<tokio::fs::File>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocumentId, NoSecrets, Request, RequestPipeline};
    use futures_util::{StreamExt, TryStreamExt};

    fn prepared_file(path: &std::path::Path) -> crate::PreparedRequest {
        let mut request = Request::new(
            DocumentId::new("upload").unwrap(),
            "Upload",
            "POST",
            "http://localhost/",
        );
        request.body = serde_json::from_value(serde_json::json!({
            "kind":"multipart", "parts":[{"id":"file", "name":"file", "kind":"file", "value":"", "file_path":path, "enabled":true}]
        })).unwrap();
        RequestPipeline::new(None, &NoSecrets)
            .prepare(&request)
            .unwrap()
    }

    #[tokio::test]
    async fn multipart_32_mib_file_retains_bounded_buffers_and_emits_bounded_frames() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("payload.bin");
        File::create(&path)
            .unwrap()
            .set_len(32 * 1024 * 1024)
            .unwrap();
        let prepared = prepared_file(&path);
        let byte_count = prepared.body_byte_count();
        let (_, _, _, PreparedBodySource::Multipart(body)) = prepared.into_parts() else {
            panic!("large multipart must stream")
        };
        let retained: usize = body
            .segments
            .iter()
            .map(|segment| match segment {
                MultipartSegment::Bytes(bytes) => bytes.capacity(),
                MultipartSegment::File { .. } => 0,
            })
            .sum();
        assert!(retained <= UPLOAD_FRAME_BYTES, "retained {retained} bytes");
        let stream = body.into_stream();
        tokio::pin!(stream);
        let mut sent = 0;
        let mut largest_frame = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.unwrap();
            largest_frame = largest_frame.max(chunk.len());
            sent += chunk.len() as u64;
        }
        assert_eq!(sent, byte_count);
        assert!(largest_frame <= UPLOAD_FRAME_BYTES);
        eprintln!(
            "32 MiB multipart: retained buffer capacity={retained} bytes, largest emitted frame={largest_frame} bytes, total={sent} bytes"
        );
    }

    #[tokio::test]
    async fn multipart_stream_matches_reusable_encoder_byte_for_byte() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("payload.bin");
        std::fs::write(&path, [0, 1, 128, 255]).unwrap();
        let body = MultipartBody::new(vec![
            MultipartSegment::Bytes(b"header\r\n\r\n".to_vec()),
            MultipartSegment::Bytes("text café".as_bytes().to_vec()),
            MultipartSegment::File {
                path,
                byte_count: 4,
            },
            MultipartSegment::Bytes(b"\r\nfooter".to_vec()),
        ])
        .unwrap();
        let PreparedBodySource::Bytes(expected) = body.clone().into_prepared().unwrap() else {
            panic!("small body must be reusable")
        };
        let chunks: Vec<_> = body.into_stream().try_collect().await.unwrap();
        assert_eq!(chunks.concat(), expected);
    }

    #[test]
    fn small_multipart_matches_existing_wire_encoding_exactly() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("payload.bin");
        std::fs::write(&path, [0, 1, 128, 255]).unwrap();
        let prepared = prepared_file(&path);
        let mut expected = b"--wirebolt-boundary-7MA4YWxkTrZu0gW\r\nContent-Disposition: form-data; name=\"file\"; filename=\"payload.bin\"\r\nContent-Type: application/octet-stream\r\n\r\n".to_vec();
        expected.extend([0, 1, 128, 255]);
        expected.extend(b"\r\n--wirebolt-boundary-7MA4YWxkTrZu0gW--\r\n");
        assert_eq!(prepared.body(), expected);
        assert_eq!(prepared.body_byte_count(), expected.len() as u64);
    }

    #[tokio::test]
    async fn multipart_stream_rejects_truncated_files_and_limits_growing_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("payload.bin");
        std::fs::write(&path, b"old").unwrap();
        let body = MultipartBody::new(vec![MultipartSegment::File {
            path: path.clone(),
            byte_count: 3,
        }])
        .unwrap();
        std::fs::write(&path, b"old and new").unwrap();
        let chunks: Vec<_> = body.clone().into_stream().try_collect().await.unwrap();
        assert_eq!(chunks.concat(), b"old");
        assert!(body.clone().into_prepared().is_err());
        std::fs::write(&path, b"x").unwrap();
        let error = body
            .into_stream()
            .try_collect::<Vec<_>>()
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }
}
