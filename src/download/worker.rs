use std::cmp::min;
use std::sync::Arc;

use anyhow::{anyhow, ensure};

use super::queue::PieceQueue;
use crate::peer::{
    ExtensionHandshakePayload, PeerCommand, PeerConnection, PeerEvent, PeerSession,
    PeerSessionConfig, PeerSessionHandler, SessionControl,
};
use crate::torrent::TorrentMetainfo;
use crate::tracker::Peer;
use crate::utils::hash;

pub struct PeerWorker {
    peer: Peer,
    metainfo: Arc<TorrentMetainfo>,
    queue: Arc<PieceQueue>,
    client_id: String,
    output_dir: String,
    config: PeerSessionConfig,
}

impl PeerWorker {
    pub fn new(
        peer: Peer,
        metainfo: Arc<TorrentMetainfo>,
        queue: Arc<PieceQueue>,
        client_id: String,
        output_dir: String,
        config: PeerSessionConfig,
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
            PeerSessionConfig::default(),
        )
    }

    pub fn run(&mut self) -> anyhow::Result<()> {
        let session = PeerSession::new(
            self.peer.clone(),
            self.metainfo.info_hash.clone(),
            self.client_id.clone(),
            self.config.clone(),
        );

        session.run(self)
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
}

impl PeerSessionHandler for PeerWorker {
    fn should_stop(&self) -> bool {
        self.queue.is_shutdown()
    }

    fn on_connect(&mut self, conn: &PeerConnection) -> anyhow::Result<SessionControl> {
        // Send interested immediately; we tolerate extension handshake arriving anytime.
        conn.send(PeerCommand::Interested)?;
        Ok(SessionControl::Continue)
    }

    fn on_event(
        &mut self,
        conn: &PeerConnection,
        event: PeerEvent,
    ) -> anyhow::Result<SessionControl> {
        match event {
            PeerEvent::HandshakeComplete {
                extension_supported,
                ..
            } => {
                if extension_supported {
                    let cmd = PeerCommand::Extended {
                        ext_id: 0,
                        payload: ExtensionHandshakePayload::default_extensions().encode()?,
                    };
                    conn.send(cmd)?
                }
                Ok(SessionControl::Continue)
            }
            PeerEvent::Choke => {
                self.log("Choked by peer; will reconnect");
                Ok(SessionControl::Reconnect)
            }
            PeerEvent::Unchoke => {
                // Once unchoked, attempt to download until error or completion.
                if let Err(e) = self.download_all_pieces(conn) {
                    return Err(e);
                }
                if self.queue.is_shutdown() {
                    return Ok(SessionControl::Stop);
                }
                Ok(SessionControl::Continue)
            }
            PeerEvent::IoError(err) => Err(anyhow!(err)),
            _ => {
                // For now ignore other events; future: handle bitfield/have/extended.
                Ok(SessionControl::Continue)
            }
        }
    }
}
