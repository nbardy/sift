use crate::core::{Hit, ResultSet, Score, SearchBackend, SearchOpts, SqError};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Semantic embedding search backend.
/// Uses ONNX Runtime to run a sentence-transformer model,
/// then does brute-force cosine similarity search.
/// Lazy: downloads model on first use. Caches embeddings to .ag/sem/.
pub struct SemBackend {
    cwd: PathBuf,
    index_dir: PathBuf,
}

/// Persisted chunk: metadata + embedding, serialized to .ag/sem/index.json
#[derive(Serialize, Deserialize)]
struct StoredChunk {
    path: String,
    line: u32,
    text_first_line: String,
    embedding: Vec<f32>,
}

/// Index header with version info for cache invalidation.
#[derive(Serialize, Deserialize)]
struct SemIndex {
    version: u32,
    dim: usize,
    chunks: Vec<StoredChunk>,
}

const INDEX_VERSION: u32 = 1;

impl SemBackend {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        let cwd = cwd.into();
        let index_dir = cwd.join(".ag").join("sem");
        Self { cwd, index_dir }
    }

    fn index_path(&self) -> PathBuf {
        self.index_dir.join("index.json")
    }

    /// Load cached index from disk, or return None if stale/missing.
    fn load_index(&self) -> Option<SemIndex> {
        let data = fs::read_to_string(self.index_path()).ok()?;
        let index: SemIndex = serde_json::from_str(&data).ok()?;
        if index.version != INDEX_VERSION {
            return None;
        }
        Some(index)
    }

    /// Save index to disk.
    fn save_index(&self, index: &SemIndex) -> Result<(), SqError> {
        fs::create_dir_all(&self.index_dir)
            .map_err(|e| SqError::Other(format!("failed to create sem index dir: {e}")))?;
        let data = serde_json::to_string(index)
            .map_err(|e| SqError::Other(format!("serialize error: {e}")))?;
        fs::write(self.index_path(), data)
            .map_err(|e| SqError::Other(format!("write index error: {e}")))?;
        Ok(())
    }

    fn ensure_model(&self) -> Result<(ort::session::Session, tokenizers::Tokenizer), SqError> {
        let model_name = "sentence-transformers/all-MiniLM-L6-v2";

        let api = hf_hub::api::sync::Api::new()
            .map_err(|e| SqError::Other(format!("hf-hub init error: {e}")))?;
        let repo = api.model(model_name.to_string());

        eprintln!("Loading embedding model...");

        let model_path = repo
            .get("onnx/model.onnx")
            .map_err(|e| SqError::Other(format!("model download error: {e}")))?;
        let tokenizer_path = repo
            .get("tokenizer.json")
            .map_err(|e| SqError::Other(format!("tokenizer download error: {e}")))?;

        let session = ort::session::Session::builder()
            .map_err(|e| SqError::Other(format!("ort session builder error: {e}")))?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)
            .map_err(|e| SqError::Other(format!("ort optimization error: {e}")))?
            .commit_from_file(&model_path)
            .map_err(|e| SqError::Other(format!("ort model load error: {e}")))?;

        let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| SqError::Other(format!("tokenizer load error: {e}")))?;

        Ok((session, tokenizer))
    }

    fn build_chunks(&self) -> Result<Vec<(String, u32, String)>, SqError> {
        let mut chunks = Vec::new();

        for entry in crate::util::walk_files(&self.cwd) {
            let path = entry.path();
            if crate::util::is_binary_extension(path) {
                continue;
            }

            let content = match fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let rel_path = path
                .strip_prefix(&self.cwd)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            let lines: Vec<&str> = content.lines().collect();
            let chunk_size = 10;
            let step = 5;
            let mut i = 0;
            while i < lines.len() {
                let end = (i + chunk_size).min(lines.len());
                let chunk_text = lines[i..end].join("\n");
                if !chunk_text.trim().is_empty() {
                    chunks.push((rel_path.clone(), (i + 1) as u32, chunk_text));
                }
                i += step;
            }
        }

        Ok(chunks)
    }

    /// Build or load the embedding index.
    /// Returns stored chunks with pre-computed embeddings.
    fn ensure_index(
        &self,
        session: &mut ort::session::Session,
        tokenizer: &tokenizers::Tokenizer,
    ) -> Result<SemIndex, SqError> {
        // Try loading cached index
        if let Some(index) = self.load_index() {
            eprintln!("Loaded cached sem index ({} chunks)", index.chunks.len());
            return Ok(index);
        }

        // Build from scratch
        let raw_chunks = self.build_chunks()?;
        eprintln!("Embedding {} chunks...", raw_chunks.len());

        let chunk_texts: Vec<String> = raw_chunks.iter().map(|(_, _, t)| t.clone()).collect();
        let chunk_embeddings = self.embed(session, tokenizer, &chunk_texts)?;

        let dim = chunk_embeddings.first().map_or(384, |e| e.len());

        let chunks: Vec<StoredChunk> = raw_chunks
            .iter()
            .zip(chunk_embeddings.into_iter())
            .map(|((path, line, text), embedding)| StoredChunk {
                path: path.clone(),
                line: *line,
                text_first_line: text.lines().next().unwrap_or("").to_string(),
                embedding,
            })
            .collect();

        let index = SemIndex {
            version: INDEX_VERSION,
            dim,
            chunks,
        };

        // Persist to disk
        if let Err(e) = self.save_index(&index) {
            eprintln!("Warning: failed to cache sem index: {e}");
        } else {
            eprintln!("Cached sem index to {}", self.index_dir.display());
        }

        Ok(index)
    }

    fn embed(
        &self,
        session: &mut ort::session::Session,
        tokenizer: &tokenizers::Tokenizer,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, SqError> {
        use ort::value::Tensor;

        let mut all_embeddings = Vec::new();

        let batch_size = 32;
        for batch in texts.chunks(batch_size) {
            let encodings = tokenizer
                .encode_batch(batch.to_vec(), true)
                .map_err(|e| SqError::Other(format!("tokenize error: {e}")))?;

            let max_len = encodings
                .iter()
                .map(|e| e.get_ids().len())
                .max()
                .unwrap_or(0);

            let mut input_ids = Vec::new();
            let mut attention_mask = Vec::new();
            let mut token_type_ids = Vec::new();

            for enc in &encodings {
                let ids = enc.get_ids();
                let mask = enc.get_attention_mask();

                let mut padded_ids = ids.to_vec();
                let mut padded_mask = mask.to_vec();
                let mut padded_types = vec![0i64; ids.len()];

                padded_ids.resize(max_len, 0);
                padded_mask.resize(max_len, 0);
                padded_types.resize(max_len, 0);

                input_ids.extend(padded_ids.into_iter().map(|x| x as i64));
                attention_mask.extend(padded_mask.into_iter().map(|x| x as i64));
                token_type_ids.extend(padded_types);
            }

            let batch_len = encodings.len();

            let ids_tensor = Tensor::from_array(([batch_len, max_len], input_ids))
                .map_err(|e| SqError::Other(format!("tensor error: {e}")))?;
            let mask_tensor = Tensor::from_array(([batch_len, max_len], attention_mask))
                .map_err(|e| SqError::Other(format!("tensor error: {e}")))?;
            let types_tensor = Tensor::from_array(([batch_len, max_len], token_type_ids))
                .map_err(|e| SqError::Other(format!("tensor error: {e}")))?;

            let outputs = session
                .run(ort::inputs![
                    "input_ids" => ids_tensor,
                    "attention_mask" => mask_tensor,
                    "token_type_ids" => types_tensor,
                ])
                .map_err(|e| SqError::Other(format!("inference error: {e}")))?;

            let (out_shape, out_data) = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| SqError::Other(format!("output extract error: {e}")))?;

            let seq_len_out = out_shape[1] as usize;
            let hidden_dim = out_shape[2] as usize;

            for i in 0..batch_len {
                let mut embedding = vec![0.0f32; hidden_dim];
                let actual_seq_len = encodings[i]
                    .get_attention_mask()
                    .iter()
                    .filter(|&&m| m == 1)
                    .count();

                for j in 0..actual_seq_len {
                    let offset = i * seq_len_out * hidden_dim + j * hidden_dim;
                    for k in 0..hidden_dim {
                        embedding[k] += out_data[offset + k];
                    }
                }
                for v in &mut embedding {
                    *v /= actual_seq_len as f32;
                }
                let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
                if norm > 0.0 {
                    for v in &mut embedding {
                        *v /= norm;
                    }
                }
                all_embeddings.push(embedding);
            }
        }

        Ok(all_embeddings)
    }
}

