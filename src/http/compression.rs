//! Negotiated gzip for buffered JSON responses.

use http_body_util::{BodyExt, Full};
use hyper::body::Body as _;
use hyper::header::{HeaderValue, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, VARY};
use hyper::Response;
use miniz_oxide::deflate::compress_to_vec;

use super::server::{set, Body};

const THRESHOLD: u64 = 1024;

pub(super) async fn negotiate(mut response: Response<Body>, accepts_gzip: bool) -> Response<Body> {
    if response.headers().contains_key(CONTENT_ENCODING)
        || !response
            .body()
            .size_hint()
            .exact()
            .is_some_and(|n| n > THRESHOLD)
        || !response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
    {
        return response;
    }

    response
        .headers_mut()
        .append(VARY, HeaderValue::from_static("Accept-Encoding"));
    if !accepts_gzip {
        return response;
    }

    let (parts, body) = response.into_parts();
    let raw = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(never) => match never {},
    };
    let mut response = Response::from_parts(parts, Full::new(gzip(&raw).into()).boxed_unsync());
    response.headers_mut().remove(CONTENT_LENGTH);
    set(&mut response, CONTENT_ENCODING, "gzip");
    response
}

fn gzip(raw: &[u8]) -> Vec<u8> {
    // RFC 1952: raw DEFLATE, preceded by a fixed header (no name or timestamp)
    // and followed by CRC-32 and uncompressed length modulo 2^32.
    let deflate = compress_to_vec(raw, 1);
    let mut out = Vec::with_capacity(deflate.len() + 18);
    out.extend_from_slice(&[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 4, 255]);
    out.extend_from_slice(&deflate);
    out.extend_from_slice(&crc32(raw).to_le_bytes());
    out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    out
}

fn crc32(raw: &[u8]) -> u32 {
    const TABLE: [u32; 256] = {
        let mut table = [0; 256];
        let mut index = 0;
        while index < table.len() {
            let mut value = index as u32;
            let mut bit = 0;
            while bit < 8 {
                value = (value >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(value & 1));
                bit += 1;
            }
            table[index] = value;
            index += 1;
        }
        table
    };
    !raw.iter().fold(u32::MAX, |crc, byte| {
        (crc >> 8) ^ TABLE[(crc as u8 ^ byte) as usize]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::server::full;
    use hyper::StatusCode;

    #[tokio::test]
    async fn large_json_varies_and_gzip_decodes_to_identity() {
        let raw = format!(r#"{{"data":"{}"}}"#, "abcd".repeat(1024));
        for accepts in [false, true] {
            let mut response = full(StatusCode::OK, raw.clone());
            set(
                &mut response,
                CONTENT_TYPE,
                "application/json; charset=utf-8",
            );
            set(&mut response, CONTENT_LENGTH, &raw.len().to_string());
            set(&mut response, VARY, "Cookie");
            let response = negotiate(response, accepts).await;
            let vary: Vec<_> = response.headers().get_all(VARY).iter().collect();
            assert_eq!(vary, ["Cookie", "Accept-Encoding"]);
            assert_eq!(response.headers().contains_key(CONTENT_ENCODING), accepts);
            assert_eq!(response.headers().contains_key(CONTENT_LENGTH), !accepts);
            let body = response.into_body().collect().await.unwrap().to_bytes();
            if accepts {
                assert_eq!(&body[..3], &[0x1f, 0x8b, 8]);
                let trailer = body.len() - 8;
                let decoded = miniz_oxide::inflate::decompress_to_vec(&body[10..trailer]).unwrap();
                assert_eq!(decoded, raw.as_bytes());
                assert_eq!(&body[trailer..trailer + 4], &crc32(&decoded).to_le_bytes());
                assert_eq!(&body[trailer + 4..], &(decoded.len() as u32).to_le_bytes());
                assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
            } else {
                assert_eq!(body, raw);
            }
        }
    }

    #[tokio::test]
    async fn small_and_already_encoded_json_keep_their_bodies() {
        let encoded = "already encoded".repeat(128);
        for (body, encoding) in [("{}", None), (encoded.as_str(), Some("gzip"))] {
            let mut response = full(StatusCode::OK, body.to_owned());
            set(&mut response, CONTENT_TYPE, "application/json");
            if let Some(encoding) = encoding {
                set(&mut response, CONTENT_ENCODING, encoding);
            }
            let response = negotiate(response, true).await;
            assert!(!response.headers().contains_key(VARY));
            assert_eq!(
                response
                    .headers()
                    .get(CONTENT_ENCODING)
                    .map(|v| v.to_str().unwrap()),
                encoding
            );
            assert_eq!(
                response.into_body().collect().await.unwrap().to_bytes(),
                body
            );
        }
    }
}
