use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pairing {
    pub peer_id: String,
    pub fingerprint: String,
    pub pairing_key: String,
    pub name: String,
    pub key_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Row {
    fingerprint: String,
    pairing_key: String,
    name: String,
    key_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub struct PairingStore {
    path: PathBuf,
    map: HashMap<String, Pairing>,
}

impl PairingStore {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let map = match std::fs::read_to_string(path) {
            Ok(text) => {
                let rows: HashMap<String, Row> = serde_json::from_str(&text)?;
                rows.into_iter()
                    .map(|(peer_id, r)| {
                        (
                            peer_id.clone(),
                            Pairing {
                                peer_id,
                                fingerprint: r.fingerprint,
                                pairing_key: r.pairing_key,
                                name: r.name,
                                key_id: r.key_id,
                            },
                        )
                    })
                    .collect()
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self { path: path.to_path_buf(), map })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn upsert(&mut self, p: Pairing) -> Result<(), StoreError> {
        self.map.insert(p.peer_id.clone(), p);
        self.save()
    }

    pub fn remove(&mut self, peer_id: &str) -> Result<(), StoreError> {
        self.map.remove(peer_id);
        self.save()
    }

    pub fn find_by_key_id(&self, key_id: &str) -> Option<&Pairing> {
        self.map.values().find(|p| p.key_id == key_id)
    }

    pub fn find_by_peer_id(&self, peer_id: &str) -> Option<&Pairing> {
        self.map.get(peer_id)
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn first(&self) -> Option<&Pairing> {
        self.map.values().next()
    }

    fn save(&self) -> Result<(), StoreError> {
        let rows: HashMap<&str, Row> = self
            .map
            .iter()
            .map(|(k, p)| {
                (
                    k.as_str(),
                    Row {
                        fingerprint: p.fingerprint.clone(),
                        pairing_key: p.pairing_key.clone(),
                        name: p.name.clone(),
                        key_id: p.key_id.clone(),
                    },
                )
            })
            .collect();
        let text = serde_json::to_string_pretty(&rows)?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}
