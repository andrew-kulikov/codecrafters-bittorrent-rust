use std::fs::OpenOptions;
use std::sync::{Arc, Mutex};
use std::thread;

use super::queue::PieceQueue;
use super::worker::PeerWorker;
use crate::peer::PeerSessionConfig;
use crate::torrent::TorrentMetainfo;
use crate::tracker;
use crate::{log_error, log_info};

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
        log_info!("DownloadManager", "Found {} peers", peers.len());

        let num_pieces = self.metainfo.get_piece_count() as u64;
        log_info!(
            "DownloadManager",
            "Total pieces to download: {}",
            num_pieces
        );

        let piece_ids = (0..num_pieces as u32).collect::<Vec<u32>>();
        let queue = Arc::new(PieceQueue::new(&piece_ids));

        let mut output_file = OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .truncate(true)
            .open(&self.output_path)?;
        output_file.set_len(self.metainfo.length)?;
        let shared_file = Arc::new(Mutex::new(output_file));

        let mut handles = vec![];

        for peer in peers {
            let peer = peer.clone();
            let metainfo = self.metainfo.clone();
            let queue = queue.clone();
            let client_id = self.client_id.clone();
            let file = shared_file.clone();

            let handle = thread::spawn(move || {
                let mut worker = PeerWorker::new(
                    peer,
                    metainfo,
                    queue,
                    client_id,
                    file,
                    PeerSessionConfig::default(),
                );
                if let Err(e) = worker.run() {
                    log_error!("DownloadManager", "Worker failed: {}", e);
                }
            });
            handles.push(handle);
        }

        // Wait for all pieces to be downloaded and persisted
        queue.wait_until_finished();

        for handle in handles {
            let _ = handle.join();
        }

        Ok(())
    }
}
