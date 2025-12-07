use crate::utils::RawBytesExt;
use crate::{bencode, utils};

pub struct TorrentMetainfo {
    pub announce: String,
    // From "info" dictionary
    pub piece_length: u64,
    pub pieces: Vec<u8>,
    pub length: u64,
    // Metadata
    pub info_hash: Vec<u8>,
}

impl TorrentMetainfo {
    pub fn parse(file_path: &str) -> Self {
        // Read file contents
        let torrent_bytes = std::fs::read(file_path).expect("Failed to read torrent file");
        let torrent_info = bencode::parse_bytes(torrent_bytes);

        // Extract fields
        let announce = torrent_info
            .get("announce")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let info_section = torrent_info
            .get("info")
            .expect("Missing 'info' dictionary in torrent file");

        let piece_length = info_section
            .get("piece length")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let pieces_str = info_section
            .get("pieces")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let pieces = pieces_str.to_raw_bytes();

        let length = info_section
            .get("length")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let info_hash = {
            let encoded_info = bencode::encode(info_section);
            utils::sha1(&encoded_info)
        };

        TorrentMetainfo {
            announce,
            piece_length,
            pieces,
            length,
            info_hash,
        }
    }

    pub fn get_info_hash_hex(&self) -> String {
        hex::encode(&self.info_hash)
    }

    pub fn get_piece_count(&self) -> usize {
        self.pieces.len() / 20
    }

    pub fn get_piece_hashes(&self) -> Vec<String> {
        let mut piece_hashes = Vec::new();
        for chunk in self.pieces.chunks(20) {
            let hash = hex::encode(chunk);
            piece_hashes.push(hash);
        }
        piece_hashes
    }

    pub fn get_piece_hash_bytes(&self, index: usize) -> &[u8] {
        let start = index * 20;
        let end = start + 20;
        &self.pieces[start..end]
    }
}
