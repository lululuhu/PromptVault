# PromptVault

<p align="center">
  <img src="docs/logo.svg" width="180" alt="PromptVault logo">
</p>

<p align="center">
  <strong>Git for AI Prompts.</strong> Version, diff, branch, and roll back your prompts — local-first, blazingly fast.
</p>

<p align="center">
  <img src="docs/demo.gif" alt="PromptVault demo">
</p>

<p align="center">
  <a href="https://github.com/lululuhu/promptvault/actions/workflows/ci.yml"><img src="https://github.com/lululuhu/promptvault/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="License: MIT"></a>
  <a href="https://crates.io/crates/promptvault"><img src="https://img.shields.io/crates/v/promptvault.svg" alt="crates.io"></a>
  <img src="https://img.shields.io/badge/rust-1.74%2B-orange.svg" alt="Rust">
  <a href="https://github.com/lululuhu/promptvault/stargazers"><img src="https://img.shields.io/github/stars/lululuhu/promptvault?style=social" alt="stars"></a>
</p>

---

You tweak a prompt 40 times a day. You paste it into ChatGPT, it kind of works, you change one word, it works better — and tomorrow you have **no idea which version was the good one**, no diff between "v3 final" and "v3 final FINAL", and a folder full of `prompt_old.txt`, `prompt_old2.txt`, `prompt_REALLY_final.txt`.

PromptVault fixes this. It's `git`, but purpose-built for prompts.

```bash
pv add prompts/summarize.md
pv commit -m "refine: summarize now reports tone + title"
pv diff          # see exactly what changed since the last good version
pv log           # walk back through every iteration
```

> **Why not just use git?** You can — and PromptVault borrows git's model (content-addressed objects, trees, commits). But prompts live next to code, mixed into repos, notebooks, and chat exports. PromptVault is a focused, prompt-native tool: it tracks `.txt/.md/.prompt/.j2/.yaml`, lives in a `.pv/` folder that won't collide with your code repo, and is built for the *iterate-and-compare* workflow that prompt engineering actually is.

## ✨ Features

- **Snapshots** — commit prompt versions with messages, just like git.
- **Semantic diff** — line-level `+`/`-` diff so you can see *exactly* which instruction you changed.
- **Branches & A/B testing** — `pv branch experiment` → `pv checkout experiment`. Switch branches and the working tree restores instantly. Run two prompt variants side by side.
- **Tags & rollback** — `pv tag v1.0` marks a version; `pv revert v1.0` restores the working tree to any past commit (by hash, tag, or branch).
- **`.pvignore`** — a `.gitignore`-style file so drafts and scratch files stay out of the vault.
- **Ref-to-ref diff** — `pv diff v1 v2` compares any two commits, tags, or branches.
- **Stash** — `pv stash push` shelves uncommitted changes; `pv stash pop` restores them. One-slot, simple, fast.
- **Reset** — `pv reset <path>` unstages a file (or `pv reset` to unstage everything). Working tree untouched.
- **Clean** — `pv clean -f` deletes untracked prompt files (`-n` to preview). Like `git clean`.
- **Grep** — `pv grep <pattern>` searches across all tracked prompts in HEAD (case-insensitive by default).
- **Export** — `pv export HEAD -o snap.zip` bundles a commit's tree as a zip archive for sharing.
- **Stats** — `pv stats` shows commit/blob/branch counts and disk usage.
- **Blame** — `pv blame <path>` shows which commit last touched each line.
- **Config** — `pv config set author Alice` persists per-repo settings (author, etc.).
- **Remote sync** — `pv remote add origin <url>` then `pv push` / `pv pull` syncs the vault to any git host (GitHub, GitLab, Gitea…). No servers, no accounts.
- **TUI** — `pv tui` launches an interactive commit browser (arrow keys to navigate, `q` to quit).
- **Model runner** *(optional, `--features run`)* — `pv run` renders a prompt and sends it to OpenAI, Anthropic, or Ollama. **API keys live only in env vars; nothing is ever logged or stored.**
- **A/B testing** — `pv ab main:s.md experiment:s.md -d cases.jsonl` renders two prompt versions against the same dataset and diffs them line by line. Pure local, no model calls.
- **Evals** — `pv eval` renders a prompt against a JSON Lines dataset and runs assertions. **No model calls, no API keys, no network** — pipe the rendered prompts to any runner you trust.
- **History** — `pv log` walks every commit; `pv show <hash>` restores any past version.
- **Status** — `pv status` tells you what's staged, modified, or untracked.
- **Local-first** — everything lives in `.pv/` on your machine. No account, no cloud, no telemetry.
- **Blazingly fast** — written in Rust, content-addressed with SHA-256, single static binary.
- **Zero config** — `pv init` and go. No server, no API keys, no setup.

