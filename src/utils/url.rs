/// Percent-encode arbitrary bytes into a URL-safe string (RFC 3986).
///
/// Unreserved characters (ALPHA / DIGIT / '-' / '.' / '_' / '~') are left as-is.
/// All other bytes are encoded as `%HH` (uppercase hex).
pub fn url_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    // In the worst case, every byte becomes "%XX" (3 chars)
    let mut out = String::with_capacity(bytes.len() * 3);

    for &b in bytes {
        match b {
            // Unreserved characters according to RFC 3986
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(HEX[(b >> 4) as usize] as char);
                out.push(HEX[(b & 0x0F) as usize] as char);
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_unreserved_as_is() {
        let s = "Az09-._~";
        assert_eq!(url_encode(s.as_bytes()), s);
    }

    #[test]
    fn encodes_spaces_and_binary() {
        let bytes = vec![b' ', 0x00, 0xFF, b'a'];
        assert_eq!(url_encode(&bytes), "%20%00%FFa");
    }
}
