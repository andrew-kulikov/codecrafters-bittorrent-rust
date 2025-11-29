use sha1::{Digest, Sha1};

/// Extension trait to convert a `&str` into raw bytes by casting each char to a single byte.
///
/// NOTE: This assumes a 1:1 mapping of `char` to byte (i.e. only ASCII). Multi-byte UTF-8
/// characters will be truncated. Use `str::as_bytes()` if you need the real UTF-8 encoding.
pub trait RawBytesExt {
    fn to_raw_bytes(&self) -> Vec<u8>;
}

pub trait RawStringExt {
    fn to_raw_string(&self) -> String;
}

impl RawStringExt for [u8] {
    fn to_raw_string(&self) -> String {
        self.iter().map(|&b| b as char).collect()
    }
}

impl RawBytesExt for str {
    fn to_raw_bytes(&self) -> Vec<u8> {
        self.chars().map(|ch| ch as u8).collect()
    }
}

pub fn compute_sha1_hash(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha1::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

/// Percent-encode arbitrary bytes into a URL-safe string (RFC 3986).
///
/// Unreserved characters (ALPHA / DIGIT / '-' / '.' / '_' / '~') are left as-is.
/// All other bytes are encoded as `%HH` (uppercase hex).
///
/// This is useful for encoding fields like `info_hash` and `peer_id` in tracker URLs.
pub fn url_encode_bytes(bytes: &[u8]) -> String {
    fn is_unreserved(b: u8) -> bool {
        matches!(b,
            b'-' | b'.' | b'_' | b'~'
            | b'0'..=b'9'
            | b'A'..=b'Z'
            | b'a'..=b'z'
        )
    }

    let mut out = String::with_capacity(bytes.len() * 3);
    for &b in bytes {
        if is_unreserved(b) {
            out.push(b as char);
            continue;
        }
        // Encode as %HH with uppercase hex
        out.push('%');
        out.push_str(&format!("{:02X}", b));
    }
    out
}

pub fn url_encode(bytes: &Vec<u8>) -> String {
    // In the worst case, every byte becomes "%XX" (3 chars)
    let mut out = String::with_capacity(bytes.len() * 3);

    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    for &b in bytes {
        match b {
            // Unreserved characters according to RFC 3986
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-' | b'_' | b'.' | b'~' => {
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
    use super::url_encode_bytes;

    #[test]
    fn encodes_unreserved_as_is() {
        let s = "Az09-._~";
        assert_eq!(url_encode_bytes(s.as_bytes()), s);
    }

    #[test]
    fn encodes_spaces_and_binary() {
        let bytes = vec![b' ', 0x00, 0xFF, b'a'];
        assert_eq!(url_encode_bytes(&bytes), "%20%00%FFa");
    }
}
