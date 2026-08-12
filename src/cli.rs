use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "pv",
    version,
    about = "Git for AI Prompts — version, diff, branch, and roll back your prompts",
    long_about = "prv (Prove your prompts) is a local-first version control system for AI prompts.\n\
                  Version your prompts like code: snapshot, diff, log, and roll back — \
                  all stored locally as content-addressed objects."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Initialize a new prompt vault in the current (or given) directory
    Init {
        /// Directory to initialize (defaults to current directory)
        path: Option<PathBuf>,
    },

    /// List, create, or delete branches
    Branch {
        /// Name of the branch to create
        name: Option<String>,

        /// Delete a branch by name
        #[arg(short, long, value_name = "BRANCH")]
        delete: Option<String>,
    },

    /// Switch to a branch (restores the working tree)
    Checkout {
        /// Branch name to switch to
        branch: String,
    },

    /// Merge another branch into the current branch
    Merge {
        /// Branch to merge into the current branch
        branch: String,
    },

    /// List, create, or delete tags
    Tag {
        /// Name of the tag to create (points at HEAD)
        name: Option<String>,

        /// Delete a tag by name
        #[arg(short, long, value_name = "TAG")]
        delete: Option<String>,
    },

    /// Restore the working tree and index to a past commit (HEAD stays put)
    Revert {
        /// Commit hash (or prefix), tag, or branch to revert to
        commit: String,
    },

    /// Launch the interactive TUI to browse commit history
    Tui,

    /// Render two prompt versions against the same dataset and diff them (A/B test)
    Ab {
        /// First prompt (ref:path, e.g. main:summarize.md)
        a: String,
        /// Second prompt (ref:path, e.g. experiment:summarize.md)
        b: String,
        /// Path to a JSON Lines dataset (.jsonl)
        #[arg(short, long)]
        dataset: PathBuf,
        /// Fail if a template references a variable not present in a case
        #[arg(short, long)]
        strict: bool,
        /// Print the A-vs-B line diff for each differing case
        #[arg(long)]
        show: bool,
    },

    /// Manage git-backed remotes for syncing the vault
    #[command(subcommand)]
    Remote(RemoteCommand),

    /// Push the vault to a git remote
    Push {
        /// Remote name (defaults to "origin")
        #[arg(default_value = "origin")]
        remote: String,
    },

    /// Pull vault changes from a git remote
    Pull {
        /// Remote name (defaults to "origin")
        #[arg(default_value = "origin")]
        remote: String,
    },

    /// Render a prompt and send it to an LLM provider (requires `run` feature)
    #[cfg(feature = "run")]
    Run {
        /// Prompt file path, `HEAD:path`, or `branch:path`
        prompt: String,

        /// Provider: openai | anthropic | ollama
        #[arg(short, long)]
        provider: String,

        /// Model name (provider-specific default if omitted)
        #[arg(short, long)]
        model: Option<String>,

        /// Max tokens for the response (provider-specific default if omitted)
        #[arg(long)]
        max_tokens: Option<u32>,

        /// Variable bindings as `key=value` (repeatable)
        #[arg(short = 'v', long = "var", value_name = "KEY=VALUE")]
        vars: Vec<String>,

        /// Print the rendered prompt before sending
        #[arg(long)]
        show_prompt: bool,
    },

    /// Start a local HTTP server + Web GUI for the vault (requires `serve` feature)
    #[cfg(feature = "serve")]
    Serve {
        /// Host to bind (default: 127.0.0.1, use 0.0.0.0 to expose)
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Port to bind (default: 8787)
        #[arg(short, long, default_value_t = 8787)]
        port: u16,
    },

    /// Add prompt files to the staging area
    Add {
        /// Files or directories to add (use '.' for all)
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },

    /// Remove a prompt from the staging area (does not delete the file)
    #[command(alias = "remove")]
    Rm {
        /// Tracked paths to unstage
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },

    /// Record a snapshot of the staged prompts
    Commit {
        #[arg(short, long, help = "Commit message")]
        message: String,
    },

    /// Show commit history
    Log {
        /// Maximum number of commits to show
        #[arg(short = 'n', long)]
        max_count: Option<usize>,
        /// One line per commit (hash + first line of message)
        #[arg(long)]
        oneline: bool,
    },

    /// Show changes: working tree vs HEAD, or between two refs
    Diff {
        /// Optional base ref (commit/tag/branch/HEAD). Defaults to HEAD vs working tree.
        a: Option<String>,
        /// Optional target ref. When given, compares `a` against `b` (both must be refs).
        b: Option<String>,
        /// Show only a summary of which files changed (with +/- counts), not the full diff
        #[arg(long)]
        stat: bool,
    },

    /// Show the content of an object by hash, ref, or tracked path
    Show {
        /// Object hash (or prefix), HEAD, or a tracked prompt path
        target: String,
    },

    /// List all tracked prompts with their status
    #[command(alias = "ls")]
    List,

    /// Show the working tree status
    #[command(alias = "st")]
    Status,

    /// Print the current content of a prompt file
    Cat {
        path: PathBuf,
    },

    /// Render a prompt template against a dataset (JSON Lines) and run assertions
    #[command(
        long_about = "Evaluate a prompt template by rendering it against each case in a \
        JSON Lines dataset. Each line is a JSON object whose keys fill the {{variables}} \
        in the prompt.\n\
        \n\
        Three modes:\n\
          1. Render-only (default): assert rendered prompt contains `expected`.\n\
          2. LLM-output (--llm): call LLM, assert its output contains `expected_output`.\n\
          3. LLM-judge (--llm --judge): call LLM, score output 0-10 against `rubric`.\n\
        \n\
        LLM modes require the `run` feature and provider API keys in env vars."
    )]
    Eval {
        /// Prompt file path, `HEAD:path`, or blob hash
        prompt: String,

        /// Path to a JSON Lines dataset (.jsonl)
        #[arg(short, long)]
        dataset: PathBuf,

        /// Fail if the template references a variable not present in a case
        #[arg(short, long)]
        strict: bool,

        /// Print the fully rendered prompt for each case
        #[arg(long)]
        show: bool,

        /// Call an LLM and assert/judge its output (requires `run` feature)
        /// Provider: openai | anthropic | ollama
        #[cfg(feature = "run")]
        #[arg(long, value_name = "PROVIDER")]
        llm: Option<String>,

        /// Model name (provider-specific default if omitted)
        #[cfg(feature = "run")]
        #[arg(short, long)]
        model: Option<String>,

        /// Use LLM-as-judge: score each output 0-10 against the case's `rubric`
        #[cfg(feature = "run")]
        #[arg(long)]
        judge: bool,

        /// Don't record this eval to `.pv/evals/` (history is recorded by default)
        #[arg(long)]
        no_record: bool,
    },

    /// Show eval history for a prompt (recorded by `pv eval`)
    #[command(alias = "evals")]
    EvalLog {
        /// Prompt name (same spec as `pv eval`)
        prompt: String,
    },

    /// Stash uncommitted changes (push/pop/drop/list)
    #[command(subcommand)]
    Stash(StashCommand),

    /// Unstage a file (or all files). Leaves the working tree untouched.
    Reset {
        /// Paths to unstage. If empty, unstages everything back to HEAD.
        paths: Vec<PathBuf>,
    },

    /// Remove untracked prompt files from the working tree
    Clean {
        /// Preview what would be removed (no files are deleted)
        #[arg(short = 'n', long = "dry-run")]
        dry_run: bool,
        /// Actually delete untracked files (required to perform the clean)
        #[arg(short, long)]
        force: bool,
    },

    /// Export a commit's tree as a zip archive
    Export {
        /// Ref: commit hash/tag/branch/HEAD
        spec: String,
        /// Output zip file path
        #[arg(short, long, value_name = "FILE")]
        output: PathBuf,
    },

    /// Search across all tracked prompts in HEAD (like `git grep`)
    Grep {
        /// Search pattern (substring match)
        pattern: String,
        /// Case-sensitive (default: case-insensitive)
        #[arg(short = 's', long)]
        case_sensitive: bool,
    },

    /// Show vault statistics (commits, blobs, branches, disk usage)
    Stats,

    /// Count tokens in a rendered prompt and estimate API cost across models
    Tokens {
        /// Prompt file path, `HEAD:path`, `branch:path`, or blob hash
        prompt: String,

        /// Variable bindings as `key=value` (repeatable)
        #[arg(short = 'v', long = "var", value_name = "KEY=VALUE")]
        vars: Vec<String>,

        /// Specific model ids to estimate (e.g. `gpt-4o`, `claude-3-5-sonnet-latest`).
        /// Defaults to a curated set of popular models.
        #[arg(short = 'm', long = "model", value_name = "MODEL")]
        models: Vec<String>,

        /// Override the assumed output token count (default: half of input, 200..=2000)
        #[arg(long, value_name = "N")]
        max_tokens: Option<u32>,

        /// Output as JSON (for scripting / dashboards)
        #[arg(long)]
        json: bool,
    },

    /// Full prompt analytics: tokens, variables, complexity, readability, cost
    Metrics {
        /// Prompt file path, `HEAD:path`, `branch:path`, or blob hash
        prompt: String,

        /// Variable bindings as `key=value` (repeatable)
        #[arg(short = 'v', long = "var", value_name = "KEY=VALUE")]
        vars: Vec<String>,

        /// Specific model ids to estimate (defaults to a curated set)
        #[arg(short = 'm', long = "model", value_name = "MODEL")]
        models: Vec<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },

    /// Show which commit last touched each line of a prompt
    Blame {
        /// Path of the tracked prompt to blame
        path: String,
    },

    /// Get / set repository configuration (author, etc.)
    #[command(subcommand)]
    Config(ConfigCommand),

    /// Manage `.pvignore` patterns (add / list / remove)
    #[command(subcommand)]
    Ignore(IgnoreCommand),

    /// Print shell completion script (bash/zsh/fish/elvish/powershell)
    Completions {
        /// Shell name: bash | zsh | fish | elvish | powershell
        shell: String,
        /// Write to a file instead of stdout (optional)
        #[arg(short, long)]
        out: Option<PathBuf>,
    },

    /// Import prompts from an external source (e.g. a ChatGPT data export)
    Import {
        /// Import source: `chatgpt` (expects a `conversations.json` export)
        #[arg(long, value_name = "SOURCE")]
        from: String,

        /// Path to the export file (e.g. `conversations.json`)
        source: PathBuf,

        /// Output directory inside the vault (default: `prompts/imported`)
        #[arg(short, long, value_name = "DIR")]
        dir: Option<PathBuf>,

        /// Only import prompts with at least this many characters (default: 20)
        #[arg(long, value_name = "N", default_value_t = 20)]
        min_length: usize,

        /// Stage imported prompts after writing (does not commit)
        #[arg(long)]
        add: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum IgnoreCommand {
    /// Add one or more patterns to `.pvignore`
    Add {
        /// Patterns to add (one per arg)
        #[arg(required = true)]
        patterns: Vec<String>,
    },
    /// List all active ignore patterns
    #[command(alias = "ls")]
    List,
    /// Remove one or more patterns from `.pvignore`
    #[command(alias = "rm")]
    Remove {
        /// Patterns to remove (exact match)
        #[arg(required = true)]
        patterns: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    /// Print the value of a config key
    Get { key: String },
    /// Set a config key (writes to `.pv/config`)
    Set { key: String, value: String },
    /// List all config entries
    #[command(alias = "ls")]
    List,
}

#[derive(Subcommand, Debug)]
pub enum StashCommand {
    /// Save current changes and reset the working tree to HEAD
    Push,
    /// Restore the stashed changes and drop the stash
    Pop,
    /// Drop the stash without restoring
    Drop,
    /// List stashed files
    #[command(alias = "ls")]
    List,
}

#[derive(Subcommand, Debug)]
pub enum RemoteCommand {
    /// Add a remote: `pv remote add <name> <url>`
    Add { name: String, url: String },
    /// List configured remotes
    #[command(alias = "ls")]
    List,
    /// Remove a remote by name
    #[command(alias = "rm")]
    Remove { name: String },
}
