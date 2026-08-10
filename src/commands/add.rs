use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use crate::core::ignore::IgnoreSet;
use crate::core::objects::{self, ObjectType};
use crate::core::repository::{is_prompt_file, Repo};
use crate::ui::printer;

pub fn run(paths: Vec<PathBuf>) -> Result<()> {
    let repo = Repo::find()?;
    let mut idx = repo.index()?;
    let ignore = IgnoreSet::load(&repo.root);
    let mut added = 0usize;

    for p in &paths {
        let files = collect_files(&repo.root, p)?;
        for f in files {
            if !is_prompt_file(&f) {
                continue;
            }
            let rel = relpath(&repo.root, &f);
            if ignore.is_ignored(&rel) {
                continue;
            }
            let data = std::fs::read(&f)?;
            let h = objects::write_object(&repo.pv_dir, ObjectType::Blob, &data)?;
            idx.add(&rel, &h);
            println!("{} {}", printer::bold("added:"), rel);
            added += 1;
        }
    }

    if added == 0 {
        bail!("no prompt files found in: {:?}", paths);
    }
    repo.save_index(&idx)?;
    Ok(())
}

fn collect_files(root: &Path, p: &Path) -> Result<Vec<PathBuf>> {
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    };
    if abs.is_file() {
        return Ok(vec![abs]);
    }
    if abs.is_dir() {
        let mut out = Vec::new();
        walk(&abs, &mut out)?;
        Ok(out)
    } else {
        bail!("path not found: {}", p.display());
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        let name = entry.file_name();
        // Skip vault / vcs internals.
        if crate::core::safe::is_skip_dir(&name.to_string_lossy()) {
            continue;
        }
        if p.is_dir() {
            walk(&p, out)?;
        } else if is_prompt_file(&p) {
            out.push(p);
        }
    }
    Ok(())
}

fn relpath(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}
