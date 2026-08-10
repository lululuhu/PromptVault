//! Repository discovery and high-level helpers.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use crate::core::{index, lock, refs};

pub struct Repo {
    pub root: PathBuf,
    pub pv_dir: PathBuf,
}

impl Repo {
    /// Walk up from the current directory to find a `.pv` folder.
    pub fn find() -> Result<Self> {
        let mut cur = std::env::current_dir()?;
        loop {
            let pv = cur.join(".pv");
            if pv.is_dir() {
                return Ok(Repo {
                    root: cur,
                    pv_dir: pv,
                });
            }
            if !cur.pop() {
                bail!("not a prompt vault (or any parent up to /): run `pv init` first");
            }
        }
    }

    pub fn index(&self) -> Result<index::Index> {
        index::Index::load(&self.pv_dir)
    }

    pub fn save_index(&self, idx: &index::Index) -> Result<()> {
        idx.save(&self.pv_dir)
    }

    /// Acquire an exclusive repository lock. Drop the guard to release.
    ///
    /// Call this at the start of any command that mutates shared state
    /// (index, refs, HEAD) to prevent concurrent writes from corrupting
    /// the vault.
    pub fn lock(&self) -> Result<lock::FileLock> {
        lock::FileLock::acquire(&self.pv_dir)
    }

    pub fn head_commit(&self) -> Result<Option<String>> {
        refs::resolve_head(&self.pv_dir)
    }

    pub fn current_branch(&self) -> Result<Option<String>> {
        refs::current_branch(&self.pv_dir)
    }
}

/// File extensions treated as prompt files when adding a directory.
const PROMPT_EXTS: &[&str] = &[
    "txt", "md", "prompt", "prom", "j2", "jinja", "jinja2", "tmpl", "mustache", "liquid", "yaml",
    "yml",
];

pub fn is_prompt_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| PROMPT_EXTS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Initialize a vault at `path`.
pub fn init(path: &Path) -> Result<()> {
    let pv_dir = path.join(".pv");
    if pv_dir.exists() {
        bail!("already a prompt vault: {}", pv_dir.display());
    }
    fs::create_dir_all(pv_dir.join("objects"))?;
    fs::create_dir_all(pv_dir.join("refs").join("heads"))?;
    refs::init_head(&pv_dir, refs::DEFAULT_BRANCH)?;
    index::Index::default().save(&pv_dir)?;
    Ok(())
}
