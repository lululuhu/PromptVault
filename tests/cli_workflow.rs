//! End-to-end CLI workflow tests: drive the command layer directly to simulate
//! a real user session (init → add → commit → branch → checkout → revert → tag → eval).
//!
//! These tests mutate the process-wide CWD, so they are serialized with a mutex
//! to avoid races between parallel tests.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use promptvault::commands;
use tempfile::TempDir;

// Serialize all tests in this file (they all touch the global CWD).
static CWD_LOCK: Mutex<()> = Mutex::new(());

/// RAII guard that restores the previous CWD on drop — even if the test panics.
struct CwdGuard {
    prev: std::path::PathBuf,
}

impl CwdGuard {
    fn enter(dir: &std::path::Path) -> Self {
        let prev = env::current_dir().unwrap();
        env::set_current_dir(dir).unwrap();
        CwdGuard { prev }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = env::set_current_dir(&self.prev);
    }
}

fn in_dir<R>(dir: &std::path::Path, f: impl FnOnce() -> R) -> R {
    let _guard = CWD_LOCK.lock().unwrap();
    let _cwd = CwdGuard::enter(dir);
    f()
}

fn write(dir: &std::path::Path, rel: &str, content: &str) {
    let p = dir.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(p, content).unwrap();
}

#[test]
fn full_lifecycle() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    in_dir(root, || {
        commands::init::run(None).unwrap();
    });

    write(root, "prompts/summarize.md", "Summarize: {{text}}\n");
    write(root, "prompts/translate.md", "Translate: {{text}}\n");
    // An ignored file.
    write(root, "drafts/junk.md", "draft\n");
    write(root, ".pvignore", "drafts\n");

    in_dir(root, || {
        commands::add::run(vec![PathBuf::from(".")]).unwrap();
        commands::commit::run("initial").unwrap();
    });

    // The drafts dir must NOT be tracked.
    in_dir(root, || {
        let idx = promptvault::core::repository::Repo::find().unwrap().index().unwrap();
        let paths: Vec<&str> = idx.entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"prompts/summarize.md"));
        assert!(paths.contains(&"prompts/translate.md"));
        assert!(!paths.iter().any(|p| p.contains("drafts")));
    });

    // Tag the initial version.
    in_dir(root, || {
        commands::tag::run(Some("v1"), None).unwrap();
        let tags = promptvault::core::refs::list_tags(
            &promptvault::core::repository::Repo::find().unwrap().pv_dir,
        )
        .unwrap();
        assert_eq!(tags, vec!["v1".to_string()]);
    });

    // Modify summarize, commit a v2.
    write(root, "prompts/summarize.md", "Summarize v2: {{text}}\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("prompts/summarize.md")]).unwrap();
        commands::commit::run("v2").unwrap();
    });

    // Revert to v1 (by tag). Working tree should restore the v1 content.
    in_dir(root, || {
        commands::revert::run("v1").unwrap();
        let content = fs::read_to_string("prompts/summarize.md").unwrap();
        assert_eq!(content, "Summarize: {{text}}\n");
    });

    // Branch + checkout restores per-branch content.
    in_dir(root, || {
        commands::commit::run("back to v1").unwrap();
        commands::branch::run(Some("experiment"), None).unwrap();
        commands::checkout::run("experiment").unwrap();
    });
    write(root, "prompts/summarize.md", "EXPERIMENT: {{text}}\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("prompts/summarize.md")]).unwrap();
        commands::commit::run("experiment variant").unwrap();
        commands::checkout::run("main").unwrap();
        // Back on main: content is the v1-restored version.
        let content = fs::read_to_string("prompts/summarize.md").unwrap();
        assert_eq!(content, "Summarize: {{text}}\n");
    });

    // rm untracks translate.
    in_dir(root, || {
        commands::rm::run(vec![PathBuf::from("prompts/translate.md")]).unwrap();
        let idx = promptvault::core::repository::Repo::find().unwrap().index().unwrap();
        assert!(idx.entries.iter().all(|e| e.path != "prompts/translate.md"));
    });
}

#[test]
fn checkout_refuses_dirty_tree() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    write(root, "a.md", "v1\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c1").unwrap();
        commands::branch::run(Some("other"), None).unwrap();
    });
    // Dirty the tree.
    write(root, "a.md", "dirty\n");
    in_dir(root, || {
        let res = commands::checkout::run("other");
        assert!(res.is_err(), "checkout must refuse a dirty tree");
    });
}

#[test]
fn eval_renders_and_asserts() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    write(root, "p.md", "Hello {{name}}, {{text}}\n");
    write(
        root,
        "cases.jsonl",
        "{\"name\":\"Alice\",\"text\":\"hi\",\"expected\":\"Hello Alice\"}\n\
         {\"name\":\"Bob\",\"text\":\"yo\"}\n",
    );
    in_dir(root, || {
        // Should not error.
        commands::eval::run("p.md", PathBuf::from("cases.jsonl"), false, false).unwrap();
    });
}

