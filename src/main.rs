mod parser;

use std::env;

// Available if you need it!
// use serde_bencode

struct TorrentDesc {
    announce: String,
    info: TorrentInfo,
}

struct TorrentInfo {
    length: u64,
    name: String,
}

fn print_torrent_info(info: &serde_json::Value) {
    let info = TorrentDesc {
        announce: info
            .get("announce")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        info: TorrentInfo {
            length: info
                .get("info")
                .and_then(|info_dict| info_dict.get("length"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            name: info
                .get("info")
                .and_then(|info_dict| info_dict.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        },
    };
    println!("Tracker URL: {}", info.announce);
    println!("Length: {}", info.info.length);
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let command = &args[1];

    if command == "decode" {
        let encoded_value = &args[2];
        let decoded_value = parser::parse_string(encoded_value);
        println!("{}", decoded_value.to_string());
    } else if command == "info" {
        let torrent_file_path = &args[2];
        let torrent_bytes = std::fs::read(torrent_file_path).expect("Failed to read torrent file");
        let decoded_value = parser::parse_bytes(torrent_bytes);
        print_torrent_info(&decoded_value);
    } else {
        println!("unknown command: {}", args[1])
    }
}
