use std::convert::TryInto;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::ensure;

use super::message::{has_extension_support, HandshakeRequest, PeerMessageType};
use crate::tracker::Peer;
use crate::utils::{log, RawStringExt};

/// Events produced by the reader thread for a peer connection.
#[derive(Debug)]
pub enum PeerEvent {
    HandshakeComplete {
        peer_id: Option<Vec<u8>>,
        reserved: [u8; 8],
        extension_supported: bool,
    },
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have(u32),
    Bitfield(Vec<u8>),
    Request {
        index: u32,
        begin: u32,
        length: u32,
    },
    Piece {
        index: u32,
        begin: u32,
        data: Vec<u8>,
    },
    Cancel {
        index: u32,
        begin: u32,
        length: u32,
    },
    Extended {
        ext_id: u8,
        payload: Vec<u8>,
    },
    Unknown {
        id: u8,
        payload: Vec<u8>,
    },
    IoError(String),
}

impl PeerEvent {
    pub fn print_simple(&self) -> String {
        match self {
            PeerEvent::Piece { index, begin, data } => {
                format!(
                    "Piece {{ index: {}, begin: {}, data_len: {} }}",
                    index,
                    begin,
                    data.len()
                )
            }
            _ => format!("{:?}", self),
        }
    }
}

/// Commands sent to the writer thread.
#[derive(Debug, Clone)]
pub enum PeerCommand {
    KeepAlive,
    Interested,
    NotInterested,
    Request { index: u32, begin: u32, length: u32 },
    Cancel { index: u32, begin: u32, length: u32 },
    Extended { ext_id: u8, payload: Vec<u8> },
}

#[derive(Debug, Clone, Copy)]
pub struct PeerStateSnapshot {
    pub choked: bool,
    pub interested: bool,
    pub peer_interested: bool,
    pub extension_supported: bool,
}

pub struct PeerConnection {
    _peer: Peer,
    event_rx: mpsc::Receiver<PeerEvent>,
    shutdown: Arc<AtomicBool>,
    pub peer_id: Option<Vec<u8>>,
    _reserved: [u8; 8],
    state: Arc<Mutex<PeerStateSnapshot>>,
    stream: Arc<Mutex<TcpStream>>,
}

impl PeerConnection {
    pub fn new(addr: Peer, req: &HandshakeRequest) -> anyhow::Result<PeerConnection> {
        log::debug("PeerConnection", &format!("Connecting to {}", addr));
        let mut stream = TcpStream::connect(addr.clone())?;
        stream.set_read_timeout(Some(Duration::from_secs(60)))?;
        stream.set_write_timeout(Some(Duration::from_secs(30)))?;

        // Handshake format: <pstrlen><pstr><reserved><info_hash><peer_id>
        log::debug("PeerConnection", "Sending handshake request");
        let payload = req.as_bytes()?;
        stream.write_all(&payload)?;

        // Response: 1 + pstrlen + pstr + 8 + 20 + 20
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

        let supports_ext = has_extension_support(&reserved);
        log::debug(
            "PeerConnection",
            &format!(
                "Handshake successful with peer {} (extensions: {})",
                addr, supports_ext
            ),
        );

        // Split stream into read and write halves to avoid mutex contention between reader and writer.
        let stream_read = stream.try_clone()?;
        let stream_write = Arc::new(Mutex::new(stream));
        let shutdown = Arc::new(AtomicBool::new(false));

        let (event_tx, event_rx) = mpsc::channel::<PeerEvent>();

        // Seed initial state
        let state = Arc::new(Mutex::new(PeerStateSnapshot {
            choked: true,
            interested: false,
            peer_interested: false,
            extension_supported: supports_ext,
        }));

        // Send initial handshake event so consumers know reserved bits/peer_id.
        event_tx
            .send(PeerEvent::HandshakeComplete {
                peer_id: Some(peer_id.clone()),
                reserved,
                extension_supported: supports_ext,
            })
            .ok();

        // Reader thread: continuously read and emit events.
        {
            let stream_read = stream_read;
            let shutdown = Arc::clone(&shutdown);
            let event_tx = event_tx.clone();
            let state = Arc::clone(&state);
            thread::spawn(move || {
                let stream_read = Arc::new(Mutex::new(stream_read));
                while !shutdown.load(Ordering::Relaxed) {
                    match read_one_message(&stream_read) {
                        Ok(evt) => {
                            update_state(&state, &evt);
                            if event_tx.send(evt).is_err() {
                                break;
                            }
                        }
                        Err(err) => {
                            let _ = event_tx.send(PeerEvent::IoError(format!("{}", err)));
                            break;
                        }
                    }
                }
            });
        }

        // Drop event_tx clone so that when reader thread closes it, channel is cleanly shut down.
        drop(event_tx);

        Ok(PeerConnection {
            _peer: addr,
            event_rx,
            shutdown,
            peer_id: Some(peer_id),
            _reserved: reserved,
            state,
            stream: stream_write,
        })
    }

    pub fn send(&self, cmd: PeerCommand) -> anyhow::Result<()> {
        write_one_message(&self.stream, cmd)
    }

    pub fn next_event(&self) -> Option<PeerEvent> {
        self.event_rx.recv().ok()
    }

    pub fn state(&self) -> PeerStateSnapshot {
        *self.state.lock().unwrap()
    }

    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }

    pub fn extension_supported(&self) -> bool {
        self.state.lock().unwrap().extension_supported
    }
}

