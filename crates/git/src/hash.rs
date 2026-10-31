//! Object hashes as git prints them.

/// A full object hash: 40 (SHA-1) or 64 (SHA-256) lowercase hex digits. Anything else is `None`.
pub fn parse_hash(bytes: &[u8]) -> Option<String> {
    let valid = matches!(bytes.len(), 40 | 64) && bytes.iter().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b));
    // Only ASCII hex digits remain, so converting to a String can't fail.
    if valid { String::from_utf8(bytes.to_vec()).ok() } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_sha1_and_sha256() {
        assert!(parse_hash(&[b'a'; 40]).is_some());
        assert!(parse_hash(&[b'0'; 64]).is_some());
    }

    #[test]
    fn rejects_everything_else() {
        assert_eq!(parse_hash(&[b'a'; 39]), None);
        assert_eq!(parse_hash(&[b'A'; 40]), None);
        assert_eq!(parse_hash(&[b'g'; 40]), None);
        assert_eq!(parse_hash(b""), None);
    }
}
