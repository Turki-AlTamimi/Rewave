use rewave_core::pairing::store::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

fn tempfile_dir() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("rewave-test-{}-{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn schema_matches_frozen_json() {
    let dir = tempfile_dir();
    let mut s = PairingStore::open(&dir.join("pairings.json")).unwrap();
    s.upsert(Pairing {
        peer_id: "b66d0e4c94f4d364".into(),
        fingerprint: "b66d0e4c94f4d364".into(),
        pairing_key: hex::encode([7u8; 32]),
        name: "Tab".into(),
        key_id: "c134ef58cb87e775".into(),
    })
    .unwrap();
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("pairings.json")).unwrap()).unwrap();
    let row = &raw["b66d0e4c94f4d364"];
    assert_eq!(row["fingerprint"], "b66d0e4c94f4d364");
    assert_eq!(row["pairing_key"], hex::encode([7u8; 32]));
    assert_eq!(row["name"], "Tab");
    assert_eq!(row["key_id"], "c134ef58cb87e775");
    let s2 = PairingStore::open(&dir.join("pairings.json")).unwrap();
    assert!(s2.find_by_key_id("c134ef58cb87e775").is_some());
}

#[test]
fn open_missing_file_starts_empty_and_creates_on_write() {
    let dir = tempfile_dir();
    let path = dir.join("nested").join("pairings.json");
    let mut s = PairingStore::open(&path).unwrap();
    assert!(s.find_by_peer_id("nope").is_none());
    s.upsert(Pairing {
        peer_id: "aa".into(),
        fingerprint: "aa".into(),
        pairing_key: hex::encode([1u8; 32]),
        name: "A".into(),
        key_id: "bb".into(),
    })
    .unwrap();
    assert!(path.exists());
    let s2 = PairingStore::open(&path).unwrap();
    assert_eq!(s2.find_by_peer_id("aa").unwrap().name, "A");
}

#[test]
fn upsert_overwrites_and_remove_deletes() {
    let dir = tempfile_dir();
    let path = dir.join("pairings.json");
    let mut s = PairingStore::open(&path).unwrap();
    for name in ["one", "two"] {
        s.upsert(Pairing {
            peer_id: "pp".into(),
            fingerprint: "pp".into(),
            pairing_key: hex::encode([2u8; 32]),
            name: name.into(),
            key_id: "kk".into(),
        })
        .unwrap();
    }
    assert_eq!(s.find_by_peer_id("pp").unwrap().name, "two");
    s.remove("pp").unwrap();
    assert!(s.find_by_peer_id("pp").is_none());
    let s2 = PairingStore::open(&path).unwrap();
    assert!(s2.find_by_peer_id("pp").is_none());
}

#[test]
fn no_tmp_file_left_behind() {
    let dir = tempfile_dir();
    let path = dir.join("pairings.json");
    let mut s = PairingStore::open(&path).unwrap();
    s.upsert(Pairing {
        peer_id: "pp".into(),
        fingerprint: "pp".into(),
        pairing_key: hex::encode([3u8; 32]),
        name: "n".into(),
        key_id: "kk".into(),
    })
    .unwrap();
    assert!(!dir.join("pairings.json.tmp").exists());
}
