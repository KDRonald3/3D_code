//! Stable content hashing for [`crate::map::File::content_hash`].
//!
//! Lives here (not in the engine) so both writers and readers of a saved map —
//! the extractor and, later, the source-serving server — call the identical
//! function. Uses SHA-256 rather than `std`'s `DefaultHasher`, whose output is
//! not stable across Rust versions or machines and therefore cannot be
//! persisted in JSON.

use sha2::{Digest, Sha256};

/// Hex-encoded SHA-256 of `bytes` (lowercase, 64 hex digits, no algorithm prefix).
///
/// Callers must pass the **raw file bytes as read from disk** — the same slice
/// `std::fs::read` would return — not a line-ending-normalised or re-encoded
/// view. On Windows that distinction is load-bearing: a CRLF edit changes the
/// digest even when the logical Rust source is unchanged.
///
/// The algorithm is fixed by the JSON contract as SHA-256. The stored value is
/// bare hex (no `sha256:` prefix) so a staleness check is a string equality
/// against a re-hash of the path. Switching algorithms later would be a
/// wire-format bump (new field or an explicit prefix), not a silent
/// reinterpretation of this string.
pub fn content_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    to_hex(&digest)
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_is_stable_for_identical_bytes() {
        // Empty input has a universally known SHA-256 digest — pins algorithm
        // and hex format without depending on our own helper for the oracle.
        assert_eq!(
            content_hash(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        let bytes = b"fn alpha() {}\n";
        let a = content_hash(bytes);
        let b = content_hash(bytes);
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
        // Known SHA-256 of these exact bytes (no BOM / CRLF).
        assert_eq!(
            a,
            "960b7ddc8ead20ef925e0e6ceedba3fcc8a872cf3e7c6b354620455014dd487a"
        );
    }

    #[test]
    fn content_hash_changes_when_bytes_change() {
        let original = content_hash(b"fn alpha() {}\n");
        let edited = content_hash(b"fn alpha() {}\r\n");
        assert_ne!(
            original, edited,
            "CRLF vs LF must change the digest (Windows drift)"
        );
        assert_ne!(original, content_hash(b"fn alpha() { }\n"));
    }
}
