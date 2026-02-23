# ag — Search DSL Cheatsheet

Binary: `ag`. S-expression DSL. Every expression returns a `ResultSet`. All combinators run children in parallel.

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

## 12 Forms Total

Primitives: `rg` `lex` `sem` | Combinators: `&` `|` `mix` `-` `pipe` | Filters: `top` `>` | Bindings: `let` + var refs

## Backends

- `rg`: always available (needs ripgrep installed). No index. Fast. Exact.
- `lex`: `--features lex`. Tantivy BM25. Lazy index to `.ag/lex/`.
- `sem`: `--features sem`. ONNX embeddings. Lazy model download + index to `.ag/sem/`.
