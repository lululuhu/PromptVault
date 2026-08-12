use clap::Parser;
use prv::cli::{Cli, Command};
use prv::commands;
use prv::ui::printer;

fn main() {
    if let Err(e) = run() {
        printer::error(&format!("{e:#}"));
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { path } => commands::init::run(path.as_deref()),
        Command::Branch { name, delete } => commands::branch::run(name.as_deref(), delete.as_deref()),
        Command::Checkout { branch } => commands::checkout::run(&branch),
        Command::Merge { branch } => commands::merge::run(&branch),
        Command::Tag { name, delete } => commands::tag::run(name.as_deref(), delete.as_deref()),
        Command::Revert { commit } => commands::revert::run(&commit),
        Command::Tui => prv::tui::run(),
        Command::Ab { a, b, dataset, strict, show } => {
            commands::ab::run(&a, &b, dataset, strict, show)
        }
        Command::Remote(cmd) => match cmd {
            prv::cli::RemoteCommand::Add { name, url } => commands::remote::add(&name, &url),
            prv::cli::RemoteCommand::List => commands::remote::list(),
            prv::cli::RemoteCommand::Remove { name } => commands::remote::remove(&name),
        },
        Command::Push { remote } => commands::remote::push(&remote),
        Command::Pull { remote } => commands::remote::pull(&remote),
        #[cfg(feature = "run")]
        Command::Run { prompt, provider, model, max_tokens, vars, show_prompt } => {
            let parsed_vars = parse_kv(vars)?;
            commands::run::run(&prompt, &provider, model.as_deref(), max_tokens, parsed_vars, show_prompt)
        }

        #[cfg(feature = "serve")]
        Command::Serve { host, port } => commands::serve::run(&host, port),
        Command::Add { paths } => commands::add::run(paths),
        Command::Rm { paths } => commands::rm::run(paths),
        Command::Commit { message } => commands::commit::run(&message),
        Command::Log { max_count, oneline } => commands::log::run(max_count, oneline),
        Command::Diff { a, b, stat } => {
            let mut args = Vec::new();
            if let Some(a) = a {
                args.push(a);
            }
            if let Some(b) = b {
                args.push(b);
            }
            commands::diff::run(args, stat)
        }
        Command::Show { target } => commands::show::run(&target),
        Command::List => commands::list::run(),
        Command::Status => commands::status::run(),
        Command::Cat { path } => commands::cat::run(path),
        #[cfg(feature = "run")]
        Command::Eval { prompt, dataset, strict, show, llm, model, judge, no_record } => {
            let record = !no_record;
            if let Some(provider) = llm.as_deref() {
                commands::eval::run_llm(
                    &prompt, dataset, strict, show, provider, model.as_deref(), judge, record,
                )
            } else {
                commands::eval::run(&prompt, dataset, strict, show, record)
            }
        }
        #[cfg(not(feature = "run"))]
        Command::Eval { prompt, dataset, strict, show, no_record } => {
            commands::eval::run(&prompt, dataset, strict, show, !no_record)
        }

        Command::EvalLog { prompt } => commands::eval_log::run(&prompt),
        Command::Stash(cmd) => match cmd {
            prv::cli::StashCommand::Push => commands::stash::push(),
            prv::cli::StashCommand::Pop => commands::stash::pop(),
            prv::cli::StashCommand::Drop => commands::stash::drop(),
            prv::cli::StashCommand::List => commands::stash::list(),
        },
        Command::Reset { paths } => commands::reset::run(paths),
        Command::Clean { dry_run, force } => commands::clean::run(dry_run, force),
        Command::Export { spec, output } => commands::export::run(&spec, output),
        Command::Grep { pattern, case_sensitive } => commands::grep::run(&pattern, case_sensitive),
        Command::Stats => commands::stats::run(),
        Command::Tokens { prompt, vars, models, max_tokens, json } => {
            let parsed_vars = parse_kv(vars)?;
            commands::tokens::run(&prompt, parsed_vars, models, max_tokens, json)
        }
        Command::Metrics { prompt, vars, models, json } => {
            let parsed_vars = parse_kv(vars)?;
            commands::metrics::run(&prompt, parsed_vars, models, json)
        }
        Command::Blame { path } => commands::blame::run(&path),
        Command::Config(cmd) => match cmd {
            prv::cli::ConfigCommand::Get { key } => commands::config::get(&key),
            prv::cli::ConfigCommand::Set { key, value } => {
                commands::config::set(&key, &value)
            }
            prv::cli::ConfigCommand::List => commands::config::list(),
        },
        Command::Ignore(cmd) => match cmd {
            prv::cli::IgnoreCommand::Add { patterns } => commands::ignore::add(&patterns),
            prv::cli::IgnoreCommand::List => commands::ignore::list(),
            prv::cli::IgnoreCommand::Remove { patterns } => {
                commands::ignore::remove(&patterns)
            }
        },
        Command::Completions { shell, out } => commands::completions::run(&shell, out),
    }
}

/// Parse a list of `KEY=VALUE` strings into `(String, String)` pairs.
/// Used by `--var key=value` flags across `pv run`, `pv tokens`, `pv metrics`.
fn parse_kv(pairs: Vec<String>) -> anyhow::Result<Vec<(String, String)>> {
    pairs
        .into_iter()
        .map(|kv| {
            let (k, v) = kv
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("--var expects KEY=VALUE, got: {kv}"))?;
            Ok::<_, anyhow::Error>((k.to_string(), v.to_string()))
        })
        .collect()
}
