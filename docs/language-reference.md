# Sift Language Reference

> See also: [Cheatsheet](cheatsheet.md) — dense single-page reference
> See also: [Examples](../examples/) — runnable `.sq` files for every feature

Sift is an S-expression DSL for composing search tools. Every expression evaluates to a `ResultSet` — a list of scored hits at specific file:line locations. The DSL has **12 forms**: 3 search primitives, 5 combinators, 2 filters, and let bindings.

---

## Search Primitives

Three backends, each with different strengths. All accept optional keyword filters.

| Form | Backend | When to use |
|---|---|---|
| `(rg "pattern")` | ripgrep | Exact string/regex. No index. Always fresh. Ground truth. |
| `(lex "query")` | tantivy BM25 | Ranked text retrieval. Handles stemming, tokenization. Fast repeated queries. |
| `(sem "query")` | ONNX embeddings | Conceptual similarity. Finds what you mean, not what you typed. |

```lisp
(rg "fn authenticate")
(rg "TODO|FIXME|HACK" :lang "rust" :x "*test*")
(lex "connection pool timeout" :lang "go")
(sem "error handling and recovery" :in "src/")
```

### Keyword Filters

Every search primitive accepts these optional filters:

| Keyword | Meaning | Example |
|---|---|---|
| `:in` | Scope to directory | `:in "src/auth/"` |
| `:lang` | Filter by file type | `:lang "rust"`, `:lang "py"`, `:lang "ts"` |
| `:x` | Exclude glob pattern | `:x "*test*"`, `:x "vendor/**"` |
| `:i` | Include glob pattern | `:i "*.rs"`, `:i "src/**"` |

Multiple `:x` and `:i` can be stacked:

```lisp
(rg "TODO" :x "*test*" :x "*vendor*" :lang "rust")
```

### Language Names

Both full names and common abbreviations work: `rust`/`rs`, `python`/`py`, `javascript`/`js`, `typescript`/`ts`, `ruby`/`rb`, `c++`/`cpp`/`cxx`, `shell`/`sh`/`bash`, `yaml`/`yml`, `markdown`/`md`.

---

## Combinators

All combinators that take multiple children fan them out as parallel async tasks. Total latency = max(children), not sum.

### Intersection: `&`

Returns only hits present in **all** children. Scored by minimum score across inputs.

```lisp
;; Lines containing both "async" and "tokio"
(& (rg "async") (rg "tokio"))

;; Rust files about both authentication and database
(& (sem "authentication" :lang "rust") (sem "database" :lang "rust"))

;; Three-way intersection
(& (rg "pub fn") (rg "Result") (rg "async"))
```

### Union: `|`

Returns hits from **any** child. When a hit appears in multiple children, keeps the highest score.

```lisp
;; Error-like patterns from any source
(| (rg "Error") (rg "panic") (rg "unwrap"))

;; Find any kind of import
(| (rg "use ") (rg "import ") (rg "require\\("))
```

### Mix (RRF Fusion): `mix`

Blends results from multiple sources using Reciprocal Rank Fusion. This is the primary way to combine heterogeneous backends — it normalizes across different scoring systems.

**Equal weight** (default):

```lisp
;; Blend exact + semantic for code debt
(mix (rg "TODO|FIXME|HACK") (sem "technical debt"))

;; Three-way blend: all backends
(mix (rg "fn main") (lex "main entry point") (sem "program entry"))
```

