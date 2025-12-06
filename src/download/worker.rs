use super::queue::PieceQueue;
use crate::peer::{HandshakeRequest, PeerConnection};
use crate::torrent::TorrentMetainfo;
use crate::tracker::Peer;
use crate::utils::RawBytesExt;
use std::sync::Arc;

pub struct PeerWorker {
    peer: Peer,
    metainfo: Arc<TorrentMetainfo>,
    queue: Arc<PieceQueue>,
    client_id: String,
    output_dir: String,
}

impl PeerWorker {
    pub fn new(
        peer: Peer,
        metainfo: Arc<TorrentMetainfo>,
        queue: Arc<PieceQueue>,
        client_id: String,
        output_dir: String,
    ) -> Self {
        Self {
            peer,
            metainfo,
            queue,
            client_id,
            output_dir,
        }
    }

    pub fn run(&mut self) -> anyhow::Result<()> {
        loop {
            println!("Worker connecting to {}", self.peer);

            let handshake_req = HandshakeRequest {
                pstr: "BitTorrent protocol".to_string(),
                reserved: [0u8; 8],
                info_hash: self.metainfo.info_hash.clone(),
                peer_id: self.client_id.to_raw_bytes(),
            };

            let mut connection = match PeerConnection::new(self.peer.clone(), &handshake_req) {
                Ok(conn) => {
                    self.log("Handshake successful");
                    conn
                }
                Err(e) => {
                    self.log(&format!("Failed to connect: {}. Retrying in 5s...", e));
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    continue;
                }
            };

            while let Some(piece_index) = self.queue.pop() {
                let total_length = self.metainfo.length;
                let piece_len = self.metainfo.piece_length;
                let num_pieces = (total_length + piece_len - 1) / piece_len;

                let current_piece_len = if piece_index as u64 == num_pieces - 1 {
                    let rem = total_length % piece_len;
                    if rem == 0 {
                        piece_len
                    } else {
                        rem
                    }
                } else {
                    piece_len
                };

                self.log(&format!(
                    "Downloading piece {}. Len: {}",
                    piece_index, current_piece_len
                ));

                let mut buffer = vec![0u8; current_piece_len as usize];

                match connection.download_piece(&self.metainfo, piece_index, &mut buffer) {
                    Ok(_) => {
                        self.log(&format!("Finished piece {}", piece_index));
                        let path = std::path::Path::new(&self.output_dir)
                            .join(format!("piece_{}", piece_index));
                        if let Err(e) = std::fs::write(path, &buffer) {
                            self.log(&format!("Failed to write piece {}: {}", piece_index, e));
                            self.queue.push(piece_index);
                        } else {
                            self.queue.mark_completed();
                        }
                    }
                    Err(e) => {
                        self.log(&format!("Failed to download piece {}: {}", piece_index, e));
                        self.queue.push(piece_index); // Return to queue
                                                      // Break inner loop to reconnect
                        break;
                    }
                }
            }

            // Check if we should exit
            // If the queue is shutdown, pop() returns None, and we exit the while loop naturally.
            // If we broke out due to error, we want to continue the outer loop (reconnect).
            // We can check if the queue is shutdown to decide whether to break the outer loop.
            if self.queue.is_shutdown() {
                break;
            }
        }
        Ok(())
    }

    fn log(&self, message: &str) {
        println!("[PeerWorker {}] {}", self.peer, message);
    }
}
