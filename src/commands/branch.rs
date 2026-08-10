use anyhow::{bail, Result};

use crate::core::refs;
use crate::core::repository::Repo;
use crate::ui::printer;

pub fn run(name: Option<&str>, delete: Option<&str>) -> Result<()> {
    let repo = Repo::find()?;

    // Delete mode.
    if let Some(n) = delete {
        let cur = repo.current_branch()?;
        if cur.as_deref() == Some(n) {
            bail!("cannot delete the currently checked out branch '{n}'");
        }
        refs::delete_branch(&repo.pv_dir, n)?;
        printer::ok(&format!("Deleted branch '{n}'"));
        return Ok(());
    }

    // Create mode.
    if let Some(n) = name {
        if refs::branch_exists(&repo.pv_dir, n) {
            bail!("branch '{n}' already exists");
        }
        let Some(commit) = repo.head_commit()? else {
            bail!("cannot create branch with no commits yet");
        };
        refs::create_branch(&repo.pv_dir, n, &commit)?;
        printer::ok(&format!("Created branch '{n}' at {short}", short = crate::core::safe::short_hash(&commit)));
        return Ok(());
    }

    // List mode.
    let branches = refs::list_branches(&repo.pv_dir)?;
    if branches.is_empty() {
        printer::warn("no branches yet");
        return Ok(());
    }
    let cur = repo.current_branch()?;
    for b in &branches {
        if cur.as_deref() == Some(b.as_str()) {
            println!("* {}", printer::bold(b));
        } else {
            println!("  {b}");
        }
    }
    Ok(())
}
