//! Slack request signature verification (Events API).

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Maximum age of a signed request (replay protection).
const MAX_SIGNATURE_AGE_SECS: i64 = 60 * 5;

/// Verify `X-Slack-Signature` and `X-Slack-Request-Timestamp` headers.
pub fn verify_slack_signature(
    signing_secret: &str,
    timestamp: &str,
    body: &[u8],
    signature: &str,
) -> bool {
    if signing_secret.is_empty() {
        return false;
    }

    let Ok(ts) = timestamp.trim().parse::<i64>() else {
        return false;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if (now - ts).abs() > MAX_SIGNATURE_AGE_SECS {
        return false;
    }

    let Ok(mut mac) = HmacSha256::new_from_slice(signing_secret.as_bytes()) else {
        return false;
    };
    let base = format!("v0:{timestamp}:{}", String::from_utf8_lossy(body));
    mac.update(base.as_bytes());
    let expected = format!("v0={}", hex_encode(mac.finalize().into_bytes().as_slice()));
    constant_time_eq(signature.trim(), &expected)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.bytes().zip(right.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(secret: &str, timestamp: &str, body: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac");
        mac.update(format!("v0:{timestamp}:{body}").as_bytes());
        format!("v0={}", hex_encode(mac.finalize().into_bytes().as_slice()))
    }

    fn now_timestamp() -> String {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_else(|_| "0".to_string())
    }

    #[test]
    fn accepts_valid_signature() {
        let secret = "test-secret";
        let body = r#"{"type":"url_verification","challenge":"xyz"}"#;
        let ts = now_timestamp();
        let sig = sign(&secret, &ts, body);
        assert!(verify_slack_signature(secret, &ts, body.as_bytes(), &sig));
    }

    #[test]
    fn rejects_tampered_body() {
        let secret = "test-secret";
        let body = r#"{"type":"url_verification","challenge":"xyz"}"#;
        let ts = now_timestamp();
        let sig = sign(&secret, &ts, body);
        assert!(!verify_slack_signature(
            secret,
            &ts,
            br#"{"type":"url_verification","challenge":"evil"}"#,
            &sig
        ));
    }

    #[test]
    fn rejects_empty_secret() {
        assert!(!verify_slack_signature("", "1", b"{}", "v0=00"));
    }
}