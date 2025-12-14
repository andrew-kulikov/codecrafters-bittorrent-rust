use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use serde_bencode::value::Value;
use std::collections::HashSet;

use crate::{
    peer::{
        extension::ExtensionHandshakePayload, PeerCommand, PeerConnection, PeerEvent, PeerSession,
        PeerSessionConfig, PeerSessionHandler, SessionControl,
    },
    torrent::{MagnetLink, TorrentMetainfo},
    tracker,
    utils::log,
};

const METADATA_EXTENSION_NAME: &str = "ut_metadata";
const MY_METADATA_EXTENSION_MESSAGE_ID: u8 = 42;

pub struct MetadataFetcher {
    magnet_link: MagnetLink,
    client_id: String,

    ext_handshake_sent: bool,
    metadata_bytes: Option<Vec<u8>>,
    total_size: Option<usize>,
    requested_pieces: HashSet<u64>,
    received_pieces: HashSet<u64>,

    peer_metadata_id: Option<u8>,
    peer_id: Option<Vec<u8>>,

    // For debugging purposes only
    handshake_only: bool,
}

pub struct MetadataFetchResult {
    pub peer_id: Option<Vec<u8>>,
    pub peer_metadata_id: Option<u8>,
    pub metainfo: Option<TorrentMetainfo>,
}

enum MetadataMessageType {
    Request = 0,
    Data = 1,
    Reject = 2,
}

impl MetadataFetcher {
    pub fn new(link: &str, client_id: String, handshake_only: bool) -> anyhow::Result<Self> {
        let magnet_link = MagnetLink::parse(link).context("Failed to parse magnet link")?;
        Ok(Self {
            magnet_link,
            ext_handshake_sent: false,
            metadata_bytes: None,
            total_size: None,
            requested_pieces: HashSet::new(),
            received_pieces: HashSet::new(),
            client_id,
            handshake_only,
            peer_metadata_id: None,
            peer_id: None,
        })
    }

    pub fn run(&mut self) -> anyhow::Result<MetadataFetchResult> {
        // For now assume there is only one tracker
        let tracker_url = self
            .magnet_link
            .trackers
            .iter()
            .next()
            .map(|url| url.to_string())
            .context("No trackers found")?;

        // And only one info hash
        let topic = self
            .magnet_link
            .exact_topics
            .iter()
            .next()
            .context("No info hash found")?;

        let info_hash = {
            let hash = topic.get_hash().context("Unsupported scheme")?;
            hex::decode(hash).context("Invalid info hash hex")?
        };

        // 2. Announce to tracker and get peers
        let peers = {
            let tracker_request = tracker::TrackerRequest {
                info_hash: info_hash.clone(),
                peer_id: self.client_id.clone(),
                port: 6881,
                uploaded: 0,
                downloaded: 0,
                left: 999,
                compact: 1,
            };

            let tracker_response = tracker::announce(tracker_url, tracker_request)
                .context("Failed to get tracker response")?;
            tracker_response.peers
        };

        for peer in &peers {
            // 3. Run a lightweight session that sends extension handshake
            let session = PeerSession::new(
                peer.clone(),
                info_hash.clone(),
                self.client_id.clone(),
                PeerSessionConfig::default(),
            );

            match session.run(self) {
                Ok(_) => {
                    let metainfo = match &self.metadata_bytes {
                        Some(bytes) => {
                            let v: Value = serde_bencode::from_bytes(bytes)?;
                            log::debug("MetadataFetcher", &format!("Parsed metainfo: {:#?}", v));
                            log::debug("MetadataFetcher", &format!("Source bytes: {:#?}", bytes));
                            Some(
                                TorrentMetainfo::from_bytes(bytes)
                                    .context("Failed to parse received metadata")?,
                            )
                        }
                        None => None,
                    };

                    return Ok(MetadataFetchResult {
                        peer_id: self.peer_id.clone(),
                        peer_metadata_id: self.peer_metadata_id,
                        metainfo,
                    });
                }
                Err(e) => {
                    log::error(
                        "MetadataFetcher",
                        &format!("[{}] Session error: {:#}", peer, e),
                    );
                    continue;
                }
            }
        }

        bail!("No peers responded with extension handshake");
    }

