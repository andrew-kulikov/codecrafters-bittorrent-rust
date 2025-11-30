use std::env;
use codecrafters_bittorrent::{bencode, torrent, tracker::{self, Peer}};

// Available if you need it!
// use serde_bencode

fn main() {
    let args: Vec<String> = env::args().collect();
    let command = &args[1];

    if command == "decode" {
        let encoded_value = &args[2];
        let decoded_value = bencode::parse_string(encoded_value);
        println!("{}", decoded_value.to_string());
    } else if command == "info" {
        print_torrent_info(&args[2]);
    } else if command == "peers" {
        request_tracker_peers(&args[2]);
    } else {
        println!("unknown command: {}", args[1])
    }
}

fn request_tracker_peers(file_path: &str) {
    let info = torrent::parse_metainfo_file(file_path);

    let peer_id = "-CT0001-123456789012".to_string(); // Example peer ID
    let tracker_request = tracker::TrackerRequest {
        info_hash: info.info_hash.clone(),
        peer_id: peer_id.clone(),
        port: 6881,
        uploaded: 0,
        downloaded: 0,
        left: info.length,
        compact: 1,
    };

    let tracker_response = tracker::get_tracker(info.announce, tracker_request)
        .expect("Failed to get tracker response");

    for peer in tracker_response.peers {
        println!("{}", peer);
    }
}

fn print_torrent_info(file_path: &str) {
    let info = torrent::parse_metainfo_file(file_path);

    println!("Tracker URL: {}", info.announce);
    println!("Length: {}", info.length);
    println!("Info Hash: {}", info.get_info_hash_hex());
    println!("Piece Length: {}", info.piece_length);
    println!("Piece Hashes:");
    for hash in info.get_piece_hashes() {
        println!("{}", hash);
    }
}

fn peer_handshake(file_path: &str, peer: Peer) {
    tracker::handshake();
}