# Sift Language Reference

> See also: [Examples](../examples/) — runnable `.sq` files for every feature

## Search Primitives

All take optional keyword filters: `:in`, `:lang`, `:x` (exclude), `:i` (include).

| Form | Backend | Description |
|---|---|---|
| `(rg "pattern")` | ripgrep | Exact regex, no index, line-level |
| `(lex "query")` | tantivy BM25 | Ranked text retrieval, indexed |
| `(sem "query")` | embeddings | Semantic similarity search |

```lisp
(rg "pattern")
(rg "pattern" :lang "rust" :x "*test*")
(sem "natural language query" :in "src/")
(lex "connection pool" :lang "go")
```

### Keyword Filters

| Keyword | Meaning | Example |
|---|---|---|
| `:in` | Directory scope | `:in "src/auth/"` |
| `:lang` | File type | `:lang "rust"` |
| `:x` | Exclude glob | `:x "*test*"` |
| `:i` | Include glob | `:i "*.rs"` |

## Combinators

All fan out children in parallel automatically.

| Form | Operation | Description |
|---|---|---|
| `(& e ...)` | Intersection | Hits in ALL children |
| `(\| e ...)` | Union | Hits in ANY child, best score wins |
| `(mix e ...)` | RRF blend | Reciprocal Rank Fusion, equal weight |
| `(mix [w ...] e ...)` | Weighted RRF | Explicit weights per child |
| `(- e1 e2)` | Difference | Hits in e1 but NOT in e2 |

```lisp
(& (rg "async") (rg "tokio"))
(| (rg "Error") (rg "panic"))
(mix [0.6 0.4] (rg "retry") (rg "backoff"))
(- (rg "authenticate\\(") (rg "fn authenticate"))
```

## Filters

| Form | Description |
|---|---|
| `(top k e)` | Top k results by score |
| `(> t e)` | Score threshold >= t |

```lisp
(top 10 (rg "TODO"))
(> 0.5 (mix (rg "auth") (rg "login")))
```

## Output

Outermost wrapper, or use CLI flags.

| Form | CLI flag | Description |
|---|---|---|
| `(files e)` | `--files` | Deduplicated file paths |
| `(scores e)` | `--scores` | file:line [score] snippet |
| `(json e)` | `--json` | Machine-readable JSON |

## Bindings

Name intermediate results to reuse.

```lisp
(let [x (rg "TODO")]
  (top 5 x))

(let [auth  (rg "authenticate" :lang "rust")
      tests (rg "test" :lang "rust")]
  (- auth tests))
```

## CLI

```
ag '(rg "TODO")'              # inline query
ag -f query.sq                 # from file
ag -g "TODO"                   # grep shorthand
ag --json '(rg "TODO")'       # JSON output
ag --files '(rg "TODO")'      # paths only
ag --scores '(rg "TODO")'     # with scores
ag -C /path/to/repo '(...)'   # set working directory
```

## Backend Availability

| Backend | Status | Required |
|---|---|---|
| `rg` | Available | `ripgrep` installed on system |
| `lex` | Planned | Compile with `--features lex` |
| `sem` | Planned | Compile with `--features sem` |

## Execution Model

Every combinator fans out children as concurrent async tasks:

```
(mix (rg "x") (rg "y") (rg "z"))

     mix
    / | \
   rg rg rg     <- 3 parallel tasks
    \ | /
     RRF         <- fuse when all complete
```

Total latency = max(children), not sum.
