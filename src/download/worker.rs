use std::cmp::min;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, ensure};

use super::queue::PieceQueue;
use crate::peer::{ExtensionHandshakePayload, HandshakeRequest, PeerCommand, PeerConnection, PeerEvent, PeerMessageType};
use crate::torrent::TorrentMetainfo;
use crate::tracker::Peer;
use crate::utils::{hash, RawBytesExt};

pub struct PeerWorker {
    peer: Peer,
    metainfo: Arc<TorrentMetainfo>,
    queue: Arc<PieceQueue>,
    client_id: String,
    output_dir: String,
    config: PeerWorkerConfig,
}

#[derive(Clone, Debug)]
pub struct PeerWorkerConfig {
    pub backoff_base_secs: u64,
    pub backoff_cap_secs: u64,
}

impl Default for PeerWorkerConfig {
    fn default() -> Self {
        Self {
            backoff_base_secs: 3,
            backoff_cap_secs: 30,
        }
    }
}

impl PeerWorker {
    pub fn new(
        peer: Peer,
        metainfo: Arc<TorrentMetainfo>,
        queue: Arc<PieceQueue>,
        client_id: String,
        output_dir: String,
        config: PeerWorkerConfig,
    ) -> Self {
        Self {
            peer,
            metainfo,
            queue,
            client_id,
            output_dir,
            config,
        }
    }

    pub fn with_defaults(
        peer: Peer,
        metainfo: Arc<TorrentMetainfo>,
        queue: Arc<PieceQueue>,
        client_id: String,
        output_dir: String,
    ) -> Self {
        Self::new(
            peer,
            metainfo,
            queue,
            client_id,
            output_dir,
            PeerWorkerConfig::default(),
        )
    }

    pub fn run(&mut self) -> anyhow::Result<()> {
        let mut attempts = 0u32;

        while !self.queue.is_shutdown() {
            self.log(&format!("Connecting (attempt {})", attempts + 1));

            let handshake_req = HandshakeRequest::new_with_extension_support(
                self.metainfo.info_hash.clone(),
                self.client_id.to_raw_bytes(),
            );

            let connection = match PeerConnection::new(self.peer.clone(), &handshake_req) {
                Ok(conn) => {
                    attempts = 0;
                    conn
                }
                Err(e) => {
                    self.log(&format!("Failed to connect: {}", e));
                    attempts += 1;
                    std::thread::sleep(self.backoff_delay(attempts));
                    continue;
                }
            };

            if let Err(e) = self.drive_connection(connection) {
                self.log(&format!("Connection error: {}", e));
                attempts += 1;
                std::thread::sleep(self.backoff_delay(attempts));
            }
        }

        Ok(())
    }

    fn drive_connection(&mut self, conn: PeerConnection) -> anyhow::Result<()> {
        // Send interested immediately; we tolerate extension handshake arriving anytime.
        conn.send(PeerCommand::Interested)?;

        // Keep a simple loop over events.
        while let Some(event) = conn.next_event() {
            match event {
                PeerEvent::HandshakeComplete {
                    extension_supported,
                    ..
                } => {
                    if extension_supported {
                        let cmd = PeerCommand::Extended {
                            ext_id: PeerMessageType::Extended as u8,
                            payload: ExtensionHandshakePayload::default_extensions().encode()?,
                        };
                        conn.send(cmd)?
                    }
                }
                PeerEvent::Choke => {
                    self.log("Choked by peer; will reconnect");
                    return Err(anyhow!("choked"));
                }
                PeerEvent::Unchoke => {
                    // Once unchoked, attempt to download until error or completion.
                    if let Err(e) = self.download_all_pieces(&conn) {
                        return Err(e);
                    }
                    if self.queue.is_shutdown() {
                        return Ok(());
                    }
                }
                PeerEvent::IoError(err) => return Err(anyhow!(err)),
                _ => {
                    // For now ignore other events; future: handle bitfield/have/extended.
                }
            }
        }

        Err(anyhow!("connection closed"))
    }

    fn download_all_pieces(&self, conn: &PeerConnection) -> anyhow::Result<()> {
        while let Some(piece_index) = self.queue.pop() {
            if self.queue.is_shutdown() {
                break;
            }

            let piece_len = self.get_piece_len(piece_index);
            self.log(&format!(
                "Downloading piece {} ({} bytes)",
                piece_index, piece_len
            ));

            let mut buffer = vec![0u8; piece_len as usize];
            match self.download_single_piece(conn, piece_index, &mut buffer) {
                Ok(_) => {
                    self.persist_piece(piece_index, &buffer)?;
                    self.queue.mark_completed();
                }
                Err(e) => {
                    self.log(&format!("Failed piece {}: {}", piece_index, e));
                    self.queue.push(piece_index);
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    fn download_single_piece(
        &self,
        conn: &PeerConnection,
        piece_index: u32,
        buffer: &mut [u8],
    ) -> anyhow::Result<()> {
        let mut offset: u32 = 0;
        let piece_len: u32 = buffer.len() as u32;

        while offset < piece_len {
            let request_len = min(1 << 14, piece_len - offset);
            conn.send(PeerCommand::Request {
                index: piece_index,
                begin: offset,
                length: request_len,
            })?;

            loop {
                match conn.next_event() {
                    Some(PeerEvent::Piece { index, begin, data })
                        if index == piece_index && begin == offset =>
                    {
                        let end = (begin + data.len() as u32) as usize;
                        buffer[begin as usize..end].copy_from_slice(&data);
                        offset += data.len() as u32;
                        break;
                    }
                    Some(PeerEvent::Choke) => return Err(anyhow!("choked mid-piece")),
                    Some(PeerEvent::IoError(err)) => return Err(anyhow!(err)),
                    Some(PeerEvent::Extended { .. }) => {
                        // TODO: handle extension messages when implemented.
                    }
                    Some(_) => {
                        // Ignore unrelated events.
                    }
                    None => return Err(anyhow!("connection closed")),
                }
            }
        }

        // Validate hash
        let expected = self.metainfo.get_piece_hash_bytes(piece_index as usize);
        let actual = hash::sha1(buffer);
        ensure!(
            expected == actual.as_slice(),
            "hash mismatch for piece {}",
            piece_index
        );

        Ok(())
    }

    fn persist_piece(&self, piece_index: u32, data: &[u8]) -> anyhow::Result<()> {
        let path = std::path::Path::new(&self.output_dir).join(format!("piece_{}", piece_index));
        std::fs::write(path, data)?;
        Ok(())
    }

    fn get_piece_len(&self, piece_index: u32) -> u32 {
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
        current_piece_len as u32
    }

    fn log(&self, message: &str) {
        println!("[PeerWorker] [{}] {}", self.peer, message);
    }

    fn backoff_delay(&self, attempts: u32) -> Duration {
        let base = Duration::from_secs(self.config.backoff_base_secs.max(1));
        let cap = Duration::from_secs(
            self.config
                .backoff_cap_secs
                .max(self.config.backoff_base_secs),
        );
        let shift = attempts.min(10);
        let factor = 1u32 << shift;
        let wait = base.saturating_mul(factor);
        if wait > cap {
            cap
        } else {
            wait
        }
    }
}
