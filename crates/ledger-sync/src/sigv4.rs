//! AWS Signature Version 4 for the handful of S3 requests this crate makes.
//!
//! Written by hand on purpose: the whole surface is `GET`/`HEAD`/`PUT`/`DELETE`
//! on a single object with a fixed set of signed headers, and pulling in the
//! AWS SDK for that would multiply the dependency tree (and the Android build
//! time) for no benefit.

use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone)]
pub struct Credentials {
    pub access_key: String,
    pub secret_key: String,
    pub region: String,
}

/// The signed headers to replay on the request, `authorization` last.
pub fn signed_headers(
    method: &str,
    url: &str,
    body: &[u8],
    extra: &[(&str, &str)],
    creds: &Credentials,
    at: DateTime<Utc>,
) -> Result<Vec<(String, String)>, String> {
    let (host, path) = split_url(url)?;
    let amz_date = at.format("%Y%m%dT%H%M%SZ").to_string();
    let short_date = at.format("%Y%m%d").to_string();
    let payload_hash = hex(&Sha256::digest(body));

    let mut headers: Vec<(String, String)> = vec![
        ("host".to_string(), host.clone()),
        ("x-amz-content-sha256".to_string(), payload_hash.clone()),
        ("x-amz-date".to_string(), amz_date.clone()),
    ];
    for (key, value) in extra {
        headers.push((key.to_ascii_lowercase(), value.trim().to_string()));
    }
    headers.sort_by(|a, b| a.0.cmp(&b.0));

    let canonical_headers: String = headers
        .iter()
        .map(|(k, v)| format!("{k}:{v}\n"))
        .collect::<String>()
        .trim_start_matches('\n')
        .to_string();
    let signed_names: String = headers
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");

    let canonical_request = format!(
        "{method}\n{}\n\n{canonical_headers}\n{signed_names}\n{payload_hash}",
        encode_path(&path)
    );

    let scope = format!("{short_date}/{}/s3/aws4_request", creds.region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex(&Sha256::digest(canonical_request.as_bytes()))
    );

    let signing_key = signing_key(&creds.secret_key, &short_date, &creds.region);
    let signature = hex(&hmac(&signing_key, string_to_sign.as_bytes()));

    let mut out: Vec<(String, String)> = headers;
    out.push((
        "authorization".to_string(),
        format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_names}, Signature={signature}",
            creds.access_key
        ),
    ));
    Ok(out)
}

fn signing_key(secret: &str, date: &str, region: &str) -> Vec<u8> {
    let k_date = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k_region = hmac(&k_date, region.as_bytes());
    let k_service = hmac(&k_region, b"s3");
    hmac(&k_service, b"aws4_request")
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn split_url(url: &str) -> Result<(String, String), String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(|| format!("S3 endpoint must be http(s): {url}"))?;
    match rest.find('/') {
        Some(index) => Ok((rest[..index].to_string(), rest[index..].to_string())),
        None => Ok((rest.to_string(), "/".to_string())),
    }
}

/// RFC 3986 encoding, keeping `/` so object keys stay readable in logs.
pub fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 25, 16, 45, 0).unwrap()
    }

    /// Cross-checked against an independent HMAC-SHA256 implementation of the
    /// SigV4 recipe, so a refactor that changes the canonical request fails
    /// here instead of only against a live bucket.
    #[test]
    fn signs_a_conditional_put() {
        let creds = Credentials {
            access_key: "AKIATEST".to_string(),
            secret_key: "secretTEST".to_string(),
            region: "us-east-1".to_string(),
        };
        let headers = signed_headers(
            "PUT",
            "https://api.minio.example.com/ledger/blobs/abc123",
            br#"{"hello":1}"#,
            &[("if-none-match", "*")],
            &creds,
            at(),
        )
        .unwrap();
        let auth = headers
            .iter()
            .find(|(k, _)| k == "authorization")
            .map(|(_, v)| v.clone())
            .unwrap();
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIATEST/20261025/us-east-1/s3/aws4_request, \
             SignedHeaders=host;if-none-match;x-amz-content-sha256;x-amz-date, \
             Signature=7e5b6819c26bd63331e9e92376934c41f96570bfaf0dd05dc151b751cff07c39"
        );
    }

    #[test]
    fn encodes_keys_but_keeps_slashes() {
        assert_eq!(
            encode_path("/ledger/2026/2026-10.ledger"),
            "/ledger/2026/2026-10.ledger"
        );
        assert_eq!(encode_path("/a b/ü.ledger"), "/a%20b/%C3%BC.ledger");
    }
}
