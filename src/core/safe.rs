//! Shared safety / correctness helpers.
//!
//! Centralizes:
//!   - hash validation (rejects anything not `^[0-9a-f]{64}$`)
//!   - short hash (never panics on short input)
//!   - ref-name validation (rejects `..`, `/`, control chars, etc.)
//!   - path-traversal guard for tree entries
//!   - atomic file writes (write temp → rename, crash-safe)
//!   - working-tree cleanliness check (reused by checkout / stash pop / revert)

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{bail, Result};

use crate::core::objects::{self, hash_blob, TreeEntry};
use crate::core::repository::Repo;

/// A SHA-256 hex hash is exactly 64 lowercase hex chars.
pub fn is_valid_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Directory names that should always be skipped during filesystem walks.
/// These are well-known build caches, VCS dirs, dependency folders, etc.
const SKIP_DIRS: &[&str] = &[
    ".pv",
    ".git",
    ".hg",
    ".svn",
    "target",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    "dist",
    "build",
    ".next",
    ".nuxt",
    "out",
    "bin",
    "obj",
    ".idea",
    ".vscode",
];

/// True if `name` is a directory that should never be recursed into.
pub fn is_skip_dir(name: &str) -> bool {
    SKIP_DIRS.contains(&name)
}

/// Return a 7-char short hash, or the full hash if shorter than 7.
/// Never panics.
pub fn short_hash(hash: &str) -> &str {
    if hash.len() >= 7 {
        &hash[..7]
    } else {
        hash
    }
}

/// Validate a branch / tag name. Rejects anything that could escape `refs/`.
pub fn validate_ref_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("ref name is empty");
    }
    if name == "HEAD" {
        bail!("'HEAD' is reserved");
    }
    if name.contains("..") {
        bail!("ref name cannot contain '..': {name}");
    }
    if name.contains('/') || name.contains('\\') {
        bail!("ref name cannot contain '/' or '\\': {name}");
    }
    if name.starts_with('.') || name.ends_with('.') {
        bail!("ref name cannot start or end with '.': {name}");
    }
    if name.contains(|c: char| c.is_control() || c == ':' || c == '~' || c == '^' || c == ' ' || c == '~') {
        bail!("ref name contains forbidden character: {name}");
    }
    Ok(())
}

/// Ensure a tree entry path is safe to join onto `repo.root`:
///   - relative (no leading `/`)
///   - no `..` segments
///   - no NUL
///   - no backslash (we normalize to forward slashes)
pub fn validate_tree_path(path: &str) -> Result<()> {
    if path.is_empty() {
        bail!("tree entry path is empty");
    }
    if path.contains('\0') {
        bail!("tree entry path contains NUL: {path:?}");
    }
    if path.starts_with('/') {
        bail!("tree entry path is absolute: {path}");
    }
    if path.contains('\\') {
        bail!("tree entry path contains backslash: {path}");
    }
    for seg in path.split('/') {
        if seg == ".." || seg == "." {
            bail!("tree entry path contains '.' or '..' segment: {path}");
        }
    }
    Ok(())
}

/// Verify every entry in a tree has a safe path and a valid-looking hash.
pub fn validate_tree_entries(entries: &[TreeEntry]) -> Result<()> {
    for e in entries {
        validate_tree_path(&e.path)?;
        if !is_valid_hash(&e.hash) {
            bail!("tree entry '{}' has invalid hash: {}", e.path, e.hash);
        }
    }
    Ok(())
}

/// Atomically write `data` to `path`: write to a temp file in the same dir,
/// fsync, then rename over the target. Crash-safe.
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("atomic")
    ));

    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    // Best-effort fsync of the directory (not all platforms support it).
    if let Ok(d) = fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

