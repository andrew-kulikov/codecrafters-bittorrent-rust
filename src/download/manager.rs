use std::io::Write;
use std::sync::Arc;
use std::thread;

use crate::torrent::TorrentMetainfo;
use crate::tracker;

use super::queue::PieceQueue;
use super::worker::PeerWorker;

/// DownloadManager handles the overall download process of a torrent file.
/// It manages peer connections, piece downloading, and file assembly.
pub struct DownloadManager {
    metainfo: Arc<TorrentMetainfo>,
    client_id: String,
    output_path: String,
}

impl DownloadManager {
    pub fn new(metainfo: TorrentMetainfo, client_id: String, output_path: String) -> Self {
        Self {
            metainfo: Arc::new(metainfo),
            client_id,
            output_path,
        }
    }

    pub fn download(&self) -> anyhow::Result<()> {
        // 1. Get peers
        let tracker_request = tracker::TrackerRequest {
            info_hash: self.metainfo.info_hash.clone(),
            peer_id: self.client_id.clone(),
            port: 6881,
            uploaded: 0,
            downloaded: 0,
            left: self.metainfo.length,
            compact: 1,
        };

        let tracker_response = tracker::announce(self.metainfo.announce.clone(), tracker_request)?;
        let peers = tracker_response.peers;
        println!("[DownloadManager] Found {} peers", peers.len());

        let num_pieces = self.metainfo.get_piece_count() as u64;
        println!("[DownloadManager] Total pieces to download: {}", num_pieces);

        let queue = Arc::new(PieceQueue::new(num_pieces as u32));

        // Create a temporary directory for pieces
        let temp_dir = std::path::Path::new(&self.output_path)
            .parent()
            .unwrap()
            .join("temp");
        std::fs::create_dir_all(&temp_dir)?;

        let mut handles = vec![];

        for peer in peers {
            let peer = peer.clone();
            let metainfo = self.metainfo.clone();
            let queue = queue.clone();
            let client_id = self.client_id.clone();
            let output_dir = temp_dir.clone();

            let handle = thread::spawn(move || {
                let mut worker = PeerWorker::with_defaults(
                    peer,
                    metainfo,
                    queue,
                    client_id,
                    output_dir.to_str().unwrap().to_string(),
                );
                if let Err(e) = worker.run() {
                    eprintln!("Worker failed: {}", e);
                }
            });
            handles.push(handle);
        }

        // Wait for all pieces to be downloaded
        queue.wait_until_finished();

        // Combine pieces
        let mut output_file = std::fs::File::create(&self.output_path)?;
        for i in 0..num_pieces {
            let piece_path = std::path::Path::new(&temp_dir).join(format!("piece_{}", i));
            let piece_data = std::fs::read(&piece_path)?;
            output_file.write_all(&piece_data)?;
        }

        // Cleanup
        std::fs::remove_dir_all(temp_dir)?;

        Ok(())
    }
}
