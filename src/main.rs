use codecrafters_bittorrent::{
    bencode,
    download::{manager::DownloadManager, queue::PieceQueue, worker::PeerWorker},
    peer,
    torrent::{self, MagnetLink},
    tracker::{self, Peer},
    utils::RawBytesExt,
};
use std::env;
use std::sync::Arc;

const PEER_ID: &str = "-CT0001-123456789012";

fn main() {
    let args: Vec<String> = env::args().collect();
    let command = &args[1];

    if command == "decode" {
        // decode <bencoded string>
        let encoded_value = &args[2];
        let decoded_value = bencode::parse_string(encoded_value);
        println!("{}", decoded_value.to_string());
    } else if command == "info" {
        // info <metainfo file>
        print_torrent_info(&args[2]);
    } else if command == "peers" {
        // peers <metainfo file>
        request_tracker_peers(&args[2]);
    } else if command == "handshake" {
        // handshake <metainfo file> <peer address>
        peer_handshake(&args[2], args[3].parse().expect("Invalid peer address"));
    } else if command == "download_piece" {
        // download_piece -o <output file> <metainfo file> <piece index>
        download_piece(
            &args[3],
            &args[4],
            args[5].parse().expect("Invalid piece index"),
        );
    } else if command == "download" {
        // download -o <output file> <metainfo file>
        download_file(&args[3], &args[4]);
    } else if command == "magnet_parse" {
        // magnet_parse <magnet link>
        parse_magnet_link(&args[2]);
    } else {
        println!("unknown command: {}", args[1])
    }
}

fn print_torrent_info(metainfo_file_path: &str) {
    let info = torrent::parse_metainfo_file(metainfo_file_path);

    println!("Tracker URL: {}", info.announce);
    println!("Length: {}", info.length);
    println!("Info Hash: {}", info.get_info_hash_hex());
    println!("Piece Length: {}", info.piece_length);
    println!("Piece Hashes:");
    for hash in info.get_piece_hashes() {
        println!("{}", hash);
    }
}

fn request_tracker_peers(metainfo_file_path: &str) {
    let info = torrent::parse_metainfo_file(metainfo_file_path);

    let tracker_request = tracker::TrackerRequest {
        info_hash: info.info_hash.clone(),
        peer_id: PEER_ID.to_string(),
        port: 6881,
        uploaded: 0,
        downloaded: 0,
        left: info.length,
        compact: 1,
    };

    let tracker_response =
        tracker::announce(info.announce, tracker_request).expect("Failed to get tracker response");

    for peer in tracker_response.peers {
        println!("{}", peer);
    }
}

fn peer_handshake(metainfo_file_path: &str, peer: Peer) {
    let meta = torrent::parse_metainfo_file(metainfo_file_path);
    let request = peer::HandshakeRequest {
        pstr: "BitTorrent protocol".to_string(),
        reserved: [0u8; 8],
        info_hash: meta.info_hash.clone(),
        peer_id: PEER_ID.to_raw_bytes(),
    };
    let connection = peer::PeerConnection::new(peer.clone(), &request)
        .expect("Failed to establish peer connection");
    let peer_id_hex = hex::encode(&connection.peer_id.unwrap());
    println!("Peer ID: {}", peer_id_hex);
}

fn download_piece(output_file_path: &str, metainfo_file_path: &str, piece_index: u32) {
    // 1. Parse metainfo file
    let meta = Arc::new(torrent::parse_metainfo_file(metainfo_file_path));

    // 2. Announce to tracker and get peers
    let peers = {
        let tracker_request = tracker::TrackerRequest {
            info_hash: meta.info_hash.clone(),
            peer_id: PEER_ID.to_string(),
            port: 6881,
            uploaded: 0,
            downloaded: 0,
            left: meta.length,
            compact: 1,
        };

        let tracker_response = tracker::announce(meta.announce.clone(), tracker_request)
            .expect("Failed to get tracker response");
        tracker_response.peers
    };

    // 3. Setup queue
    let queue = Arc::new(PieceQueue::empty());
    queue.push(piece_index);
    // Signal that no more pieces will be added, so the worker can exit after downloading
    queue.shutdown();

    // 4. Start worker
    let peer = peers.first().expect("No peers available").clone();

    // Use current directory as temp output
    let output_dir = ".";

    let mut worker = PeerWorker::new(
        peer,
        meta.clone(),
        queue,
        PEER_ID.to_string(),
        output_dir.to_string(),
    );

    worker.run().expect("Worker failed");

    // 5. Move/Rename file to desired output
    let temp_path = format!("piece_{}", piece_index);
    std::fs::rename(temp_path, output_file_path).expect("Failed to rename output file");
}

fn download_file(output_file_path: &str, metainfo_file_path: &str) {
    let meta = torrent::parse_metainfo_file(metainfo_file_path);
    let client_id = PEER_ID.to_string();
    let manager = DownloadManager::new(meta, client_id, output_file_path.to_string());
    manager.download().expect("Download failed");
}

fn parse_magnet_link(link: &str) {
    let magnet_link = MagnetLink::parse(link).expect("Failed to parse magnet link");

    // For now assume there is only one tracker
    let tracker_url = magnet_link
        .trackers
        .iter()
        .next()
        .map(|url| url.to_string())
        .expect("No trackers found");

    // And only one info hash
    let info_hash_hex = magnet_link
        .exact_topics
        .iter()
        .next()
        .map(|topic| topic.get_hash().expect("Unsupported scheme").to_string())
        .expect("No info hash found");

    println!("Tracker URL: {}", tracker_url);
    println!("Info Hash: {}", info_hash_hex);
}
