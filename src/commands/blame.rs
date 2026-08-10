//! `pv blame <path>` — show which commit last touched each line of a prompt.
//!
//! Walks history from HEAD backwards; for each line in the current version,
//! reports the most recent commit whose version of that line differs from its
//! parent's version (i.e. the commit that introduced that exact line).

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

    // For each line, find the commit that introduced it by walking history.
    let mut blame: Vec<Option<String>> = vec![None; cur_lines.len()];
    let mut remaining = cur_lines.len();

    let mut cur_hash = Some(head.clone());
    let mut prev_lines: Option<Vec<String>> = None;
    let mut seen = std::collections::HashSet::new();

    while let Some(h) = cur_hash {
        if !seen.insert(h.clone()) {
            break; // cycle guard
        }
        let commit = objects::read_commit(&repo.pv_dir, &h)?;
        let tree = objects::read_tree(&repo.pv_dir, &commit.tree)?;
        let this_entry = tree.iter().find(|e| e.path == path);

        let this_lines: Vec<String> = if let Some(e) = this_entry {
            let blob = objects::read_object(&repo.pv_dir, &e.hash)?;
            split_lines(&String::from_utf8_lossy(&blob.data))
        } else {
            // File didn't exist in this commit — treat as empty.
            Vec::new()
        };

        // Lines present in `this_lines` but not in `prev_lines` were introduced
        // by this commit. (We process newest → oldest, so first hit wins.)
        if let Some(prev) = &prev_lines {
            for (i, line) in cur_lines.iter().enumerate() {
                if blame[i].is_none() && this_lines.contains(line) && !prev.contains(line) {
                    blame[i] = Some(h.clone());
                    remaining -= 1;
                }
            }
        } else {
            // Newest commit — every line present in its version is "introduced" here
            // unless it also existed in the parent (handled next iteration).
            // Defer: we'll mark remaining lines on the parent step.
        }

        if remaining == 0 {
            break;
        }

        prev_lines = Some(this_lines);
        cur_hash = commit.parent;
    }

    // Any still-unblamed lines belong to the oldest reachable version of the file
    // (or the file's creation). Attribute them to the oldest commit we visited.
    if remaining > 0 {
        if let Some(oldest) = seen.iter().min_by_key(|h| {
            // Approximate "oldest" by smallest timestamp. Cheap and good enough for blame.
            objects::read_commit(&repo.pv_dir, h)
                .map(|c| c.timestamp)
                .unwrap_or(0)
        }) {
            for b in blame.iter_mut() {
                if b.is_none() {
                    *b = Some(oldest.clone());
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
