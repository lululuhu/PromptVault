//! `pv stats` — show vault statistics.

use std::collections::HashSet;

use anyhow::Result;

use crate::core::objects::{self, ObjectType};
use crate::core::refs;
use crate::core::repository::Repo;
use crate::ui::printer;

pub fn run() -> Result<()> {
    let repo = Repo::find()?;

    let mut blobs: HashSet<String> = HashSet::new();
    let mut trees: HashSet<String> = HashSet::new();
    let mut total_blob_bytes = 0usize;

    // Collect all reachable commits from every branch tip and tag — not just
    // HEAD's first-parent chain. This ensures commits on other branches are
    // counted too.
    let all_commits = collect_all_commits(&repo)?;

    for hash in &all_commits {
        let commit = objects::read_commit(&repo.pv_dir, hash)?;
        walk_tree(&repo, &commit.tree, &mut blobs, &mut trees, &mut total_blob_bytes)?;
    }

    let commits = all_commits.len();
    let branches = refs::list_branches(&repo.pv_dir)?.len();
    let tags = refs::list_tags(&repo.pv_dir)?.len();

    // Disk usage of .pv/objects
    let obj_bytes = dir_size(&repo.pv_dir.join("objects"));

    println!("{}", printer::bold("prv stats"));
    println!("  commits:        {}", commits);
    println!("  blobs (unique): {}", blobs.len());
    println!("  trees (unique): {}", trees.len());
    println!("  branches:       {}", branches);
    println!("  tags:           {}", tags);
    println!("  prompt bytes:   {} ({})", total_blob_bytes, human_bytes(total_blob_bytes as f64));
    println!("  .pv/objects:    {} ({})", obj_bytes, human_bytes(obj_bytes as f64));
    Ok(())
}

fn walk_tree(
    repo: &Repo,
    tree_hash: &str,
    blobs: &mut HashSet<String>,
    trees: &mut HashSet<String>,
    total_blob_bytes: &mut usize,
) -> Result<()> {
    if !trees.insert(tree_hash.to_string()) {
        return Ok(());
    }
    let entries = objects::read_tree(&repo.pv_dir, tree_hash)?;
    for e in &entries {
        if blobs.insert(e.hash.clone()) {
            if let Ok(obj) = objects::read_object(&repo.pv_dir, &e.hash) {
                if obj.kind == ObjectType::Blob {
                    *total_blob_bytes += obj.data.len();
                }
            }
        }
    }
    Ok(())
}

fn dir_size(path: &std::path::Path) -> usize {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(meta) = entry.metadata() {
                total += meta.len() as usize;
            }
        }
    }
    total
}

fn human_bytes(n: f64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    if n >= GB {
        format!("{:.2} GB", n / GB)
    } else if n >= MB {
        format!("{:.2} MB", n / MB)
    } else if n >= KB {
        format!("{:.2} KB", n / KB)
    } else {
        format!("{} B", n as u64)
    }
}

/// Walk all branch tips and tags, following every commit's parent chain,
/// collecting every reachable commit hash. This ensures commits on branches
/// that are not reachable from HEAD are counted too.
fn collect_all_commits(repo: &Repo) -> Result<HashSet<String>> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: Vec<String> = Vec::new();

    // Seed with HEAD.
    if let Some(h) = repo.head_commit()? {
        queue.push(h);
    }
    // Seed with every branch tip.
    for name in refs::list_branches(&repo.pv_dir)? {
        if let Some(h) = refs::resolve_branch(&repo.pv_dir, &name)? {
            queue.push(h);
        }
    }
    // Seed with every tag.
    for name in refs::list_tags(&repo.pv_dir)? {
        if let Some(h) = refs::resolve_tag(&repo.pv_dir, &name)? {
            queue.push(h);
        }
    }

    while let Some(hash) = queue.pop() {
        if !seen.insert(hash.clone()) {
            continue; // already visited
        }
        let commit = objects::read_commit(&repo.pv_dir, &hash)?;
        if let Some(p) = commit.parent {
            queue.push(p);
        }
    }

    Ok(seen)
}
