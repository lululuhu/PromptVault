//! `pv import --from chatgpt` — import prompts from a ChatGPT data export.
//!
//! Reads ChatGPT's `conversations.json` export (Settings → Data Controls →
//! Export Data) and writes the first user message of each conversation as a
//! standalone prompt file in the vault. This lets you migrate your existing
//! prompt library into prv in one step.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::core::ignore::IgnoreSet;
use crate::core::objects::{self, ObjectType};
use crate::core::repository::Repo;
use crate::ui::printer;

pub fn run(from: &str, source: &Path, dir: Option<&Path>, min_length: usize, add: bool) -> Result<()> {
    if from != "chatgpt" {
        bail!("unsupported import source: '{from}'. Supported: `chatgpt`");
    }

    let repo = Repo::find()?;
    let _lock = repo.lock()?;

    let data = std::fs::read_to_string(source)
        .with_context(|| format!("failed to read export: {}", source.display()))?;

    let convos: Vec<ChatConversation> = serde_json::from_str(&data)
        .context("failed to parse ChatGPT export (expected a conversations.json array)")?;

    let out_dir = dir
        .map(|d| d.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("prompts/imported"));
    let abs_out = repo.root.join(&out_dir);
    std::fs::create_dir_all(&abs_out)?;

    let mut imported = 0usize;
    let mut skipped = 0usize;
    let mut used_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    for convo in &convos {
        let text = match convo.first_user_message() {
            Some(t) => t,
            None => {
                skipped += 1;
                continue;
            }
        };
        let trimmed = text.trim();
        if trimmed.len() < min_length {
            skipped += 1;
            continue;
        }

        let title = convo.title.as_deref().unwrap_or("untitled");
        let name = unique_filename(&sanitize(title), &mut used_names);
        let rel = out_dir.join(format!("{name}.md"));
        let abs = repo.root.join(&rel);

        let body = format_prompt(convo, trimmed);
        crate::core::safe::atomic_write(&abs, body.as_bytes())?;

        if add {
            let ignore = IgnoreSet::load(&repo.root);
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if !ignore.is_ignored(&rel_str) {
                let data_bytes = body.as_bytes();
                let h = objects::write_object(&repo.pv_dir, ObjectType::Blob, data_bytes)?;
                let mut idx = repo.index()?;
                idx.add(&rel_str, &h);
                repo.save_index(&idx)?;
            }
        }

        println!("{} {}", printer::bold("imported:"), rel.display());
        imported += 1;
    }

    println!();
    printer::ok(&format!(
        "Imported {} prompt{} from {} conversation{} (skipped {})",
        imported,
        if imported == 1 { "" } else { "s" },
        convos.len(),
        if convos.len() == 1 { "" } else { "s" },
        skipped,
    ));
    if add && imported > 0 {
        println!("  Run `pv commit -m \"...\"` to snapshot the imports.");
    } else if imported > 0 {
        println!("  Files written to {}. Run `pv add {}` to stage them.", out_dir.display(), out_dir.display());
    }

    Ok(())
}

/// Build the prompt file body from a conversation's first user message.
fn format_prompt(convo: &ChatConversation, text: &str) -> String {
    let title = convo.title.as_deref().unwrap_or("untitled");
    let mut out = String::new();
    out.push_str("<!-- imported from ChatGPT -->\n");
    out.push_str(&format!("<!-- conversation: {} -->\n", title));
    if let Some(ts) = convo.create_time {
        if let Some(dt) = chrono::DateTime::from_timestamp(ts, 0) {
            out.push_str(&format!("<!-- date: {} -->\n", dt.format("%Y-%m-%d %H:%M:%S UTC")));
        }
    }
    out.push('\n');
    out.push_str(text);
    out.push('\n');
    out
}

/// Make a string safe to use as a filename: keep alphanumerics, dashes,
/// underscores; collapse other chars to `-`; trim and lowercase.
fn sanitize(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    // Collapse runs of dashes.
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out = out.trim_matches('-').to_lowercase();
    if out.is_empty() {
        out = "untitled".into();
    }
    // Cap length so we don't blow filesystem limits.
    if out.len() > 60 {
        out.truncate(60);
        out = out.trim_matches('-').to_string();
    }
    out
}

