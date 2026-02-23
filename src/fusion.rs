use crate::core::{Hit, ResultSet, Score};
use std::collections::{HashMap, HashSet};

// ── Reciprocal Rank Fusion ──────────────────────────────────────────

const RRF_K: f64 = 60.0;

/// RRF with equal weights: score = Σ 1/(k + rank_i)
pub fn rrf(sets: &[ResultSet]) -> ResultSet {
    let weights: Vec<f64> = vec![1.0; sets.len()];
    rrf_weighted(sets, &weights)
}

/// RRF with explicit weights: score = Σ w_i / (k + rank_i)
pub fn rrf_weighted(sets: &[ResultSet], weights: &[f64]) -> ResultSet {
    let mut scores: HashMap<(String, u32), (f64, Hit)> = HashMap::new();

    for (set_idx, set) in sets.iter().enumerate() {
        let w = weights.get(set_idx).copied().unwrap_or(1.0);
        for (rank, hit) in set.hits.iter().enumerate() {
            let key = (hit.path.clone(), hit.line);
            let rrf_score = w / (RRF_K + rank as f64 + 1.0);
            scores
                .entry(key)
                .and_modify(|(acc, _)| *acc += rrf_score)
                .or_insert((rrf_score, hit.clone()));
        }
    }

    let mut hits: Vec<Hit> = scores
        .into_values()
        .map(|(score, mut hit)| { hit.score = Score(score); hit })
        .collect();

    hits.sort_by(|a, b| b.score.0.partial_cmp(&a.score.0).unwrap_or(std::cmp::Ordering::Equal));

    // Normalize to [0, 1] so scores are comparable across backends.
    // Raw RRF scores are tiny (w/(k+rank)) — dividing by max makes them readable.
    normalize_scores(&mut hits);

    ResultSet::from_hits(hits)
}

// ── Set operations ──────────────────────────────────────────────────

/// Intersection: hits present in ALL result sets. Best score wins.
pub fn intersect(sets: &[ResultSet]) -> ResultSet {
    if sets.is_empty() {
        return ResultSet::empty();
    }

    let mut counts: HashMap<(String, u32), (usize, Hit)> = HashMap::new();
    let n = sets.len();

    for set in sets {
        let mut seen_in_set: HashSet<(String, u32)> = HashSet::new();
        for hit in &set.hits {
            let key = (hit.path.clone(), hit.line);
            if seen_in_set.insert(key.clone()) {
                counts
                    .entry(key)
                    .and_modify(|(count, best)| {
                        *count += 1;
                        if hit.score.0 > best.score.0 { *best = hit.clone(); }
                    })
                    .or_insert((1, hit.clone()));
            }
        }
    }

    let mut hits: Vec<Hit> = counts
        .into_values()
        .filter(|(count, _)| *count == n)
        .map(|(_, hit)| hit)
        .collect();

    hits.sort_by(|a, b| b.score.0.partial_cmp(&a.score.0).unwrap_or(std::cmp::Ordering::Equal));
    ResultSet::from_hits(hits)
}

/// Union: hits present in ANY result set. Best score wins.
pub fn union(sets: &[ResultSet]) -> ResultSet {
    let mut best: HashMap<(String, u32), Hit> = HashMap::new();

    for set in sets {
        for hit in &set.hits {
            let key = (hit.path.clone(), hit.line);
            best.entry(key)
                .and_modify(|existing| {
                    if hit.score.0 > existing.score.0 { *existing = hit.clone(); }
                })
                .or_insert(hit.clone());
        }
    }

    let mut hits: Vec<Hit> = best.into_values().collect();
    hits.sort_by(|a, b| b.score.0.partial_cmp(&a.score.0).unwrap_or(std::cmp::Ordering::Equal));
    ResultSet::from_hits(hits)
}

/// Difference: hits in `left` but NOT in `right` (by path:line).
pub fn difference(left: &ResultSet, right: &ResultSet) -> ResultSet {
    let right_keys: HashSet<(String, u32)> = right.hits.iter().map(|h| (h.path.clone(), h.line)).collect();
    let hits: Vec<Hit> = left.hits.iter().filter(|h| !right_keys.contains(&(h.path.clone(), h.line))).cloned().collect();
    ResultSet::from_hits(hits)
}

// ── Filters ─────────────────────────────────────────────────────────

/// Top k by score.
pub fn top_k(set: &ResultSet, k: usize) -> ResultSet {
    let mut sorted = set.clone().sorted();
    sorted.hits.truncate(k);
    sorted
}

