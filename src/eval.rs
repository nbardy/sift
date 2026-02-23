use crate::core::{Env, Expr, ResultSet, SearchBackend, SqError, Weights};
use crate::rg::RgBackend;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

/// Execution context: holds backends and environment.
pub struct Ctx {
    pub cwd: PathBuf,
    pub rg: RgBackend,
    #[cfg(feature = "lex")]
    pub lex: crate::lex::LexBackend,
    #[cfg(feature = "sem")]
    pub sem: crate::sem::SemBackend,
    pub env: Env,
}

impl Ctx {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            rg: RgBackend::new(&cwd),
            #[cfg(feature = "lex")]
            lex: crate::lex::LexBackend::new(&cwd),
            #[cfg(feature = "sem")]
            sem: crate::sem::SemBackend::new(&cwd),
            env: Env::new(),
            cwd,
        }
    }

    /// Create a child context with a new env (for let bindings).
    fn fork(&self, env: Env) -> Self {
        Self {
            rg: RgBackend::new(&self.cwd),
            #[cfg(feature = "lex")]
            lex: crate::lex::LexBackend::new(&self.cwd),
            #[cfg(feature = "sem")]
            sem: crate::sem::SemBackend::new(&self.cwd),
            env,
            cwd: self.cwd.clone(),
        }
    }
}

/// Evaluate an expression, returning a ResultSet.
///
/// Thin dispatcher delta: one match arm per Expr variant, each delegates
/// to a single handler. Combinators fan out children as parallel tasks.
pub fn eval<'a>(
    expr: &'a Expr,
    ctx: &'a Ctx,
) -> Pin<Box<dyn Future<Output = Result<ResultSet, SqError>> + Send + 'a>> {
    Box::pin(async move {
        match expr {
            Expr::Rg(query, opts) => ctx.rg.search(query, opts).await,

            #[cfg(feature = "lex")]
            Expr::Lex(query, opts) => ctx.lex.search(query, opts).await,
            #[cfg(not(feature = "lex"))]
            Expr::Lex(_, _) => Err(SqError::LexUnavailable),

            #[cfg(feature = "sem")]
            Expr::Sem(query, opts) => ctx.sem.search(query, opts).await,
            #[cfg(not(feature = "sem"))]
            Expr::Sem(_, _) => Err(SqError::SemUnavailable),

            Expr::And(children) => {
                let results = eval_parallel(children, ctx).await?;
                Ok(crate::fusion::intersect(&results))
            }
            Expr::Or(children) => {
                let results = eval_parallel(children, ctx).await?;
                Ok(crate::fusion::union(&results))
            }
            Expr::Mix(weights, children) => {
                let results = eval_parallel(children, ctx).await?;
                Ok(match weights {
                    Weights::Equal => crate::fusion::rrf(&results),
                    Weights::Explicit(ws) => crate::fusion::rrf_weighted(&results, ws),
                })
            }
            Expr::Diff(left, right) => {
                let (l, r) = tokio::join!(eval(left, ctx), eval(right, ctx));
                Ok(crate::fusion::difference(&l?, &r?))
            }
            Expr::Pipe(source, target) => {
                let source_result = eval(source, ctx).await?;
                let file_paths: Vec<String> = source_result
                    .hits
                    .iter()
                    .map(|h| h.path.clone())
                    .collect::<std::collections::HashSet<_>>()
                    .into_iter()
                    .collect();
                // Scope the target search to files from the source
                let scoped = scope_expr_to_files(target, &file_paths);
                eval(&scoped, ctx).await
            }

            Expr::Top(k, child) => Ok(crate::fusion::top_k(&eval(child, ctx).await?, *k)),
            Expr::Threshold(t, child) => {
                Ok(crate::fusion::threshold(&eval(child, ctx).await?, *t))
            }

            Expr::Let(bindings, body) => {
                let mut inner_ctx = ctx.fork(ctx.env.clone());
                for binding in bindings {
                    let val = eval(&binding.value, &inner_ctx).await?;
                    inner_ctx.env.insert(binding.name.clone(), val);
                }
                eval(body, &inner_ctx).await
            }

            Expr::Var(name) => ctx
                .env
                .get(name)
                .cloned()
                .ok_or_else(|| SqError::UnboundVar(name.clone())),
        }
    })
}

/// Rewrite an expression tree so all search primitives are scoped to the given file paths.
/// This powers the (pipe source target) sequential combinator.
fn scope_expr_to_files(expr: &Expr, files: &[String]) -> Expr {
    match expr {
        Expr::Rg(q, opts) | Expr::Lex(q, opts) | Expr::Sem(q, opts) => {
            let mut new_opts = opts.clone();
            new_opts.include.extend(files.iter().cloned());
            match expr {
                Expr::Rg(..) => Expr::Rg(q.clone(), new_opts),
                Expr::Lex(..) => Expr::Lex(q.clone(), new_opts),
                Expr::Sem(..) => Expr::Sem(q.clone(), new_opts),
                _ => unreachable!(),
            }
        }
        Expr::And(cs) => Expr::And(cs.iter().map(|c| scope_expr_to_files(c, files)).collect()),
        Expr::Or(cs) => Expr::Or(cs.iter().map(|c| scope_expr_to_files(c, files)).collect()),
        Expr::Mix(w, cs) => Expr::Mix(
            w.clone(),
            cs.iter().map(|c| scope_expr_to_files(c, files)).collect(),
        ),
        Expr::Diff(l, r) => Expr::Diff(
            Box::new(scope_expr_to_files(l, files)),
            Box::new(scope_expr_to_files(r, files)),
        ),
        Expr::Pipe(s, t) => Expr::Pipe(
            Box::new(scope_expr_to_files(s, files)),
            Box::new(scope_expr_to_files(t, files)),
        ),
        Expr::Top(k, c) => Expr::Top(*k, Box::new(scope_expr_to_files(c, files))),
        Expr::Threshold(t, c) => Expr::Threshold(*t, Box::new(scope_expr_to_files(c, files))),
        Expr::Let(bindings, body) => {
            let new_bindings = bindings
                .iter()
                .map(|b| crate::core::Binding {
                    name: b.name.clone(),
                    value: scope_expr_to_files(&b.value, files),
                })
                .collect();
            Expr::Let(
                new_bindings,
                Box::new(scope_expr_to_files(body, files)),
            )
        }
        Expr::Var(_) => expr.clone(),
    }
}

async fn eval_parallel(children: &[Expr], ctx: &Ctx) -> Result<Vec<ResultSet>, SqError> {
    let futures: Vec<_> = children.iter().map(|child| eval(child, ctx)).collect();
    let results = futures::future::join_all(futures).await;
    results.into_iter().collect()
}

