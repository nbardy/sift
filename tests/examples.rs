use std::process::Command;

fn ag(args: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_ag"))
        .args(args)
        .output()
        .expect("failed to run ag");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{stdout}{stderr}");
    (output.status.success(), combined)
}

// ── Every example file must run without error ───────────────────

#[test]
fn example_basics()          { let (ok, out) = ag(&["-f", "examples/basics.sq"]);          assert!(ok, "{out}"); assert!(!out.is_empty()); }
#[test]
fn example_set_operations()  { let (ok, out) = ag(&["-f", "examples/set-operations.sq"]);  assert!(ok, "{out}"); }
#[test]
fn example_ranking()         { let (ok, out) = ag(&["-f", "examples/ranking.sq"]);         assert!(ok, "{out}"); assert!(!out.is_empty()); }
#[test]
fn example_filters()         { let (ok, out) = ag(&["-f", "examples/filters.sq"]);         assert!(ok, "{out}"); assert!(!out.is_empty()); }
#[test]
fn example_let_bindings()    { let (ok, out) = ag(&["-f", "examples/let-bindings.sq"]);    assert!(ok, "{out}"); assert!(!out.is_empty()); }
#[test]
fn example_agent_patterns()  { let (ok, out) = ag(&["-f", "examples/agent-patterns.sq"]);  assert!(ok, "{out}"); }
#[test]
fn example_output_modes()    { let (ok, out) = ag(&["-f", "examples/output-modes.sq"]);    assert!(ok, "{out}"); assert!(!out.is_empty()); }
#[test]
fn example_intersection()    { let (ok, out) = ag(&["-f", "examples/intersection.sq"]);    assert!(ok, "{out}"); assert!(!out.is_empty()); }
#[test]
fn example_union()           { let (ok, out) = ag(&["-f", "examples/union.sq"]);           assert!(ok, "{out}"); assert!(!out.is_empty()); }
#[test]
fn example_threshold()       { let (ok, out) = ag(&["-f", "examples/threshold.sq"]);       assert!(ok, "{out}"); }
#[test]
fn example_weighted_mix()    { let (ok, out) = ag(&["-f", "examples/weighted-mix.sq"]);    assert!(ok, "{out}"); assert!(!out.is_empty()); }

// ── Feature coverage: inline queries ────────────────────────────

