use clap::Parser;
use promptvault::cli::{Cli, Command};
use promptvault::commands;
use promptvault::ui::printer;

fn main() {
    if let Err(e) = run() {
        printer::error(&format!("{e:#}"));
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init => commands::init::run(),
        Command::Branch { name, delete } => commands::branch::run(name.as_deref(), delete.as_deref()),
        Command::Checkout { branch } => commands::checkout::run(&branch),
        Command::Tag { name, delete } => commands::tag::run(name.as_deref(), delete.as_deref()),
        Command::Revert { commit } => commands::revert::run(&commit),
        Command::Tui => promptvault::tui::run(),
        Command::Ab { a, b, dataset, strict, show } => {
            commands::ab::run(&a, &b, dataset, strict, show)
        }
        Command::Remote(cmd) => match cmd {
            promptvault::cli::RemoteCommand::Add { name, url } => commands::remote::add(&name, &url),
            promptvault::cli::RemoteCommand::List => commands::remote::list(),
            promptvault::cli::RemoteCommand::Remove { name } => commands::remote::remove(&name),
        },
        Command::Push { remote } => commands::remote::push(&remote),
        Command::Pull { remote } => commands::remote::pull(&remote),
        #[cfg(feature = "run")]
        Command::Run { prompt, provider, model, vars, show_prompt } => {
            let parsed_vars = vars
                .into_iter()
                .map(|kv| {
                    let (k, v) = kv
                        .split_once('=')
                        .ok_or_else(|| anyhow::anyhow!("--var expects KEY=VALUE, got: {kv}"))?;
                    Ok::<_, anyhow::Error>((k.to_string(), v.to_string()))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            commands::run::run(&prompt, &provider, model.as_deref(), parsed_vars, show_prompt)
        }
        Command::Add { paths } => commands::add::run(paths),
        Command::Rm { paths } => commands::rm::run(paths),
        Command::Commit { message } => commands::commit::run(&message),
        Command::Log => commands::log::run(),
        Command::Diff { a, b } => {
            let mut args = Vec::new();
            if let Some(a) = a {
                args.push(a);
            }
            if let Some(b) = b {
                args.push(b);
            }
            commands::diff::run(args)
        }
        Command::Show { target } => commands::show::run(&target),
        Command::List => commands::list::run(),
        Command::Status => commands::status::run(),
        Command::Cat { path } => commands::cat::run(path),
        Command::Eval { prompt, dataset, strict, show } => {
            commands::eval::run(&prompt, dataset, strict, show)
        }
        Command::Stash(cmd) => match cmd {
            promptvault::cli::StashCommand::Push => commands::stash::push(),
            promptvault::cli::StashCommand::Pop => commands::stash::pop(),
            promptvault::cli::StashCommand::Drop => commands::stash::drop(),
            promptvault::cli::StashCommand::List => commands::stash::list(),
        },
        Command::Reset { paths } => commands::reset::run(paths),
        Command::Clean { dry_run, force } => commands::clean::run(dry_run, force),
        Command::Export { spec, output } => commands::export::run(&spec, output),
        Command::Grep { pattern, case_sensitive } => commands::grep::run(&pattern, case_sensitive),
        Command::Stats => commands::stats::run(),
    }
}
