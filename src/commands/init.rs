use std::path::Path;

use crate::core::repository;
use crate::ui::printer;

pub fn run(path: Option<&Path>) -> anyhow::Result<()> {
    let dir = match path {
        Some(p) => {
            std::fs::create_dir_all(p)?;
            std::fs::canonicalize(p)?
        }
        None => std::env::current_dir()?,
    };
    repository::init(&dir)?;
    printer::ok(&format!(
        "Initialized empty prompt vault in {}",
        dir.join(".pv").display()
    ));
    Ok(())
}
