use std::collections::HashSet;

use anyhow::Result;

use crate::core::ignore::IgnoreSet;
use crate::core::objects::{self, hash_blob};
use crate::core::repository::{is_prompt_file, Repo};
use crate::ui::printer;

pub fn run() -> Result<()> {
    let repo = Repo::find()?;
    let idx = repo.index()?;
    let head_entries = head_tree_entries(&repo)?;

    let head_map: std::collections::HashMap<String, String> = head_entries
        .iter()
        .map(|e| (e.path.clone(), e.hash.clone()))
        .collect();

    // Staged: index differs from HEAD (or new in index).
    let mut staged_new = Vec::new();
    let mut staged_mod = Vec::new();
    for e in &idx.entries {
        match head_map.get(&e.path) {
            None => staged_new.push(e.path.clone()),
            Some(h) if *h != e.hash => staged_mod.push(e.path.clone()),
            _ => {}
        }
    }

    // Not staged: tracked (in HEAD) files whose working tree differs.
    let mut unstaged = Vec::new();
    for e in &head_entries {
        let abs = repo.root.join(&e.path);
        if let Ok(data) = std::fs::read(&abs) {
            if hash_blob(&data) != e.hash {
                unstaged.push(e.path.clone());
            }
        } else {
            unstaged.push(e.path.clone());
        }
    }

    // Untracked: prompt files not in HEAD and not in index.
    let known: HashSet<String> = idx.entries.iter().map(|e| e.path.clone()).collect();
    let ignore = IgnoreSet::load(&repo.root);
    let mut untracked = Vec::new();
    collect_prompt_files(&repo.root, &repo.root, &known, &ignore, &mut untracked)?;

    let branch = repo.current_branch()?.unwrap_or_else(|| "HEAD".into());
    let head_state = match repo.head_commit()? {
        Some(h) => format!("{} on {branch}", crate::core::safe::short_hash(&h)),
        None => format!("no commits on {branch}"),
    };
    println!("On branch {branch} ({})", printer::dim(&head_state));
    println!();

    if !staged_new.is_empty() || !staged_mod.is_empty() {
        println!("{}", printer::bold("Changes to be committed:"));
        for p in &staged_new {
            println!("      {}  {}", printer::dim("new"), p);
        }
        for p in &staged_mod {
            println!("      {}  {}", printer::dim("modified"), p);
        }
        println!();
    }

    if !unstaged.is_empty() {
        println!("{}", printer::bold("Changes not staged for commit:"));
        for p in &unstaged {
            println!("      {}  {}", printer::dim("modified"), p);
        }
        println!();
    }

    if !untracked.is_empty() {
        println!("{}", printer::bold("Untracked files:"));
        for p in &untracked {
            println!("      {}", p);
        }
        println!();
    }

    if staged_new.is_empty()
        && staged_mod.is_empty()
        && unstaged.is_empty()
        && untracked.is_empty()
    {
        printer::ok("working tree clean");
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

fn collect_prompt_files(
    root: &std::path::Path,
    dir: &std::path::Path,
    known: &HashSet<String>,
    ignore: &IgnoreSet,
    out: &mut Vec<String>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        let name = entry.file_name();
        if crate::core::safe::is_skip_dir(&name.to_string_lossy()) {
            continue;
        }
        if p.is_dir() {
            collect_prompt_files(root, &p, known, ignore, out)?;
        } else if is_prompt_file(&p) {
            let rel = p
                .strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            if !known.contains(&rel) && !ignore.is_ignored(&rel) {
                out.push(rel);
            }
        }
    }
    Ok(())
}
