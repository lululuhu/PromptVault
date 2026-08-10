use anyhow::{bail, Result};

use crate::core::refs;
use crate::core::repository::Repo;
use crate::ui::printer;

pub fn run(name: Option<&str>, delete: Option<&str>) -> Result<()> {
    let repo = Repo::find()?;

    if let Some(n) = delete {
        refs::delete_tag(&repo.pv_dir, n)?;
        printer::ok(&format!("Deleted tag '{n}'"));
        return Ok(());
    }

    if let Some(n) = name {
        if refs::tag_exists(&repo.pv_dir, n) {
            bail!("tag '{n}' already exists");
        }
        let Some(commit) = repo.head_commit()? else {
            bail!("cannot create tag with no commits yet");
        };
        refs::create_tag(&repo.pv_dir, n, &commit)?;
        printer::ok(&format!("Created tag '{n}' at {short}", short = &commit[..7]));
        return Ok(());
    }

    let tags = refs::list_tags(&repo.pv_dir)?;
    if tags.is_empty() {
        printer::warn("no tags yet");
        return Ok(());
    }
    let head = repo.head_commit()?;
    for t in &tags {
        let tip = refs::resolve_tag(&repo.pv_dir, t)?;
        let marker = match (&head, &tip) {
            (Some(h), Some(tip)) if h == tip => " -> HEAD",
            _ => "",
        };
        println!("{t}{marker}");
    }
    Ok(())
}
