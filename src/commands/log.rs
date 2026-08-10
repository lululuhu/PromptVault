use anyhow::Result;
use chrono::TimeZone;
use chrono::Utc;

use crate::core::objects;
use crate::core::repository::Repo;
use crate::ui::printer;

pub fn run() -> Result<()> {
    let repo = Repo::find()?;
    let mut cur = repo.head_commit()?;

    if cur.is_none() {
        printer::warn("no commits yet");
        return Ok(());
    }

    while let Some(hash) = cur {
        let commit = objects::read_commit(&repo.pv_dir, &hash)?;
        let dt = Utc.timestamp_opt(commit.timestamp, 0).single();
        let when = dt
            .map(|d| d.format("%a %b %e %H:%M:%S %Y %z").to_string())
            .unwrap_or_default();

        println!("{}", printer::bold(&format!("commit {hash}")));
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

        cur = commit.parent;
    }
    Ok(())
}
