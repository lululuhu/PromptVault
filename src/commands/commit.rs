use anyhow::{bail, Result};
use chrono::Utc;

use crate::core::config;
use crate::core::objects::{self, Commit};
use crate::core::refs;
use crate::core::repository::Repo;
use crate::ui::printer;

pub fn run(message: &str) -> Result<()> {
    let repo = Repo::find()?;
    let idx = repo.index()?;
    if idx.entries.is_empty() {
        bail!("nothing to commit: stage prompts with `pv add` first");
    }

    let tree_entries = idx.to_tree_entries();
    let tree = objects::write_tree(&repo.pv_dir, &tree_entries)?;
    let parent = repo.head_commit()?;
    let author = config::resolve_author(&repo.pv_dir)?;

    let commit = Commit {
        tree,
        parent: parent.clone(),
        author: Some(author),
        timestamp: Utc::now().timestamp(),
        message: message.to_string(),
    };
    let hash = objects::write_commit(&repo.pv_dir, &commit)?;
    refs::update_current(&repo.pv_dir, &hash)?;

    let short = &hash[..7];
    let branch = repo.current_branch()?.unwrap_or_else(|| "HEAD".into());
    printer::ok(&format!("[{branch} {short}] {message}"));

    let n = tree_entries.len();
    println!(
        " {} prompt{}",
        n,
        if n == 1 { "" } else { "s" }
    );
    Ok(())
}
