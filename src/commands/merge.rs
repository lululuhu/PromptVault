use std::collections::HashMap;
use std::fs;

use anyhow::{bail, Result};

use crate::core::config;
use crate::core::objects::{self, Commit, TreeEntry};
use crate::core::refs;
use crate::core::repository::Repo;
use crate::core::safe;
use crate::ui::printer;

/// `pv merge <branch>` — merge <branch> into the current branch.
///
/// Two strategies:
///   - **Fast-forward**: if the current branch is an ancestor of <branch>,
///     just move the current branch pointer forward. No merge commit.
///   - **Three-way merge**: otherwise, find the merge base (lowest common
///     ancestor of the two tips), and for each path:
///       - unchanged on both sides        → keep
///       - changed on one side only        → take the changed side
///       - changed on both, same result   → take either
///       - changed on both, different      → CONFLICT: write a file with
///         `<<<<<<< / ======= / >>>>>>>` markers and report. The user edits
///         the file, `pv add` it, and `pv commit` to finish the merge.
///
/// Conflicts do NOT touch HEAD — the working tree and index are updated so
/// the user can resolve and commit. This mirrors `git merge` behavior.
pub fn run(branch: &str) -> Result<()> {
    let repo = Repo::find()?;
    let _lock = repo.lock()?;

    // Resolve the target branch tip.
    let Some(target) = refs::resolve_branch(&repo.pv_dir, branch)? else {
        bail!("branch '{branch}' does not exist");
    };

    // Must be on a branch (not detached) to merge into it.
    let Some(cur_name) = repo.current_branch()? else {
        bail!("cannot merge while in detached HEAD; `pv checkout <branch>` first");
    };
    let Some(cur) = repo.head_commit()? else {
        // Current branch has no commits — fast-forward to target.
        refs::update_current(&repo.pv_dir, &target)?;
        restore_tree(&repo, &target)?;
        printer::ok(&format!("Fast-forwarded empty branch '{cur_name}' to '{branch}'"));
        return Ok(());
    };

    if cur == target {
        printer::info(&format!("already up to date with '{branch}'"));
        return Ok(());
    }

    // Fast-forward: current commit is an ancestor of target.
    if is_ancestor(&repo, &cur, &target)? {
        refs::update_current(&repo.pv_dir, &target)?;
        restore_tree(&repo, &target)?;
        printer::ok(&format!("Fast-forward to '{branch}'"));
        return Ok(());
    }

    // Target is already merged into current — nothing to do.
    if is_ancestor(&repo, &target, &cur)? {
        printer::info(&format!("'{branch}' is already merged"));
        return Ok(());
    }

    // Real three-way merge.
    let base = merge_base(&repo, &cur, &target)?;
    let base_tree = tree_entries_for_commit(&repo, &base)?;
    let cur_tree = tree_entries_for_commit(&repo, &cur)?;
    let tgt_tree = tree_entries_for_commit(&repo, &target)?;

    let merged = three_way_merge(&repo, &base_tree, &cur_tree, &tgt_tree, &cur_name, branch)?;

    if merged.conflicts.is_empty() {
        // No conflicts: create a merge commit with two parents.
        // We store only the first parent in the commit object (prv's
        // commit format has a single parent field), and record the second
        // parent in the commit message for traceability.
        apply_tree(&repo, &cur_tree, &merged.entries)?;
        rebuild_index(&repo, &merged.entries)?;

        let tree = objects::write_tree(&repo.pv_dir, &merged.entries)?;
        let author = config::resolve_author(&repo.pv_dir)?;
        let commit = Commit {
            tree,
            parent: Some(cur.clone()),
            author: Some(author),
            timestamp: chrono::Utc::now().timestamp(),
            message: format!(
                "merge branch '{branch}'\n\nmerge-parent: {target}"
            ),
        };
        let hash = objects::write_commit(&repo.pv_dir, &commit)?;
        refs::update_current(&repo.pv_dir, &hash)?;

        printer::ok(&format!(
            "Merge made by three-way strategy: [{cur_name} {}] merge '{branch}'",
            safe::short_hash(&hash)
        ));
    } else {
        // Conflicts: update working tree + index with merged content (including
        // conflict markers), but do NOT commit. Let the user resolve.
        apply_tree(&repo, &cur_tree, &merged.entries)?;
        rebuild_index(&repo, &merged.entries)?;
        printer::error(&format!(
            "CONFLICT in {} file{}:",
            merged.conflicts.len(),
            if merged.conflicts.len() == 1 { "" } else { "s" }
        ));
        for c in &merged.conflicts {
            println!("  {}", printer::dim(c));
        }
        printer::info("resolve the conflicts, `pv add <path>`, then `pv commit` to finish");
    }
    Ok(())
}

#[derive(Debug)]
struct MergeResult {
    entries: Vec<TreeEntry>,
    conflicts: Vec<String>,
}

