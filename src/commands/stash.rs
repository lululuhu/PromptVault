//! `pv stash` — temporarily shelve uncommitted changes (staged + working tree).
//!
//! Stores the working tree state in `.pv/stash` (binary-safe). Re-applies on `pop`.
//! One-slot stash (simple, like git's default). The HEAD tree is not touched.

use std::fs;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::core::ignore::IgnoreSet;
use crate::core::objects::{self, ObjectType};
use crate::core::repository::{is_prompt_file, Repo};
use crate::core::safe;
use crate::ui::printer;

const STASH_FILE: &str = "stash";

#[derive(Serialize, Deserialize, Default)]
struct Stash {
    /// path -> blob content (the working tree version at stash time), binary-safe
    files: Vec<(String, Vec<u8>)>,
}

pub fn push() -> Result<()> {
    let repo = Repo::find()?;
    let stash_path = repo.pv_dir.join(STASH_FILE);
    if stash_path.exists() {
        bail!("a stash already exists; run `pv stash pop` or `pv stash drop` first");
    }

    let idx = repo.index()?;
    let head_entries = head_tree_entries(&repo)?;

    // Collect current working tree content for every tracked (index or HEAD) prompt.
    let mut paths: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for e in &idx.entries {
        paths.insert(e.path.clone());
    }
    for e in &head_entries {
        paths.insert(e.path.clone());
    }

    let mut stash = Stash::default();
    let mut dirty = false;
    for p in &paths {
        let abs = repo.root.join(p);
        let content = match fs::read(&abs) {
            Ok(c) => c,
            Err(_) => {
                // File deleted from working tree — record as deleted (skip content).
                dirty = true;
                continue;
            }
        };
        // Is it different from HEAD?
        let head_hash = head_entries.iter().find(|e| &e.path == p).map(|e| &e.hash);
        let cur_hash = objects::hash_blob(&content);
        if head_hash.map(|h| h != &cur_hash).unwrap_or(true) {
            dirty = true;
        }
        stash.files.push((p.clone(), content));
    }

    if !dirty {
        bail!("no local changes to stash");
    }

    let json = serde_json::to_string(&stash)?;
    safe::atomic_write(&stash_path, json.as_bytes())?;

    // Reset working tree to HEAD.
    let ignore = IgnoreSet::load(&repo.root);
    restore_head_tree(&repo, &head_entries, &ignore)?;

    printer::ok(&format!(
        "Stashed {} file(s). Working tree reset to HEAD.",
        stash.files.len()
    ));
    printer::info("run `pv stash pop` to restore");
    Ok(())
}

pub fn pop() -> Result<()> {
    let repo = Repo::find()?;
    let stash_path = repo.pv_dir.join(STASH_FILE);
    if !stash_path.exists() {
        bail!("no stash to pop");
    }

    // Safety: refuse to overwrite uncommitted changes.
    if let Err(e) = safe::check_clean_working_tree(&repo) {
        bail!(
            "cannot pop stash: {e}\n\n\
             commit or stash your current changes first, or use `pv stash drop` to discard the stash."
        );
    }

    let json = fs::read_to_string(&stash_path)?;
    let stash: Stash = serde_json::from_str(&json)?;

    for (path, content) in &stash.files {
        let abs = repo.root.join(path);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&abs, content)?;
        let h = objects::write_object(&repo.pv_dir, ObjectType::Blob, content)?;
        let mut idx = repo.index()?;
        idx.add(path, &h);
        repo.save_index(&idx)?;
        println!("{}  {}", printer::dim("restored"), path);
    }

    // Only drop the stash after a successful restore.
    fs::remove_file(&stash_path)?;
    printer::ok(&format!("Popped stash ({} file(s)).", stash.files.len()));
    Ok(())
}

pub fn drop() -> Result<()> {
    let repo = Repo::find()?;
    let stash_path = repo.pv_dir.join(STASH_FILE);
    if !stash_path.exists() {
        bail!("no stash to drop");
    }
    fs::remove_file(&stash_path)?;
    printer::ok("Dropped stash.");
    Ok(())
}

pub fn list() -> Result<()> {
    let repo = Repo::find()?;
    let stash_path = repo.pv_dir.join(STASH_FILE);
    if !stash_path.exists() {
        printer::info("no stash");
        return Ok(());
    }
    let json = fs::read_to_string(&stash_path)?;
    let stash: Stash = serde_json::from_str(&json)?;
    println!("stash ({} file(s)):", stash.files.len());
    for (p, _) in &stash.files {
        println!("  {}", p);
    }
    Ok(())
}

fn head_tree_entries(repo: &Repo) -> Result<Vec<objects::TreeEntry>> {
    let Some(h) = repo.head_commit()? else {
        return Ok(Vec::new());
    };
    let commit = objects::read_commit(&repo.pv_dir, &h)?;
    Ok(objects::read_tree(&repo.pv_dir, &commit.tree)?)
}

fn restore_head_tree(
    repo: &Repo,
    head_entries: &[objects::TreeEntry],
    ignore: &IgnoreSet,
) -> Result<()> {
    let head_paths: std::collections::HashSet<&String> =
        head_entries.iter().map(|e| &e.path).collect();

    // Remove any prompt file in working tree that is not in HEAD (and not ignored).
    let mut to_remove = Vec::new();
    collect_prompt_files(&repo.root, &repo.root, ignore, &mut to_remove)?;
    for p in to_remove {
        if !head_paths.contains(&p) {
            let abs = repo.root.join(&p);
            if abs.exists() {
                let _ = fs::remove_file(&abs);
            }
        }
    }

    // Restore HEAD versions.
    for e in head_entries {
        let abs = repo.root.join(&e.path);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent)?;
        }
        let blob = objects::read_object(&repo.pv_dir, &e.hash)?;
        fs::write(&abs, &blob.data)?;
    }

    // Reset index to HEAD.
    let mut idx = repo.index()?;
    idx.entries.clear();
    for e in head_entries {
        idx.add(&e.path, &e.hash);
    }
    repo.save_index(&idx)?;
    Ok(())
}

fn collect_prompt_files(
    root: &std::path::Path,
    dir: &std::path::Path,
    ignore: &IgnoreSet,
    out: &mut Vec<String>,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        let name = entry.file_name();
        if crate::core::safe::is_skip_dir(&name.to_string_lossy()) {
            continue;
        }
        // Use symlink_metadata so we don't follow symlinks (avoid cycles / escapes).
        let ft = entry.file_type()?;
        if ft.is_dir() {
            collect_prompt_files(root, &p, ignore, out)?;
        } else if ft.is_file() && is_prompt_file(&p) {
            let rel = p
                .strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            if !ignore.is_ignored(&rel) {
                out.push(rel);
            }
        }
    }
    Ok(())
}
