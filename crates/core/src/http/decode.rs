//! Response body decompression (`Content-Encoding`).

use bytes::Bytes;
use std::io::Read;

/// Largest decompressed body. A small compressed body can expand enormously
/// (a "zip bomb"), so decoding stops here instead of exhausting memory.
const MAX_DECODED_BYTES: u64 = 256 * 1024 * 1024;

/// Decodes `body` according to a `Content-Encoding` header value.
///
/// Returns the body unchanged for `identity` or unknown encodings, and an
/// error message when the data is corrupt or decompresses to more than
/// [`MAX_DECODED_BYTES`].
pub fn decode(encoding: &str, body: Bytes) -> Result<Bytes, String> {
    // Encodings are listed in the order they were applied, so undo them in reverse.
    let mut data = body;
    for coding in encoding.split(',').map(str::trim).rev() {
        data = match coding.to_ascii_lowercase().as_str() {
            "" | "identity" => data,
            "gzip" | "x-gzip" => read_all(flate2::read::MultiGzDecoder::new(&data[..]), "gzip")?,
            "deflate" => read_all(flate2::read::ZlibDecoder::new(&data[..]), "deflate")
                // Some servers send raw deflate without the zlib wrapper.
                .or_else(|_| read_all(flate2::read::DeflateDecoder::new(&data[..]), "deflate"))?,
            "br" => read_all(brotli::Decompressor::new(&data[..], 64 * 1024), "brotli")?,
            _ => return Ok(data),
        };
    }
    Ok(data)
}

fn read_all(reader: impl Read, name: &str) -> Result<Bytes, String> {
    let mut out = Vec::new();
    // One byte more than allowed, to tell "exactly the limit" from "too large".
    reader.take(MAX_DECODED_BYTES + 1).read_to_end(&mut out).map_err(|e| format!("could not decompress {name} body: {e}"))?;
    if out.len() as u64 > MAX_DECODED_BYTES {
        return Err(format!("the {name} body decompresses to more than {} MB, so it is shown compressed", MAX_DECODED_BYTES / 1024 / 1024));
    }
    Ok(out.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn gzip_round_trip() {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"hello").unwrap();
        let compressed = Bytes::from(encoder.finish().unwrap());
        assert_eq!(decode("gzip", compressed).unwrap(), Bytes::from_static(b"hello"));
    }

    #[test]
    fn decompression_is_limited() {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        let zeros = vec![0u8; 1024 * 1024];
        for _ in 0..=(MAX_DECODED_BYTES / zeros.len() as u64) {
            encoder.write_all(&zeros).unwrap();
        }
        let compressed = Bytes::from(encoder.finish().unwrap());
        assert!(compressed.len() < 2 * 1024 * 1024);
        let err = decode("gzip", compressed).unwrap_err();
        assert!(err.contains("more than 256 MB"), "{err}");
    }

    #[test]
    fn unknown_encoding_is_left_alone() {
        assert_eq!(decode("zstd", Bytes::from_static(b"x")).unwrap(), Bytes::from_static(b"x"));
    }
}
