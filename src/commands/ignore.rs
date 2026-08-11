//! `pv ignore add/list/rm` — manage `.pvignore` patterns.
//!
//! Lets users add, list, and remove ignore patterns without editing the file
//! by hand. Patterns are stored one per line in `<root>/.pvignore`.

use std::fs;

use anyhow::{bail, Result};

use crate::core::repository::Repo;
use crate::ui::printer;

pub fn add(patterns: &[String]) -> Result<()> {
    if patterns.is_empty() {
        bail!("no patterns given: `pv ignore add <pattern>...`");
    }
    let repo = Repo::find()?;
    let _lock = repo.lock()?;
    let path = repo.root.join(".pvignore");

    let mut content = fs::read_to_string(&path).unwrap_or_default();
    // Ensure the existing content ends with a newline so appended patterns
    // don't get glued onto a previous line.
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }

    // Collect existing patterns so we don't add duplicates.
    let existing: std::collections::HashSet<String> = content
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();

    let mut added = 0usize;
    for p in patterns {
        let trimmed = p.trim();
        if trimmed.is_empty() {
            continue;
        }
        if existing.contains(trimmed) {
            println!("{}  {} (already ignored)", printer::dim("skip"), trimmed);
            continue;
        }
        content.push_str(trimmed);
        content.push('\n');
        added += 1;
        println!("{}  {}", printer::dim("added"), trimmed);
    }

    if added == 0 {
        printer::info("nothing to add");
        return Ok(());
    }

    crate::core::safe::atomic_write(&path, content.as_bytes())?;
    printer::ok(&format!("{added} pattern(s) added to .pvignore"));
    Ok(())
}

pub fn list() -> Result<()> {
    let repo = Repo::find()?;
    let path = repo.root.join(".pvignore");
    if !path.exists() {
        printer::info("no .pvignore file");
        return Ok(());
    }
    let content = fs::read_to_string(&path)?;
    let mut seen = std::collections::HashSet::new();
    let mut count = 0usize;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Deduplicate: a pattern duplicated in the file is only listed once.
        if !seen.insert(line.to_string()) {
            continue;
        }
        println!("{line}");
        count += 1;
    }
    if count == 0 {
        printer::info(".pvignore has no patterns");
    }
    Ok(())
}

pub fn remove(patterns: &[String]) -> Result<()> {
    if patterns.is_empty() {
        bail!("no patterns given: `pv ignore rm <pattern>...`");
    }
    let repo = Repo::find()?;
    let _lock = repo.lock()?;
    let path = repo.root.join(".pvignore");
    if !path.exists() {
        bail!("no .pvignore file to remove from");
    }

    let content = fs::read_to_string(&path)?;
    let mut kept = Vec::new();
    let mut removed = 0usize;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            // Preserve blanks and comments verbatim.
            kept.push(line.to_string());
            continue;
        }
        if patterns.iter().any(|p| p.trim() == trimmed) {
            println!("{}  {}", printer::dim("removed"), trimmed);
            removed += 1;
        } else {
            kept.push(line.to_string());
        }
    }

    if removed == 0 {
        printer::info("no matching patterns found");
        return Ok(());
    }

    // Rebuild, trimming trailing blank lines for tidiness.
    while kept.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        kept.pop();
    }
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }

    if out.is_empty() {
        // All patterns removed — delete the now-empty file.
        let _ = fs::remove_file(&path);
    } else {
        crate::core::safe::atomic_write(&path, out.as_bytes())?;
    }

    printer::ok(&format!("{removed} pattern(s) removed from .pvignore"));
    Ok(())
}