fn update_state(state: &Arc<Mutex<PeerStateSnapshot>>, evt: &PeerEvent) {
    let mut s = state.lock().unwrap();
    match evt {
        PeerEvent::Choke => s.choked = true,
        PeerEvent::Unchoke => s.choked = false,
        PeerEvent::Interested => s.peer_interested = true,
        PeerEvent::NotInterested => s.peer_interested = false,
        PeerEvent::HandshakeComplete {
            extension_supported,
            ..
        } => {
            s.extension_supported = *extension_supported;
        }
        _ => {}
    }
}

fn read_one_message(stream: &Arc<Mutex<TcpStream>>) -> anyhow::Result<PeerEvent> {
    let mut stream = stream.lock().unwrap();
    let mut length_buf = [0u8; 4];
    stream.read_exact(&mut length_buf)?;
    let length = u32::from_be_bytes(length_buf);
    log::debug(
        "PeerConnection",
        &format!("Reading message of length {}", length),
    );

    if length == 0 {
        return Ok(PeerEvent::KeepAlive);
    }

    let mut message_id_buf = [0u8; 1];
    stream.read_exact(&mut message_id_buf)?;
    let message_id = message_id_buf[0];

    let payload_length = length - 1;
    let mut payload_buf = vec![0u8; payload_length as usize];
    stream.read_exact(&mut payload_buf)?;

    let evt = match message_id {
        0 => PeerEvent::Choke,
        1 => PeerEvent::Unchoke,
        2 => PeerEvent::Interested,
        3 => PeerEvent::NotInterested,
        4 => {
            let idx = u32::from_be_bytes(payload_buf[0..4].try_into()?);
            PeerEvent::Have(idx)
        }
        5 => PeerEvent::Bitfield(payload_buf),
        6 => {
            let index = u32::from_be_bytes(payload_buf[0..4].try_into()?);
            let begin = u32::from_be_bytes(payload_buf[4..8].try_into()?);
            let length = u32::from_be_bytes(payload_buf[8..12].try_into()?);
            PeerEvent::Request {
                index,
                begin,
                length,
            }
        }
        7 => {
            let index = u32::from_be_bytes(payload_buf[0..4].try_into()?);
            let begin = u32::from_be_bytes(payload_buf[4..8].try_into()?);
            let data = payload_buf[8..].to_vec();
            PeerEvent::Piece { index, begin, data }
        }
        8 => {
            let index = u32::from_be_bytes(payload_buf[0..4].try_into()?);
            let begin = u32::from_be_bytes(payload_buf[4..8].try_into()?);
            let length = u32::from_be_bytes(payload_buf[8..12].try_into()?);
            PeerEvent::Cancel {
                index,
                begin,
                length,
            }
        }
        20 => {
            let ext_id = payload_buf.first().copied().unwrap_or(0);
            let payload = if payload_buf.is_empty() {
                Vec::new()
            } else {
                payload_buf[1..].to_vec()
            };
            // TODO: Handle ext_id == 0 (extension handshake) when implementing LTEP.
            PeerEvent::Extended { ext_id, payload }
        }
        other => PeerEvent::Unknown {
            id: other,
            payload: payload_buf,
        },
    };

    log::debug(
        "PeerConnection",
        &format!("Received event: {}", evt.print_simple()),
    );

    Ok(evt)
}

fn write_one_message(stream: &Arc<Mutex<TcpStream>>, cmd: PeerCommand) -> anyhow::Result<()> {
    log::debug("PeerConnection", &format!("Sending command: {:?}", cmd));
    let mut stream = stream.lock().unwrap();
    match cmd {
        PeerCommand::KeepAlive => {
            stream.write_all(&0u32.to_be_bytes())?;
        }
        PeerCommand::Interested => {
            stream.write_all(&1u32.to_be_bytes())?;
            stream.write_all(&[PeerMessageType::Interested as u8])?;
        }
        PeerCommand::NotInterested => {
            stream.write_all(&1u32.to_be_bytes())?;
            stream.write_all(&[PeerMessageType::NotInterested as u8])?;
        }
        PeerCommand::Request {
            index,
            begin,
            length,
        } => {
            let mut buf = Vec::with_capacity(4 + 1 + 12);
            buf.extend_from_slice(&13u32.to_be_bytes());
            buf.push(PeerMessageType::Request as u8);
            buf.extend_from_slice(&index.to_be_bytes());
            buf.extend_from_slice(&begin.to_be_bytes());
            buf.extend_from_slice(&length.to_be_bytes());
            stream.write_all(&buf)?;
        }
        PeerCommand::Cancel {
            index,
            begin,
            length,
        } => {
            let mut buf = Vec::with_capacity(4 + 1 + 12);
            buf.extend_from_slice(&13u32.to_be_bytes());
            buf.push(PeerMessageType::Cancel as u8);
            buf.extend_from_slice(&index.to_be_bytes());
            buf.extend_from_slice(&begin.to_be_bytes());
            buf.extend_from_slice(&length.to_be_bytes());
            stream.write_all(&buf)?;
        }
        PeerCommand::Extended { ext_id, payload } => {
            let mut buf = Vec::with_capacity(4 + 1 + 1 + payload.len());
            let len = 2 + payload.len() as u32; // msg_id + ext_id + payload
            buf.extend_from_slice(&len.to_be_bytes());
            buf.push(PeerMessageType::Extended as u8);
            buf.push(ext_id);
            buf.extend_from_slice(&payload);
            stream.write_all(&buf)?;
        }
    }
    Ok(())
}
