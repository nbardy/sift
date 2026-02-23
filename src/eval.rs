use crate::core::{Env, Expr, OutputFormat, ResultSet, SearchBackend, SqError, Weights};
use crate::rg::RgBackend;
use std::future::Future;
use std::pin::Pin;

/// Execution context: holds backends and environment.
pub struct Ctx {
    pub rg: RgBackend,
    pub env: Env,
}

impl Ctx {
    pub fn new(rg: RgBackend) -> Self {
        Self { rg, env: Env::new() }
    }
}

/// Evaluate an expression, returning a ResultSet.
///
/// Thin dispatcher δ: one match arm per Expr variant, each delegates
/// to a single handler. Combinators fan out children as parallel tasks.
pub fn eval<'a>(
    expr: &'a Expr,
    ctx: &'a Ctx,
) -> Pin<Box<dyn Future<Output = Result<ResultSet, SqError>> + Send + 'a>> {
    Box::pin(async move {
        match expr {
            Expr::Rg(query, opts)  => ctx.rg.search(query, opts).await,
            Expr::Lex(_, _)        => Err(SqError::LexUnavailable),
            Expr::Sem(_, _)        => Err(SqError::SemUnavailable),

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
                    Weights::Equal      => crate::fusion::rrf(&results),
                    Weights::Explicit(ws) => crate::fusion::rrf_weighted(&results, ws),
                })
            }
            Expr::Diff(left, right) => {
                let (l, r) = tokio::join!(eval(left, ctx), eval(right, ctx));
                Ok(crate::fusion::difference(&l?, &r?))
            }

            Expr::Top(k, child)       => Ok(crate::fusion::top_k(&eval(child, ctx).await?, *k)),
            Expr::Threshold(t, child)  => Ok(crate::fusion::threshold(&eval(child, ctx).await?, *t)),

            Expr::Files(child) | Expr::Scores(child) | Expr::Json(child) => eval(child, ctx).await,

            Expr::Let(bindings, body) => {
                let mut inner_ctx = Ctx { rg: RgBackend::new(&ctx.rg.cwd), env: ctx.env.clone() };
                for binding in bindings {
                    let val = eval(&binding.value, &inner_ctx).await?;
                    inner_ctx.env.insert(binding.name.clone(), val);
                }
                eval(body, &inner_ctx).await
            }

            Expr::Var(name) => ctx.env.get(name).cloned().ok_or_else(|| SqError::UnboundVar(name.clone())),
        }
    })
}

async fn eval_parallel(children: &[Expr], ctx: &Ctx) -> Result<Vec<ResultSet>, SqError> {
    let futures: Vec<_> = children.iter().map(|child| eval(child, ctx)).collect();
    let results = futures::future::join_all(futures).await;
    results.into_iter().collect()
}

/// Extract the output format from the outermost AST node.
pub fn output_format(expr: &Expr) -> OutputFormat {
    match expr {
        Expr::Files(_)  => OutputFormat::Files,
        Expr::Scores(_) => OutputFormat::Scores,
        Expr::Json(_)   => OutputFormat::Json,
        _               => OutputFormat::Default,
    }
}
