# ag — Search DSL Cheatsheet

Binary: `ag`. S-expression DSL. Every expression returns a `ResultSet`. All combinators run children in parallel.

## Why This Is Fast

Every combinator fans out children as parallel async tasks. A single `ag` call replaces multiple sequential searches:

```bash
# One call, five parallel searches, labeled results — replaces 5 sequential greps
ag '(batch {:top 10}
  :todos    (mix (rg "TODO|FIXME") (rg "HACK|WORKAROUND"))
  :configs  (- (rg "learning_rate|lr.*=" :lang "py") (rg "test|mock"))
  :training (pipe (rg "def train") (rg "loss"))
  :logging  (& (rg "wandb|mlflow") (rg "loss|metric"))
  :debt     (top 20 (mix (rg "TODO|FIXME") (sem "technical debt"))))'

# Deep codebase audit in one shot
ag -C /path/to/repo --scores '(top 30
  (| (mix (rg "TODO|FIXME") (rg "bug|broken|regression"))
     (- (rg "learning_rate|lr.*=" :lang "py") (rg "test|mock"))
     (pipe (rg "def train") (rg "loss"))))'

# Named parallel security scan
ag '(batch
  :creds    (top 10 (- (rg "SECRET|TOKEN|API_KEY") (rg "test|mock")))
  :sql      (pipe (sem "authentication") (rg "format!.*SELECT"))
  :unsafe   (& (rg "unsafe") (- (rg "unsafe") (rg "// SAFETY"))))'
```

## Primitives

```bash
ag '(rg "regex")'                    # ripgrep — exact, no index
ag '(lex "query terms")'             # BM25 tantivy — ranked, indexed
ag '(sem "natural language")'        # embedding similarity — conceptual
```

Optional filters on any primitive: `:in "dir/"` `:lang "rust"` `:x "glob"` `:i "glob"`

## Combinators

```bash
ag '(& e ...)'                       # intersection — hits in ALL
ag '(| e ...)'                       # union — hits in ANY
ag '(mix e ...)'                     # blend via RRF (equal weight)
ag '(mix [w1 w2 ...] e ...)'        # weighted blend
ag '(- e1 e2)'                      # difference — e1 minus e2
ag '(pipe e1 e2)'                   # sequential — run e1, scope e2 to those files
```

## Filters

```bash
ag '(top k e)'                       # top k by score
ag '(> t e)'                         # score threshold >= t
```

## Batch (Named Parallel)

```bash
ag '(batch :label1 e1 :label2 e2)'   # parallel eval, labeled sections
ag '(batch {:top 5} :a e1 :b e2)'    # shared opts applied to each entry
ag --json '(batch :a e1 :b e2)'      # JSON: {"a": [...], "b": [...]}
```

Composes with `let`:
```bash
ag '(let [auth (rg "auth")]
  (batch :callers (- auth (rg "fn auth"))
         :tests   (& auth (rg "test"))))'
```

## Bindings

```bash
ag '(let [name expr ...] body)'      # bind intermediate results
```

## CLI

```bash
ag '(rg "TODO")'               # inline query
ag "TODO"                       # auto mode — wraps as (rg "TODO")
ag -g "TODO"                    # grep shorthand
ag -f query.sq                  # from file
ag --json '...'                 # JSON output
ag --files '...'                # paths only
ag --scores '...'               # with scores
ag -C dir '...'                 # working directory
ag --index                      # build lex/sem indexes
ag --index-status               # show index info
ag --index-clean                # remove .ag/ indexes
```

## Patterns

```bash
# find symbol
ag '(rg "fn parseConfig")'

# callers minus definition
ag '(- (rg "authenticate\\(") (rg "fn authenticate"))'

# find by concept
ag '(sem "error handling and recovery")'

# blend exact + semantic
ag '(mix (rg "TODO|FIXME") (sem "technical debt"))'

# multi-criteria intersection
ag '(& (sem "authentication" :lang "rust") (sem "database" :lang "rust"))'

# weighted blend, top results
ag '(top 10 (mix [0.6 0.4] (sem "retry backoff") (rg "retry|backoff")))'

# narrow then refine (sequential pipeline)
ag '(pipe (rg "pub struct") (rg "impl"))'

# three-way fusion
ag '(top 10 (mix (sem "entry point") (rg "fn main") (lex "main entry")))'

# reuse with let
ag '(let [auth (rg "auth" :lang "rs")] (top 5 (- auth (rg "test"))))'
```

## 13 Forms Total

Primitives: `rg` `lex` `sem` | Combinators: `&` `|` `mix` `-` `pipe` | Filters: `top` `>` | Parallel: `batch` | Bindings: `let` + var refs

## Backends

- `rg`: always available (needs ripgrep installed). No index. Fast. Exact.
- `lex`: `--features lex`. Tantivy BM25. Lazy index to `.ag/lex/`.
- `sem`: `--features sem`. ONNX embeddings. Lazy model download + index to `.ag/sem/`.
