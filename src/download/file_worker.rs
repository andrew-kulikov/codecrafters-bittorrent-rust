use crate::torrent::TorrentMetainfo;
use crate::tracker;
use std::sync::Arc;

pub struct FileWorker {
    metainfo: Arc<TorrentMetainfo>,
    client_id: String,
    output_dir: String,
}

impl FileWorker {
    pub fn new(metainfo: Arc<TorrentMetainfo>, client_id: String, output_dir: String) -> Self {
        Self {
            metainfo,
            client_id,
            output_dir,
        }
    }

    pub fn run(&mut self) -> anyhow::Result<()> {
        let tracker_request = tracker::TrackerRequest {
            info_hash: self.metainfo.info_hash.clone(),
            peer_id: self.client_id.to_string(),
            port: 6881,
            uploaded: 0,
            downloaded: 0,
            left: self.metainfo.length,
            compact: 1,
        };
        Ok(())
    }
}
