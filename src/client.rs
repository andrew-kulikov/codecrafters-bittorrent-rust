use std::net::Ipv4Addr;

use reqwest;

use crate::bencode;
use crate::utils::{self, RawBytesExt};

pub struct TrackerRequest {
    pub info_hash: Vec<u8>,
    pub peer_id: String,
    pub port: u16,
    pub uploaded: u64,
    pub downloaded: u64,
    pub left: u64,
    pub compact: u8,
}

pub struct TrackerResponse {
    pub interval: u32,
    pub peers: Vec<Peer>,
}

pub struct Peer {
    pub ip: Ipv4Addr,
    pub port: u16,
}

impl std::fmt::Display for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.ip, self.port)
    }
}

pub fn get_tracker(
    announce_url: String,
    request: TrackerRequest,
) -> Result<TrackerResponse, Box<dyn std::error::Error>> {
    let url = format!(
        "{}?info_hash={}&peer_id={}&port={}&uploaded={}&downloaded={}&left={}&compact={}",
        announce_url,
        utils::url_encode(&request.info_hash),
        request.peer_id,
        request.port,
        request.uploaded,
        request.downloaded,
        request.left,
        request.compact
    );
    println!("Tracker URL: {}", url);

    let response = reqwest::blocking::get(&url)?.bytes()?;
    let response_str = String::from_utf8_lossy(&response);
    println!("Tracker Response: {}", response_str);

    let parsed_response = bencode::parse_bytes(response.to_vec());
    println!("Parsed Response: {:?}", parsed_response);

    let result = TrackerResponse {
        interval: parsed_response
            .get("interval")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        peers: parsed_response
            .get("peers")
            .and_then(|v| v.as_str())
            .map(parse_peers)
            .unwrap_or(vec![]),
    };
    Ok(result)
}

pub fn parse_peers(peers: &str) -> Vec<Peer> {
    let bytes = peers.to_raw_bytes();
    let mut result = Vec::new();

    for chunk in bytes.chunks(6) {
        if chunk.len() < 6 {
            panic!("Invalid peers binary string");
        }
        // First 4 bytes are IP address, last 2 bytes are port
        result.push(Peer {
            ip: Ipv4Addr::new(chunk[0], chunk[1], chunk[2], chunk[3]),
            port: ((chunk[4] as u16) << 8) | (chunk[5] as u16),
        });
    }

    result
}