/// Three-way merge of tree entries.
///
/// For each path present in any of the three trees:
///   - base = version at merge base
///   - ours = version on current branch
///   - theirs = version on target branch
fn three_way_merge(
    repo: &Repo,
    base: &[TreeEntry],
    ours: &[TreeEntry],
    theirs: &[TreeEntry],
    ours_label: &str,
    theirs_label: &str,
) -> Result<MergeResult> {
    let base_map: HashMap<String, String> = base.iter().cloned().map(|e| (e.path, e.hash)).collect();
    let ours_map: HashMap<String, String> = ours.iter().cloned().map(|e| (e.path, e.hash)).collect();
    let theirs_map: HashMap<String, String> = theirs.iter().cloned().map(|e| (e.path, e.hash)).collect();

    let mut paths: Vec<&String> = base_map.keys().chain(ours_map.keys()).chain(theirs_map.keys()).collect();
    paths.sort();
    paths.dedup();

    let mut entries = Vec::new();
    let mut conflicts = Vec::new();

    for path in paths {
        let b = base_map.get(path);
        let o = ours_map.get(path);
        let t = theirs_map.get(path);

        // If ours == theirs, take it (no change or identical edit).
        if o == t {
            if let Some(h) = o {
                entries.push(TreeEntry { path: path.clone(), hash: h.clone() });
            }
            // If both are None, file was deleted on both sides — drop it.
            continue;
        }

        // If ours == base, theirs changed → take theirs.
        if o == b {
            if let Some(h) = t {
                entries.push(TreeEntry { path: path.clone(), hash: h.clone() });
            }
            continue;
        }

        // If theirs == base, ours changed → take ours.
        if t == b {
            if let Some(h) = o {
                entries.push(TreeEntry { path: path.clone(), hash: h.clone() });
            }
            continue;
        }

        // Both sides changed differently → content conflict.
        // Build a conflict-marked blob from the two sides' contents.
        let ours_content = blob_content(repo, o)?;
        let theirs_content = blob_content(repo, t)?;
        let conflict = format!(
            "<<<<<<< {ours_label}\n{ours_content}=======\n{theirs_content}>>>>>>> {theirs_label}\n"
        );
        let h = objects::write_object(&repo.pv_dir, objects::ObjectType::Blob, conflict.as_bytes())?;
        entries.push(TreeEntry { path: path.clone(), hash: h });
        conflicts.push(path.clone());
    }

    Ok(MergeResult { entries, conflicts })
}

fn blob_content(repo: &Repo, hash: Option<&String>) -> Result<String> {
    match hash {
        Some(h) => {
            let obj = objects::read_object(&repo.pv_dir, h)?;
            Ok(String::from_utf8_lossy(&obj.data).into_owned())
        }
        None => Ok(String::new()),
    }
}

/// Is `a` an ancestor of `b` (i.e. reachable by following parent links from b)?
fn is_ancestor(repo: &Repo, a: &str, b: &str) -> Result<bool> {
    let mut cur = Some(b.to_string());
    while let Some(h) = cur {
        if h == a {
            return Ok(true);
        }
        let commit = objects::read_commit(&repo.pv_dir, &h)?;
        cur = commit.parent;
    }
    Ok(false)
}

/// Find the merge base (lowest common ancestor) of two commits.
/// Walks both parent chains and finds the first shared commit.
fn merge_base(repo: &Repo, a: &str, b: &str) -> Result<String> {
    // Collect all ancestors of `a`.
    let mut ancestors_a = std::collections::HashSet::new();
    let mut cur = Some(a.to_string());
    while let Some(h) = cur {
        if !ancestors_a.insert(h.clone()) {
            break; // cycle guard
        }
        let commit = objects::read_commit(&repo.pv_dir, &h)?;
        cur = commit.parent;
    }

    // Walk `b`'s parent chain until we hit an ancestor of `a`.
    let mut cur = Some(b.to_string());
    while let Some(h) = cur {
        if ancestors_a.contains(&h) {
            return Ok(h);
        }
        let commit = objects::read_commit(&repo.pv_dir, &h)?;
        cur = commit.parent;
    }
    // No common ancestor — shouldn't happen for valid branches, but fall back
    // to an empty tree (treat as merge from nothing).
    bail!("no common ancestor between {a} and {b}");
}

/// Restore the working tree to match the given commit's tree.
fn restore_tree(repo: &Repo, commit_hash: &str) -> Result<()> {
    let entries = tree_entries_for_commit(repo, commit_hash)?;
    let cur_entries = head_tree_entries(repo)?;
    apply_tree(repo, &cur_entries, &entries)?;
    rebuild_index(repo, &entries)?;
    Ok(())
}

/// Apply the target tree to the working tree (remove files absent in target,
/// write/update files from target).
fn apply_tree(repo: &Repo, current_tree: &[TreeEntry], target_tree: &[TreeEntry]) -> Result<()> {
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
        let cur_hash = fs::read(&abs).ok().map(|d| objects::hash_blob(&d));
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

fn tree_entries_for_commit(repo: &Repo, hash: &str) -> Result<Vec<TreeEntry>> {
    let commit = objects::read_commit(&repo.pv_dir, hash)?;
    Ok(objects::read_tree(&repo.pv_dir, &commit.tree)?)
}

fn head_tree_entries(repo: &Repo) -> Result<Vec<TreeEntry>> {
    let Some(h) = repo.head_commit()? else {
        return Ok(Vec::new());
    };
    let commit = objects::read_commit(&repo.pv_dir, &h)?;
    Ok(objects::read_tree(&repo.pv_dir, &commit.tree)?)
}
