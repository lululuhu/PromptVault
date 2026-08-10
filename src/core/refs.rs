//! Refs: HEAD pointer and branch tips.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::core::safe;

pub const DEFAULT_BRANCH: &str = "main";

pub fn head_path(pv_dir: &Path) -> PathBuf {
    pv_dir.join("HEAD")
}

pub fn refs_dir(pv_dir: &Path) -> PathBuf {
    pv_dir.join("refs").join("heads")
}

pub fn read_head(pv_dir: &Path) -> Result<Option<String>> {
    let p = head_path(pv_dir);
    if !p.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&p)?;
    let content = content.trim().to_string();
    if content.is_empty() {
        Ok(None)
    } else {
        Ok(Some(content))
    }
}

/// The current branch name, if HEAD is attached to a branch.
pub fn current_branch(pv_dir: &Path) -> Result<Option<String>> {
    if let Some(h) = read_head(pv_dir)? {
        if let Some(name) = h.strip_prefix("ref: refs/heads/") {
            return Ok(Some(name.to_string()));
        }
    }
    Ok(None)
}

/// Resolve HEAD to a concrete commit hash (None if no commits yet).
pub fn resolve_head(pv_dir: &Path) -> Result<Option<String>> {
    let Some(h) = read_head(pv_dir)? else {
        return Ok(None);
    };
    if let Some(name) = h.strip_prefix("ref: refs/heads/") {
        let branch_file = refs_dir(pv_dir).join(name);
        if !branch_file.exists() {
            return Ok(None);
        }
        let hash = fs::read_to_string(&branch_file)?.trim().to_string();
        return Ok(if hash.is_empty() { None } else { Some(hash) });
    }
    // Detached: HEAD holds a hash directly.
    Ok(Some(h))
}

pub fn init_head(pv_dir: &Path, branch: &str) -> Result<()> {
    fs::create_dir_all(refs_dir(pv_dir))?;
    safe::atomic_write(&head_path(pv_dir), format!("ref: refs/heads/{branch}\n").as_bytes())?;
    Ok(())
}

/// Point the current branch (or HEAD if detached) at `commit`.
pub fn update_current(pv_dir: &Path, commit: &str) -> Result<()> {
    if let Some(name) = current_branch(pv_dir)? {
        let branch_file = refs_dir(pv_dir).join(name);
        safe::atomic_write(&branch_file, format!("{commit}\n").as_bytes())?;
    } else {
        safe::atomic_write(&head_path(pv_dir), format!("{commit}\n").as_bytes())?;
    }
    Ok(())
}

/// List all branch names (recursive, so nested refs are visible too).
pub fn list_branches(pv_dir: &Path) -> Result<Vec<String>> {
    list_refs_recursive(&refs_dir(pv_dir))
}

pub fn branch_exists(pv_dir: &Path, name: &str) -> bool {
    refs_dir(pv_dir).join(name).exists()
}

/// Create a branch pointing at `commit`. Validates the name first.
pub fn create_branch(pv_dir: &Path, name: &str, commit: &str) -> Result<()> {
    safe::validate_ref_name(name)?;
    fs::create_dir_all(refs_dir(pv_dir))?;
    safe::atomic_write(
        &refs_dir(pv_dir).join(name),
        format!("{commit}\n").as_bytes(),
    )?;
    Ok(())
}

/// Resolve a branch name to its tip commit hash.
pub fn resolve_branch(pv_dir: &Path, name: &str) -> Result<Option<String>> {
    let f = refs_dir(pv_dir).join(name);
    if !f.exists() {
        return Ok(None);
    }
    let h = fs::read_to_string(&f)?.trim().to_string();
    Ok(if h.is_empty() { None } else { Some(h) })
}

pub fn delete_branch(pv_dir: &Path, name: &str) -> Result<()> {
    safe::validate_ref_name(name)?;
    let f = refs_dir(pv_dir).join(name);
    if !f.exists() {
        anyhow::bail!("branch '{name}' does not exist");
    }
    fs::remove_file(&f)?;
    Ok(())
}

/// Point HEAD at a branch (attach).
pub fn set_head_to_branch(pv_dir: &Path, name: &str) -> Result<()> {
    safe::validate_ref_name(name)?;
    safe::atomic_write(
        &head_path(pv_dir),
        format!("ref: refs/heads/{name}\n").as_bytes(),
    )?;
    Ok(())
}

// ---- Tags ----------------------------------------------------------------

pub fn tags_dir(pv_dir: &Path) -> PathBuf {
    pv_dir.join("refs").join("tags")
}

pub fn list_tags(pv_dir: &Path) -> Result<Vec<String>> {
    list_refs_recursive(&tags_dir(pv_dir))
}

pub fn tag_exists(pv_dir: &Path, name: &str) -> bool {
    tags_dir(pv_dir).join(name).exists()
}

pub fn create_tag(pv_dir: &Path, name: &str, commit: &str) -> Result<()> {
    safe::validate_ref_name(name)?;
    fs::create_dir_all(tags_dir(pv_dir))?;
    safe::atomic_write(
        &tags_dir(pv_dir).join(name),
        format!("{commit}\n").as_bytes(),
    )?;
    Ok(())
}

pub fn resolve_tag(pv_dir: &Path, name: &str) -> Result<Option<String>> {
    let f = tags_dir(pv_dir).join(name);
    if !f.exists() {
        return Ok(None);
    }
    let h = fs::read_to_string(&f)?.trim().to_string();
    Ok(if h.is_empty() { None } else { Some(h) })
}

pub fn delete_tag(pv_dir: &Path, name: &str) -> Result<()> {
    safe::validate_ref_name(name)?;
    let f = tags_dir(pv_dir).join(name);
    if !f.exists() {
        anyhow::bail!("tag '{name}' does not exist");
    }
    fs::remove_file(&f)?;
    Ok(())
}

/// Recursively list all files under `dir`, returning paths relative to `dir`.
/// Used so nested refs (e.g. `feature/x`) are visible in `branch`/`tag` listings.
fn list_refs_recursive(dir: &Path) -> Result<Vec<String>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    walk_refs(dir, dir, &mut names)?;
    names.sort();
    Ok(names)
}

fn walk_refs(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        // Use symlink_metadata so we don't follow symlinks out of the refs dir.
        let ft = entry.file_type()?;
        if ft.is_dir() {
            walk_refs(root, &p, out)?;
        } else if ft.is_file() {
            let rel = p
                .strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            out.push(rel);
        }
    }
    Ok(())
}

