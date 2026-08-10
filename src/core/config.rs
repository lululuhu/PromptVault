//! Repository + user configuration.

use std::path::Path;

use anyhow::Result;

/// Resolve the commit author name.
///
/// Priority:
///   1. `PV_AUTHOR` environment variable
///   2. `PV_AUTHOR` in `.pv/config`
///   3. `GIT_AUTHOR_NAME` environment variable
///   4. fallback: `"anonymous"`
pub fn resolve_author(pv_dir: &Path) -> Result<String> {
    if let Ok(a) = std::env::var("PV_AUTHOR") {
        if !a.trim().is_empty() {
            return Ok(a);
        }
    }
    if let Some(a) = read_config(pv_dir)?.get("author") {
        return Ok(a.clone());
    }
    if let Ok(a) = std::env::var("GIT_AUTHOR_NAME") {
        if !a.trim().is_empty() {
            return Ok(a);
        }
    }
    Ok("anonymous".to_string())
}

/// Parse the simple `key = value` config file at `.pv/config`.
pub fn read_config(pv_dir: &Path) -> Result<std::collections::HashMap<String, String>> {
    let mut map = std::collections::HashMap::new();
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
