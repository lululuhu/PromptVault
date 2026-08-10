use std::collections::HashMap;
use std::fs;

use anyhow::{bail, Result};

use crate::core::objects::{self, hash_blob, TreeEntry};
use crate::core::refs;
use crate::core::repository::Repo;
use crate::core::safe;
use crate::ui::printer;

/// `pv checkout <target>` — switch to a branch or enter detached HEAD at a commit.
///
/// - If `<target>` is a branch name: attach HEAD to that branch (normal switch).
/// - If `<target>` is a commit hash / tag / `HEAD`: detach HEAD at that commit
///   (working tree is restored to that commit's tree).
pub fn run(target: &str) -> Result<()> {
    let repo = Repo::find()?;

    // Branch switch path.
    if refs::branch_exists(&repo.pv_dir, target) {
        return checkout_branch(&repo, target);
    }

    // Detached HEAD path: resolve as tag / HEAD / hash-prefix.
    let Some(commit_hash) = resolve_target(&repo, target)? else {
        bail!("'{target}' is not a branch, tag, or commit");
    };

    checkout_detached(&repo, &commit_hash, target)
}

fn checkout_branch(repo: &Repo, name: &str) -> Result<()> {
    let cur = repo.current_branch()?;
    if cur.as_deref() == Some(name) {
        printer::info(&format!("already on '{name}'"));
        return Ok(());
    }

    // Safety: refuse if tracked files have uncommitted modifications.
    safe::check_clean_working_tree(repo)?;

    let target_commit = match refs::resolve_branch(&repo.pv_dir, name)? {
        Some(h) => h,
        None => bail!("branch '{name}' has no commits"),
    };

    let target_tree = tree_entries_for_commit(repo, &target_commit)?;
    let current_tree = head_tree_entries(repo)?;
    apply_tree(repo, &current_tree, &target_tree)?;

    refs::set_head_to_branch(&repo.pv_dir, name)?;
    rebuild_index(repo, &target_tree)?;

    printer::ok(&format!("Switched to branch '{name}'"));
    Ok(())
}

fn checkout_detached(repo: &Repo, commit_hash: &str, target_label: &str) -> Result<()> {
    safe::check_clean_working_tree(repo)?;

    let target_tree = tree_entries_for_commit(repo, commit_hash)?;
    let current_tree = head_tree_entries(repo)?;
    apply_tree(repo, &current_tree, &target_tree)?;

    // Detach HEAD: write the commit hash directly into HEAD.
    safe::atomic_write(
        &refs::head_path(&repo.pv_dir),
        format!("{commit_hash}\n").as_bytes(),
    )?;

    rebuild_index(repo, &target_tree)?;

    printer::ok(&format!(
        "HEAD is now detached at {short} ({target_label})",
        short = safe::short_hash(commit_hash)
    ));
    printer::info("you are not on a branch; commits here are not on any branch");
    Ok(())
}

/// Apply the target tree to the working tree (remove files absent in target,
/// write/update files from target). Index is not touched here.
fn apply_tree(
    repo: &Repo,
    current_tree: &[TreeEntry],
    target_tree: &[TreeEntry],
) -> Result<()> {
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

    for path in current_map.keys() {
        if !target_map.contains_key(path) {
            let abs = repo.root.join(path);
            if abs.exists() {
                let _ = fs::remove_file(&abs);
                println!("{}  {}", printer::dim("removed"), path);
            }
        }
    }

    for e in target_tree {
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
    Ok(())
}

fn rebuild_index(repo: &Repo, target_tree: &[TreeEntry]) -> Result<()> {
    let mut idx = repo.index()?;
    idx.entries.clear();
    for e in target_tree {
        idx.add(&e.path, &e.hash);
    }
    repo.save_index(&idx)?;
    Ok(())
}

fn resolve_target(repo: &Repo, target: &str) -> Result<Option<String>> {
    if target == "HEAD" {
        return repo.head_commit();
    }
    if let Some(h) = refs::resolve_tag(&repo.pv_dir, target)? {
        return Ok(Some(h));
    }
    safe::resolve_hash_prefix(&repo.pv_dir.join("objects"), target)
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