    fn request_metadata_piece(&self, conn: &PeerConnection, piece: u64) -> anyhow::Result<()> {
        let metadata_ext_id = self
            .peer_metadata_id
            .context("Missing metadata extension id from handshake")?;

        let payload = serde_bencode::to_bytes(&PieceRequestPayloadSerde {
            msg_type: MetadataMessageType::Request as u64,
            piece,
        })?;

        // For ut_metadata the extended message id is the per-peer metadata id we got from the
        // handshake, and the payload itself is just the bencoded dictionary.
        conn.send(PeerCommand::Extended {
            ext_id: metadata_ext_id,
            payload,
        })?;
        Ok(())
    }

    fn request_missing_pieces(
        &mut self,
        conn: &PeerConnection,
        expected_piece_count: usize,
    ) -> anyhow::Result<()> {
        for piece in 0..(expected_piece_count as u64) {
            if self.requested_pieces.contains(&piece) {
                continue;
            }
            self.request_metadata_piece(conn, piece)?;
            self.requested_pieces.insert(piece);
        }
        Ok(())
    }

    fn handle_metadata_message(
        &mut self,
        conn: &PeerConnection,
        payload: &[u8],
    ) -> anyhow::Result<bool> {
        const METADATA_RESPONSE_BENCODE_LEN: usize = 44;

        let mininal_allowed_len = METADATA_RESPONSE_BENCODE_LEN + 1 /* at least 1 byte of data */;
        if payload.len() < mininal_allowed_len {
            bail!(
                "Metadata message too short, expected at least {} bytes, got {}",
                mininal_allowed_len,
                payload.len()
            );
        }

        // Format: {'msg_type': 1, 'piece': 0, 'total_size': XXXX} - always 44 bytes
        let response: DataResponsePayloadSerde =
            serde_bencode::from_bytes(&payload[0..METADATA_RESPONSE_BENCODE_LEN - 1])?;

        match response.msg_type {
            x if x == MetadataMessageType::Request as u64 => {
                bail!("Unexpected metadata request from peer");
            }
            x if x == MetadataMessageType::Reject as u64 => {
                bail!("Metadata request was rejected by peer");
            }
            x if x != MetadataMessageType::Data as u64 => {
                // From spec: "In order to support future extensability, an unrecognized message ID MUST be ignored."
                return Ok(false);
            }
            _ => {}
        }

        let data = &payload[METADATA_RESPONSE_BENCODE_LEN - 1..];
        if data.is_empty() {
            bail!("Empty metadata piece received");
        }

        // total_size is learned from the handshake; per-piece response total_size conveys piece length here.
        const NOT_LAST_PIECE_SIZE: usize = 16 * 1024; // 16 KiB
        let metadata_size = self
            .total_size
            .context("Metadata size missing from handshake")?;
        let piece_size = response.total_size as usize;
        let last_piece_size = metadata_size % NOT_LAST_PIECE_SIZE;

        // Validate piece size
        if data.len() != piece_size {
            bail!(
                "Metadata piece length mismatch, expected {} bytes, got {}",
                piece_size,
                data.len()
            );
        }

        if piece_size != NOT_LAST_PIECE_SIZE && piece_size != last_piece_size {
            bail!(
                "Invalid metadata piece size {}, expected {} or {}",
                piece_size,
                NOT_LAST_PIECE_SIZE,
                last_piece_size
            );
        }

        let buf = self
            .metadata_bytes
            .as_mut()
            .context("Metadata buffer not initialized")?;

        // Place this piece at its offset.
        let offset = (response.piece as usize)
            .checked_mul(NOT_LAST_PIECE_SIZE) // 16 KiB pieces
            .context("Piece offset overflow")?;
        if offset >= metadata_size {
            bail!(
                "Metadata piece offset {} out of total size {}",
                offset,
                metadata_size
            );
        }

        let end = (offset + data.len()).min(metadata_size);
        buf[offset..end].copy_from_slice(&data[..end - offset]);
        self.received_pieces.insert(response.piece);

        // Calculate how many pieces we expect and whether we're done.
        let expected_piece_count = (metadata_size + NOT_LAST_PIECE_SIZE - 1) / NOT_LAST_PIECE_SIZE;
        let complete = self.received_pieces.len() >= expected_piece_count;

        if !complete {
            self.request_missing_pieces(conn, expected_piece_count)?;
        }

        Ok(complete)
    }
}

