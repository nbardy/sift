mod core;
mod eval;
mod fusion;
mod parse;
mod rg;

use anyhow::{Context, Result};
use clap::Parser;
use colored::Colorize;
use core::{OutputFormat, ResultSet};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "ag", about = "Sift — a search DSL for agents. Compose, parallelize, and fuse code searches.")]
struct Cli {
    /// S-expression query, e.g. '(rg "TODO")'
    query: Option<String>,

    /// Read query from file
    #[arg(short = 'f', long)]
    file: Option<PathBuf>,

    /// Shorthand for (rg "PATTERN")
    #[arg(short = 'g', long)]
    grep: Option<String>,

    /// Output as JSON
    #[arg(long)]
    json: bool,

    /// Output with scores
    #[arg(long)]
    scores: bool,

    /// Output file paths only
    #[arg(long)]
    files: bool,

    /// Working directory (default: current dir)
    #[arg(short = 'C', long)]
    dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let cwd = cli
        .dir
        .clone()
        .unwrap_or_else(|| std::env::current_dir().expect("cannot determine cwd"));

    let query_str = resolve_query(&cli.query, &cli.file, &cli.grep)?;
    let expr = parse::parse(&query_str)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to parse query")?;

    let format = resolve_format(&cli, &expr);
    let ctx = eval::Ctx::new(rg::RgBackend::new(&cwd));
    let result = eval::eval(&expr, &ctx).await.map_err(|e| anyhow::anyhow!("{e}"))?;

    render(&result, format);
    Ok(())
}

fn resolve_query(
    query: &Option<String>,
    file: &Option<PathBuf>,
    grep: &Option<String>,
) -> Result<String> {
    if let Some(g) = grep {
        return Ok(format!(r#"(rg "{}")"#, g.replace('"', r#"\""#)));
    }
    if let Some(f) = file {
        return std::fs::read_to_string(f)
            .with_context(|| format!("reading query file: {}", f.display()));
    }
    if let Some(q) = query {
        return Ok(q.clone());
    }
    anyhow::bail!("no query provided. Usage: ag '(rg \"TODO\")' or ag -g TODO")
}

fn resolve_format(cli: &Cli, expr: &core::Expr) -> OutputFormat {
    if cli.json { return OutputFormat::Json; }
    if cli.scores { return OutputFormat::Scores; }
    if cli.files { return OutputFormat::Files; }
    eval::output_format(expr)
}

fn render(result: &ResultSet, format: OutputFormat) {
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&result.hits).unwrap_or_default());
        }
        OutputFormat::Files => {
            let mut seen = std::collections::HashSet::new();
            for hit in &result.hits {
                if seen.insert(&hit.path) {
                    println!("{}", hit.path.purple());
                }
            }
        }
        OutputFormat::Scores => render_scores(&result.hits),
        OutputFormat::Default => render_default(&result.hits),
    }
}

fn render_default(hits: &[core::Hit]) {
    let mut current_file = String::new();
    for hit in hits {
        if hit.path != current_file {
            if !current_file.is_empty() { println!(); }
            println!("{}", hit.path.purple().bold());
            current_file = hit.path.clone();
        }
        println!("  {}  {}", format!("{:>4}", hit.line).green(), hit.snippet);
    }
}

fn render_scores(hits: &[core::Hit]) {
    let mut current_file = String::new();
    for hit in hits {
        if hit.path != current_file {
            if !current_file.is_empty() { println!(); }
            println!("{}", hit.path.purple().bold());
            current_file = hit.path.clone();
        }
        let score_str = format!("[{:.2}]", hit.score.0);
        let colored_score = if hit.score.0 >= 0.5 {
            score_str.green().bold()
        } else if hit.score.0 >= 0.1 {
            score_str.yellow()
        } else {
            score_str.dimmed()
        };
        println!("  {} {} {}", format!("{:>4}", hit.line).green(), colored_score, hit.snippet);
    }
}
