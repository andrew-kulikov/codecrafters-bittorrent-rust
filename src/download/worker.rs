use std::cmp::min;
use std::sync::Arc;

use anyhow::anyhow;

use super::queue::PieceQueue;
use crate::peer::{
    PeerCommand, PeerConnection, PeerEvent, PeerSession, PeerSessionConfig, PeerSessionHandler,
    SessionControl,
};
use crate::torrent::TorrentMetainfo;
use crate::tracker::Peer;
use crate::utils::{hash, log};

pub struct PeerWorker {
    peer: Peer,
    metainfo: Arc<TorrentMetainfo>,
    queue: Arc<PieceQueue>,
    client_id: String,
    output_dir: String,
    config: PeerSessionConfig,
    active_download: Option<DownloadState>,
}

struct DownloadState {
    index: u32,
    offset: u32,
    length: u32,
    buffer: Vec<u8>,
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
            active_download: None,
        }
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

    fn start_next_piece(&mut self, conn: &PeerConnection) -> anyhow::Result<()> {
        if self.queue.is_shutdown() {
            return Ok(());
        }

        while let Some(piece_index) = self.queue.pop() {
            let piece_len = self.get_piece_len(piece_index);
            self.log(&format!(
                "Downloading piece {} ({} bytes)",
                piece_index, piece_len
            ));

            self.active_download = Some(DownloadState {
                index: piece_index,
                offset: 0,
                length: piece_len,
                buffer: vec![0u8; piece_len as usize],
            });

            self.send_next_request(conn)?;
            return Ok(());
        }

        Ok(())
    }

    fn send_next_request(&self, conn: &PeerConnection) -> anyhow::Result<()> {
        if let Some(state) = &self.active_download {
            if state.offset >= state.length {
                return Ok(());
            }

            let request_len = min(1 << 14, state.length - state.offset);
            conn.send(PeerCommand::Request {
                index: state.index,
                begin: state.offset,
                length: request_len,
            })?;
        }
        Ok(())
    }

    fn handle_piece_event(
        &mut self,
        conn: &PeerConnection,
        index: u32,
        begin: u32,
        data: Vec<u8>,
    ) -> anyhow::Result<SessionControl> {
        let Some(state) = self.active_download.as_mut() else {
            return Ok(SessionControl::Continue);
        };

        if state.index != index || state.offset != begin {
            return Ok(SessionControl::Continue);
        }

        let end = (begin + data.len() as u32) as usize;
        if end > state.buffer.len() {
            self.abandon_active_piece();
            return Err(anyhow!("piece block overruns buffer"));
        }
        state.buffer[begin as usize..end].copy_from_slice(&data);
        state.offset += data.len() as u32;

        if state.offset >= state.length {
            let finished = self
                .active_download
                .take()
                .expect("active download should exist when finishing piece");

            let expected = self.metainfo.get_piece_hash_bytes(finished.index as usize);
            let actual = hash::sha1(&finished.buffer);
            if expected != actual.as_slice() {
                self.queue.push(finished.index);
                return Err(anyhow!("hash mismatch for piece {}", finished.index));
            }

            self.persist_piece(finished.index, &finished.buffer)?;
            self.queue.mark_completed();

            if self.queue.is_shutdown() {
                return Ok(SessionControl::Stop);
            }

            self.start_next_piece(conn)?;
        } else {
            self.send_next_request(conn)?;
        }

        Ok(SessionControl::Continue)
    }

    fn abandon_active_piece(&mut self) {
        if let Some(state) = self.active_download.take() {
            self.queue.push(state.index);
        }
    }

    fn log(&self, message: &str) {
        log::debug("PeerWorker", &format!("[{}] {}", self.peer, message));
    }
}

impl PeerSessionHandler for PeerWorker {
    fn should_stop(&self) -> bool {
        self.queue.is_shutdown()
    }

    fn on_connect(&mut self, conn: &PeerConnection) -> anyhow::Result<SessionControl> {
        // Send interested immediately; we tolerate extension handshake arriving anytime.
        self.abandon_active_piece();
        conn.send(PeerCommand::Interested)?;
        Ok(SessionControl::Continue)
    }

    fn on_event(
        &mut self,
        conn: &PeerConnection,
        event: PeerEvent,
    ) -> anyhow::Result<SessionControl> {
        match event {
            PeerEvent::Choke => {
                self.log("Choked by peer; will reconnect");
                self.abandon_active_piece();
                Ok(SessionControl::Reconnect)
            }
            PeerEvent::Unchoke => {
                if self.active_download.is_some() {
                    self.send_next_request(conn)?;
                } else {
                    self.start_next_piece(conn)?;
                }

                if self.queue.is_shutdown() && self.active_download.is_none() {
                    Ok(SessionControl::Stop)
                } else {
                    Ok(SessionControl::Continue)
                }
            }
            PeerEvent::Piece { index, begin, data } => {
                self.handle_piece_event(conn, index, begin, data)
            }
            PeerEvent::IoError(err) => {
                self.log(&format!("I/O error from peer: {}; reconnecting", err));
                self.abandon_active_piece();
                Ok(SessionControl::Reconnect)
            }
            _ => {
                // For now ignore other events; future: handle bitfield/have/extended.
                Ok(SessionControl::Continue)
            }
        }
    }
}