impl PeerSessionHandler for MetadataFetcher {
    fn on_connect(&mut self, conn: &PeerConnection) -> anyhow::Result<SessionControl> {
        //conn.send(PeerCommand::Interested)?;
        if let Some(peer_id) = &conn.peer_id {
            self.peer_id = Some(peer_id.clone());
        }
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
                if extension_supported && !self.ext_handshake_sent {
                    let extensions = vec![(
                        METADATA_EXTENSION_NAME.to_string(),
                        MY_METADATA_EXTENSION_MESSAGE_ID,
                    )];
                    let payload = ExtensionHandshakePayload::new(extensions).encode()?;
                    conn.send(PeerCommand::Extended { ext_id: 0, payload })?;
                    self.ext_handshake_sent = true;
                }
                Ok(SessionControl::Continue)
            }
            PeerEvent::Extended { ext_id: 0, payload } => {
                let ext_payload = ExtensionHandshakePayload::decode(&payload)?;
                if let Some(metadata_ext_id) = ext_payload.get_extension_id(METADATA_EXTENSION_NAME)
                {
                    self.peer_metadata_id = Some(metadata_ext_id);
                }

                let metadata_size = ext_payload
                    .metadata_size
                    .context("Metadata size was not received on extended handshake")?;
                self.total_size = Some(metadata_size as usize);
                self.metadata_bytes = Some(vec![0u8; metadata_size as usize]);

                log::debug(
                    "MetadataFetcher",
                    &format!(
                        "Received extension handshake. id: {:?}, metadata size: {} bytes",
                        self.peer_metadata_id, metadata_size
                    ),
                );

                // Terminate for magnet_handshake test
                match self.handshake_only {
                    true => Ok(SessionControl::Stop),
                    false => {
                        self.request_metadata_piece(conn, 0)?;
                        self.requested_pieces.insert(0);
                        Ok(SessionControl::Continue)
                    }
                }
            }
            PeerEvent::Extended {
                ext_id: MY_METADATA_EXTENSION_MESSAGE_ID,
                payload,
            } => {
                let complete = self.handle_metadata_message(conn, &payload)?;
                match complete {
                    true => Ok(SessionControl::Stop),
                    false => Ok(SessionControl::Continue),
                }
            }
            PeerEvent::IoError(err) => Err(anyhow::anyhow!(err)),
            _ => Ok(SessionControl::Continue),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PieceRequestPayloadSerde {
    pub msg_type: u64,
    pub piece: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DataResponsePayloadSerde {
    pub msg_type: u64,
    pub piece: u64,
    pub total_size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_piece_request_message() {
        let payload = serde_bencode::to_bytes(&PieceRequestPayloadSerde {
            msg_type: MetadataMessageType::Request as u64,
            piece: 0,
        })
        .unwrap();

        let deserialized: PieceRequestPayloadSerde = serde_bencode::from_bytes(&payload).unwrap();
        assert_eq!(deserialized.msg_type, 0);
        assert_eq!(deserialized.piece, 0);
    }

    #[test]
    fn test_bencode_decode_with_extra_data() {
        let encoded = "d4:city4:test6:street4:teste_kekekekek".as_bytes();
        let decoded: Address = serde_bencode::from_bytes(&encoded).unwrap();
        assert_eq!(decoded.city, "test");
        assert_eq!(decoded.street, "test");
    }

    #[test]
    fn test_metadata_response_message_len() {
        let payload = serde_bencode::to_bytes(&DataResponsePayloadSerde {
            msg_type: MetadataMessageType::Data as u64,
            piece: 0,
            total_size: 1234,
        })
        .unwrap();

        assert_eq!(payload.len(), 44);
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct Address {
        city: String,
        street: String,
    }
}
