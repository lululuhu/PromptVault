//! Remote sync via a nested git repo inside `.pv/`.
//!
//! Design: PromptVault stores its content-addressed objects under `.pv/`.
//! To sync, we initialize `.pv/` itself as a git repo, commit the objects,
//! and push/pull to any git remote. This avoids native libgit2 dependencies
//! and works with any git host (GitHub, GitLab, Gitea, a bare repo, …).
//!
//! The user's prompts themselves are NOT committed to this sync repo — only
//! the `.pv/` vault internals (objects, refs, index) are. Prompts stay where
//! they are on the user's working tree.

use std::process::Command;

use anyhow::{bail, Context, Result};

use crate::core::repository::Repo;
use crate::ui::printer;

pub fn add(name: &str, url: &str) -> Result<()> {
    let repo = Repo::find()?;
    ensure_git_init(&repo.pv_dir)?;
    // `git remote add` fails if it already exists; remove first for idempotency.
    let _ = run_git(&repo.pv_dir, &["remote", "remove", name]);
    run_git(&repo.pv_dir, &["remote", "add", name, url])
        .with_context(|| format!("failed to add remote '{name}'"))?;
    printer::ok(&format!("Added remote '{name}' -> {url}"));
    Ok(())
}

pub fn list() -> Result<()> {
    let repo = Repo::find()?;
    let out = run_git(&repo.pv_dir, &["remote", "-v"])?;
    if out.trim().is_empty() {
        printer::warn("no remotes configured (use `pv remote add <name> <url>`)");
        return Ok(());
    }
    print!("{out}");
    Ok(())
}

pub fn remove(name: &str) -> Result<()> {
    let repo = Repo::find()?;
    run_git(&repo.pv_dir, &["remote", "remove", name])?;
    printer::ok(&format!("Removed remote '{name}'"));
    Ok(())
}

pub fn push(remote: &str) -> Result<()> {
    let repo = Repo::find()?;
    ensure_git_init(&repo.pv_dir)?;
    commit_vault(&repo.pv_dir, "auto-sync before push")?;
    printer::info(&format!("Pushing to '{remote}'…"));
    run_git(&repo.pv_dir, &["push", "-u", remote, "HEAD"])?;
    printer::ok(&format!("Pushed to '{remote}'"));
    Ok(())
}

pub fn pull(remote: &str) -> Result<()> {
    let repo = Repo::find()?;
    ensure_git_init(&repo.pv_dir)?;

    // If the sync repo has no commits yet, the incoming merge would collide
    // with the freshly-generated `HEAD` / `.gitignore` files. In that case we
    // fetch and hard-reset to the remote — those files are regenerated from
    // objects anyway, so overwriting them is safe.
    let has_local_commits = run_git(&repo.pv_dir, &["rev-parse", "--verify", "HEAD"]).is_ok();

    printer::info(&format!("Pulling from '{remote}'…"));
    if !has_local_commits {
        // Fresh sync repo: fetch + reset to remote HEAD.
        run_git(&repo.pv_dir, &["fetch", "--quiet", remote])?;
        run_git(&repo.pv_dir, &["reset", "--hard", "FETCH_HEAD"])?;
    } else {
        let out = run_git(&repo.pv_dir, &["pull", "--no-rebase", remote, "HEAD"])?;
        if !out.trim().is_empty() {
            print!("{out}");
        }
    }
    printer::ok(&format!("Pulled from '{remote}'"));
    Ok(())
}

fn ensure_git_init(pv_dir: &std::path::Path) -> Result<()> {
    if !pv_dir.join(".git").exists() {
        run_git(pv_dir, &["init", "--quiet"])?;
        run_git(pv_dir, &["config", "user.email", "pv@local"])?;
        run_git(pv_dir, &["config", "user.name", "PromptVault"])?;
        // The index.json may conflict with git's own index; ignore it from sync
        // to avoid confusion — it gets rebuilt from objects on next operation anyway.
        let ignore = "index.json\n";
        std::fs::write(pv_dir.join(".gitignore"), ignore)?;
    }
    Ok(())
}

fn commit_vault(pv_dir: &std::path::Path, msg: &str) -> Result<()> {
    run_git(pv_dir, &["add", "-A"])?;
    // `commit` returns non-zero if nothing to commit — treat that as success.
    let res = run_git(pv_dir, &["commit", "--quiet", "-m", msg]);
    match res {
        Ok(_) => Ok(()),
        Err(_) => Ok(()), // nothing to commit
    }
}

fn run_git(dir: &std::path::Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .with_context(|| "failed to run `git` — is git installed?")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        bail!(
            "git {} failed:\n{}{}",
            args.join(" "),
            stdout,
            stderr
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
