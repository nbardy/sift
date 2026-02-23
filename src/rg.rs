use crate::core::{Hit, ResultSet, Score, SearchBackend, SearchOpts, SqError};
use serde::Deserialize;
use std::process::Stdio;
use tokio::process::Command;

/// Ripgrep backend — shells out to `rg --json`.
/// No index, scans filesystem, always fresh.
pub struct RgBackend {
    pub cwd: std::path::PathBuf,
}

impl RgBackend {
    pub fn new(cwd: impl Into<std::path::PathBuf>) -> Self {
        Self { cwd: cwd.into() }
    }

    fn build_args(&self, query: &str, opts: &SearchOpts) -> Vec<String> {
        let mut args = vec!["--json".to_string(), "--no-heading".to_string()];

        if let Some(lang) = &opts.lang {
            args.push("--type".to_string());
            args.push(lang.clone());
        }

        for glob in &opts.exclude {
            args.push("--glob".to_string());
            args.push(format!("!{glob}"));
        }

        for glob in &opts.include {
            args.push("--glob".to_string());
            args.push(glob.clone());
        }

        args.push(query.to_string());

        if let Some(scope) = &opts.scope {
            args.push(scope.clone());
        }

        args
    }
}

impl SearchBackend for RgBackend {
    async fn search(&self, query: &str, opts: &SearchOpts) -> Result<ResultSet, SqError> {
        let args = self.build_args(query, opts);

        let output = Command::new("rg")
            .args(&args)
            .current_dir(&self.cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    SqError::Rg("ripgrep not found. Install: brew install ripgrep".into())
                } else {
                    SqError::Rg(format!("failed to run rg: {e}"))
                }
            })?;

        // rg exits 1 when no matches found — not an error
        if !output.status.success() && output.status.code() != Some(1) {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SqError::Rg(format!("rg failed: {stderr}")));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let hits = parse_rg_json(&stdout);

        Ok(ResultSet::from_hits(hits).with_positional_scores())
    }
}

/// Parse rg --json output into Hits.
/// Each JSON line is a message; we only care about "match" type.
fn parse_rg_json(output: &str) -> Vec<Hit> {
    let mut hits = Vec::new();

    for line in output.lines() {
        if line.is_empty() {
            continue;
        }

        let msg: RgMessage = match serde_json::from_str(line) {
            Ok(m) => m,
            Err(_) => continue,
        };

        // rg --json format: {"type":"match","data":{"path":{"text":"..."},...}}
        if msg.r#type == "match" {
            if let Some(data) = msg.data {
                if let (Some(path), Some(line_number), Some(lines)) =
                    (data.get("path"), data.get("line_number"), data.get("lines"))
                {
                    let path_text = path.get("text").and_then(|v| v.as_str()).unwrap_or_default();
                    let line_num = line_number.as_u64().unwrap_or(0) as u32;
                    let snippet = lines
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .trim_end();

                    hits.push(Hit {
                        path: path_text.to_string(),
                        line: line_num,
                        snippet: snippet.to_string(),
                        score: Score::ZERO,
                    });
                }
            }
        }
    }

    hits
}

#[derive(Deserialize)]
struct RgMessage {
    r#type: String,
    data: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rg_json_output() {
        let json = r#"{"type":"match","data":{"path":{"text":"src/main.rs"},"lines":{"text":"fn main() {\n"},"line_number":1,"absolute_offset":0,"submatches":[{"match":{"text":"main"},"start":3,"end":7}]}}"#;
        let hits = parse_rg_json(json);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "src/main.rs");
        assert_eq!(hits[0].line, 1);
        assert_eq!(hits[0].snippet, "fn main() {");
    }

    #[test]
    fn parse_rg_json_no_matches() {
        let json = r#"{"type":"summary","data":{"stats":{"matches":0}}}"#;
        let hits = parse_rg_json(json);
        assert!(hits.is_empty());
    }

    #[test]
    fn parse_rg_json_multiple() {
        let json = concat!(
            r#"{"type":"begin","data":{"path":{"text":"a.rs"}}}"#, "\n",
            r#"{"type":"match","data":{"path":{"text":"a.rs"},"lines":{"text":"line1\n"},"line_number":1,"absolute_offset":0,"submatches":[]}}"#, "\n",
            r#"{"type":"match","data":{"path":{"text":"b.rs"},"lines":{"text":"line2\n"},"line_number":5,"absolute_offset":0,"submatches":[]}}"#, "\n",
            r#"{"type":"end","data":{"path":{"text":"a.rs"}}}"#,
        );
        let hits = parse_rg_json(json);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].path, "a.rs");
        assert_eq!(hits[1].path, "b.rs");
        assert_eq!(hits[1].line, 5);
    }
}
