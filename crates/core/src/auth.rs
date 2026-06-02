// Auth hardening — constant-time comparisons, nonce generation, and CSP headers.
// No `unwrap()` in production code; use `expect()` with context strings.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore as _;
use sha2::{Digest, Sha256};

const BLOCK_SIZE: usize = 64;

fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut key_block = [0u8; BLOCK_SIZE];

    if key.len() > BLOCK_SIZE {
        let hashed = Sha256::digest(key);
        key_block[..32].copy_from_slice(&hashed);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; BLOCK_SIZE];
    let mut opad = [0x5cu8; BLOCK_SIZE];
    for (i, byte) in key_block.iter().enumerate() {
        ipad[i] ^= *byte;
        opad[i] ^= *byte;
    }

    let inner_input: Vec<u8> = ipad.iter().chain(msg.iter()).cloned().collect();
    let inner_hash = Sha256::digest(&inner_input);

    let outer_input: Vec<u8> = opad.iter().chain(inner_hash.iter()).cloned().collect();
    let outer_hash = Sha256::digest(&outer_input);

    let mut result = [0u8; 32];
    result.copy_from_slice(&outer_hash);
    result
}

fn constant_time_compare(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ---------------------------------------------------------------------------
// Public functions
// ---------------------------------------------------------------------------

/// Constant-time HMAC-SHA256 comparison. Returns `false` if lengths differ.
pub fn timing_safe_equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let h_a = hmac_sha256(a, b"constant");
    let h_b = hmac_sha256(b, b"constant");

    constant_time_compare(&h_a, &h_b)
}

/// Generate a 16-byte random nonce, base64url-encoded (no padding).
/// Uses `OsRng` (kernel CSPRNG) explicitly so the source of randomness is
/// unambiguous to auditors and static analyzers.
pub fn create_viewer_nonce() -> String {
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Build a Content-Security-Policy header value with the given nonce.
pub fn build_viewer_csp(nonce: &str) -> String {
    [
        "default-src 'self'",
        &format!("script-src 'self' 'nonce-{nonce}' 'strict-dynamic'"),
        "style-src 'self' 'unsafe-inline'",
        "img-src 'self' data: blob:",
        "font-src 'self' data:",
        "connect-src 'self'",
        "frame-ancestors 'none'",
        "base-uri 'self'",
        "form-action 'self'",
        "object-src 'none'",
        "upgrade-insecure-requests",
    ]
    .join("; ")
}

/// Constant-time bearer-token comparison. Returns `false` if either is empty.
pub fn verify_bearer_token(provided: &str, expected: &str) -> bool {
    if provided.is_empty() || expected.is_empty() {
        return false;
    }
    let provided_token = provided.strip_prefix("Bearer ").unwrap_or(provided);
    let expected_token = expected.strip_prefix("Bearer ").unwrap_or(expected);
    timing_safe_equal(provided_token.as_bytes(), expected_token.as_bytes())
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timing_safe_equal_matches() {
        let input = b"same-secret-key-12345";
        assert!(timing_safe_equal(input, input));
    }

    #[test]
    fn test_timing_safe_equal_different_length() {
        let a = b"short";
        let b = b"much-longer-input";
        assert!(!timing_safe_equal(a, b));
    }

    #[test]
    fn test_timing_safe_equal_same_length_different_content() {
        let a = b"aaaaaaab";
        let b = b"aaaaaaac";
        assert!(!timing_safe_equal(a, b));
    }

    #[test]
    fn test_csp_contains_nonce() {
        let nonce = "abc123xyz";
        let csp = build_viewer_csp(nonce);
        assert!(
            csp.contains("'nonce-abc123xyz'"),
            "CSP should contain nonce: {csp}"
        );
        assert!(
            csp.contains("default-src 'self'"),
            "CSP should contain default-src: {csp}"
        );
    }

    #[test]
    fn test_create_viewer_nonce_length() {
        let nonce = create_viewer_nonce();
        assert_eq!(nonce.len(), 22, "nonce={nonce}");
    }

    #[test]
    fn test_create_viewer_nonce_deterministic() {
        let nonce1 = create_viewer_nonce();
        let nonce2 = create_viewer_nonce();
        assert_ne!(nonce1, nonce2, "two nonces should differ");
    }

    #[test]
    fn test_verify_bearer_token_same() {
        let token = "my-secret-token";
        assert!(verify_bearer_token(token, token));
    }

    #[test]
    fn test_verify_bearer_token_with_bearer_prefix() {
        let token = "my-secret-token";
        assert!(verify_bearer_token(&format!("Bearer {token}"), token));
        assert!(verify_bearer_token(token, &format!("Bearer {token}")));
    }

    #[test]
    fn test_verify_bearer_token_empty() {
        assert!(!verify_bearer_token("", "token"));
        assert!(!verify_bearer_token("token", ""));
        assert!(!verify_bearer_token("", ""));
    }

    #[test]
    fn test_verify_bearer_token_mismatch() {
        assert!(!verify_bearer_token("token-a", "token-b"));
    }
}
