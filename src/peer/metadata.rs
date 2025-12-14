use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{
    peer::{
        extension::ExtensionHandshakePayload, PeerCommand, PeerConnection, PeerEvent, PeerSession,
        PeerSessionConfig, PeerSessionHandler, SessionControl,
    },
    torrent::MagnetLink,
    tracker,
};

pub struct MetadataFetcher {
    magnet_link: MagnetLink,
    client_id: String,

    ext_handshake_sent: bool,

    peer_metadata_id: Option<u8>,
    peer_id: Option<Vec<u8>>,

    // For debugging purposes only
    handshake_only: bool,
}

pub struct MetadataFetchResult {
    pub peer_id: Option<Vec<u8>>,
    pub peer_metadata_id: Option<u8>,
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

            if let Ok(_) = session.run(self) {
                return Ok(MetadataFetchResult {
                    peer_id: self.peer_id.clone(),
                    peer_metadata_id: self.peer_metadata_id,
                });
            }
        }

        Err(anyhow::anyhow!(
            "No peers responded with extension handshake"
        ))
    }

    fn request_metadata(&self, conn: &PeerConnection) -> anyhow::Result<()> {
        let metadata_ext_id = self
            .peer_metadata_id
            .context("Missing metadata extension id from handshake")?;

        let payload = serde_bencode::to_bytes(&PieceRequestPayloadSerde {
            msg_type: MetadataMessageType::Request as u64,
            piece: 0,
        })?;

        // For ut_metadata the extended message id is the per-peer metadata id we got from the
        // handshake, and the payload itself is just the bencoded dictionary.
        conn.send(PeerCommand::Extended {
            ext_id: metadata_ext_id,
            payload,
        })?;
        Ok(())
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
                    let payload = ExtensionHandshakePayload::default_extensions().encode()?;
                    conn.send(PeerCommand::Extended { ext_id: 0, payload })?;
                    self.ext_handshake_sent = true;
                }
                Ok(SessionControl::Continue)
            }
            PeerEvent::Extended { ext_id: 0, payload } => {
                let ext_payload = ExtensionHandshakePayload::decode(&payload)?;
                if let Some(metadata_ext_id) = ext_payload.get_metadata_extension_id() {
                    self.peer_metadata_id = Some(metadata_ext_id);
                }

                // Terminate for magnet_handshake test
                match self.handshake_only {
                    true => Ok(SessionControl::Stop),
                    false => {
                        self.request_metadata(conn)?;
                        Ok(SessionControl::Continue)
                    }
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
}
