//! `pv clean` — remove untracked prompt files from the working tree.
//!
//! Like `git clean -f`. Only deletes prompt files (not arbitrary files).
//! Use `--dry-run` / `-n` to preview. Use `-f` to actually delete.

use std::collections::HashSet;

use anyhow::Result;

use crate::core::ignore::IgnoreSet;
use crate::core::repository::{is_prompt_file, Repo};
use crate::ui::printer;

pub fn run(dry_run: bool, force: bool) -> Result<()> {
    if !dry_run && !force {
        anyhow::bail!("refusing to clean: pass -f to delete, or -n to preview");
    }

    let repo = Repo::find()?;
    let idx = repo.index()?;
    let head_entries = head_tree_entries(&repo)?;

    let known: HashSet<String> = idx.entries.iter().map(|e| e.path.clone()).collect();
    let ignore = IgnoreSet::load(&repo.root);
    let mut untracked = Vec::new();
    collect_prompt_files(&repo.root, &repo.root, &known, &head_entries, &ignore, &mut untracked)?;

    if untracked.is_empty() {
        printer::info("no untracked files");
        return Ok(());
    }

    let action = if dry_run { "would remove" } else { "removing" };
    for p in &untracked {
        println!("{}  {}", printer::dim(action), p);
        if !dry_run {
            let abs = repo.root.join(p);
            let _ = std::fs::remove_file(&abs);
        }
    }
    printer::ok(&format!("{} file(s) {}", untracked.len(), if dry_run { "previewed" } else { "removed" }));
    Ok(())
}

fn head_tree_entries(repo: &Repo) -> Result<Vec<crate::core::objects::TreeEntry>> {
    let Some(h) = repo.head_commit()? else {
        return Ok(Vec::new());
    };
    let commit = crate::core::objects::read_commit(&repo.pv_dir, &h)?;
    Ok(crate::core::objects::read_tree(&repo.pv_dir, &commit.tree)?)
}

fn collect_prompt_files(
    root: &std::path::Path,
    dir: &std::path::Path,
    known: &HashSet<String>,
    head_entries: &[crate::core::objects::TreeEntry],
    ignore: &IgnoreSet,
    out: &mut Vec<String>,
) -> Result<()> {
    let head_known: HashSet<String> = head_entries.iter().map(|e| e.path.clone()).collect();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        let name = entry.file_name();
        if crate::core::safe::is_skip_dir(&name.to_string_lossy()) {
            continue;
        }
        if p.is_dir() {
            collect_prompt_files(root, &p, known, head_entries, ignore, out)?;
        } else if is_prompt_file(&p) {
            let rel = p
                .strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            if !known.contains(&rel) && !head_known.contains(&rel) && !ignore.is_ignored(&rel) {
                out.push(rel);
            }
        }
    }
    Ok(())
}
