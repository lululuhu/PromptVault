use std::io;
use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::CommandFactory;
use clap_complete::{generate, Shell};

use crate::cli::Cli;

/// `pv completions <shell>` — print a completion script to stdout.
///
/// Users pipe the output into their shell's completion directory, e.g.:
///   pv completions bash > /etc/bash_completion.d/pv
///   pv completions zsh  > ~/.zfunc/_pv
///   pv completions fish > ~/.config/fish/completions/pv.fish
pub fn run(shell: &str, out: Option<PathBuf>) -> Result<()> {
    let shell_kind = match shell.to_ascii_lowercase().as_str() {
        "bash" => Shell::Bash,
        "zsh" => Shell::Zsh,
        "fish" => Shell::Fish,
        "elvish" => Shell::Elvish,
        "powershell" | "ps" => Shell::PowerShell,
        other => bail!(
            "unknown shell '{other}'; expected one of: bash, zsh, fish, elvish, powershell"
        ),
    };

    let mut cmd = Cli::command();
    if let Some(path) = out {
        let mut file = std::fs::File::create(&path)?;
        generate(shell_kind, &mut cmd, "pv", &mut file);
        printer_ok(&path, shell);
    } else {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        generate(shell_kind, &mut cmd, "pv", &mut handle);
    }
    Ok(())
}

fn printer_ok(path: &std::path::Path, shell: &str) {
    crate::ui::printer::ok(&format!(
        "{} completion script written to {}",
        shell,
        path.display()
    ));
}