## 📦 Install

```bash
cargo install promptvault
```

Or build from source:

```bash
git clone https://github.com/lululuhu/promptvault
cd promptvault
cargo build --release
# binary: target/release/pv  (put it on your PATH)
```

Then, in any folder where you keep prompts:

```bash
pv init
```

## 🚀 Quick start

```bash
$ pv init
Initialized empty prompt vault in ./.pv

$ pv add .
added: prompts/summarize.md
added: prompts/code-review.md
added: prompts/translate.md

$ pv commit -m "feat: initial prompt set"
[main 4100199] feat: initial prompt set
 3 prompts
```

Now iterate and see what changed:

```bash
$ pv diff prompts/summarize.md
diff -- prompts/summarize.md
 You are a precise summarizer.

-Summarize the text below in 3 concise bullets, then propose a title.
+Summarize the text below in 3 concise bullets, propose a title, and note the tone.

 {{text}}

$ pv add prompts/summarize.md
$ pv commit -m "refine: summarize now reports tone + title"
[main 791151d] refine: summarize now reports tone + title
 1 prompt
```

Walk the history and restore any version:

```bash
$ pv log
commit 791151d…
parent 4100199…
Date:   Mon Aug 10 10:04:33 2026 +0000

    refine: summarize now reports tone + title

commit 4100199…
Date:   Mon Aug 10 10:04:33 2026 +0000

    feat: initial prompt set

$ pv list
HASH     PATH                    STATUS
c073600  prompts/code-review.md  clean
6b916fa  prompts/summarize.md    clean
052facf  prompts/translate.md    clean

$ pv show 4100199   # prefix works too
```

### A/B test two prompt variants

```bash
$ pv branch experiment
Created branch 'experiment' at fecff04

$ pv checkout experiment
Switched to branch 'experiment'

# ... tweak the prompt, commit it ...

$ pv checkout main       # working tree restores to main's version
$ pv checkout experiment # ...and back to experiment's version
```

### Evaluate a prompt against a dataset

Write a JSON Lines dataset (`cases.jsonl`) — each line fills the prompt's `{{variables}}`:

```jsonl
{"text": "The quick brown fox...", "expected": "3 concise bullets"}
{"text": "Rust is a systems language...", "expected": "3 concise bullets"}
{"text": "Short."}
```

Then render and assert:

```bash
$ pv eval summarize.md --dataset cases.jsonl --show
Eval: summarize.md  (3 cases)

[1/3] PASS  contains "3 concise bullets"
--- rendered prompt ---
You are a precise summarizer.
...
--- end ---
[2/3] PASS  contains "3 concise bullets"
[3/3] OK    rendered (no assertion)

Summary: 2/2 passed (100%)
```

PromptVault **never calls a model** — it only renders templates and checks assertions.
Pipe `--show` output to any model runner (curl, the OpenAI CLI, ollama…) you trust.
Use `HEAD:summarize.md` to eval the committed version, or `--strict` to fail on missing variables.

## 🧭 Commands

