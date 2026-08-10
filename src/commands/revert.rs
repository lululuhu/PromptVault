use std::fs;

use anyhow::{bail, Result};

use crate::core::objects;
use crate::core::refs;
use crate::core::repository::Repo;
use crate::core::safe;
use crate::ui::printer;

/// Restore the working tree and index to match `<commit>`, leaving HEAD untouched.
/// The user can then `pv commit` to record the rollback as a new commit.
///
/// Refuses to run if the working tree has uncommitted changes (would overwrite them).
pub fn run(commit_spec: &str) -> Result<()> {
    let repo = Repo::find()?;
    let hash = resolve_commit(&repo, commit_spec)?
        .ok_or_else(|| anyhow::anyhow!("unknown commit: {commit_spec}"))?;

    let target_commit = objects::read_commit(&repo.pv_dir, &hash)?;
    let target_entries = objects::read_tree(&repo.pv_dir, &target_commit.tree)?;

    // Safety: refuse if tracked files have uncommitted modifications.
    if let Err(e) = safe::check_clean_working_tree(&repo) {
        bail!(
            "cannot revert: {e}\n\n\
             commit or stash your changes first."
        );
    }

    // Current tracked files (from HEAD), so we can remove ones absent in the target.
    let head_entries = head_tree_entries(&repo)?;

    let target_paths: std::collections::HashSet<&String> =
        target_entries.iter().map(|e| &e.path).collect();

    // Remove files tracked now but not in the target tree.
    for e in &head_entries {
        if !target_paths.contains(&e.path) {
            let abs = repo.root.join(&e.path);
            if abs.exists() {
                let _ = fs::remove_file(&abs);
                println!("{}  {}", printer::dim("removed"), e.path);
            }
        }
    }

    // Write/overwrite files from the target tree.
    for e in &target_entries {
        let abs = repo.root.join(&e.path);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent)?;
        }
        let blob = objects::read_object(&repo.pv_dir, &e.hash)?;
        fs::write(&abs, &blob.data)?;
        println!("{}  {}", printer::dim("restored"), e.path);
    }

    // Rebuild the index from the target tree.
    let mut idx = repo.index()?;
    idx.entries.clear();
    for e in &target_entries {
        idx.add(&e.path, &e.hash);
    }
    repo.save_index(&idx)?;

    printer::ok(&format!(
        "Reverted working tree to {short} ({msg})",
        short = crate::core::safe::short_hash(&hash),
        msg = first_line(&target_commit.message)
    ));
    printer::info("HEAD is unchanged — `pv commit` to record this rollback");
    Ok(())
}

fn resolve_commit(repo: &Repo, spec: &str) -> Result<Option<String>> {
    // Tag?
    if let Some(h) = refs::resolve_tag(&repo.pv_dir, spec)? {
        return Ok(Some(h));
    }
    // Branch tip?
    if let Some(h) = refs::resolve_branch(&repo.pv_dir, spec)? {
        return Ok(Some(h));
    }
    // HEAD?
    if spec == "HEAD" {
        return repo.head_commit();
    }
    // Hex prefix? (errors on ambiguity)
    crate::core::safe::resolve_hash_prefix(&repo.pv_dir.join("objects"), spec)
}

fn head_tree_entries(repo: &Repo) -> Result<Vec<objects::TreeEntry>> {
    let Some(h) = repo.head_commit()? else {
        return Ok(Vec::new());
    };
    let commit = objects::read_commit(&repo.pv_dir, &h)?;
    Ok(objects::read_tree(&repo.pv_dir, &commit.tree)?)
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}

