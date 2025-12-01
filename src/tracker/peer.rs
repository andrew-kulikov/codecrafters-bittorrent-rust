use std::io::{Read, Write};
use std::net::TcpStream;

use anyhow::ensure;

use crate::tracker::Peer;
use crate::utils::{RawBytesExt, RawStringExt};

pub struct HandshakeRequest {
    pub pstr: String,
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

fn build_handshake_bytes(req: &HandshakeRequest) -> anyhow::Result<Vec<u8>> {
    // BitTorrent handshake format:
    // <pstrlen><pstr><reserved><info_hash><peer_id>
    // pstr typically "BitTorrent protocol"
    let pstr_bytes = req.pstr.to_raw_bytes();
    if pstr_bytes.len() > u8::MAX as usize {
        anyhow::bail!("pstr too long");
    }
    if req.info_hash.len() != 20 {
        anyhow::bail!("info_hash must be 20 bytes");
    }
    if req.peer_id.len() != 20 {
        anyhow::bail!("peer_id must be 20 bytes");
    }

    let mut buf = Vec::with_capacity(1 + pstr_bytes.len() + 8 + 20 + 20);
    buf.push(pstr_bytes.len() as u8);
    buf.extend_from_slice(&pstr_bytes);
    buf.extend_from_slice(&req.reserved);
    buf.extend_from_slice(&req.info_hash);
    buf.extend_from_slice(&req.peer_id);
    Ok(buf)
}

pub fn handshake(addr: Peer, req: &HandshakeRequest) -> anyhow::Result<HandshakeResponse> {
    let payload = build_handshake_bytes(req)?;

    let mut stream = TcpStream::connect(addr)?;
    stream.write_all(&payload)?;

    // Response is also 1 + pstrlen + len(pstr) + 8 + 20 + 20
    // We first read pstrlen, then the rest based on that.
    let mut pstrlen_buf = [0u8; 1];
    stream.read_exact(&mut pstrlen_buf)?;
    let pstrlen = pstrlen_buf[0] as usize;

    let mut pstr_buf = vec![0u8; pstrlen];
    stream.read_exact(&mut pstr_buf)?;

    let mut reserved = [0u8; 8];
    stream.read_exact(&mut reserved)?;

    let mut info_hash = vec![0u8; 20];
    stream.read_exact(&mut info_hash)?;
    ensure!(info_hash == req.info_hash.as_slice(), "info_hash mismatch in handshake response");

    let mut peer_id = vec![0u8; 20];
    stream.read_exact(&mut peer_id)?;

    let pstr = pstr_buf.to_raw_string();

    Ok(HandshakeResponse {
        pstr,
        reserved,
        info_hash,
        peer_id,
    })
}