| Command | What it does |
|---|---|
| `pv init [path]` | Create a `.pv/` vault in the current (or given) directory. |
| `pv add <path…>` | Stage prompt files (dirs recurse into `.txt/.md/.prompt/.j2/.yaml…`). |
| `pv rm <path…>` | Unstage a prompt (the file on disk is left alone). |
| `pv commit -m "msg"` | Snapshot staged prompts. Refuses empty messages and no-op commits. |
| `pv status` | Show staged / modified / untracked prompts. |
| `pv diff [a] [b]` | Diff working tree vs HEAD, or compare two refs (tags/branches/commits). |
| `pv log [-n <count>] [--oneline]` | Show commit history (optionally limited / one-line-per-commit). |
| `pv list` | List tracked prompts and their status. |
| `pv branch [name]` | List branches, or create one from HEAD. Use `-d <name>` to delete. |
| `pv checkout <branch\|commit>` | Switch branch, or detach HEAD at a commit/tag (restores the working tree; refuses if dirty). |
| `pv tag [name]` | List tags, or create one at HEAD. Use `-d <name>` to delete. |
| `pv revert <commit>` | Restore the working tree + index to a past commit (by hash, tag, or branch). Refuses if the working tree is dirty. HEAD is unchanged — commit to record the rollback. |
| `pv eval <prompt> -d <file>` | Render a prompt against a JSON Lines dataset and assert `expected`. Flags: `--strict`, `--show`. |
| `pv ab <a> <b> -d <file>` | Render two prompt versions (`ref:path`) against the same dataset and diff them. Flags: `--strict`, `--show`. |
| `pv remote add/list/remove` | Manage git-backed remotes for vault sync. |
| `pv push [remote]` / `pv pull [remote]` | Sync the vault to/from a git remote. |
| `pv tui` | Interactive commit history browser. |
| `pv run <prompt> --provider ...` | *(opt-in feature)* Render + send to OpenAI/Anthropic/Ollama. Keys from env vars only. Flags: `--model`, `--max-tokens`, `--var k=v`, `--show-prompt`. 60s timeout. |
| `pv show <hash\|HEAD\|path>` | Print any object (blob/tree/commit) by hash, prefix, or path. |
| `pv cat <path>` | Print a file's current content. |
| `pv stash push` / `pop` / `drop` / `list` | Shelve and restore uncommitted changes. `pop` refuses to overwrite a dirty tree. |
| `pv reset [<path…>]` | Unstage a file (or everything). Working tree untouched. |
| `pv clean -f` / `-n` | Delete (or preview) untracked prompt files. |
| `pv grep <pattern> [-s]` | Search across all tracked prompts (case-insensitive by default). |
| `pv export <ref> -o <file>` | Export a commit's tree as a zip archive. |
| `pv stats` | Show vault statistics (commits, blobs, branches, disk usage). |
| `pv blame <path>` | Show which commit last touched each line of a prompt. |
| `pv config get/set/list` | Read / write repository config (e.g. `pv config set author Alice`). |

## 🏗️ How it works

PromptVault is a tiny git. On `pv init` it creates:

```
.pv/
├── HEAD              → "ref: refs/heads/main"
├── index.json        → staging area (path → blob hash)
├── objects/          → content-addressed store (SHA-256)
│   └── ab/cdef…      → "<type>\0<data>"  (blob / tree / commit)
└── refs/heads/main   → latest commit hash
```

Every prompt version is a **blob** addressed by the SHA-256 of `blob\0<content>`. A **tree** maps paths to blobs. A **commit** points to a tree + parent + message. Identical content is stored once. Nothing ever leaves your machine.

## 🆚 How it compares

| | PromptVault | git | PromptLayer / LangSmith | A `prompts/` folder |
|---|---|---|---|---|
| Version history | ✅ | ✅ (but mixes with code) | ✅ (cloud) | ❌ |
| Prompt-aware diff | ✅ | ✅ | ⚠️ | ❌ |
| Local / offline | ✅ | ✅ | ❌ | ✅ |
| Zero setup, no account | ✅ | ✅ | ❌ | ✅ |
| Built for iterate-and-compare | ✅ | ❌ | ✅ | ❌ |
| Free, self-hosted, private | ✅ | ✅ | ❌ | ✅ |

## 🗺️ Roadmap

