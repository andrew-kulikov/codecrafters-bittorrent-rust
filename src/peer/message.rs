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
    /// BEP-10 extended message (msg_id = 20)
    Extended = 20,
}

pub struct PeerMessage {
    pub len: u32,
    pub msg_type: PeerMessageType,
    pub payload: Vec<u8>,
}

pub struct HandshakeRequest {
    pub pstr: &'static str,
    pub reserved: [u8; 8],
    pub info_hash: Vec<u8>,
    pub peer_id: Vec<u8>,
}

pub struct HandshakeResponse {
    pub pstr: String,
    pub reserved: [u8; 8],
    pub info_hash: Vec<u8>,
    pub peer_id: Vec<u8>,
}

impl HandshakeRequest {
    pub fn new(info_hash: Vec<u8>, peer_id: Vec<u8>) -> Self {
        Self {
            pstr: "BitTorrent protocol",
            reserved: [0u8; 8],
            info_hash,
            peer_id,
        }
    }

    pub fn new_with_extension_support(info_hash: Vec<u8>, peer_id: Vec<u8>) -> Self {
        Self {
            pstr: "BitTorrent protocol",
            reserved: get_reserved_extension_support_bytes(),
            info_hash,
            peer_id,
        }
    }

    pub fn as_bytes(&self) -> anyhow::Result<Vec<u8>> {
        // BitTorrent handshake format:
        // <pstrlen><pstr><reserved><info_hash><peer_id>
        // pstr typically "BitTorrent protocol"
        let pstr_bytes = self.pstr.as_bytes();
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

fn get_reserved_extension_support_bytes() -> [u8; 8] {
    // Set extension protocol support bit (BEP-10)
    let mut reserved = [0u8; 8];
    reserved[5] |= 0b0001_0000;
    reserved
}

/// Check if reserved bytes indicate extension support (BEP-10).
pub fn has_extension_support(reserved: &[u8; 8]) -> bool {
    reserved[5] & 0b0001_0000 != 0
}

#[cfg(test)]
mod test {
    #[test]
    fn get_reserved_bytes() {
        let reserved = super::get_reserved_extension_support_bytes();
        let hex_reserved = hex::encode(&reserved);
        assert_eq!(hex_reserved, "0000000000100000");
    }
}
