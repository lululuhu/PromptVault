//! `pv run` — render a prompt template and send it to an LLM provider.
//!
//! SECURITY DESIGN:
//!   - Only compiled when the `run` cargo feature is enabled. The default
//!     build has **zero** HTTP code and no network dependencies.
//!   - API keys are read **only** from environment variables. They are never
//!     written to disk, never logged, and never stored in vault history.
//!   - Only the rendered prompt leaves the machine; no vault internals are sent.
//!   - A clear notice is printed before any network call.
//!
//! Supported providers (selected by `--provider`):
//!   - openai    (OPENAI_API_KEY, model defaults to gpt-4o-mini)
//!   - anthropic (ANTHROPIC_API_KEY, model defaults to claude-3-5-sonnet-latest)
//!   - ollama    (local, no key, host defaults to http://localhost:11434)

#![cfg(feature = "run")]

use std::collections::HashMap;

use anyhow::{anyhow, bail, Context, Result};

use crate::core::repository::Repo;
use crate::render::render;
use crate::ui::printer;

#[derive(Debug, Clone, Copy)]
pub enum Provider {
    OpenAI,
    Anthropic,
    Ollama,
}

impl Provider {
    fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "openai" => Ok(Provider::OpenAI),
            "anthropic" => Ok(Provider::Anthropic),
            "ollama" => Ok(Provider::Ollama),
            other => bail!("unknown provider '{other}' (expected openai|anthropic|ollama)"),
        }
    }
}

pub fn run(
    prompt: &str,
    provider_str: &str,
    model: Option<&str>,
    vars: Vec<(String, String)>,
    show_prompt: bool,
) -> Result<()> {
    let repo = Repo::find()?;
    let provider = Provider::parse(provider_str)?;

    let template = load_template(&repo, prompt)?;
    let mut var_map: HashMap<String, String> = HashMap::new();
    for (k, v) in vars {
        var_map.insert(k, v);
    }
    let rendered = render(&template, &var_map, true)
        .context("failed to render prompt template (undefined variable)")?;

    if show_prompt {
        println!("{}", nu_ansi_term::Color::DarkGray.paint("--- rendered prompt ---"));
        println!("{rendered}");
        println!("{}", nu_ansi_term::Color::DarkGray.paint("--- end ---"));
    }

    // Explicit consent before any network call.
    printer::warn(&format!(
        "About to send the rendered prompt to {provider:?}. Press Ctrl-C to abort."
    ));

    let response = match provider {
        Provider::OpenAI => call_openai(&rendered, model)?,
        Provider::Anthropic => call_anthropic(&rendered, model)?,
        Provider::Ollama => call_ollama(&rendered, model)?,
    };

    println!("{response}");
    Ok(())
}

fn load_template(repo: &Repo, spec: &str) -> Result<String> {
    // Reuse the same resolution logic as `eval`: path or HEAD:path.
    if let Some(path) = spec.strip_prefix("HEAD:") {
        let path = path.replace('\\', "/");
        let Some(h) = repo.head_commit()? else {
            bail!("no commits yet: cannot resolve HEAD:{path}");
        };
        let commit = crate::core::objects::read_commit(&repo.pv_dir, &h)?;
        let entries = crate::core::objects::read_tree(&repo.pv_dir, &commit.tree)?;
        let entry = entries
            .iter()
            .find(|e| e.path == path)
            .with_context(|| format!("path not tracked at HEAD: {path}"))?;
        let blob = crate::core::objects::read_object(&repo.pv_dir, &entry.hash)?;
        return Ok(String::from_utf8_lossy(&blob.data).to_string());
    }
    let abs = repo.root.join(spec);
    let data = std::fs::read_to_string(&abs)
        .with_context(|| format!("cannot read prompt: {spec}"))?;
    Ok(data)
}

fn call_openai(prompt: &str, model: Option<&str>) -> Result<String> {
    let key = std::env::var("OPENAI_API_KEY")
        .context("OPENAI_API_KEY is not set — refusing to call the API")?;
    let model = model.unwrap_or("gpt-4o-mini").to_string();
    let url = std::env::var("OPENAI_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_string())
        + "/chat/completions";

    let body = serde_json::json!({
        "model": model,
        "messages": [{ "role": "user", "content": prompt }],
    });

    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(&url)
        .bearer_auth(&key)
        .json(&body)
        .send()
        .context("OpenAI request failed")?;
    let status = resp.status();
    let text = resp.text().context("reading OpenAI response failed")?;
    if !status.is_success() {
        bail!("OpenAI API error ({status}): {text}");
    }
    let v: serde_json::Value = serde_json::from_str(&text)?;
    let content = v["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow!("unexpected OpenAI response: {text}"))?;
    Ok(content.to_string())
}

fn call_anthropic(prompt: &str, model: Option<&str>) -> Result<String> {
    let key = std::env::var("ANTHROPIC_API_KEY")
        .context("ANTHROPIC_API_KEY is not set — refusing to call the API")?;
    let model = model.unwrap_or("claude-3-5-sonnet-latest").to_string();
    let url = "https://api.anthropic.com/v1/messages";

    let body = serde_json::json!({
        "model": model,
        "max_tokens": 1024,
        "messages": [{ "role": "user", "content": prompt }],
    });

    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(url)
        .header("x-api-key", &key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .context("Anthropic request failed")?;
    let status = resp.status();
    let text = resp.text().context("reading Anthropic response failed")?;
    if !status.is_success() {
        bail!("Anthropic API error ({status}): {text}");
    }
    let v: serde_json::Value = serde_json::from_str(&text)?;
    let content = v["content"][0]["text"]
        .as_str()
        .ok_or_else(|| anyhow!("unexpected Anthropic response: {text}"))?;
    Ok(content.to_string())
}

fn call_ollama(prompt: &str, model: Option<&str>) -> Result<String> {
    let model = model.unwrap_or("llama3.2").to_string();
    let host = std::env::var("OLLAMA_HOST")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());
    let url = format!("{host}/api/generate");

    let body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "stream": false,
    });

    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .context("Ollama request failed (is ollama running?)")?;
    let status = resp.status();
    let text = resp.text().context("reading Ollama response failed")?;
    if !status.is_success() {
        bail!("Ollama API error ({status}): {text}");
    }
    let v: serde_json::Value = serde_json::from_str(&text)?;
    let content = v["response"]
        .as_str()
        .ok_or_else(|| anyhow!("unexpected Ollama response: {text}"))?;
    Ok(content.to_string())
}