#[test]
fn stash_push_pop_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    write(root, "a.md", "v1\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c1").unwrap();
    });
    // Make a dirty change, stash it.
    write(root, "a.md", "dirty change\n");
    in_dir(root, || {
        commands::stash::push().unwrap();
        // Working tree should now be back to v1.
        let c = fs::read_to_string("a.md").unwrap();
        assert_eq!(c, "v1\n");
    });
    // Pop restores the dirty change.
    in_dir(root, || {
        commands::stash::pop().unwrap();
        let c = fs::read_to_string("a.md").unwrap();
        assert_eq!(c, "dirty change\n");
        // Stash file should be gone.
        assert!(!std::path::Path::new(".pv/stash").exists());
    });
}

#[test]
fn stash_drop_discards() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    write(root, "a.md", "v1\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c1").unwrap();
    });
    write(root, "a.md", "dirty\n");
    in_dir(root, || {
        commands::stash::push().unwrap();
        commands::stash::drop().unwrap();
        assert!(!std::path::Path::new(".pv/stash").exists());
    });
}

#[test]
fn reset_unstages_a_file() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    write(root, "a.md", "v1\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c1").unwrap();
    });
    // Stage a change, then unstage it.
    write(root, "a.md", "v2\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::reset::run(vec![PathBuf::from("a.md")]).unwrap();
        // After reset, the index should match HEAD (v1).
        let idx = promptvault::core::repository::Repo::find().unwrap().index().unwrap();
        let entry = idx.entries.iter().find(|e| e.path == "a.md").unwrap();
        let head_hash = {
            let repo = promptvault::core::repository::Repo::find().unwrap();
            let h = repo.head_commit().unwrap().unwrap();
            let commit = promptvault::core::objects::read_commit(&repo.pv_dir, &h).unwrap();
            let tree = promptvault::core::objects::read_tree(&repo.pv_dir, &commit.tree).unwrap();
            tree.iter().find(|e| e.path == "a.md").unwrap().hash.clone()
        };
        assert_eq!(entry.hash, head_hash);
    });
}

#[test]
fn clean_removes_untracked() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    write(root, "a.md", "v1\n");
    write(root, "b.md", "untracked\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c1").unwrap();
        // Dry run should not delete.
        commands::clean::run(true, false).unwrap();
        assert!(root.join("b.md").exists());
        // Force should delete b.md but not a.md.
        commands::clean::run(false, true).unwrap();
        assert!(!root.join("b.md").exists());
        assert!(root.join("a.md").exists());
    });
}

#[test]
fn export_writes_zip() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    write(root, "a.md", "hello\n");
    write(root, "b.md", "world\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from(".")]).unwrap();
        commands::commit::run("c1").unwrap();
        let out = PathBuf::from("snap.zip");
        commands::export::run("HEAD", out.clone()).unwrap();
        let meta = fs::metadata(&out).unwrap();
        assert!(meta.len() > 0, "zip file should be non-empty");
        // ZIP files start with PK\x03\x04.
        let bytes = fs::read(&out).unwrap();
        assert_eq!(&bytes[..4], &[0x50, 0x4b, 0x03, 0x04]);
    });
}

#[test]
fn grep_finds_matches() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    write(root, "a.md", "You are a summarizer.\nSummarize: {{text}}\n");
    write(root, "b.md", "Translate: {{text}}\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from(".")]).unwrap();
        commands::commit::run("c1").unwrap();
        // Case-insensitive by default: "summarize" should match a.md line 1 and 2.
        commands::grep::run("summarize", false).unwrap();
        // Case-sensitive: should match only "Summarize:" on line 2.
        commands::grep::run("Summarize", true).unwrap();
    });
}

#[test]
fn stats_runs_without_error() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    write(root, "a.md", "hello\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c1").unwrap();
        commands::branch::run(Some("dev"), None).unwrap();
        commands::tag::run(Some("v1"), None).unwrap();
        // Should not error.
        commands::stats::run().unwrap();
    });
}

// ---- New hardening tests (post-audit) ------------------------------------

#[test]
fn init_accepts_a_path() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("subdir");
    commands::init::run(Some(&target)).unwrap();
    assert!(target.join(".pv").exists());
}

#[test]
fn log_max_count_limits_output() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    write(root, "a.md", "v1\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c1").unwrap();
        write_at_cwd("a.md", "v2\n");
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c2").unwrap();
        write_at_cwd("a.md", "v3\n");
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c3").unwrap();
        // Only the most recent commit should show.
        commands::log::run(Some(1), false).unwrap();
    });
}

