use sha1::{Digest, Sha1};
use hex;

pub fn bytes_to_raw_string(data: &[u8]) -> String {
    data.iter().map(|&b| b as char).collect()
}

pub fn raw_string_to_bytes(s: &str) -> Vec<u8> {
    s.chars().map(|ch| ch as u8).collect()
}

pub fn compute_sha1_hash(data: &[u8]) -> String  {
    let mut hasher = Sha1::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}