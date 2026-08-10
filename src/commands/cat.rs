use std::path::PathBuf;

use anyhow::{bail, Result};

pub fn run(path: PathBuf) -> Result<()> {
    let content = std::fs::read_to_string(&path).map_err(|_| {
        anyhow::anyhow!("cannot read: {}", path.display())
    })?;
    if content.is_empty() {
        bail!("file is empty: {}", path.display());
    }
    print!("{content}");
    Ok(())
}