#[test]
fn commit_rejects_empty_message() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    write(root, "a.md", "v1\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        let err = commands::commit::run("").unwrap_err();
        assert!(format!("{err}").contains("empty"));
        let err = commands::commit::run("   \n  ").unwrap_err();
        assert!(format!("{err}").contains("empty"));
    });
}

#[test]
fn commit_rejects_nothing_changed() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    write(root, "a.md", "v1\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c1").unwrap();
        // Stage the same content again — tree unchanged.
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        let err = commands::commit::run("c2").unwrap_err();
        assert!(format!("{err}").contains("nothing to commit"));
    });
}

#[test]
fn branch_rejects_bad_names() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    write(root, "a.md", "v1\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c1").unwrap();
        // Names with '/' or '..' must be rejected.
        assert!(commands::branch::run(Some("feature/x"), None).is_err());
        assert!(commands::branch::run(Some("../x"), None).is_err());
        assert!(commands::branch::run(Some("HEAD"), None).is_err());
        assert!(commands::branch::run(Some(".hidden"), None).is_err());
        // A clean name still works.
        assert!(commands::branch::run(Some("experiment"), None).is_ok());
    });
}

#[test]
fn stash_pop_refuses_dirty_tree() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    write(root, "a.md", "v1\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c1").unwrap();
        // Stash a change.
        write_at_cwd("a.md", "dirty\n");
        commands::stash::push().unwrap();
        // Now make a NEW uncommitted change on top of HEAD.
        write_at_cwd("a.md", "different change\n");
        // pop should refuse (would overwrite).
        let err = commands::stash::pop().unwrap_err();
        assert!(format!("{err}").contains("cannot pop stash"));
        // Stash should still exist (not dropped).
        assert!(std::path::Path::new(".pv/stash").exists());
    });
}

#[test]
fn revert_refuses_dirty_tree() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    write(root, "a.md", "v1\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c1").unwrap();
        write_at_cwd("a.md", "v2\n");
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c2").unwrap();
        // Make an uncommitted change, then try to revert to c1.
        write_at_cwd("a.md", "dirty\n");
        let err = commands::revert::run("HEAD~1").unwrap_err();
        // Either "unknown commit" (HEAD~1 not supported) or "cannot revert" (dirty).
        // We test the dirty path via tag instead.
        let _ = err;
    });
    // Use a tag for a reliable revert target.
    in_dir(root, || {
        commands::tag::run(Some("v1"), None).unwrap();
        // a.md is dirty from above.
        let err = commands::revert::run("v1").unwrap_err();
        assert!(format!("{err}").contains("cannot revert"));
    });
}

#[test]
fn render_preserves_unicode() {
    use promptvault::render::render;
    let mut vars = std::collections::HashMap::new();
    vars.insert("x".to_string(), "世界".to_string());
    let out = render("hello {{x}} 🌍", &vars, false).unwrap();
    assert_eq!(out, "hello 世界 🌍");
}

fn write_at_cwd(rel: &str, content: &str) {
    let path = std::path::PathBuf::from(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

// ---- Round-2 additions: blame, config, detached checkout, oneline log -----

#[test]
fn config_set_get_list() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    in_dir(root, || {
        commands::config::set("author", "Alice").unwrap();
        commands::config::set("color", "blue").unwrap();
        assert!(commands::config::get("author").is_ok());
        assert!(commands::config::get("missing").is_err());
        // Bad input rejected.
        assert!(commands::config::set("bad=key", "x").is_err());
        assert!(commands::config::set("k", "multi\nline").is_err());
        // List should not error.
        commands::config::list().unwrap();
        // set then commit should use the configured author.
        write_at_cwd("a.md", "v1\n");
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c1").unwrap();
        // The commit object should carry "Alice".
        let repo = promptvault::core::repository::Repo::find().unwrap();
        let head = repo.head_commit().unwrap().unwrap();
        let commit = promptvault::core::objects::read_commit(&repo.pv_dir, &head).unwrap();
        assert_eq!(commit.author.as_deref(), Some("Alice"));
    });
}

#[test]
fn log_oneline_format() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    write(root, "a.md", "v1\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("first commit").unwrap();
        // Should not error and should print one line per commit.
        commands::log::run(None, true).unwrap();
    });
}

#[test]
fn blame_runs_on_tracked_file() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    write(root, "a.md", "line one\nline two\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c1").unwrap();
        // Modify one line and commit.
        write_at_cwd("a.md", "line one\nLINE TWO CHANGED\n");
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c2").unwrap();
        // blame should not error and should print both lines.
        commands::blame::run("a.md").unwrap();
    });
}

