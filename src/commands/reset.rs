//! `pv reset [<path>]` — unstage a staged file (or all staged files).
//!
//! Like `git reset <path>` / `git reset`. Leaves the working tree untouched.

use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::core::objects;
use crate::core::repository::Repo;
use crate::ui::printer;

pub fn run(paths: Vec<PathBuf>) -> Result<()> {
    let repo = Repo::find()?;
    let _lock = repo.lock()?;
    let mut idx = repo.index()?;

    if paths.is_empty() {
        // Reset everything to HEAD.
        let head_entries = head_tree_entries(&repo)?;
        idx.entries.clear();
        for e in &head_entries {
            idx.add(&e.path, &e.hash);
        }
        repo.save_index(&idx)?;
        printer::ok(&format!("Reset {} staged file(s) to HEAD.", idx.entries.len()));
        return Ok(());
    }

    let head_entries = head_tree_entries(&repo)?;
    let head_map: std::collections::HashMap<String, String> = head_entries
        .iter()
        .cloned()
        .map(|e| (e.path, e.hash))
        .collect();

    let mut reset = 0usize;
    for p in &paths {
        let rel = p.to_string_lossy().replace('\\', "/");
        let before = idx.entries.len();
        idx.entries.retain(|e| e.path != rel);
        if idx.entries.len() == before {
            bail!("not staged: {rel}");
        }
        // If HEAD tracks it, re-add HEAD's version.
        if let Some(h) = head_map.get(&rel) {
            idx.add(&rel, h);
        }
        println!("{}  {}", printer::dim("unstaged"), rel);
        reset += 1;
    }

    if reset == 0 {
        bail!("nothing to reset");
    }
    repo.save_index(&idx)?;
    Ok(())
}

fn head_tree_entries(repo: &Repo) -> Result<Vec<objects::TreeEntry>> {
    let Some(h) = repo.head_commit()? else {
        return Ok(Vec::new());
    };
    let commit = objects::read_commit(&repo.pv_dir, &h)?;
    Ok(objects::read_tree(&repo.pv_dir, &commit.tree)?)
}
