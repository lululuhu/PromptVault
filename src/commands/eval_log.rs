//! `pv eval-log <prompt>` — show eval history for a prompt.

use anyhow::Result;
use nu_ansi_term::Color;

use crate::commands::eval;
use crate::core::repository::Repo;
use crate::ui::printer;

pub fn run(prompt: &str) -> Result<()> {
    let repo = Repo::find()?;
    let history = eval::read_history(&repo, prompt)?;

    if history.is_empty() {
        printer::info(&format!("no eval history for '{prompt}'"));
        return Ok(());
    }

    println!(
        "{} {}  ({} runs)",
        printer::bold("Eval history:"),
        prompt,
        history.len()
    );
    println!();

    for (i, rec) in history.iter().enumerate() {
        let n = i + 1;
        let ts = rec["ts"].as_i64().unwrap_or(0);
        let mode = rec["mode"].as_str().unwrap_or("?");
        let total = rec["total"].as_i64().unwrap_or(0);
        let passed = rec["passed"].as_i64().unwrap_or(0);
        let dt = chrono::DateTime::from_timestamp(ts, 0)
            .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "?".into());

        let pct = if total > 0 {
            (passed as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        let score_part = if let Some(score) = rec["mean_score"].as_f64() {
            let colored = if score >= 8.0 {
                Color::Green.paint(format!("{score:.1}/10"))
            } else if score >= 5.0 {
                Color::Yellow.paint(format!("{score:.1}/10"))
            } else {
                Color::Red.paint(format!("{score:.1}/10"))
            };
            format!("  judge={colored}")
        } else {
            String::new()
        };

        println!(
            "[{n}] {}  {}  {passed}/{total} ({pct:.0}%){score_part}",
            Color::Cyan.paint(&dt),
            Color::DarkGray.paint(mode),
        );
    }

    // Summary line.
    let judged: Vec<_> = history
        .iter()
        .filter_map(|r| r["mean_score"].as_f64())
        .collect();
    if !judged.is_empty() {
        let mean = judged.iter().sum::<f64>() / judged.len() as f64;
        let latest = judged.last().copied().unwrap_or(0.0);
        let first = judged.first().copied().unwrap_or(0.0);
        let delta = latest - first;
        let delta_str = if delta >= 0.0 {
            Color::Green.paint(format!("{delta:+.1}"))
        } else {
            Color::Red.paint(format!("{delta:+.1}"))
        };
        println!();
        printer::info(&format!(
            "Judge trend: first={first:.1} → latest={latest:.1}  ({delta_str}, mean={mean:.1})"
        ));
    }

    Ok(())
}
