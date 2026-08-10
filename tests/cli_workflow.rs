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
