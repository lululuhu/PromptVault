//! Resolve a `ref:path` prompt spec to its template text.
//!
//! Supported forms:
//!   - `path`                          — file at repo root (working tree)
//!   - `HEAD:path`                     — tracked path at the current HEAD commit
//!   - `<branch>:path`                 — tracked path at the tip of a branch
//!   - `<tag>:path`                    — tracked path at the commit a tag points to
//!   - `<hash-or-prefix>:path`         — tracked path at a specific commit

use std::fs;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};

use crate::core::objects::{self, TreeEntry};
use crate::core::refs;
use crate::core::repository::Repo;

/// Load a prompt template from a `ref:path` spec (or a plain path).
pub fn load_prompt(repo: &Repo, spec: &str) -> Result<String> {
    if let Some((r, path)) = split_ref_path(spec) {
        let path = path.replace('\\', "/");
        let commit = resolve_ref_to_commit(repo, r)?
            .with_context(|| format!("could not resolve ref '{r}'"))?;
        let commit_obj = objects::read_commit(&repo.pv_dir, &commit)?;
        let entries = objects::read_tree(&repo.pv_dir, &commit_obj.tree)?;
        let entry = entries
            .iter()
            .find(|e| e.path == path)
            .with_context(|| format!("path '{path}' not tracked at {r}"))?;
        let blob = objects::read_object(&repo.pv_dir, &entry.hash)?;
        return Ok(String::from_utf8_lossy(&blob.data).to_string());
    }

    // Plain hash prefix (no colon) — treat as a blob hash.
    if spec.len() >= 4 && spec.chars().all(|c| c.is_ascii_hexdigit()) && spec.len() != 64 {
        if let Some(text) = try_read_blob_by_prefix(repo, spec)? {
            return Ok(text);
        }
    }
    if spec.len() == 64 {
        let obj = objects::read_object(&repo.pv_dir, spec)?;
        return Ok(String::from_utf8_lossy(&obj.data).to_string());
    }

    // Otherwise: file path relative to the repo root.
    let abs = repo.root.join(spec);
    let data = fs::read_to_string(&abs)
        .with_context(|| format!("cannot read prompt: {spec}"))?;
    Ok(data)
}

/// Split `HEAD:path` / `branch:path` into `(ref, path)`. Returns None if the
/// string contains no colon or looks like a plain path / hash.
fn split_ref_path(spec: &str) -> Option<(&str, &str)> {
    let (r, p) = spec.split_once(':')?;
    if r.is_empty() || p.is_empty() {
        return None;
    }
    // A plain Windows-like path (C:\...) won't reach here on unix; and a 64-char
    // hash has no colon, so this is safe.
    Some((r, p))
}

fn resolve_ref_to_commit(repo: &Repo, spec: &str) -> Result<Option<String>> {
    if spec == "HEAD" {
        return repo.head_commit();
    }
    if let Some(h) = refs::resolve_tag(&repo.pv_dir, spec)? {
        return Ok(Some(h));
    }
    if let Some(h) = refs::resolve_branch(&repo.pv_dir, spec)? {
        return Ok(Some(h));
    }
    // Hash or prefix.
    if spec.len() >= 4 && spec.chars().all(|c| c.is_ascii_hexdigit()) {
        if let Some(h) = find_object_by_prefix(&repo.pv_dir, spec)? {
            return Ok(Some(h));
        }
    }
    Ok(None)
}

fn find_object_by_prefix(pv_dir: &Path, prefix: &str) -> Result<Option<String>> {
    if prefix.len() < 4 {
        return Ok(None);
    }
    let (dir, file_prefix) = prefix.split_at(2);
    let dir_path = pv_dir.join("objects").join(dir);
    if !dir_path.exists() {
        return Ok(None);
    }
    for entry in fs::read_dir(&dir_path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(file_prefix) {
            return Ok(Some(format!("{dir}{name}")));
        }
    }
    Ok(None)
}

fn try_read_blob_by_prefix(repo: &Repo, prefix: &str) -> Result<Option<String>> {
    if let Some(hash) = find_object_by_prefix(&repo.pv_dir, prefix)? {
        let obj = objects::read_object(&repo.pv_dir, &hash)?;
        return Ok(Some(String::from_utf8_lossy(&obj.data).to_string()));
    }
    Ok(None)
}

#[allow(dead_code)]
pub fn list_tree_paths(repo: &Repo, ref_spec: &str) -> Result<Vec<TreeEntry>> {
    let Some(commit) = resolve_ref_to_commit(repo, ref_spec)? else {
        bail!("unknown ref: {ref_spec}");
    };
    let c = objects::read_commit(&repo.pv_dir, &commit)?;
    Ok(objects::read_tree(&repo.pv_dir, &c.tree)?)
}

// Avoid unused-import noise in some feature combos.
#[allow(unused_imports)]
use anyhow as _;
