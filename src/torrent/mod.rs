mod reader;

pub struct TorrentMetadata {
    pub announce: String,
    // From "info" dictionary
    pub piece_length: u64,
    pub pieces: Vec<u8>,
    pub length: u64,
    // Metadata
    pub info_hash: Vec<u8>,
}

pub use reader::parse_torrent_file;
