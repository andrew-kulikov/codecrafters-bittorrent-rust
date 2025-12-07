use std::net::{Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::str::FromStr;

use reqwest;

use crate::utils::RawBytesExt;
use crate::{bencode, utils};

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

impl Clone for Peer {
    fn clone(&self) -> Self {
        Peer {
            ip: self.ip,
            port: self.port,
        }
    }
}

impl FromStr for Peer {
    type Err = std::net::AddrParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(':').collect();
        let ip = parts[0].parse::<Ipv4Addr>()?;
        let port = parts[1].parse::<u16>().unwrap_or(6881);
        Ok(Peer { ip, port })
    }
}

impl ToSocketAddrs for Peer {
    type Iter = std::option::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self) -> std::io::Result<Self::Iter> {
        let socket_addr = SocketAddr::new(std::net::IpAddr::V4(self.ip), self.port);
        Ok(Some(socket_addr).into_iter())
    }
}

impl std::fmt::Display for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.ip, self.port)
    }
}

pub fn announce(
    announce_url: String,
    request: TrackerRequest,
) -> anyhow::Result<TrackerResponse> {
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

    let response = reqwest::blocking::get(&url)?.bytes()?;
    let parsed_response = bencode::parse_bytes(response.to_vec());

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

fn parse_peers(peers: &str) -> Vec<Peer> {
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

#[cfg(test)]
mod tests {
    use crate::utils::RawStringExt;

    use super::*;

    #[test]
    fn parse_peers_valid() {
        let peers_bytes = b"\x7F\x00\x00\x01\x1A\xE1\xC0\xA8\x01\x68\x1A\xE2";
        let peers = parse_peers(peers_bytes.to_raw_string().as_str());

        assert_eq!(peers.len(), 2);
        assert_eq!(peers[0].ip, Ipv4Addr::new(127, 0, 0, 1));
        assert_eq!(peers[0].port, 6881);
        assert_eq!(peers[1].ip, Ipv4Addr::new(192, 168, 1, 104));
        assert_eq!(peers[1].port, 6882);
    }

    #[test]
    #[should_panic(expected = "Invalid peers binary string")]
    fn parse_peers_panics_on_invalid_length() {
        let peers_bytes = b"\x7F\x00\x00\x01\x1A";
        parse_peers(peers_bytes.to_raw_string().as_str());
    }
}
