use anyhow::{bail, Result};

use crate::core::objects::{self, ObjectType};
use crate::core::repository::Repo;

pub fn run(target: &str) -> Result<()> {
    let repo = Repo::find()?;

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
        "unknown target: {target} (expected a commit/tree/blob hash, HEAD, or a tracked path)"
    );
}

fn resolve_hash(repo: &Repo, target: &str) -> Result<Option<String>> {
    if target == "HEAD" {
        return repo.head_commit();
    }
    let is_hex = !target.is_empty() && target.chars().all(|c| c.is_ascii_hexdigit());
    if !is_hex {
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
