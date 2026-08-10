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

fn in_dir<R>(dir: &std::path::Path, f: impl FnOnce() -> R) -> R {
    let _guard = CWD_LOCK.lock().unwrap();
    let prev = env::current_dir().unwrap();
    env::set_current_dir(dir).unwrap();
    let r = f();
    env::set_current_dir(prev).unwrap();
    r
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
        commands::init::run().unwrap();
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
    in_dir(root, || commands::init::run().unwrap());
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
    in_dir(root, || commands::init::run().unwrap());
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
    in_dir(root, || commands::init::run().unwrap());
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
    in_dir(root, || commands::init::run().unwrap());
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
    in_dir(root, || commands::init::run().unwrap());
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
    in_dir(root, || commands::init::run().unwrap());
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
    in_dir(root, || commands::init::run().unwrap());
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
    in_dir(root, || commands::init::run().unwrap());
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
    in_dir(root, || commands::init::run().unwrap());
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
