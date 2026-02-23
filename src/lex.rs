use crate::core::{Hit, ResultSet, Score, SearchBackend, SearchOpts, SqError};
use std::fs;
use std::path::PathBuf;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Schema, STORED, TEXT};
use tantivy::schema::Value;
use tantivy::{doc, Index, IndexWriter, ReloadPolicy};

/// BM25 text search backend powered by tantivy.
/// Builds index lazily on first query, persists to `.ag/lex/`.
pub struct LexBackend {
    cwd: PathBuf,
    index_dir: PathBuf,
}

impl LexBackend {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        let cwd = cwd.into();
        let index_dir = cwd.join(".ag").join("lex");
        Self { cwd, index_dir }
    }

    fn schema() -> Schema {
        let mut builder = Schema::builder();
        builder.add_text_field("path", STORED);
        builder.add_u64_field("line", STORED);
        builder.add_text_field("content", TEXT | STORED);
        builder.build()
    }

    fn ensure_index(&self) -> Result<Index, SqError> {
        if self.index_dir.join("meta.json").exists() {
            return Index::open_in_dir(&self.index_dir)
                .map_err(|e| SqError::Other(format!("failed to open lex index: {e}")));
        }

        eprintln!("Building lex index...");
        fs::create_dir_all(&self.index_dir)
            .map_err(|e| SqError::Other(format!("failed to create index dir: {e}")))?;

        let schema = Self::schema();
        let index = Index::create_in_dir(&self.index_dir, schema.clone())
            .map_err(|e| SqError::Other(format!("failed to create index: {e}")))?;

        let mut writer: IndexWriter = index
            .writer(50_000_000)
            .map_err(|e| SqError::Other(format!("failed to create writer: {e}")))?;

        let path_field = schema.get_field("path").unwrap();
        let line_field = schema.get_field("line").unwrap();
        let content_field = schema.get_field("content").unwrap();

        let mut file_count = 0u64;
        for entry in crate::util::walk_files(&self.cwd) {
            let path = entry.path();
            if crate::util::is_binary_extension(path) {
                continue;
            }

            let content = match fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue, // skip binary/unreadable files
            };

            let rel_path = path
                .strip_prefix(&self.cwd)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            for (line_num, line_content) in content.lines().enumerate() {
                if line_content.trim().is_empty() {
                    continue;
                }
                writer
                    .add_document(doc!(
                        path_field => rel_path.clone(),
                        line_field => (line_num as u64 + 1),
                        content_field => line_content.to_string(),
                    ))
                    .map_err(|e| SqError::Other(format!("index write error: {e}")))?;
            }
            file_count += 1;
        }

        writer
            .commit()
            .map_err(|e| SqError::Other(format!("index commit error: {e}")))?;
        eprintln!("Indexed {file_count} files.");

        Ok(index)
    }
}

impl SearchBackend for LexBackend {
    async fn search(&self, query: &str, opts: &SearchOpts) -> Result<ResultSet, SqError> {
        let index = self.ensure_index()?;
        let schema = index.schema();
        let content_field = schema.get_field("content").unwrap();
        let path_field = schema.get_field("path").unwrap();
        let line_field = schema.get_field("line").unwrap();

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .map_err(|e| SqError::Other(format!("reader error: {e}")))?;

        let searcher = reader.searcher();
        let query_parser = QueryParser::for_index(&index, vec![content_field]);
        let parsed = query_parser
            .parse_query(query)
            .map_err(|e| SqError::Other(format!("lex query parse error: {e}")))?;

        let top_docs = searcher
            .search(&parsed, &TopDocs::with_limit(1000))
            .map_err(|e| SqError::Other(format!("lex search error: {e}")))?;

        let mut hits = Vec::new();
        for (score, doc_addr) in top_docs {
            let doc = searcher
                .doc::<tantivy::TantivyDocument>(doc_addr)
                .map_err(|e| SqError::Other(format!("doc fetch error: {e}")))?;

            let path = doc
                .get_first(path_field)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();

            let line = doc
                .get_first(line_field)
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;

            let snippet = doc
                .get_first(content_field)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();

            hits.push(Hit {
                path,
                line,
                snippet,
                score: Score(score as f64),
            });
        }

        let hits = crate::util::apply_opts(hits, opts);
        Ok(ResultSet::from_hits(hits))
    }
}
