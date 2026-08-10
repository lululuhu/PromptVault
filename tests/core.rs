//! Core-layer integration tests: objects, refs, index, ignore all work together.

use std::fs;

use promptvault::core::{ignore, objects, refs, repository};
use tempfile::TempDir;

fn init_vault(dir: &std::path::Path) -> std::path::PathBuf {
    repository::init(dir).unwrap();
    dir.join(".pv")
}

#[test]
fn blob_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let pv = init_vault(tmp.path());
    let data = b"You are a helpful assistant.\n";
    let h = objects::write_object(&pv, objects::ObjectType::Blob, data).unwrap();
    assert!(objects::object_exists(&pv, &h));
    let obj = objects::read_object(&pv, &h).unwrap();
    assert_eq!(obj.kind, objects::ObjectType::Blob);
    assert_eq!(obj.data, data);
}

#[test]
fn content_addressing_dedupes() {
    let tmp = TempDir::new().unwrap();
    let pv = init_vault(tmp.path());
    let data = b"same content";
    let h1 = objects::write_object(&pv, objects::ObjectType::Blob, data).unwrap();
    let h2 = objects::write_object(&pv, objects::ObjectType::Blob, data).unwrap();
    assert_eq!(h1, h2, "identical content must hash to the same object");
}

#[test]
fn tree_roundtrip_preserves_entries() {
    let tmp = TempDir::new().unwrap();
    let pv = init_vault(tmp.path());
    let blob_a = objects::write_object(&pv, objects::ObjectType::Blob, b"aaa").unwrap();
    let blob_b = objects::write_object(&pv, objects::ObjectType::Blob, b"bbb").unwrap();
    let entries = vec![
        objects::TreeEntry { path: "z.md".into(), hash: blob_b.clone() },
        objects::TreeEntry { path: "a.md".into(), hash: blob_a.clone() },
    ];
    let tree = objects::write_tree(&pv, &entries).unwrap();
    let read_back = objects::read_tree(&pv, &tree).unwrap();
    // Tree should be sorted by path.
    assert_eq!(read_back.len(), 2);
    assert_eq!(read_back[0].path, "a.md");
    assert_eq!(read_back[0].hash, blob_a);
    assert_eq!(read_back[1].path, "z.md");
    assert_eq!(read_back[1].hash, blob_b);
}

#[test]
fn commit_roundtrip_with_author_and_parent() {
    let tmp = TempDir::new().unwrap();
    let pv = init_vault(tmp.path());
    let blob = objects::write_object(&pv, objects::ObjectType::Blob, b"x").unwrap();
    let tree = objects::write_tree(
        &pv,
        &[objects::TreeEntry { path: "p.md".into(), hash: blob }],
    )
    .unwrap();

    let c1 = objects::Commit {
        tree: tree.clone(),
        parent: None,
        author: Some("alice".into()),
        timestamp: 1000,
        message: "first".into(),
    };
    let h1 = objects::write_commit(&pv, &c1).unwrap();

    let c2 = objects::Commit {
        tree,
        parent: Some(h1.clone()),
        author: Some("bob".into()),
        timestamp: 2000,
        message: "second".into(),
    };
    let h2 = objects::write_commit(&pv, &c2).unwrap();

    let read1 = objects::read_commit(&pv, &h1).unwrap();
    assert_eq!(read1.parent, None);
    assert_eq!(read1.author.as_deref(), Some("alice"));
    assert_eq!(read1.message, "first");
    assert_eq!(read1.timestamp, 1000);

    let read2 = objects::read_commit(&pv, &h2).unwrap();
    assert_eq!(read2.parent.as_deref(), Some(h1.as_str()));
    assert_eq!(read2.author.as_deref(), Some("bob"));
    assert_eq!(read2.message, "second");
}

#[test]
fn commit_without_author_backwards_compatible() {
    // A commit written without an author field must still parse (author == None).
    let tmp = TempDir::new().unwrap();
    let pv = init_vault(tmp.path());
    // Hand-craft a commit object with no author line.
    let raw = b"tree 0000000000000000000000000000000000000000000000000000000000000000\ntimestamp 1\n\nmsg";
    let h = objects::write_object(&pv, objects::ObjectType::Commit, raw).unwrap();
    let c = objects::read_commit(&pv, &h).unwrap();
    assert_eq!(c.author, None);
    assert_eq!(c.message, "msg");
}

