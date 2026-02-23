# Sift

A search DSL for agents. Compose, parallelize, and fuse code searches.

Sift is a tiny Lisp that composes search backends (ripgrep, BM25, embeddings) into parallel pipelines. Instead of an agent making five sequential grep calls and merging results in application code, it writes one expression and gets back ranked, deduplicated, blended results — automatically parallelized across backends.

The CLI command is `ag`.

```lisp
;; Find callers of eval, excluding its definition
(- (rg "eval\\(") (rg "pub fn eval"))

;; Blend three search methods, take top 10
(top 10 (mix (sem "auth flow") (rg "authenticate") (lex "authentication")))

;; Sequential pipeline: find files with structs, then search those for impls
(pipe (rg "pub struct") (rg "impl"))
```

## Why

Agents grep. Sometimes they grep well, sometimes they miss things. The gap between "run ripgrep" and "actually find what I need" is filled with orchestration code — multiple tool calls, deduplication, re-ranking, set operations. Sift collapses that into a single expression.

- **Parallel by default** — `(mix (rg "x") (sem "x"))` runs both backends concurrently. Total time = slowest backend, not the sum.
- **Set algebra on results** — intersection, union, difference. Find lines matching A AND B. Find callers minus definitions.
- **Ranked fusion** — [Reciprocal Rank Fusion](https://plg.uwaterloo.ca/~gvcormac/cormacksigir09-rrf.pdf) blends results from different backends into a single ranked list.
- **Three backends** — `rg` (exact grep), `lex` (BM25 via tantivy), `sem` (embeddings via ONNX). Feature-gated so you only compile what you need.
- **Sequential pipelines** — `(pipe source target)` runs source first, then scopes target to matching files.
- **Auto mode** — `ag "query"` without parens auto-wraps as ripgrep search.
- **Single binary** — <2MB default, ~10MB with all features. No runtime, no config.

## Install

```bash
# Default (rg backend only, <2MB)
cargo install --path .

# With BM25 indexing
cargo install --path . --features lex

# With semantic embeddings
cargo install --path . --features sem

# Everything
cargo install --path . --features full
```

Requires [ripgrep](https://github.com/BurntSushi/ripgrep): `brew install ripgrep`

## Quick Start

```bash
# Simple grep
ag '(rg "TODO")'
ag -g "TODO"                          # shorthand
ag "TODO"                             # auto mode (wraps as rg)

# Intersection: lines with both patterns
ag '(& (rg "async") (rg "tokio"))'

# Difference: callers minus definition
ag '(- (rg "eval\\(") (rg "pub fn eval"))'

# Sequential pipeline: narrow then refine
ag '(pipe (rg "pub struct") (rg "impl"))'

# Top 5 results
ag '(top 5 (rg "unsafe" :lang "rust"))'

# Blend with RRF
ag '(mix (rg "error") (rg "panic"))'

# BM25 search (requires --features lex)
ag '(lex "connection pool")'

# Semantic search (requires --features sem)
ag '(sem "error handling and recovery")'

# Output modes
ag --files '(rg "TODO")'              # paths only
ag --json '(rg "TODO")'               # machine-readable
ag --scores '(rg "TODO")'             # with relevance scores

# Index management
ag --index                            # build lex/sem indexes
ag --index-status                     # show index info
ag --index-clean                      # remove indexes
```

## Docs

- **[Language Reference](docs/language-reference.md)** — full syntax, all forms, execution model
- **[Cheatsheet](docs/cheatsheet.md)** — dense single-page reference (ideal for LLM context)
- **[Examples](examples/)** — runnable `.sq` files covering every feature

## Examples

Every example searches this repo and is tested in CI.

| File | Feature | What it does |
|---|---|---|
| [basics.sq](examples/basics.sq) | `rg` | Simple pattern search |
| [set-operations.sq](examples/set-operations.sq) | `-` | Difference: callers minus definitions |
| [intersection.sq](examples/intersection.sq) | `&` | Lines matching ALL patterns |
| [union.sq](examples/union.sq) | `\|` | Lines matching ANY pattern |
| [ranking.sq](examples/ranking.sq) | `mix`, `top` | RRF blend + top-k |
| [weighted-mix.sq](examples/weighted-mix.sq) | `mix [w ...]` | Weighted RRF blend |
| [threshold.sq](examples/threshold.sq) | `>` | Score threshold filter |
| [filters.sq](examples/filters.sq) | `:lang`, `:x` | File type + exclude filters |
| [let-bindings.sq](examples/let-bindings.sq) | `let` | Named intermediate results |
| [pipe.sq](examples/pipe.sq) | `pipe` | Sequential pipeline search |
| [agent-patterns.sq](examples/agent-patterns.sq) | combined | Multi-step agent strategies |
| [output-modes.sq](examples/output-modes.sq) | output | files, scores, json rendering |

Run any example: `ag -f examples/basics.sq`

## Architecture

```
ag '(& (rg "async") (rg "fn"))'

        &
       / \
     rg   rg      <- parallel tokio tasks
      \  /
    intersect      <- fuse results
        |
     output
```

Eight modules, one binary:

| Module | Purpose |
|---|---|
| [`core`](src/core.rs) | Hit, ResultSet, Score, Expr AST, errors |
| [`parse`](src/parse.rs) | S-expression tokenizer + recursive descent parser |
| [`rg`](src/rg.rs) | Ripgrep backend (shells out to `rg --json`) |
| [`lex`](src/lex.rs) | BM25 backend via tantivy (feature-gated) |
| [`sem`](src/sem.rs) | Embedding backend via ONNX Runtime (feature-gated) |
| [`fusion`](src/fusion.rs) | RRF, intersect, union, difference, top-k, threshold |
| [`eval`](src/eval.rs) | Async evaluator — thin dispatcher, parallel fan-out |
| [`util`](src/util.rs) | Shared helpers: file filtering, lang matching, glob |

### Feature Flags

| Feature | Adds | Binary size |
|---|---|---|
| _(default)_ | `rg` backend + DSL | ~2MB |
| `lex` | tantivy BM25 indexing | ~5MB |
| `sem` | ONNX embedding search | ~8MB |
| `full` | everything | ~10MB |

## Technical Details

### Reciprocal Rank Fusion (RRF)

Different search backends produce incomparable scores — ripgrep doesn't score at all, BM25 returns term frequencies, embeddings return cosine similarities. Comparing or averaging these raw numbers is meaningless.

RRF sidesteps this entirely by ignoring scores and using only **rank position**:

```
RRF_score(hit) = Σ  weight_i / (k + rank_i)
```

Where `k=60` is a smoothing constant. A hit ranked #1 by two backends scores higher than one ranked #1 by one and #50 by the other — regardless of what the raw scores were. This makes `(mix (rg "x") (sem "x"))` meaningful even though the backends measure completely different things.

After fusion, scores are **normalized to [0, 1]** by dividing by the maximum, so thresholds and display are intuitive.

### Parallel Fan-Out

Every combinator (`&`, `|`, `mix`, `-`) spawns its children as concurrent tokio tasks via `futures::join_all`. A query like:

```lisp
(mix (rg "auth") (rg "login") (rg "session"))
```

runs three ripgrep processes simultaneously. Total latency = slowest child, not the sum. This extends to nested expressions — the evaluator recurses into children in parallel at every level of the AST.

### Sequential Pipelines

The `pipe` combinator provides tiered search — narrow first, refine second:

```lisp
(pipe (rg "pub struct") (rg "impl"))
```

This evaluates the source (`rg "pub struct"`) first, extracts the set of matching files, then rewrites the target expression to scope its searches to only those files. This powers patterns like "find files about authentication, then search those for SQL queries."

### AST as Sum Type, Evaluator as Thin Dispatcher

The entire DSL is a single enum (`Expr`) with one variant per form. The evaluator is a single `match` that delegates to one handler per variant — no `if/else` chains, no fallbacks, no type checks inside handlers. Adding a new form means adding one variant and one match arm.

```
Expr = Rg | Lex | Sem | And | Or | Mix | Diff | Pipe | Top | Threshold | ...
eval = match expr { Rg => ..., And => ..., Mix => ..., Pipe => ..., ... }
```

### Why Shell Out to ripgrep

The `rg` backend runs ripgrep as a subprocess rather than linking it as a library. This keeps the binary small (~1.6MB), avoids pulling in ripgrep's dependency tree, and means `rg` is always the same version the user already has installed. Sift parses `rg --json` output, which gives structured match data with file paths, line numbers, and match offsets.

### Positional Scoring for Unranked Backends

ripgrep returns matches in file order, not ranked by relevance. To make these results compatible with RRF (which needs ranks), hits are assigned **positional scores** — the first result gets score 1.0, linearly decreasing to 0.0 for the last. This preserves the "earlier matches are probably more relevant" heuristic from ripgrep's file-order traversal while giving RRF meaningful ranks to work with.

## Roadmap

- [x] `rg` backend — exact grep, always fresh
- [x] Combinators — `&`, `|`, `mix`, `-`, `top`, `>`
- [x] Let bindings
- [x] Output modes — files, scores, json
- [x] Sequential pipelines — `pipe`
- [x] Auto mode — `ag "query"` without parens
- [x] `lex` backend — tantivy BM25 indexing (feature-gated)
- [x] `sem` backend — embedding similarity via ONNX (feature-gated)
- [x] `ag --index` — build/manage indexes
- [x] 55 tests (24 unit + 31 integration)
- [ ] Streaming progressive output
- [ ] `ag index --watch` — background index daemon
- [ ] Tree-sitter aware chunking for sem backend

## License

MIT