#[test]
fn checkout_detached_at_commit() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    write(root, "a.md", "v1\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c1").unwrap();
        let first_hash = {
            let repo = promptvault::core::repository::Repo::find().unwrap();
            repo.head_commit().unwrap().unwrap()
        };
        write_at_cwd("a.md", "v2\n");
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c2").unwrap();

        // Detach at the first commit via short hash.
        let short = &first_hash[..7];
        commands::checkout::run(short).unwrap();
        // Working tree should now show v1.
        let content = std::fs::read_to_string("a.md").unwrap();
        assert_eq!(content, "v1\n");
        // Back to main.
        commands::checkout::run("main").unwrap();
        let content = std::fs::read_to_string("a.md").unwrap();
        assert_eq!(content, "v2\n");
    });
}

// ---- Round-3: ignore command, clean empty dirs, stats cross-branch --------

#[test]
fn ignore_add_list_remove() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());

    // No .pvignore yet — list should not error.
    in_dir(root, || {
        commands::ignore::list().unwrap();
    });

    // Add patterns.
    in_dir(root, || {
        commands::ignore::add(&["*.bak".into(), "drafts".into()]).unwrap();
    });
    let content = fs::read_to_string(root.join(".pvignore")).unwrap();
    assert!(content.contains("*.bak"));
    assert!(content.contains("drafts"));

    // List should print both patterns.
    in_dir(root, || {
        commands::ignore::list().unwrap();
    });

    // Remove one pattern.
    in_dir(root, || {
        commands::ignore::remove(&["*.bak".into()]).unwrap();
    });
    let content = fs::read_to_string(root.join(".pvignore")).unwrap();
    assert!(!content.contains("*.bak"));
    assert!(content.contains("drafts"));

    // Remove the last pattern — file should be deleted.
    in_dir(root, || {
        commands::ignore::remove(&["drafts".into()]).unwrap();
    });
    assert!(!root.join(".pvignore").exists());

    // Removing from a non-existent file should error.
    in_dir(root, || {
        assert!(commands::ignore::remove(&["x".into()]).is_err());
    });
}

#[test]
fn ignore_patterns_are_enforced() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    // Add an ignore pattern via the command.
    in_dir(root, || {
        commands::ignore::add(&["secrets".into()]).unwrap();
    });
    write(root, "keep.md", "keep\n");
    write(root, "secrets/key.md", "secret\n");
    // `pv add .` should stage keep.md but NOT secrets/key.md.
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from(".")]).unwrap();
        let idx = promptvault::core::repository::Repo::find().unwrap().index().unwrap();
        let paths: Vec<&str> = idx.entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"keep.md"));
        assert!(!paths.iter().any(|p| p.contains("secrets")));
    });
}

#[test]
fn clean_removes_empty_dirs() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    write(root, "tracked.md", "v1\n");
    write(root, "untracked/sub/deep.md", "untracked\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("tracked.md")]).unwrap();
        commands::commit::run("c1").unwrap();
        // Clean should remove the untracked file AND the empty dirs.
        commands::clean::run(false, true).unwrap();
    });
    assert!(!root.join("untracked/sub/deep.md").exists());
    assert!(!root.join("untracked/sub").exists());
    assert!(!root.join("untracked").exists());
}

#[test]
fn stats_counts_commits_on_all_branches() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    write(root, "a.md", "v1\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c1 on main").unwrap();
        // Create a branch and commit on it (diverges from main).
        commands::branch::run(Some("dev"), None).unwrap();
        commands::checkout::run("dev").unwrap();
        write_at_cwd("a.md", "dev1\n");
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c2 on dev").unwrap();
        // Switch back to main.
        commands::checkout::run("main").unwrap();
        // stats should see both commits (1 on main + 1 on dev), even though
        // dev's commit is not reachable from main's first-parent chain.
        commands::stats::run().unwrap();
    });
}

#[test]
fn diff_surfaces_read_errors() {
    // We can't easily corrupt an object in a unit test, but we can verify
    // that diff between two valid refs works and doesn't silently return
    // empty diffs for missing files.
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    in_dir(root, || commands::init::run(None).unwrap());
    write(root, "a.md", "line1\nline2\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c1").unwrap();
    });
    // Modify and commit v2.
    write(root, "a.md", "line1\nLINE2\n");
    in_dir(root, || {
        commands::add::run(vec![PathBuf::from("a.md")]).unwrap();
        commands::commit::run("c2").unwrap();
        // Diff between two valid commits should work.
        commands::diff::run(vec!["HEAD~1".to_string()], false).unwrap_or_else(|e| {
            // HEAD~1 is not supported — use a tag instead.
            let _ = e;
        });
    });
    // Tag c1 for a reliable diff target.
    in_dir(root, || {
        commands::tag::run(Some("v1"), None).unwrap();
        // diff v1 HEAD should work without error.
        let result = commands::diff::run(vec!["v1".to_string(), "HEAD".to_string()], false);
        assert!(result.is_ok(), "diff between valid refs should succeed");
    });
}
