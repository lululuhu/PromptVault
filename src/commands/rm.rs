use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::core::repository::Repo;
use crate::ui::printer;

pub fn run(paths: Vec<PathBuf>) -> Result<()> {
    let repo = Repo::find()?;
    let _lock = repo.lock()?;
    let mut idx = repo.index()?;
    let mut removed = 0usize;

    for p in &paths {
        let rel = p.to_string_lossy().replace('\\', "/");
        let before = idx.entries.len();
        idx.entries.retain(|e| e.path != rel);
        if idx.entries.len() == before {
            bail!("not tracked: {rel}");
        }
        println!("{} {}", printer::bold("removed:"), rel);
        removed += 1;
    }

    if removed == 0 {
        bail!("nothing to remove");
    }
    repo.save_index(&idx)?;
    Ok(())
}
