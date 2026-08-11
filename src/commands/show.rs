use anyhow::{bail, Result};

use crate::core::objects::{self, ObjectType};
use crate::core::refs;
use crate::core::repository::Repo;

/// `pv show <target>` — show an object, a tracked path's HEAD version, or a
/// file from a specific ref.
///
/// Accepted forms:
///   - `HEAD`                       → the HEAD commit
///   - `<hash>` / `<prefix>`        → any object by hash (blob/tree/commit)
///   - `<tag>`                      → the commit a tag points at
///   - `<branch>`                   → the commit a branch points at
///   - `<tracked-path>`             → the blob at that path in HEAD
///   - `HEAD:<path>`                → the blob at <path> in HEAD
///   - `<branch>:<path>`            → the blob at <path> in <branch>
///   - `<tag>:<path>`               → the blob at <path> at <tag>
///   - `<commit>:<path>`            → the blob at <path> at <commit>
pub fn run(target: &str) -> Result<()> {
    let repo = Repo::find()?;

    // `ref:path` form — show a blob at a path under a given ref.
    if let Some((ref_spec, path)) = target.split_once(':') {
        if path.is_empty() {
            bail!("empty path in '{target}'");
        }
        return show_blob_at_ref(&repo, ref_spec, path);
    }

    if let Some(hash) = resolve_hash(&repo, target)? {
        let obj = objects::read_object(&repo.pv_dir, &hash)?;
        match obj.kind {
            ObjectType::Blob => print!("{}", String::from_utf8_lossy(&obj.data)),
            ObjectType::Tree => {
                let entries = objects::read_tree(&repo.pv_dir, &hash)?;
                for e in entries {
                    println!("{}  {}", crate::core::safe::short_hash(&e.hash), e.path);
                }
            }
            ObjectType::Commit => {
                let c = objects::read_commit(&repo.pv_dir, &hash)?;
                println!("tree {}", c.tree);
                if let Some(p) = &c.parent {
                    println!("parent {p}");
                }
                println!("timestamp {}", c.timestamp);
                println!();
                println!("{}", c.message);
            }
        }
        return Ok(());
    }

    // Otherwise, treat `target` as a tracked path and show its HEAD version.
    if let Some(h) = head_blob_for_path(&repo, target)? {
        let obj = objects::read_object(&repo.pv_dir, &h)?;
        print!("{}", String::from_utf8_lossy(&obj.data));
        return Ok(());
    }

    bail!(
        "unknown target: {target} (expected a commit/tree/blob hash, HEAD, a tracked path, or REF:PATH)"
    );
}

/// Resolve `ref_spec:path` to a blob hash and print its contents.
fn show_blob_at_ref(repo: &Repo, ref_spec: &str, path: &str) -> Result<()> {
    let path = path.replace('\\', "/");
    let Some(commit_hash) = resolve_commit(repo, ref_spec)? else {
        bail!("could not resolve '{ref_spec}' to a commit");
    };
    let commit = objects::read_commit(&repo.pv_dir, &commit_hash)?;
    let entries = objects::read_tree(&repo.pv_dir, &commit.tree)?;
    let entry = entries
        .into_iter()
        .find(|e| e.path == path)
        .ok_or_else(|| anyhow::anyhow!("path '{path}' not found in {ref_spec}"))?;
    let obj = objects::read_object(&repo.pv_dir, &entry.hash)?;
    if obj.kind != ObjectType::Blob {
        bail!("{path} in {ref_spec} is not a blob");
    }
    print!("{}", String::from_utf8_lossy(&obj.data));
    Ok(())
}

fn resolve_commit(repo: &Repo, spec: &str) -> Result<Option<String>> {
    if spec == "HEAD" {
        return repo.head_commit();
    }
    if let Some(h) = refs::resolve_tag(&repo.pv_dir, spec)? {
        return Ok(Some(h));
    }
    if let Some(h) = refs::resolve_branch(&repo.pv_dir, spec)? {
        return Ok(Some(h));
    }
    crate::core::safe::resolve_hash_prefix(&repo.pv_dir.join("objects"), spec)
}

fn resolve_hash(repo: &Repo, target: &str) -> Result<Option<String>> {
    if target == "HEAD" {
        return repo.head_commit();
    }
    let is_hex = !target.is_empty() && target.chars().all(|c| c.is_ascii_hexdigit());
    if !is_hex {
        // Not a hash — but it might be a tag or branch name.
        if let Some(h) = refs::resolve_tag(&repo.pv_dir, target)? {
            return Ok(Some(h));
        }
        if let Some(h) = refs::resolve_branch(&repo.pv_dir, target)? {
            return Ok(Some(h));
        }
        return Ok(None);
    }
    if target.len() == 64 {
        return Ok(objects::object_exists(&repo.pv_dir, target)
            .then(|| target.to_string()));
    }
    // Prefix match (>= 4 chars for safety).
    if target.len() >= 4 {
        let (dir, file_prefix) = target.split_at(2);
        let dir_path = repo.pv_dir.join("objects").join(dir);
        if let Ok(entries) = std::fs::read_dir(&dir_path) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with(file_prefix) {
                    return Ok(Some(format!("{dir}{name}")));
                }
            }
        }
    }
    Ok(None)
}

fn head_blob_for_path(repo: &Repo, path: &str) -> Result<Option<String>> {
    let path = path.replace('\\', "/");
    let Some(h) = repo.head_commit()? else {
        return Ok(None);
    };
    let commit = objects::read_commit(&repo.pv_dir, &h)?;
    let entries = objects::read_tree(&repo.pv_dir, &commit.tree)?;
    Ok(entries
        .into_iter()
        .find(|e| e.path == path)
        .map(|e| e.hash))
}
