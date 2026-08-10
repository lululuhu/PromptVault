//! Content-addressable object store, git-style.
//!
//! Objects are stored under `.pv/objects/<2-char-prefix>/<rest-of-hash>`.
//! Each object file begins with a NUL-separated type header:
//!   `<type>\0<raw-data>`
//! where `<type>` is one of `blob`, `tree`, `commit`.

use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::core::hash::hash_bytes;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectType {
    Blob,
    Tree,
    Commit,
}

impl ObjectType {
    fn as_str(&self) -> &'static str {
        match self {
            ObjectType::Blob => "blob",
            ObjectType::Tree => "tree",
            ObjectType::Commit => "commit",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "blob" => Some(ObjectType::Blob),
            "tree" => Some(ObjectType::Tree),
            "commit" => Some(ObjectType::Commit),
            _ => None,
        }
    }
}

pub struct Object {
    pub kind: ObjectType,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct TreeEntry {
    pub path: String,
    pub hash: String,
}

fn split_hash(hash: &str) -> (&str, &str) {
    hash.split_at(2)
}

fn framed(kind: ObjectType, data: &[u8]) -> Vec<u8> {
    let header = kind.as_str();
    let mut out = Vec::with_capacity(header.len() + 1 + data.len());
    out.extend_from_slice(header.as_bytes());
    out.push(0);
    out.extend_from_slice(data);
    out
}

/// Write an object of the given kind and return its SHA-256 hash.
/// If the object already exists, this is a no-op (content addressing).
pub fn write_object(pv_dir: &Path, kind: ObjectType, data: &[u8]) -> Result<String> {
    let content = framed(kind, data);
    let hash = hash_bytes(&content);
    let (dir, file) = split_hash(&hash);
    let obj_dir = pv_dir.join("objects").join(dir);
    fs::create_dir_all(&obj_dir)?;
    let obj_path = obj_dir.join(file);
    if !obj_path.exists() {
        fs::write(&obj_path, &content)?;
    }
    Ok(hash)
}

pub fn read_object(pv_dir: &Path, hash: &str) -> Result<Object> {
    let (dir, file) = split_hash(hash);
    let path = pv_dir.join("objects").join(dir).join(file);
    let content = fs::read(&path).with_context(|| format!("object {hash} not found"))?;
    let nul = content
        .iter()
        .position(|&b| b == 0)
        .context("corrupt object: missing type header")?;
    let kind_str = std::str::from_utf8(&content[..nul])?;
    let kind = ObjectType::from_str(kind_str).context("unknown object type")?;
    let data = content[nul + 1..].to_vec();
    Ok(Object { kind, data })
}

pub fn object_exists(pv_dir: &Path, hash: &str) -> bool {
    let (dir, file) = split_hash(hash);
    pv_dir.join("objects").join(dir).join(file).exists()
}

/// Hash the bytes of a blob the same way [`write_object`] would, without writing.
pub fn hash_blob(data: &[u8]) -> String {
    let content = framed(ObjectType::Blob, data);
    hash_bytes(&content)
}

// ---- Trees ---------------------------------------------------------------

/// Serialize and store a tree from sorted-unstable entries.
pub fn write_tree(pv_dir: &Path, entries: &[TreeEntry]) -> Result<String> {
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    let mut data = Vec::new();
    for e in &sorted {
        let line = format!("blob {}\t{}\n", e.hash, e.path);
        data.extend_from_slice(line.as_bytes());
    }
    write_object(pv_dir, ObjectType::Tree, &data)
}

pub fn read_tree(pv_dir: &Path, hash: &str) -> Result<Vec<TreeEntry>> {
    let obj = read_object(pv_dir, hash)?;
    if obj.kind != ObjectType::Tree {
        bail!("{hash} is not a tree");
    }
    let text = std::str::from_utf8(&obj.data)?;
    let mut entries = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let rest = line.strip_prefix("blob ").unwrap_or(line);
        let (hash, path) = rest
            .split_once('\t')
            .context("malformed tree entry")?;
        entries.push(TreeEntry {
            path: path.to_string(),
            hash: hash.to_string(),
        });
    }
    Ok(entries)
}

// ---- Commits -------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Commit {
    pub tree: String,
    pub parent: Option<String>,
    pub author: Option<String>,
    pub timestamp: i64,
    pub message: String,
}

pub fn write_commit(pv_dir: &Path, commit: &Commit) -> Result<String> {
    let mut data = String::new();
    data.push_str(&format!("tree {}\n", commit.tree));
    if let Some(p) = &commit.parent {
        data.push_str(&format!("parent {}\n", p));
    }
    if let Some(a) = &commit.author {
        data.push_str(&format!("author {}\n", a));
    }
    data.push_str(&format!("timestamp {}\n", commit.timestamp));
    data.push('\n');
    data.push_str(&commit.message);
    write_object(pv_dir, ObjectType::Commit, data.as_bytes())
}

pub fn read_commit(pv_dir: &Path, hash: &str) -> Result<Commit> {
    let obj = read_object(pv_dir, hash)?;
    if obj.kind != ObjectType::Commit {
        bail!("{hash} is not a commit");
    }
    let text = std::str::from_utf8(&obj.data)?;
    let mut tree = None;
    let mut parent = None;
    let mut author = None;
    let mut timestamp = 0i64;
    let mut message_lines = Vec::new();
    let mut header_done = false;
    for line in text.lines() {
        if !header_done {
            if line.is_empty() {
                header_done = true;
                continue;
            }
            if let Some(v) = line.strip_prefix("tree ") {
                tree = Some(v.to_string());
            } else if let Some(v) = line.strip_prefix("parent ") {
                parent = Some(v.to_string());
            } else if let Some(v) = line.strip_prefix("author ") {
                author = Some(v.to_string());
            } else if let Some(v) = line.strip_prefix("timestamp ") {
                timestamp = v.parse().unwrap_or(0);
            }
        } else {
            message_lines.push(line);
        }
    }
    Ok(Commit {
        tree: tree.context("commit missing tree")?,
        parent,
        author,
        timestamp,
        message: message_lines.join("\n"),
    })
}
