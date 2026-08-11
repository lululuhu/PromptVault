//! `pv blame <path>` — show which commit last touched each line of a prompt.
//!
//! For each line in the current (HEAD) version, reports the most recent commit
//! whose version of that line differs from its parent's version — i.e. the
//! commit that introduced that exact line content. This mirrors `git blame`
//! semantics: a line is attributed to the commit that last added it (not the
//! commit that last touched the file).

use anyhow::Result;

use crate::core::objects;
use crate::core::repository::Repo;
use crate::core::safe;
use crate::ui::printer;
use crate::diff::split_lines;

pub fn run(path: &str) -> Result<()> {
    let repo = Repo::find()?;
    let head = repo
        .head_commit()?
        .ok_or_else(|| anyhow::anyhow!("no commits yet"))?;

    // Resolve the path to its current blob hash in HEAD's tree.
    let head_commit = objects::read_commit(&repo.pv_dir, &head)?;
    let head_tree = objects::read_tree(&repo.pv_dir, &head_commit.tree)?;
    let entry = head_tree
        .iter()
        .find(|e| e.path == path)
        .ok_or_else(|| anyhow::anyhow!("'{path}' is not tracked in HEAD"))?;
    let cur_blob = objects::read_object(&repo.pv_dir, &entry.hash)?;
    let cur_lines = split_lines(&String::from_utf8_lossy(&cur_blob.data));

    // Walk history from HEAD backwards, collecting (commit_hash, lines_at_this_commit)
    // in newest→oldest order. For each commit we also record its parent's version
    // of the file (empty if the file didn't exist yet).
    //
    // A line L in `cur_lines` is attributed to the newest commit C such that:
    //   L is present in C's version  AND  L is NOT present in C's parent's version.
    // (If C has no parent — i.e. it's the root — then L is attributed to C iff
    // L is present in C's version.)
    let mut blame: Vec<Option<String>> = vec![None; cur_lines.len()];
    let mut remaining = cur_lines.len();

    // Collect the chain newest → oldest, with each commit's file content and
    // its parent's file content.
    let mut chain: Vec<(String, Vec<String>, Vec<String>)> = Vec::new();
    let mut cur_hash_opt: Option<String> = Some(head.clone());
    let mut seen = std::collections::HashSet::new();
    while let Some(h) = cur_hash_opt {
        if !seen.insert(h.clone()) {
            break; // cycle guard
        }
        let commit = objects::read_commit(&repo.pv_dir, &h)?;
        let tree = objects::read_tree(&repo.pv_dir, &commit.tree)?;
        let this_lines: Vec<String> = match tree.iter().find(|e| e.path == path) {
            Some(e) => {
                let blob = objects::read_object(&repo.pv_dir, &e.hash)?;
                split_lines(&String::from_utf8_lossy(&blob.data))
            }
            None => Vec::new(),
        };
        // Parent's version of the file.
        let parent_lines: Vec<String> = match &commit.parent {
            Some(p) => {
                let pc = objects::read_commit(&repo.pv_dir, p)?;
                let pt = objects::read_tree(&repo.pv_dir, &pc.tree)?;
                match pt.iter().find(|e| e.path == path) {
                    Some(e) => {
                        let blob = objects::read_object(&repo.pv_dir, &e.hash)?;
                        split_lines(&String::from_utf8_lossy(&blob.data))
                    }
                    None => Vec::new(),
                }
            }
            None => Vec::new(),
        };
        chain.push((h.clone(), this_lines, parent_lines));
        cur_hash_opt = commit.parent;
    }

    // Walk newest → oldest. First commit that "introduces" a line wins.
    for (hash, this_lines, parent_lines) in &chain {
        if remaining == 0 {
            break;
        }
        for (i, line) in cur_lines.iter().enumerate() {
            if blame[i].is_none()
                && this_lines.contains(line)
                && !parent_lines.contains(line)
            {
                blame[i] = Some(hash.clone());
                remaining -= 1;
            }
        }
    }

    // Any still-unblamed lines (e.g. the root commit's lines that weren't caught
    // because of dedup edge cases) fall back to the oldest commit that has them.
    if remaining > 0 {
        for (hash, this_lines, _parent_lines) in chain.iter().rev() {
            if remaining == 0 {
                break;
            }
            for (i, line) in cur_lines.iter().enumerate() {
                if blame[i].is_none() && this_lines.contains(line) {
                    blame[i] = Some(hash.clone());
                    remaining -= 1;
                }
            }
        }
    }

    // Render.
    println!("{}", printer::bold(&format!("blame: {path}")));
    for (i, line) in cur_lines.iter().enumerate() {
        let who = blame[i]
            .as_ref()
            .map(|h| safe::short_hash(h).to_string())
            .unwrap_or_else(|| "???????".to_string());
        println!("{}  {:>4}  {}", printer::dim(&who), i + 1, line);
    }
    Ok(())
}
