//! Staging area — a JSON file listing path → blob-hash pairs.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::core::objects::TreeEntry;
use crate::core::safe;

#[derive(Default, Serialize, Deserialize, Clone)]
pub struct Index {
    pub entries: Vec<IndexEntry>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct IndexEntry {
    pub path: String,
    pub hash: String,
}

impl Index {
    pub fn load(pv_dir: &Path) -> Result<Index> {
        let path = pv_dir.join("index.json");
        if !path.exists() {
            return Ok(Index::default());
        }
        let data = std::fs::read_to_string(&path)?;
        let idx: Index = serde_json::from_str(&data).context("failed to parse index.json")?;
        // Defensive: validate every entry. Drop nothing silently — surface errors.
        for e in &idx.entries {
            safe::validate_tree_path(&e.path)
                .with_context(|| format!("index has unsafe path: {:?}", e.path))?;
            if !safe::is_valid_hash(&e.hash) {
                anyhow::bail!("index entry '{}' has invalid hash: {}", e.path, e.hash);
            }
        }
        Ok(idx)
    }

    pub fn save(&self, pv_dir: &Path) -> Result<()> {
        let path = pv_dir.join("index.json");
        let data = serde_json::to_string_pretty(self)?;
        safe::atomic_write(&path, data.as_bytes())
    }

    pub fn add(&mut self, path: &str, hash: &str) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.path == path) {
            e.hash = hash.to_string();
        } else {
            self.entries.push(IndexEntry {
                path: path.to_string(),
                hash: hash.to_string(),
            });
        }
    }

    #[allow(dead_code)]
    pub fn remove(&mut self, path: &str) {
        self.entries.retain(|e| e.path != path);
    }

    #[allow(dead_code)]
    pub fn get(&self, path: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| e.path == path)
            .map(|e| e.hash.as_str())
    }

    pub fn to_tree_entries(&self) -> Vec<TreeEntry> {
        self.entries
            .iter()
            .map(|e| TreeEntry {
                path: e.path.clone(),
                hash: e.hash.clone(),
            })
            .collect()
    }
}

