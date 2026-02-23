# Sift

A search DSL for agents. Compose, parallelize, and fuse code searches.

Sift is a tiny Lisp that composes search backends (ripgrep, BM25, embeddings) into parallel pipelines. Instead of an agent making five sequential grep calls and merging results in application code, it writes one expression and gets back ranked, deduplicated, blended results — automatically parallelized across backends.

The CLI command is `ag`.

```lisp
;; Find callers of eval, excluding its definition
(- (rg "eval\\(") (rg "pub fn eval"))

;; Blend three search methods, take top 10
(top 10 (mix (sem "auth flow") (rg "authenticate") (lex "authentication")))

;; Security audit: weighted blend, exclude tests
(top 20 (mix [0.4 0.3 0.3]
  (sem "credentials secrets" :x "*test*")
  (lex "password secret api_key" :x "*test*")
  (rg "SECRET|TOKEN|API_KEY" :x "*test*")))
```

## Why

Agents grep. Sometimes they grep well, sometimes they miss things. The gap between "run ripgrep" and "actually find what I need" is filled with orchestration code — multiple tool calls, deduplication, re-ranking, set operations. Sift collapses that into a single expression.

- **Parallel by default** — `(mix (rg "x") (sem "x"))` runs both backends concurrently. Total time = slowest backend, not the sum.
- **Set algebra on results** — intersection, union, difference. Find lines matching A AND B. Find callers minus definitions.
- **Ranked fusion** — [Reciprocal Rank Fusion](https://plg.uwaterloo.ca/~gvcormac/cormacksigir09-rrf.pdf) blends results from different backends into a single ranked list.
- **Single binary** — 1.6MB, no runtime, no config. Just `ag '(rg "TODO")'`.

## Install

```bash
cargo install --path .
```

Requires [ripgrep](https://github.com/BurntSushi/ripgrep): `brew install ripgrep`

## Quick Start

```bash
# Simple grep
ag '(rg "TODO")'
ag -g "TODO"                          # shorthand

# Intersection: lines with both patterns
ag '(& (rg "async") (rg "tokio"))'

# Difference: callers minus definition
ag '(- (rg "eval\\(") (rg "pub fn eval"))'

# Top 5 results
ag '(top 5 (rg "unsafe" :lang "rust"))'

# Blend with RRF
ag '(mix (rg "error") (rg "panic"))'

# Output modes
ag --files '(rg "TODO")'              # paths only
ag --json '(rg "TODO")'               # machine-readable
ag --scores '(rg "TODO")'             # with relevance scores
```

## Docs

- **[Language Reference](docs/language-reference.md)** — full syntax, all forms, execution model
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

Five modules, one binary:

| Module | Purpose |
|---|---|
| [`core`](src/core.rs) | Hit, ResultSet, Score, Expr AST, errors |
| [`parse`](src/parse.rs) | S-expression tokenizer + recursive descent parser |
| [`rg`](src/rg.rs) | Ripgrep backend (shells out to `rg --json`) |
| [`fusion`](src/fusion.rs) | RRF, intersect, union, difference, top-k, threshold |
| [`eval`](src/eval.rs) | Async evaluator — thin dispatcher, parallel fan-out |

## Roadmap

- [x] `rg` backend — exact grep, always fresh
- [x] Combinators — `&`, `|`, `mix`, `-`, `top`, `>`
- [x] Let bindings
- [x] Output modes — files, scores, json
- [x] 49 tests (24 unit + 25 integration)
- [ ] `lex` backend — tantivy BM25 indexing
- [ ] `sem` backend — embedding similarity via ONNX
- [ ] `ag index` — build/manage indexes
- [ ] `ag model` — download/manage embedding models
- [ ] Streaming progressive output
- [ ] Auto mode — `ag "query"` picks backends automatically

## License

MIT