/// Ensure a base name is unique within `used` by appending `-2`, `-3`, ...
fn unique_filename(base: &str, used: &mut std::collections::HashSet<String>) -> String {
    let mut candidate = base.to_string();
    let mut n = 2;
    while used.contains(&candidate) {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    used.insert(candidate.clone());
    candidate
}

// --- ChatGPT export schema (only the fields we need) -----------------------

#[derive(Deserialize)]
struct ChatConversation {
    title: Option<String>,
    create_time: Option<i64>,
    mapping: serde_json::Map<String, serde_json::Value>,
}

impl ChatConversation {
    /// Return the text of the first user message in this conversation.
    ///
    /// ChatGPT's `conversations.json` stores a `mapping` of node-id → node.
    /// Each node has a `message` with `author.role` and `content.parts`.
    /// We pick the first node (by walking from roots) whose role is "user"
    /// and whose content is text.
    fn first_user_message(&self) -> Option<String> {
        // Collect user-message texts in the order they appear in the mapping.
        // The mapping is a flat object; messages have `parent`/`children` so we
        // could topologically sort, but for "first user message" a simple scan
        // ordered by create_time (when present) is good enough.
        let mut user_msgs: Vec<(Option<i64>, String)> = Vec::new();
        for (_id, node) in &self.mapping {
            let Some(msg) = node.get("message") else { continue };
            let role = msg
                .get("author")
                .and_then(|a| a.get("role"))
                .and_then(|r| r.as_str())
                .unwrap_or("");
            if role != "user" {
                continue;
            }
            let parts = msg
                .get("content")
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.as_array());
            let Some(parts) = parts else { continue };
            let text: String = parts
                .iter()
                .filter_map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            if text.trim().is_empty() {
                continue;
            }
            let ts = msg.get("create_time").and_then(|t| t.as_i64());
            user_msgs.push((ts, text));
        }
        // Sort by create_time (None sorts last) and take the first.
        user_msgs.sort_by_key(|(ts, _)| ts.unwrap_or(i64::MAX));
        user_msgs.into_iter().next().map(|(_, t)| t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_basic() {
        assert_eq!(sanitize("My Cool Prompt!"), "my-cool-prompt");
        assert_eq!(sanitize("a///b"), "a-b");
        assert_eq!(sanitize("---"), "untitled");
    }

    #[test]
    fn unique_filename_appends_counter() {
        let mut used = std::collections::HashSet::new();
        let a = unique_filename("foo", &mut used);
        let b = unique_filename("foo", &mut used);
        let c = unique_filename("foo", &mut used);
        assert_eq!(a, "foo");
        assert_eq!(b, "foo-2");
        assert_eq!(c, "foo-3");
    }

    #[test]
    fn parse_chatgpt_export() {
        let json = r#"[
          {
            "title": "Summarize this",
            "create_time": 1700000000,
            "mapping": {
              "root": { "id": "root", "children": ["a"] },
              "a": {
                "id": "a",
                "message": {
                  "id": "a",
                  "author": { "role": "user" },
                  "content": { "content_type": "text", "parts": ["Summarize the article below"] },
                  "create_time": 1700000000
                },
                "parent": "root",
                "children": []
              }
            }
          }
        ]"#;
        let convos: Vec<ChatConversation> = serde_json::from_str(json).unwrap();
        assert_eq!(convos.len(), 1);
        let msg = convos[0].first_user_message().unwrap();
        assert_eq!(msg, "Summarize the article below");
    }

    #[test]
    fn skips_assistant_messages() {
        let json = r#"[
          {
            "title": "t",
            "mapping": {
              "a": {
                "message": {
                  "author": { "role": "assistant" },
                  "content": { "parts": ["I am a bot"] }
                }
              }
            }
          }
        ]"#;
        let convos: Vec<ChatConversation> = serde_json::from_str(json).unwrap();
        assert!(convos[0].first_user_message().is_none());
    }

    #[test]
    fn rejects_unsupported_source() {
        let result = run("notreal", Path::new("x.json"), None, 1, false);
        assert!(result.is_err());
    }
}
