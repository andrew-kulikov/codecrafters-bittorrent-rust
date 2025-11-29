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