PromptVault is early and moving fast. Planned:

- [x] **Branches** — `pv branch`, `pv checkout` for A/B prompt experiments.
- [x] **Evals** — `pv eval` renders templates against a dataset and asserts. No model calls.
- [x] **Tags & rollback** — `pv tag`, `pv revert` for first-class version pinning and rollback.
- [x] **`.pvignore`** — keep drafts and scratch files out of the vault.
- [x] **Ref-to-ref diff** — `pv diff v1 v2`.
- [x] **Remote sync** — push/pull vaults to any git host as a backing store.
- [x] **TUI** — a `ratatui` interface for browsing history.
- [x] **Model runner** — `pv run` against OpenAI/Anthropic/Ollama (opt-in feature, keys from env).
- [x] **A/B testing** — `pv ab` renders two prompt versions against a dataset and diffs them.
- [x] **Stash / reset / clean / grep / export / stats** — everyday git-class utilities, prompt-native.
- [x] **Blame / config / detached HEAD** — line-level history, per-repo config, checkout at a commit.

Have an opinion? Open an issue — good ideas ship fast.

## 🔒 Trust model & security

PromptVault is designed to be safe to run on your own prompt files. The default
build (no cargo features) has **zero network code** — no HTTP client, no SDK,
nothing that can phone home. The optional `run` feature is the only path that
makes a network call, and it is opt-in.

**What PromptVault will never do:**

- Read or send API keys anywhere except to the provider you explicitly call with `pv run`.
- API keys are read **only** from environment variables. They are never written to disk,
  never logged, never stored in vault history, and never sent anywhere except the provider
  endpoint you selected.
- Make any outbound network connection in the default build.
- Collect telemetry, analytics, or usage data.

**Path safety:** tree entries are validated on read — paths must be relative, contain no
`..` segments, no absolute paths, no backslashes, no NUL bytes. This prevents a malicious
`.pv/` (e.g. from an untrusted `pv pull`) from writing outside your working tree via
`checkout`/`revert`/`stash pop`. Branch and tag names are validated to prevent escaping
the `refs/` directory.

**Atomic writes:** `index.json`, `HEAD`, branch/tag refs, and new objects are written
atomically (temp file + fsync + rename), so a crash mid-operation will not corrupt the
vault. Existing files are never partially overwritten.

**Untrusted remotes:** `pv pull` syncs the `.pv/` directory from a git remote. Only pull
from remotes you trust. Although tree paths are validated, a hostile remote can still
replace your tracked prompt content with attacker-controlled text (e.g. a prompt that
instructs a model to leak data). Treat pulled prompts as untrusted input.

## ⚠️ Disclaimer

This software is provided "as is", without warranty of any kind, express or implied.
The authors and contributors are not liable for any damages arising from its use.

**Specifically, you are responsible for:**

- **Prompt content you version and send to models.** PromptVault does not inspect, filter,
  or moderate prompt content. Whatever you commit and whatever you send via `pv run` is
  your responsibility — including any content a model produces in response.
- **API costs.** `pv run` sends prompts to paid APIs (OpenAI, Anthropic). You are
  responsible for any charges incurred by your API keys. PromptVault does not enforce
  spending limits.
- **Model outputs.** Outputs from `pv run` are the model's, not PromptVault's. Verify
  outputs before relying on them, especially for code, legal, medical, or financial
  content.
- **Sensitive data in prompts.** If your prompts contain secrets, PII, or confidential
  information, committing them to a vault stores that data on disk in plaintext (content-
  addressed, but unencrypted). Pushing to a remote sends it to that git host. Do not
  commit secrets you would not commit to git.
- **Untrusted remotes.** Pulling from a remote you do not control may introduce malicious
  prompt content or attempt to abuse your trust in tracked files (see Trust model above).

By using PromptVault you acknowledge these risks.

## 🤝 Contributing

PRs welcome. Fork → branch → `cargo test` → PR. Be kind in issues.

```bash
cargo build        # dev
cargo build --release
```

## 📄 License

MIT © PromptVault Contributors