impl SearchBackend for SemBackend {
    async fn search(&self, query: &str, opts: &SearchOpts) -> Result<ResultSet, SqError> {
        let (mut session, tokenizer) = self.ensure_model()?;

        let index = self.ensure_index(&mut session, &tokenizer)?;

        // Embed the query (always fresh — queries aren't cached)
        let query_embedding = self.embed(&mut session, &tokenizer, &[query.to_string()])?;
        let query_emb = &query_embedding[0];

        // Cosine similarity (vectors are already L2-normalized)
        let mut scored: Vec<(f64, usize)> = index
            .chunks
            .iter()
            .enumerate()
            .map(|(i, chunk)| {
                let sim: f32 = chunk
                    .embedding
                    .iter()
                    .zip(query_emb.iter())
                    .map(|(a, b)| a * b)
                    .sum();
                (sim as f64, i)
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let hits: Vec<Hit> = scored
            .into_iter()
            .take(100)
            .map(|(score, idx)| {
                let chunk = &index.chunks[idx];
                Hit {
                    path: chunk.path.clone(),
                    line: chunk.line,
                    snippet: chunk.text_first_line.clone(),
                    score: Score(score),
                }
            })
            .collect();

        let hits = crate::util::apply_opts(hits, opts);
        Ok(ResultSet::from_hits(hits))
    }
}
