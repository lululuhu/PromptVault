//! `pv config` — get / set repository configuration values.
//!
//! Reads/writes the simple `key = value` format used by `.pv/config`.
//! Used for `author` and any future settings.

use std::collections::BTreeMap;

use anyhow::{bail, Result};

use crate::core::repository::Repo;
use crate::core::safe;
use crate::ui::printer;

pub fn get(key: &str) -> Result<()> {
    let repo = Repo::find()?;
    let map = read_config_map(&repo.pv_dir)?;
    match map.get(key) {
        Some(v) => println!("{v}"),
        None => bail!("'{key}' is not set"),
    }
    Ok(())
}

pub fn set(key: &str, value: &str) -> Result<()> {
    if key.is_empty() {
        bail!("config key is empty");
    }
    if key.contains('=') || key.contains('\n') {
        bail!("invalid config key: {key:?}");
    }
    if value.contains('\n') {
        bail!("config value cannot contain newlines");
    }
    let repo = Repo::find()?;
    let _lock = repo.lock()?;
    let mut map = read_config_map(&repo.pv_dir)?;
    map.insert(key.to_string(), value.to_string());

    let mut text = String::new();
    text.push_str("# prv repository config\n");
    for (k, v) in &map {
        text.push_str(&format!("{k} = {v}\n"));
    }
    safe::atomic_write(&repo.pv_dir.join("config"), text.as_bytes())?;
    printer::ok(&format!("set {key} = {value}"));
    Ok(())
}

pub fn list() -> Result<()> {
    let repo = Repo::find()?;
    let map = read_config_map(&repo.pv_dir)?;
    if map.is_empty() {
        printer::info("no config set");
        return Ok(());
    }
    for (k, v) in &map {
        println!("{k} = {v}");
    }
    Ok(())
}

/// Read config preserving insertion order (BTreeMap for stable output).
fn read_config_map(pv_dir: &std::path::Path) -> Result<BTreeMap<String, String>> {
    let mut map = BTreeMap::new();
    let path = pv_dir.join("config");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Ok(map);
    };
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    Ok(map)
}
