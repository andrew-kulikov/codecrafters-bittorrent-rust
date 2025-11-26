mod bencode_encoder;
mod bencode_parser;
mod utils;

use std::env;

// Available if you need it!
// use serde_bencode

fn print_torrent_info(info: &serde_json::Value) {
    let announce = info
        .get("announce")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let length = info
        .get("info")
        .and_then(|info_dict| info_dict.get("length"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let encoded_info = bencode_encoder::encode(
        info.get("info")
            .expect("Missing 'info' dictionary in torrent file"),
    );

    let info_hash = utils::compute_sha1_hash(&encoded_info);
    println!("Tracker URL: {}", announce);
    println!("Length: {}", length);
    println!("Info Hash: {}", info_hash);
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