/// Check that every file tracked at HEAD matches its committed blob hash.
/// Returns an error listing any dirty/deleted paths. Used by checkout, stash pop, revert.
pub fn check_clean_working_tree(repo: &Repo) -> Result<()> {
    let entries = head_tree_entries(repo)?;
    let mut dirty = Vec::new();
    for e in &entries {
        let abs = repo.root.join(&e.path);
        match fs::read(&abs) {
            Ok(data) => {
                if hash_blob(&data) != e.hash {
                    dirty.push(e.path.clone());
                }
            }
            Err(_) => dirty.push(format!("{} (deleted)", e.path)),
        }
    }
    if !dirty.is_empty() {
        bail!(
            "working tree has uncommitted changes to tracked files:\n  {}",
            dirty.join("\n  ")
        );
    }
    Ok(())
}

/// Collect every reachable commit hash from HEAD's first-parent chain.
/// (For stats; full reachability is a future improvement.)
pub fn collect_commits_from_head(repo: &Repo) -> Result<HashSet<String>> {
    let mut seen = HashSet::new();
    let mut cur = repo.head_commit()?;
    while let Some(h) = cur {
        if !seen.insert(h.clone()) {
            break;
        }
        let commit = objects::read_commit(&repo.pv_dir, &h)?;
        cur = commit.parent;
    }
    Ok(seen)
}

fn head_tree_entries(repo: &Repo) -> Result<Vec<TreeEntry>> {
    let Some(h) = repo.head_commit()? else {
        return Ok(Vec::new());
    };
    let commit = objects::read_commit(&repo.pv_dir, &h)?;
    Ok(objects::read_tree(&repo.pv_dir, &commit.tree)?)
}

/// Resolve a short hash prefix to a full hash, scanning `.pv/objects/`.
/// Returns:
///   - `Ok(Some(hash))` if exactly one match
///   - `Ok(None)` if no match
///   - `Err` if the prefix is ambiguous (matches 2+ objects)
pub fn resolve_hash_prefix(objects_dir: &Path, prefix: &str) -> Result<Option<String>> {
    if prefix.len() < 4 || !prefix.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(None);
    }
    let (dir_part, file_prefix) = prefix.split_at(2);
    let dir_path = objects_dir.join(dir_part);
    if !dir_path.exists() {
        return Ok(None);
    }
    let mut matches = Vec::new();
    for entry in fs::read_dir(&dir_path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(file_prefix) {
            matches.push(format!("{dir_part}{name}"));
        }
    }
    match matches.len() {
        0 => Ok(None),
        1 => Ok(Some(matches.into_iter().next().unwrap())),
        n => bail!("ambiguous hash prefix '{prefix}': matches {n} objects"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_hash_check() {
        assert!(is_valid_hash(
            "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
        ));
        assert!(!is_valid_hash("abc"));
        assert!(!is_valid_hash("../config"));
        assert!(!is_valid_hash(""));
    }

    #[test]
    fn short_hash_never_panics() {
        assert_eq!(short_hash("abcdefg1234"), "abcdefg");
        assert_eq!(short_hash("abc"), "abc");
        assert_eq!(short_hash(""), "");
    }

    #[test]
    fn ref_name_validation() {
        assert!(validate_ref_name("main").is_ok());
        assert!(validate_ref_name("feature-x").is_ok());
        assert!(validate_ref_name("").is_err());
        assert!(validate_ref_name("HEAD").is_err());
        assert!(validate_ref_name("feature/x").is_err());
        assert!(validate_ref_name("../x").is_err());
        assert!(validate_ref_name(".hidden").is_err());
        assert!(validate_ref_name("a b").is_err());
    }

    #[test]
    fn tree_path_validation() {
        assert!(validate_tree_path("prompts/foo.md").is_ok());
        assert!(validate_tree_path("foo.md").is_ok());
        assert!(validate_tree_path("").is_err());
        assert!(validate_tree_path("/etc/passwd").is_err());
        assert!(validate_tree_path("../escape").is_err());
        assert!(validate_tree_path("a/../b").is_err());
        assert!(validate_tree_path("a\\b").is_err());
        assert!(validate_tree_path("a\0b").is_err());
    }
}
