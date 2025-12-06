use std::io::{Read, Write};
use std::net::TcpStream;

use anyhow::ensure;

use crate::torrent::TorrentMetainfo;
use crate::tracker::Peer;
use crate::utils::{RawBytesExt, RawStringExt, hash};

pub struct PeerConnection {
    pub stream: TcpStream,
    pub peer_id: Option<Vec<u8>>,
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum PeerMessageType {
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

struct PeerMessage {
    len: u32,
    msg_type: PeerMessageType,
    payload: Vec<u8>,
}

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

impl PeerConnection {
    pub fn new(addr: Peer, req: &HandshakeRequest) -> anyhow::Result<PeerConnection> {
        let mut stream = TcpStream::connect(addr.clone())?;

        // Handshake format:
        // <pstrlen><pstr><reserved><info_hash><peer_id>
        let payload = build_handshake_bytes(req)?;
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

        let mut peer_id = vec![0u8; 20];
        stream.read_exact(&mut peer_id)?;

        // Verify response
        let pstr = pstr_buf.to_raw_string();
        ensure!(pstr == req.pstr, "pstr mismatch in handshake response");

        ensure!(
            info_hash == req.info_hash.as_slice(),
            "info_hash mismatch in handshake response"
        );

        Ok(PeerConnection {
            stream,
            peer_id: Some(peer_id),
        })
    }

    pub fn download_piece(
        &mut self,
        metainfo: &TorrentMetainfo,
        piece_index: u32,
        output: &mut [u8],
    ) -> anyhow::Result<()> {
        let piece_length: u32 = metainfo.piece_length.try_into().unwrap();
        ensure!(
            output.len() as u32 == piece_length,
            "Output buffer length does not match piece length"
        );

        // 1. Receive bitfield message
        // TODO: Decode available pieces from bitfield.payload
        self.read_message_exact(PeerMessageType::Bitfield)?;

        // 2. Send interested message
        let interested_msg = PeerMessage {
            len: 1,
            msg_type: PeerMessageType::Interested,
            payload: vec![],
        };
        self.send_message(&interested_msg)?;

        // 3. Receive unchoke message
        // TODO: Handle choke/unchoke state properly
        self.read_message_exact(PeerMessageType::Unchoke)?;

        // 4. Request piece by blocks
        let mut requests: Vec<PeerMessage> = Vec::new();
        let mut begin: u32 = 0;

        while begin < piece_length {
            let request_length = std::cmp::min(1 << 14, piece_length - begin);
            let mut payload = Vec::with_capacity(12);
            payload.extend_from_slice(&piece_index.to_be_bytes());
            payload.extend_from_slice(&begin.to_be_bytes());
            payload.extend_from_slice(&request_length.to_be_bytes());

            let request_msg = PeerMessage {
                len: 13,
                msg_type: PeerMessageType::Request,
                payload,
            };
            requests.push(request_msg);

            begin += request_length;
        }

        for request in requests.iter() {
            println!(
                "Sending request: piece_index=0, begin={}",
                u32::from_be_bytes(request.payload[4..8].try_into().unwrap())
            );
            self.send_message(request)?;
            let piece_msg = self.read_message_exact(PeerMessageType::Piece)?;
            // TODO: Use recv_index
            //let recv_index = u32::from_be_bytes(piece_msg.payload[0..4].try_into().unwrap());
            let recv_begin =
                u32::from_be_bytes(piece_msg.payload[4..8].try_into().unwrap()) as usize;
            output[recv_begin..recv_begin + (piece_msg.len - 9) as usize]
                .copy_from_slice(&piece_msg.payload[8..]);
        }

        // 5. Validate piece hash
        let piece_hash = (&metainfo).get_piece_hash_bytes(piece_index as usize);
        let downloaded_piece_hash = hash::sha1(output);
        ensure!(
            piece_hash == downloaded_piece_hash.as_slice(),
            "Piece hash mismatch for piece index {}",
            piece_index
        );
        println!("Piece {} downloaded and verified successfully", piece_index);
        Ok(())
    }

    fn read_message_exact(
        self: &mut Self,
        expected_type: PeerMessageType,
    ) -> anyhow::Result<PeerMessage> {
        let message = self.read_message()?;
        if message.msg_type == PeerMessageType::KeepAlive {
            println!("Received keep-alive message, reading next message");
            return self.read_message_exact(expected_type);
        }
        ensure!(
            message.msg_type == expected_type,
            "Expected message type {:?}, got {:?}",
            expected_type,
            message.msg_type
        );
        Ok(message)
    }

    fn read_message(self: &mut Self) -> anyhow::Result<PeerMessage> {
        // Message format:
        // <length prefix><message ID><payload>
        // length prefix is 4 bytes big-endian integer
        let mut length_buf = [0u8; 4];
        self.stream.read_exact(&mut length_buf)?;
        let length = u32::from_be_bytes(length_buf);

        if length == 0 {
            println!("Received keep-alive message");
            // Keep-alive message
            return Ok(PeerMessage {
                len: 0,
                msg_type: PeerMessageType::KeepAlive,
                payload: vec![],
            });
        }

        let mut message_id_buf = [0u8; 1];
        self.stream.read_exact(&mut message_id_buf)?;
        let message_id = message_id_buf[0];

        let payload_length = length - 1;
        let mut payload_buf = vec![0u8; payload_length as usize];
        self.stream.read_exact(&mut payload_buf)?;

        let msg_type = match message_id {
            0 => PeerMessageType::Choke,
            1 => PeerMessageType::Unchoke,
            2 => PeerMessageType::Interested,
            3 => PeerMessageType::NotInterested,
            4 => PeerMessageType::Have,
            5 => PeerMessageType::Bitfield,
            6 => PeerMessageType::Request,
            7 => PeerMessageType::Piece,
            8 => PeerMessageType::Cancel,
            _ => anyhow::bail!("Unknown message type: {}", message_id),
        };

        println!(
            "Received message: len={}, type={:?}, payload_len={}",
            length,
            msg_type,
            payload_buf.len()
        );

        Ok(PeerMessage {
            len: length,
            msg_type,
            payload: payload_buf,
        })
    }

    fn send_message(&mut self, message: &PeerMessage) -> anyhow::Result<()> {
        // Message format:
        // <length prefix><message ID><payload>
        // length prefix is 4 bytes big-endian integer
        let mut buf = Vec::with_capacity(4 + 1 + message.payload.len());
        buf.extend_from_slice(&message.len.to_be_bytes());
        buf.push(message.msg_type as u8);
        buf.extend_from_slice(&message.payload);
        self.stream.write_all(&buf)?;
        Ok(())
    }
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
