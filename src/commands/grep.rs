//! `pv grep <pattern>` — search across all tracked prompts in HEAD.
//!
//! Like `git grep`. Case-insensitive by default. Use `-i` to force exact case.

use anyhow::Result;

use crate::core::objects;
use crate::core::repository::Repo;
use crate::ui::printer;

pub fn run(pattern: &str, case_sensitive: bool) -> Result<()> {
    let repo = Repo::find()?;
    let Some(head) = repo.head_commit()? else {
        printer::info("no commits yet");
        return Ok(());
    };
    let commit = objects::read_commit(&repo.pv_dir, &head)?;
    let entries = objects::read_tree(&repo.pv_dir, &commit.tree)?;

    let needle = if case_sensitive {
        pattern.to_string()
    } else {
        pattern.to_lowercase()
    };

    let mut matches = 0usize;
    for e in &entries {
        let blob = objects::read_object(&repo.pv_dir, &e.hash)?;
        let text = String::from_utf8_lossy(&blob.data);
        for (i, line) in text.lines().enumerate() {
            let hay = if case_sensitive {
                line.to_string()
            } else {
                line.to_lowercase()
            };
            if hay.contains(&needle) {
                println!(
                    "{}{}{}:{}{}",
                    printer::bold(&e.path),
                    printer::dim(":"),
                    printer::dim(&format!("{}", i + 1)),
                    printer::dim(":"),
                    line.trim()
                );
                matches += 1;
            }
        }
    }

    if matches == 0 {
        printer::info(&format!("no matches for '{pattern}'"));
    }
    Ok(())
}
