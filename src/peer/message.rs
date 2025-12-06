use crate::utils::RawBytesExt;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum PeerMessageType {
    KeepAlive = -1,
    Choke = 0,
    Unchoke = 1,
    Interested = 2,
    NotInterested = 3,
    Have = 4,
    Bitfield = 5,
    Request = 6,
    Piece = 7,
    Cancel = 8,
}

pub struct PeerMessage {
    pub len: u32,
    pub msg_type: PeerMessageType,
    pub payload: Vec<u8>,
}

pub struct HandshakeRequest {
    pub pstr: String,
    pub reserved: [u8; 8],
    pub info_hash: Vec<u8>,
    pub peer_id: Vec<u8>,
}

impl HandshakeRequest {
    pub fn as_bytes(&self) -> anyhow::Result<Vec<u8>> {
        // BitTorrent handshake format:
        // <pstrlen><pstr><reserved><info_hash><peer_id>
        // pstr typically "BitTorrent protocol"
        let pstr_bytes = self.pstr.to_raw_bytes();
        if pstr_bytes.len() > u8::MAX as usize {
            anyhow::bail!("pstr too long");
        }
        if self.info_hash.len() != 20 {
            anyhow::bail!("info_hash must be 20 bytes");
        }
        if self.peer_id.len() != 20 {
            anyhow::bail!("peer_id must be 20 bytes");
        }

        let mut buf = Vec::with_capacity(1 + pstr_bytes.len() + 8 + 20 + 20);
        buf.push(pstr_bytes.len() as u8);
        buf.extend_from_slice(&pstr_bytes);
        buf.extend_from_slice(&self.reserved);
        buf.extend_from_slice(&self.info_hash);
        buf.extend_from_slice(&self.peer_id);
        Ok(buf)
    }
}

pub struct HandshakeResponse {
    pub pstr: String,
    pub reserved: [u8; 8],
    pub info_hash: Vec<u8>,
    pub peer_id: Vec<u8>,
}
