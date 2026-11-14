//! Request body decoding: `Content-Encoding` gzip, deflate (zlib or raw),
//! br and zstd, with a limit on the decompressed size.

use std::io::Read;

use bytes::Bytes;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DecodeError {
    #[error("unsupported content encoding {0:?}")]
    Unsupported(String),
    #[error("decompressed body exceeds {0} bytes")]
    TooLarge(usize),
    #[error("invalid {0} data: {1}")]
    Invalid(&'static str, String),
}

fn read_limited(mut r: impl Read, limit: usize, name: &'static str) -> Result<Vec<u8>, DecodeError> {
    let mut out = Vec::new();
    let n =
        (&mut r).take(limit as u64 + 1).read_to_end(&mut out).map_err(|e| DecodeError::Invalid(name, e.to_string()))?;
    if n > limit {
        return Err(DecodeError::TooLarge(limit));
    }
    Ok(out)
}

/// Decodes `body` according to a `Content-Encoding` header value (which
/// may list several encodings, applied in order).
pub fn decode(encoding: Option<&str>, body: Bytes, limit: usize) -> Result<Bytes, DecodeError> {
    let Some(encoding) = encoding else {
        return check(body, limit);
    };
    let mut data = body;
    // Encodings are listed in the order they were applied; undo in reverse.
    let list: Vec<String> =
        encoding.split(',').map(|e| e.trim().to_ascii_lowercase()).filter(|e| !e.is_empty()).collect();
    for enc in list.iter().rev() {
        data = match enc.as_str() {
            "identity" => data,
            "gzip" | "x-gzip" => read_limited(flate2::read::MultiGzDecoder::new(&data[..]), limit, "gzip")?.into(),
            "deflate" => {
                // Both zlib-wrapped and raw deflate are seen in the wild.
                match read_limited(flate2::read::ZlibDecoder::new(&data[..]), limit, "deflate") {
                    Ok(v) => v.into(),
                    Err(DecodeError::TooLarge(n)) => return Err(DecodeError::TooLarge(n)),
                    Err(_) => read_limited(flate2::read::DeflateDecoder::new(&data[..]), limit, "deflate")?.into(),
                }
            }
            "br" => read_limited(brotli::Decompressor::new(&data[..], 4096), limit, "br")?.into(),
            "zstd" => {
                let dec = zstd::stream::read::Decoder::new(&data[..])
                    .map_err(|e| DecodeError::Invalid("zstd", e.to_string()))?;
                read_limited(dec, limit, "zstd")?.into()
            }
            other => return Err(DecodeError::Unsupported(other.to_string())),
        };
    }
    check(data, limit)
}

fn check(b: Bytes, limit: usize) -> Result<Bytes, DecodeError> {
    if b.len() > limit { Err(DecodeError::TooLarge(limit)) } else { Ok(b) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn payload() -> Vec<u8> {
        b"{\"message\":\"hello\"}\n".repeat(100)
    }

    #[test]
    fn all_encodings_round_trip() {
        let p = payload();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&p).unwrap();
        let gz = gz.finish().unwrap();
        let mut zl = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        zl.write_all(&p).unwrap();
        let zl = zl.finish().unwrap();
        let mut raw = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        raw.write_all(&p).unwrap();
        let raw = raw.finish().unwrap();
        let mut br = Vec::new();
        brotli::CompressorWriter::new(&mut br, 4096, 5, 22).write_all(&p).unwrap();
        let zs = zstd::encode_all(&p[..], 3).unwrap();
        for (enc, data) in
            [("gzip", gz.clone()), ("deflate", zl), ("deflate", raw), ("br", br), ("zstd", zs), ("identity", p.clone())]
        {
            assert_eq!(&decode(Some(enc), data.into(), 1 << 20).unwrap()[..], &p[..], "{enc}");
        }
        assert_eq!(&decode(Some("GZIP"), gz.into(), 1 << 20).unwrap()[..], &p[..]);
        assert_eq!(&decode(None, p.clone().into(), 1 << 20).unwrap()[..], &p[..]);
    }

    #[test]
    fn limits_and_errors() {
        let p = vec![b'a'; 10_000];
        let zs = zstd::encode_all(&p[..], 3).unwrap();
        assert_eq!(decode(Some("zstd"), zs.into(), 9_999), Err(DecodeError::TooLarge(9_999)));
        assert_eq!(decode(None, p.into(), 10), Err(DecodeError::TooLarge(10)));
        assert!(matches!(decode(Some("gzip"), Bytes::from_static(b"nope"), 100), Err(DecodeError::Invalid(..))));
        assert!(matches!(decode(Some("compress"), Bytes::from_static(b"x"), 100), Err(DecodeError::Unsupported(_))));
    }
}
