use std::collections::HashSet;

use anyhow::Result;
use chrono::TimeZone;
use chrono::Utc;

use crate::core::objects;
use crate::core::repository::Repo;
use crate::ui::printer;

pub fn run(max_count: Option<usize>, oneline: bool) -> Result<()> {
    let repo = Repo::find()?;
    let mut cur = repo.head_commit()?;

    if cur.is_none() {
        printer::warn("no commits yet");
        return Ok(());
    }

    let mut seen: HashSet<String> = HashSet::new();
    let mut shown = 0usize;
    while let Some(hash) = cur {
        // Cycle guard: corrupted history with a loop won't hang forever.
        if !seen.insert(hash.clone()) {
            printer::warn(&format!("cycle detected at {hash}; stopping"));
            break;
        }

        let commit = objects::read_commit(&repo.pv_dir, &hash)?;

        if oneline {
            let first = commit.message.lines().next().unwrap_or("");
            println!("{} {first}", crate::core::safe::short_hash(&hash));
        } else {
            let dt = Utc.timestamp_opt(commit.timestamp, 0).single();
            let when = dt
                .map(|d| d.format("%a %b %e %H:%M:%S %Y %z").to_string())
                .unwrap_or_default();

            println!("{}", printer::bold(&format!("commit {}", hash)));
            if let Some(p) = &commit.parent {
                println!("parent {p}");
            }
            if let Some(a) = &commit.author {
                println!("Author: {a}");
            }
            println!("Date:   {when}");
            println!();
            for line in commit.message.lines() {
                println!("    {line}");
            }
            println!();
        }

        shown += 1;
        if let Some(limit) = max_count {
            if shown >= limit {
                break;
            }
        }
        cur = commit.parent;
    }
    Ok(())
}
