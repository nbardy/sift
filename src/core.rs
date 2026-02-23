use serde::Serialize;
use std::collections::HashMap;
use std::future::Future;

// ── Canonical domain types ──────────────────────────────────────────

/// A single search hit: one match at a specific file:line.
#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub path: String,
    pub line: u32,
    pub snippet: String,
    pub score: Score,
}

/// Score: always present, always meaningful.
/// Backends that don't rank (rg) assign scores by arrival order.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize)]
pub struct Score(pub f64);

impl Score {
    pub const ZERO: Score = Score(0.0);
}

/// The universal return type. Every expression evaluates to a ResultSet.
#[derive(Debug, Clone, Serialize)]
pub struct ResultSet {
    pub hits: Vec<Hit>,
}

impl ResultSet {
    pub fn empty() -> Self {
        Self { hits: vec![] }
    }

    pub fn from_hits(hits: Vec<Hit>) -> Self {
        Self { hits }
    }

    /// Assign descending scores by position (for unranked backends like rg).
    pub fn with_positional_scores(mut self) -> Self {
        let n = self.hits.len() as f64;
        for (i, hit) in self.hits.iter_mut().enumerate() {
            hit.score = Score(1.0 - (i as f64 / n.max(1.0)));
        }
        self
    }

    /// Sort by score descending.
    pub fn sorted(mut self) -> Self {
        self.hits
            .sort_by(|a, b| b.score.0.partial_cmp(&a.score.0).unwrap_or(std::cmp::Ordering::Equal));
        self
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }
}

// ── Search options (keyword filters) ────────────────────────────────

/// Filters that any search primitive can accept.
/// :in, :lang, :x (exclude globs), :i (include globs).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchOpts {
    pub scope: Option<String>,
    pub lang: Option<String>,
    pub exclude: Vec<String>,
    pub include: Vec<String>,
}

// ── AST: the DSL as a sum type ──────────────────────────────────────

/// D = ⊕ᵢ Dᵢ — every syntactic form is a variant.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    // Search primitives
    Rg(String, SearchOpts),
    Lex(String, SearchOpts),
    Sem(String, SearchOpts),

    // Combinators
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Mix(Weights, Vec<Expr>),
    Diff(Box<Expr>, Box<Expr>),
    /// Sequential pipeline: evaluate source, extract file paths, scope target to those paths.
    /// (pipe source target) — e.g. (pipe (rg "auth") (rg "TODO"))
    Pipe(Box<Expr>, Box<Expr>),

    // Filters
    Top(usize, Box<Expr>),
    Threshold(f64, Box<Expr>),

    // Bindings
    Let(Vec<Binding>, Box<Expr>),
    Var(String),
}

/// Weights for mix: either equal (auto) or explicit.
#[derive(Debug, Clone, PartialEq)]
pub enum Weights {
    Equal,
    Explicit(Vec<f64>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub name: String,
    pub value: Expr,
}

/// How to render the final ResultSet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputFormat {
    Default,
    Files,
    Scores,
    Json,
}

// ── Errors ──────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum SqError {
    #[error("rg backend error: {0}")]
    Rg(String),

    #[error("lex backend not available (compile with --features lex)")]
    LexUnavailable,

    #[error("sem backend not available (compile with --features sem)")]
    SemUnavailable,

    #[error("unbound variable: {0}")]
    UnboundVar(String),

    #[error("{0}")]
    #[allow(dead_code)]
    Other(String),
}

// ── Backend trait ───────────────────────────────────────────────────

/// Trait that each search backend implements.
/// One method, no structural branching — the dispatcher picks the backend.
pub trait SearchBackend: Send + Sync {
    fn search(
        &self,
        query: &str,
        opts: &SearchOpts,
    ) -> impl Future<Output = Result<ResultSet, SqError>> + Send;
}

// ── Env for let bindings ────────────────────────────────────────────

pub type Env = HashMap<String, ResultSet>;