/// Score threshold: keep hits with score >= t.
pub fn threshold(set: &ResultSet, t: f64) -> ResultSet {
    let hits: Vec<Hit> = set.hits.iter().filter(|h| h.score.0 >= t).cloned().collect();
    ResultSet::from_hits(hits)
}

/// Normalize scores to [0, 1] by dividing by max.
/// This makes scores from different fusion methods comparable.
fn normalize_scores(hits: &mut [Hit]) {
    let max = hits.iter().map(|h| h.score.0).fold(0.0_f64, f64::max);
    if max > 0.0 {
        for hit in hits.iter_mut() {
            hit.score = Score(hit.score.0 / max);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(path: &str, line: u32, score: f64) -> Hit {
        Hit { path: path.to_string(), line, snippet: String::new(), score: Score(score) }
    }

    #[test]
    fn test_intersect() {
        let a = ResultSet::from_hits(vec![hit("a.rs", 1, 0.9), hit("b.rs", 2, 0.8), hit("c.rs", 3, 0.7)]);
        let b = ResultSet::from_hits(vec![hit("b.rs", 2, 0.95), hit("c.rs", 3, 0.6), hit("d.rs", 4, 0.5)]);
        let result = intersect(&[a, b]);
        assert_eq!(result.hits.len(), 2);
        assert_eq!(result.hits[0].path, "b.rs");
        assert_eq!(result.hits[0].score.0, 0.95);
        assert_eq!(result.hits[1].path, "c.rs");
    }

    #[test]
    fn test_union() {
        let a = ResultSet::from_hits(vec![hit("a.rs", 1, 0.9), hit("b.rs", 2, 0.8)]);
        let b = ResultSet::from_hits(vec![hit("b.rs", 2, 0.95), hit("c.rs", 3, 0.7)]);
        let result = union(&[a, b]);
        assert_eq!(result.hits.len(), 3);
        let b_hit = result.hits.iter().find(|h| h.path == "b.rs").unwrap();
        assert_eq!(b_hit.score.0, 0.95);
    }

    #[test]
    fn test_difference() {
        let a = ResultSet::from_hits(vec![hit("a.rs", 1, 0.9), hit("b.rs", 2, 0.8), hit("c.rs", 3, 0.7)]);
        let b = ResultSet::from_hits(vec![hit("b.rs", 2, 0.5)]);
        let result = difference(&a, &b);
        assert_eq!(result.hits.len(), 2);
        assert!(result.hits.iter().all(|h| h.path != "b.rs"));
    }

    #[test]
    fn test_top_k() {
        let set = ResultSet::from_hits(vec![hit("a.rs", 1, 0.5), hit("b.rs", 2, 0.9), hit("c.rs", 3, 0.7)]);
        let result = top_k(&set, 2);
        assert_eq!(result.hits.len(), 2);
        assert_eq!(result.hits[0].path, "b.rs");
        assert_eq!(result.hits[1].path, "c.rs");
    }

    #[test]
    fn test_threshold() {
        let set = ResultSet::from_hits(vec![hit("a.rs", 1, 0.9), hit("b.rs", 2, 0.5), hit("c.rs", 3, 0.3)]);
        let result = threshold(&set, 0.5);
        assert_eq!(result.hits.len(), 2);
    }

    #[test]
    fn test_rrf() {
        let a = ResultSet::from_hits(vec![hit("a.rs", 1, 0.9), hit("b.rs", 2, 0.8)]);
        let b = ResultSet::from_hits(vec![hit("b.rs", 2, 0.7), hit("a.rs", 1, 0.6)]);
        let result = rrf(&[a, b]);
        assert_eq!(result.hits.len(), 2);
    }

    #[test]
    fn test_rrf_weighted() {
        let a = ResultSet::from_hits(vec![hit("a.rs", 1, 0.0)]);
        let b = ResultSet::from_hits(vec![hit("b.rs", 2, 0.0)]);
        let result = rrf_weighted(&[a, b], &[2.0, 1.0]);
        assert_eq!(result.hits.len(), 2);
        assert_eq!(result.hits[0].path, "a.rs");
    }

    #[test]
    fn test_intersect_empty() { assert!(intersect(&[]).is_empty()); }

    #[test]
    fn test_intersect_no_overlap() {
        let a = ResultSet::from_hits(vec![hit("a.rs", 1, 0.9)]);
        let b = ResultSet::from_hits(vec![hit("b.rs", 2, 0.8)]);
        assert!(intersect(&[a, b]).is_empty());
    }
}