**Weighted blend** — pass weights in brackets. Weights are relative (don't need to sum to 1):

```lisp
;; Trust semantic more for conceptual queries
(mix [0.7 0.3] (sem "retry with exponential backoff") (rg "retry|backoff"))

;; Security audit: semantic leads, exact confirms
(mix [0.4 0.3 0.3]
  (sem "credentials secrets" :x "*test*")
  (lex "password secret api_key" :x "*test*")
  (rg "SECRET|TOKEN|API_KEY" :x "*test*"))
```

### Difference: `-`

Hits in the first expression that are **not** in the second. Matching is by file:line.

```lisp
;; Callers, excluding the definition
(- (rg "authenticate\\(") (rg "fn authenticate"))

;; Production code only (exclude tests)
(- (rg "pub fn") (rg "#\\[test\\]"))

;; Find TODO/FIXME but not in vendor
(- (rg "TODO|FIXME") (rg "vendor/"))
```

### Sequential Pipeline: `pipe`

Evaluates the source expression first, extracts the set of matching **files**, then rewrites the target expression to scope all its searches to only those files. This enables tiered search: narrow first, refine second.

```lisp
;; Find files with structs, then search those for impl blocks
(pipe (rg "pub struct") (rg "impl"))

;; Narrow to auth files, then look for SQL injection risks
(pipe (rg "authenticate") (rg "SELECT|INSERT|UPDATE"))

;; Concept-guided narrowing: semantic first, exact second
(pipe (sem "database connection") (rg "pool|timeout"))
```

---

## Filters

### Top K: `top`

Keep only the top `k` results by score.

```lisp
(top 10 (rg "TODO"))
(top 5 (mix (sem "auth") (rg "auth")))
```

### Score Threshold: `>`

Keep only results with score >= threshold (0.0 to 1.0).

```lisp
(> 0.5 (mix (sem "error handling") (rg "catch|rescue")))
(> 0.8 (sem "security vulnerability"))
```

---

## Bindings: `let`

Name intermediate results to reuse them without re-executing.

```lisp
;; Name a result, then filter it
(let [x (rg "TODO")]
  (top 5 x))

;; Multiple bindings
(let [auth  (rg "authenticate" :lang "rust")
      tests (rg "test" :lang "rust")]
  (- auth tests))

;; Complex composition
(let [structs (rg "pub struct" :lang "rs")
      impls   (rg "impl" :lang "rs")]
  (top 20 (& structs impls)))
```

---

## CLI

```
ag '(rg "TODO")'               # inline query
ag "TODO"                       # auto mode — plain text wraps as (rg "TODO")
ag -g "TODO"                    # grep shorthand
ag -f query.sq                  # read query from file
ag --json '...'                 # JSON output
ag --files '...'                # deduplicated file paths only
ag --scores '...'               # file:line [score] snippet
ag -C /path/to/repo '(...)'    # set working directory
ag --index                      # build lex/sem indexes
ag --index-status               # show index status and sizes
ag --index-clean                # remove .ag/ directory
```

### Auto Mode

If the query doesn't start with `(`, it's automatically wrapped as `(rg "query")`:

```bash
ag "pub struct"    # equivalent to: ag '(rg "pub struct")'
```

---

## Backend Availability

| Backend | Feature flag | Binary size | Cold start | Warm |
|---|---|---|---|---|
| `rg` | _(always)_ | +0 (shells out) | 10-50ms | 10-50ms |
| `lex` | `--features lex` | +3-5MB (tantivy) | 500ms-2s (index build) | 1-5ms |
| `sem` | `--features sem` | +5-8MB (ort) | 2-10s (model + index) | 50-150ms |

Build with all backends: `cargo build --features full`

If a query uses `lex` or `sem` without the feature, `ag` returns a clear error:
```
lex backend not available (compile with --features lex)
```

### Index Storage

Indexes persist to `.ag/` in the working directory:
- `.ag/lex/` — tantivy index files
- `.ag/sem/index.json` — cached embeddings

Pre-build indexes: `ag --index`
Check status: `ag --index-status`
Clean up: `ag --index-clean`

---

## Execution Model

Every combinator fans out children as concurrent async tasks:

```
(mix (rg "x") (lex "x") (sem "x"))

     mix
    / | \
   rg lex sem     <- 3 parallel tasks
    \ | /
     RRF          <- fuse when all complete
```

**Nested parallelism** composes naturally:

```
(& (sem "auth") (| (rg "login") (lex "authenticate")))

       &
      / \
    sem   |        <- 2 tasks at top level
         / \
       rg  lex     <- 2 subtasks inside |
```

**Sequential pipelines** are the exception — source must complete before target starts:

```
(pipe (rg "struct") (rg "impl"))

  rg "struct"     <- phase 1: find files
      |
  rg "impl"       <- phase 2: search those files only
```

---

## Composition Cookbook

### Finding Related Code

```lisp
;; Definition and all callers
(| (rg "fn handle_request") (rg "handle_request\\("))

;; All callers excluding definition
(- (rg "handle_request\\(") (rg "fn handle_request"))
```

### Multi-Signal Search

```lisp
;; Blend exact + semantic when you sort-of know the term
(mix (rg "retry|backoff|exponential") (sem "retry with exponential backoff"))

;; All three backends for maximum recall
(top 15 (mix (sem "database migration") (lex "migrate schema") (rg "migrate|migration")))
```

### Scoped Exploration

```lisp
;; Find auth-related files, then look for potential issues
(pipe (sem "authentication authorization") (rg "TODO|FIXME|HACK|unsafe"))

;; Narrow to error handling, then find unhandled cases
(pipe (rg "Result<") (rg "unwrap\\(\\)"))
```

### Security Auditing

```lisp
;; Credentials in code (excluding test fixtures)
(top 20 (mix [0.4 0.3 0.3]
  (sem "hardcoded credentials secrets" :x "*test*")
  (lex "password secret api_key token" :x "*test*")
  (rg "SECRET|TOKEN|API_KEY|password" :x "*test*")))

;; SQL injection risks in auth code
(pipe (sem "authentication") (rg "format!.*SELECT|format!.*INSERT"))
```

### Refactoring Prep

```lisp
;; Find all usage of old API
(let [usage (rg "old_client\\.request")]
  (top 20 (& usage (sem "HTTP client usage"))))

;; Struct definitions with their impl blocks
(& (rg "pub struct") (rg "impl"))
```

### Cross-Language Search

```lisp
;; Same concept across Rust and Python
(| (sem "database connection" :lang "rust") (sem "database connection" :lang "python"))

;; Config files only
(rg "database_url|DB_HOST" :lang "toml")
```