#[test]
fn inline_rg() {
    let (ok, out) = ag(&[r#"(rg "pub struct")"#]);
    assert!(ok, "{out}");
    assert!(out.contains("pub struct"));
}

#[test]
fn grep_shorthand() {
    let (ok, out) = ag(&["-g", "ResultSet"]);
    assert!(ok, "{out}");
    assert!(out.contains("ResultSet"));
}

#[test]
fn intersection_both_match() {
    let (ok, out) = ag(&["--json", r#"(& (rg "Result") (rg "SqError"))"#]);
    assert!(ok, "{out}");
    let hits: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
    // Every hit snippet must contain both strings
    for hit in &hits {
        let snippet = hit["snippet"].as_str().unwrap();
        assert!(snippet.contains("Result") && snippet.contains("SqError"),
            "intersection hit missing both terms: {snippet}");
    }
}

#[test]
fn difference_excludes() {
    let (ok, out) = ag(&[r##"(- (rg "fn ") (rg "#\[test\]"))"##]);
    assert!(ok, "{out}");
}

#[test]
fn top_k_limits() {
    let (ok, out) = ag(&["--json", r#"(top 3 (rg "fn"))"#]);
    assert!(ok, "{out}");
    let hits: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
    assert!(hits.len() <= 3, "top 3 returned {} results", hits.len());
}

#[test]
fn output_json() {
    let (ok, out) = ag(&["--json", r#"(top 2 (rg "pub struct"))"#]);
    assert!(ok, "{out}");
    let parsed: serde_json::Value = serde_json::from_str(&out)
        .expect("--json output must be valid JSON");
    assert!(parsed.is_array());
}

#[test]
fn output_files() {
    let (ok, out) = ag(&["--files", r#"(rg "pub struct")"#]);
    assert!(ok, "{out}");
    // Should be file paths, no line numbers
    for line in out.lines() {
        assert!(!line.contains(':'), "files mode should not have colons: {line}");
    }
}

#[test]
fn output_scores() {
    let (ok, out) = ag(&["--scores", r#"(top 3 (rg "fn"))"#]);
    assert!(ok, "{out}");
    // Should contain score brackets like [0.85]
    assert!(out.contains('['), "scores mode should show [score]: {out}");
}

#[test]
fn let_binding_works() {
    let (ok, out) = ag(&["--json", r#"(let [x (rg "pub fn")] (top 3 x))"#]);
    assert!(ok, "{out}");
    let hits: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
    assert!(hits.len() <= 3);
}

#[test]
fn mix_equal_weight() {
    let (ok, out) = ag(&["--scores", r#"(top 5 (mix (rg "struct") (rg "enum")))"#]);
    assert!(ok, "{out}");
    assert!(!out.is_empty());
}

#[test]
fn mix_weighted() {
    let (ok, out) = ag(&["--scores", r#"(top 5 (mix [0.8 0.2] (rg "struct") (rg "enum")))"#]);
    assert!(ok, "{out}");
    assert!(!out.is_empty());
}

#[test]
fn lex_unavailable() {
    let (ok, _) = ag(&[r#"(lex "auth")"#]);
    assert!(!ok, "lex should fail without feature");
}

#[test]
fn sem_unavailable() {
    let (ok, _) = ag(&[r#"(sem "auth")"#]);
    assert!(!ok, "sem should fail without feature");
}

#[test]
fn pipe_sequential() {
    // Pipe: find files with "struct", then search those files for "pub"
    let (ok, out) = ag(&["--json", r#"(pipe (rg "pub struct") (rg "impl"))"#]);
    assert!(ok, "{out}");
    let hits: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
    // Every hit should be in a file that also contains "pub struct"
    assert!(!hits.is_empty(), "pipe should return results");
}

#[test]
fn example_pipe() {
    let (ok, out) = ag(&["-f", "examples/pipe.sq"]);
    assert!(ok, "{out}");
    assert!(!out.is_empty());
}

#[test]
fn working_dir_flag() {
    let (ok, out) = ag(&["-C", "src", r#"(rg "pub fn")"#]);
    assert!(ok, "{out}");
    // Paths should be relative to src/
    for line in out.lines() {
        assert!(!line.starts_with("src/"), "with -C src, paths should not start with src/: {line}");
    }
}

// ── Auto mode ───────────────────────────────────────────────────

#[test]
fn auto_mode_plain_text() {
    // Plain text (no parens) should auto-wrap as rg
    let (ok, out) = ag(&["pub struct"]);
    assert!(ok, "{out}");
    assert!(out.contains("pub struct"));
}

#[test]
fn auto_mode_sexp_passthrough() {
    // S-expression should pass through unchanged
    let (ok, out) = ag(&[r#"(top 3 (rg "fn"))"#]);
    assert!(ok, "{out}");
}

// ── Batch ────────────────────────────────────────────────────────

#[test]
fn batch_labeled_sections() {
    let (ok, out) = ag(&[r#"(batch :structs (rg "pub struct") :fns (rg "pub fn"))"#]);
    assert!(ok, "{out}");
    assert!(out.contains("── structs ──"), "batch should show struct label");
    assert!(out.contains("── fns ──"), "batch should show fns label");
}

#[test]
fn batch_json_dict() {
    let (ok, out) = ag(&["--json", r#"(batch :a (rg "pub struct") :b (rg "pub fn"))"#]);
    assert!(ok, "{out}");
    let parsed: serde_json::Value = serde_json::from_str(&out)
        .expect("batch --json must be valid JSON");
    assert!(parsed.is_object(), "batch JSON should be an object, got: {parsed}");
    assert!(parsed.get("a").is_some(), "batch JSON missing key 'a'");
    assert!(parsed.get("b").is_some(), "batch JSON missing key 'b'");
}

#[test]
fn batch_with_opts() {
    let (ok, out) = ag(&["--json", r#"(batch {:top 2} :s (rg "pub struct") :f (rg "pub fn"))"#]);
    assert!(ok, "{out}");
    let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
    let s_hits = parsed["s"].as_array().unwrap();
    let f_hits = parsed["f"].as_array().unwrap();
    assert!(s_hits.len() <= 2, "batch top 2 should limit struct results, got {}", s_hits.len());
    assert!(f_hits.len() <= 2, "batch top 2 should limit fn results, got {}", f_hits.len());
}

#[test]
fn batch_let_composition() {
    let (ok, out) = ag(&[r#"(let [x (rg "pub")] (batch :all (top 3 x) :structs (& x (rg "struct"))))"#]);
    assert!(ok, "{out}");
    assert!(out.contains("── all ──"));
    assert!(out.contains("── structs ──"));
}

#[test]
fn example_batch() {
    let (ok, out) = ag(&["-f", "examples/batch.sq"]);
    assert!(ok, "{out}");
    assert!(!out.is_empty());
}

// ── Index management ────────────────────────────────────────────

#[test]
fn index_status_no_index() {
    let (ok, out) = ag(&["--index-status"]);
    assert!(ok, "{out}");
}

#[test]
fn index_clean_noop() {
    // Clean when no .ag/ exists should succeed gracefully
    let dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_ag"))
        .args(["--index-clean", "-C", dir.path().to_str().unwrap()])
        .output()
        .expect("failed to run ag");
    assert!(output.status.success());
}
