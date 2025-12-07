use codecrafters_bittorrent::torrent::TorrentMetainfo;

#[test]
fn test_parse_torrent_file() {
    let description = TorrentMetainfo::parse("sample.torrent");
    let piece_hashes = description.get_piece_hashes();

    assert_eq!(
        description.announce,
        "http://bittorrent-test-tracker.codecrafters.io/announce"
    );
    assert_eq!(description.length, 92063);
    assert_eq!(description.piece_length, 32768);
    assert_eq!(
        description.get_info_hash_hex(),
        "d69f91e6b2ae4c542468d1073a71d4ea13879a7f"
    );
    assert_eq!(description.pieces.len(), 60);
    assert_eq!(piece_hashes.len(), 3);
    assert_eq!(piece_hashes[0], "e876f67a2a8886e8f36b136726c30fa29703022d");
    assert_eq!(piece_hashes[1], "6e2275e604a0766656736e81ff10b55204ad8d35");
    assert_eq!(piece_hashes[2], "f00d937a0213df1982bc8d097227ad9e909acc17");
}
