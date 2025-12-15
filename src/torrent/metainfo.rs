use crate::utils;
use serde::{Deserialize, Serialize};
use serde_bytes;

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
    pub fn parse(file_path: &str) -> anyhow::Result<Self> {
        // Read file contents
        let bytes = std::fs::read(file_path).expect("Failed to read torrent file");
        Self::from_bytes(&bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        let parsed_metainfo: TorrentMetainfoSerde =
            serde_bencode::from_bytes(&bytes).expect("Failed to decode torrent file");

        let info_hash_bytes = serde_bencode::to_bytes(&parsed_metainfo.info)
            .expect("Failed to re-encode info dictionary for hashing");

        let length = parsed_metainfo
            .info
            .length
            .or_else(|| {
                parsed_metainfo
                    .info
                    .files
                    .as_ref()
                    .map(|files| files.iter().map(|f| f.length).sum())
            })
            .unwrap_or(0);

        Ok(TorrentMetainfo {
            announce: parsed_metainfo.announce,
            piece_length: parsed_metainfo.info.piece_length,
            pieces: parsed_metainfo.info.pieces,
            length,
            info_hash: utils::sha1(&info_hash_bytes),
        })
    }

    pub fn from_info_bytes(announce: String, info_bytes: &[u8]) -> anyhow::Result<Self> {
        let info_dict: InfoDictionary =
            serde_bencode::from_bytes(info_bytes).expect("Failed to decode info dictionary");

        let length = info_dict
            .length
            .or_else(|| {
                info_dict
                    .files
                    .as_ref()
                    .map(|files| files.iter().map(|f| f.length).sum())
            })
            .unwrap_or(0);

        Ok(TorrentMetainfo {
            announce,
            piece_length: info_dict.piece_length,
            pieces: info_dict.pieces,
            length,
            info_hash: utils::sha1(info_bytes),
        })
    }

    pub fn to_bytes(&self) -> anyhow::Result<Vec<u8>> {
        let metainfo_serde = TorrentMetainfoSerde {
            announce: self.announce.clone(),
            info: InfoDictionary {
                piece_length: self.piece_length,
                pieces: self.pieces.clone(),
                length: Some(self.length),
                files: None,
                name: None,
            },
        };

        Ok(serde_bencode::to_bytes(&metainfo_serde)?)
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TorrentMetainfoSerde {
    announce: String,
    info: InfoDictionary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InfoDictionary {
    #[serde(rename = "piece length")]
    piece_length: u64,
    #[serde(with = "serde_bytes")]
    pieces: Vec<u8>,
    length: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    files: Option<Vec<FileEntry>>, // For multi-file torrents
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileEntry {
    length: u64,
    path: Vec<String>,
}
