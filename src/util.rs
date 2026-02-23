#![allow(dead_code)]

use crate::core::{Hit, SearchOpts};
use std::path::Path;

/// Filter hits by SearchOpts (scope, lang, exclude, include).
/// Shared by lex and sem backends for post-query filtering.
pub fn apply_opts(hits: Vec<Hit>, opts: &SearchOpts) -> Vec<Hit> {
    hits.into_iter()
        .filter(|h| {
            if let Some(scope) = &opts.scope {
                if !h.path.starts_with(scope.as_str()) {
                    return false;
                }
            }
            if let Some(lang) = &opts.lang {
                if !matches_lang(&h.path, lang) {
                    return false;
                }
            }
            for pat in &opts.exclude {
                if glob_match(pat, &h.path) {
                    return false;
                }
            }
            if !opts.include.is_empty()
                && !opts.include.iter().any(|p| glob_match(p, &h.path))
            {
                return false;
            }
            true
        })
        .collect()
}

/// Walk a directory respecting .gitignore, .ignore, and hidden files.
/// Uses the `ignore` crate (same walker ripgrep uses) so lex/sem match rg behavior.
pub fn walk_files(cwd: &Path) -> impl Iterator<Item = ignore::DirEntry> {
    ignore::WalkBuilder::new(cwd)
        .hidden(true) // skip hidden files/dirs
        .git_ignore(true) // respect .gitignore
        .git_global(true) // respect global gitignore
        .git_exclude(true) // respect .git/info/exclude
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map_or(false, |t| t.is_file()))
}

pub fn is_binary_extension(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    matches!(
        ext,
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "ico" | "svg"
            | "woff" | "woff2" | "ttf" | "eot" | "otf"
            | "zip" | "tar" | "gz" | "bz2" | "xz" | "7z"
            | "exe" | "dll" | "so" | "dylib" | "o" | "a"
            | "pdf" | "doc" | "docx" | "xls" | "xlsx"
            | "mp3" | "mp4" | "avi" | "mov" | "wav"
            | "wasm" | "pyc" | "class" | "lock"
    )
}

/// Map user lang names to file extensions.
/// Accepts both full names and common abbreviations.
pub fn matches_lang(path: &str, lang: &str) -> bool {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match lang {
        "rust" | "rs" => ext == "rs",
        "python" | "py" => ext == "py",
        "javascript" | "js" => ext == "js" || ext == "jsx",
        "typescript" | "ts" => ext == "ts" || ext == "tsx",
        "go" => ext == "go",
        "java" => ext == "java",
        "c" => ext == "c" || ext == "h",
        "cpp" | "c++" | "cxx" => ext == "cpp" || ext == "hpp" || ext == "cc" || ext == "cxx",
        "ruby" | "rb" => ext == "rb",
        "toml" => ext == "toml",
        "yaml" | "yml" => ext == "yaml" || ext == "yml",
        "json" => ext == "json",
        "markdown" | "md" => ext == "md",
        "shell" | "sh" | "bash" => ext == "sh" || ext == "bash",
        _ => ext == lang,
    }
}

/// Canonicalize user lang names to rg --type names.
/// Returns None if the lang maps to an rg-native type name as-is.
pub fn rg_type_name(lang: &str) -> &str {
    match lang {
        "rs" => "rust",
        "py" => "python",
        "js" => "js",
        "ts" => "ts",
        "c++" | "cxx" => "cpp",
        "rb" => "ruby",
        "yml" => "yaml",
        "md" => "markdown",
        "sh" | "bash" => "shell",
        other => other,
    }
}

/// Glob matching using the `glob` crate's pattern syntax.
/// Supports *, **, ?, [abc] — full glob semantics.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let options = glob::MatchOptions {
        case_sensitive: true,
        require_literal_separator: false,
        require_literal_leading_dot: false,
    };
    glob::Pattern::new(pattern)
        .map(|p| p.matches_with(path, options))
        .unwrap_or(false)
}
