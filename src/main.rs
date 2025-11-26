mod bencode_encoder;
mod bencode_parser;
mod utils;

use std::env;
use hex;

// Available if you need it!
// use serde_bencode

fn calculate_piece_hashes(pieces_info_str: String) -> Vec<String> {
    let pieces_bytes = utils::raw_string_to_bytes(&pieces_info_str);
    let mut hashes = Vec::new();
    for chunk in pieces_bytes.chunks(20) {
        let hash = hex::encode(chunk);
        hashes.push(hash);
    }
    hashes
}

fn print_torrent_info(info: &serde_json::Value) {
    let announce = info
        .get("announce")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let info_section = info
        .get("info")
        .expect("Missing 'info' dictionary in torrent file");

    let length = info_section
        .get("length")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let encoded_info = bencode_encoder::encode(info_section);
    let info_hash = utils::compute_sha1_hash(&encoded_info);

    let piece_length = info_section
        .get("piece length")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let pieces_info_str = info_section
        .get("pieces")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let piece_hashes = calculate_piece_hashes(pieces_info_str);

    println!("Tracker URL: {}", announce);
    println!("Length: {}", length);
    println!("Info Hash: {}", info_hash);
    println!("Piece Length: {}", piece_length);
    println!("Piece Hashes:");
    for hash in piece_hashes {
        println!("{}", hash);
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let command = &args[1];

    if command == "decode" {
        let encoded_value = &args[2];
        let decoded_value = bencode_parser::parse_string(encoded_value);
        println!("{}", decoded_value.to_string());
    } else if command == "info" {
        let torrent_file_path = &args[2];
        let torrent_bytes = std::fs::read(torrent_file_path).expect("Failed to read torrent file");
        let decoded_value = bencode_parser::parse_bytes(torrent_bytes);
        print_torrent_info(&decoded_value);
    } else {
        println!("unknown command: {}", args[1])
    }
}