#[test]
fn refs_branch_lifecycle() {
    let tmp = TempDir::new().unwrap();
    let pv = init_vault(tmp.path());
    refs::init_head(&pv, "main").unwrap();
    refs::create_branch(&pv, "feat", "abc123").unwrap();
    assert!(refs::branch_exists(&pv, "feat"));
    let branches = refs::list_branches(&pv).unwrap();
    assert!(branches.contains(&"feat".to_string()));
    assert_eq!(refs::resolve_branch(&pv, "feat").unwrap().as_deref(), Some("abc123"));
    refs::delete_branch(&pv, "feat").unwrap();
    assert!(!refs::branch_exists(&pv, "feat"));
}

#[test]
fn refs_tag_lifecycle() {
    let tmp = TempDir::new().unwrap();
    let pv = init_vault(tmp.path());
    refs::create_tag(&pv, "v1.0", "deadbeef").unwrap();
    assert!(refs::tag_exists(&pv, "v1.0"));
    assert_eq!(refs::resolve_tag(&pv, "v1.0").unwrap().as_deref(), Some("deadbeef"));
    let tags = refs::list_tags(&pv).unwrap();
    assert_eq!(tags, vec!["v1.0".to_string()]);
    refs::delete_tag(&pv, "v1.0").unwrap();
    assert!(!refs::tag_exists(&pv, "v1.0"));
}

#[test]
fn head_resolves_through_branch() {
    let tmp = TempDir::new().unwrap();
    let pv = init_vault(tmp.path());
    refs::init_head(&pv, "main").unwrap();
    assert_eq!(refs::current_branch(&pv).unwrap().as_deref(), Some("main"));
    // No commit yet -> None.
    assert_eq!(refs::resolve_head(&pv).unwrap(), None);
    refs::create_branch(&pv, "main", "cafef00d".into()).unwrap();
    assert_eq!(refs::resolve_head(&pv).unwrap().as_deref(), Some("cafef00d"));
}

#[test]
fn index_add_and_persist() {
    let tmp = TempDir::new().unwrap();
    let pv = init_vault(tmp.path());
    let hash_a = promptvault::core::objects::hash_blob(b"content a");
    let hash_b = promptvault::core::objects::hash_blob(b"content b");
    let hash_a2 = promptvault::core::objects::hash_blob(b"content a v2");
    let mut idx = promptvault::core::index::Index::default();
    idx.add("prompts/a.md", &hash_a);
    idx.add("prompts/b.md", &hash_b);
    idx.save(&pv).unwrap();

    let loaded = promptvault::core::index::Index::load(&pv).unwrap();
    assert_eq!(loaded.entries.len(), 2);
    assert_eq!(loaded.get("prompts/a.md"), Some(hash_a.as_str()));

    // Re-adding updates in place.
    let mut idx = loaded;
    idx.add("prompts/a.md", &hash_a2);
    idx.save(&pv).unwrap();
    let loaded = promptvault::core::index::Index::load(&pv).unwrap();
    assert_eq!(loaded.entries.len(), 2);
    assert_eq!(loaded.get("prompts/a.md"), Some(hash_a2.as_str()));
}

#[test]
fn ignore_matches_patterns() {
    let ig = ignore::IgnoreSet::parse("*.bak\ndrafts\n/secret\n");
    assert!(ig.is_ignored("x.bak"));
    assert!(ig.is_ignored("drafts/old.md"));
    assert!(ig.is_ignored("secret/top.md"));
    assert!(!ig.is_ignored("prompts/summarize.md"));
    assert!(!ig.is_ignored("a/secret.md"), "unanchored secret only matches /secret");
}

#[test]
fn init_creates_layout() {
    let tmp = TempDir::new().unwrap();
    let pv = init_vault(tmp.path());
    assert!(pv.join("HEAD").exists());
    assert!(pv.join("objects").is_dir());
    assert!(pv.join("refs").join("heads").is_dir());
    assert!(pv.join("index.json").exists());
    // Re-init must fail.
    assert!(repository::init(tmp.path()).is_err());
}
