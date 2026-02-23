mod core;
mod eval;
mod fusion;
#[cfg(feature = "lex")]
mod lex;
mod parse;
mod rg;
#[cfg(feature = "sem")]
mod sem;
mod util;

use anyhow::{Context, Result};
use clap::Parser;
use colored::Colorize;
use core::{OutputFormat, ResultSet};
#[cfg(any(feature = "lex", feature = "sem"))]
use core::SearchBackend;
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

    /// Build lex/sem indexes for the current directory
    #[arg(long)]
    index: bool,

    /// Show index status
    #[arg(long)]
    index_status: bool,

    /// Remove all indexes (.ag/ directory)
    #[arg(long)]
    index_clean: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let cwd = cli
        .dir
        .clone()
        .unwrap_or_else(|| std::env::current_dir().expect("cannot determine cwd"));

    // Index management commands
    if cli.index_clean {
        return cmd_index_clean(&cwd);
    }
    if cli.index_status {
        return cmd_index_status(&cwd);
    }
    if cli.index {
        return cmd_index_build(&cwd).await;
    }

    let query_str = resolve_query(&cli.query, &cli.file, &cli.grep)?;
    // Auto mode: if query doesn't start with '(', wrap it as an rg search
    let query_str = auto_wrap(&query_str);
    let expr = parse::parse(&query_str)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to parse query")?;

    let format = resolve_format(&cli);
    let ctx = eval::Ctx::new(cwd);
    let result = eval::eval(&expr, &ctx).await.map_err(|e| anyhow::anyhow!("{e}"))?;

    render(&result, format);
    Ok(())
}

// ── Index management ─────────────────────────────────────────────

fn cmd_index_clean(cwd: &PathBuf) -> Result<()> {
    let ag_dir = cwd.join(".ag");
    if ag_dir.exists() {
        std::fs::remove_dir_all(&ag_dir)
            .with_context(|| format!("failed to remove {}", ag_dir.display()))?;
        eprintln!("{}", "Removed .ag/ index directory.".green());
    } else {
        eprintln!("No .ag/ directory found.");
    }
    Ok(())
}

fn cmd_index_status(cwd: &PathBuf) -> Result<()> {
    let ag_dir = cwd.join(".ag");
    if !ag_dir.exists() {
        println!("No indexes found. Run {} to build.", "ag --index".bold());
        return Ok(());
    }

    let lex_dir = ag_dir.join("lex");
    if lex_dir.join("meta.json").exists() {
        let size = dir_size(&lex_dir);
        println!("  {} {} ({})", "lex".green().bold(), "ready".green(), format_bytes(size));
    } else {
        println!("  {} {}", "lex".dimmed(), "not built".dimmed());
    }

    let sem_dir = ag_dir.join("sem");
    if sem_dir.exists() {
        let size = dir_size(&sem_dir);
        println!("  {} {} ({})", "sem".green().bold(), "ready".green(), format_bytes(size));
    } else {
        println!("  {} {}", "sem".dimmed(), "not built".dimmed());
    }

    Ok(())
}

async fn cmd_index_build(_cwd: &PathBuf) -> Result<()> {
    eprintln!("{}", "Building indexes...".bold());

    // Build lex index by running a dummy query
    #[cfg(feature = "lex")]
    {
        let backend = lex::LexBackend::new(_cwd);
        let opts = crate::core::SearchOpts::default();
        // Trigger index build (search for something common)
        let _ = backend.search("index_build_trigger", &opts).await;
        eprintln!("  {} {}", "lex".green().bold(), "done");
    }
    #[cfg(not(feature = "lex"))]
    {
        eprintln!("  {} {} (compile with --features lex)", "lex".dimmed(), "skipped".dimmed());
    }

    #[cfg(feature = "sem")]
    {
        eprintln!("  {} downloading model and building embeddings...", "sem".yellow());
        let backend = sem::SemBackend::new(_cwd);
        let opts = crate::core::SearchOpts::default();
        let _ = backend.search("index build trigger", &opts).await;
        eprintln!("  {} {}", "sem".green().bold(), "done");
    }
    #[cfg(not(feature = "sem"))]
    {
        eprintln!("  {} {} (compile with --features sem)", "sem".dimmed(), "skipped".dimmed());
    }

    Ok(())
}

fn dir_size(path: &std::path::Path) -> u64 {
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

// ── Auto mode ────────────────────────────────────────────────────

/// If the query isn't an s-expression (no leading paren), auto-wrap it.
/// Plain strings become (rg "query") as the universal fallback.
fn auto_wrap(query: &str) -> String {
    let trimmed = query.trim();
    if trimmed.starts_with('(') || trimmed.starts_with(';') {
        // Already an s-expression or starts with comment
        return query.to_string();
    }
    // Plain text: wrap as rg
    format!(r#"(rg "{}")"#, trimmed.replace('\\', r"\\").replace('"', r#"\""#))
}

// ── Query resolution ─────────────────────────────────────────────

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

fn resolve_format(cli: &Cli) -> OutputFormat {
    if cli.json { return OutputFormat::Json; }
    if cli.scores { return OutputFormat::Scores; }
    if cli.files { return OutputFormat::Files; }
    OutputFormat::Default
}

// ── Output rendering ─────────────────────────────────────────────

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
