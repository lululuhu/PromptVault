use std::collections::HashMap;
use std::fs;

use anyhow::{bail, Result};

use crate::core::objects::{self, hash_blob, TreeEntry};
use crate::core::refs;
use crate::core::repository::Repo;
use crate::ui::printer;

pub fn run(name: &str) -> Result<()> {
    let repo = Repo::find()?;

    if !refs::branch_exists(&repo.pv_dir, name) {
        bail!("branch '{name}' does not exist");
    }
    let cur = repo.current_branch()?;
    if cur.as_deref() == Some(name) {
        printer::info(&format!("already on '{name}'"));
        return Ok(());
    }

    // Safety: refuse if tracked files have uncommitted modifications.
    check_clean_working_tree(&repo)?;

    let target_commit = match refs::resolve_branch(&repo.pv_dir, name)? {
        Some(h) => h,
        None => bail!("branch '{name}' has no commits"),
    };

    // Restore working tree to match the target branch.
    let target_tree = tree_entries_for_commit(&repo, &target_commit)?;
    let current_tree = head_tree_entries(&repo)?;

    let target_map: HashMap<String, String> = target_tree
        .iter()
        .cloned()
        .map(|e| (e.path, e.hash))
        .collect();
    let current_map: HashMap<String, String> = current_tree
        .iter()
        .cloned()
        .map(|e| (e.path, e.hash))
        .collect();

    // Remove files tracked in current branch but absent in target.
    for path in current_map.keys() {
        if !target_map.contains_key(path) {
            let abs = repo.root.join(path);
            if abs.exists() {
                let _ = fs::remove_file(&abs);
                println!("{}  {}", printer::dim("removed"), path);
            }
        }
    }

    // Write / overwrite files from the target tree.
    for e in &target_tree {
        let abs = repo.root.join(&e.path);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent)?;
        }
        let blob = objects::read_object(&repo.pv_dir, &e.hash)?;
        let cur_hash = fs::read(&abs).ok().map(|d| hash_blob(&d));
        if cur_hash.as_deref() != Some(e.hash.as_str()) {
            fs::write(&abs, &blob.data)?;
            println!("{}  {}", printer::dim("updated"), e.path);
        }
    }

    // Switch HEAD.
    refs::set_head_to_branch(&repo.pv_dir, name)?;

    // Rebuild the index from the target tree.
    let mut idx = repo.index()?;
    idx.entries.clear();
    for e in &target_tree {
        idx.add(&e.path, &e.hash);
    }
    repo.save_index(&idx)?;

    printer::ok(&format!("Switched to branch '{name}'"));
    Ok(())
}

fn check_clean_working_tree(repo: &Repo) -> Result<()> {
    let entries = head_tree_entries(&repo)?;
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
            "cannot checkout: working tree has uncommitted changes to tracked files:\n  {}",
            dirty.join("\n  ")
        );
    }
    Ok(())
}

fn head_tree_entries(repo: &Repo) -> Result<Vec<TreeEntry>> {
    let Some(h) = repo.head_commit()? else {
        return Ok(Vec::new());
    };
    let commit = objects::read_commit(&repo.pv_dir, &h)?;
    Ok(objects::read_tree(&repo.pv_dir, &commit.tree)?)
}

fn tree_entries_for_commit(repo: &Repo, hash: &str) -> Result<Vec<TreeEntry>> {
    let commit = objects::read_commit(&repo.pv_dir, hash)?;
    Ok(objects::read_tree(&repo.pv_dir, &commit.tree)?)
}
