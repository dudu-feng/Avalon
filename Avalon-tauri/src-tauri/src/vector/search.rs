// 检索算法：semantic（暴力余弦）/ keyword（BM25 + 字符 bigram）/ hybrid（RRF）+ 时间过滤
//
// 纯函数、无状态，便于单测。索引数据由 InMemoryStore 维护，检索时传入。

use std::collections::HashMap;

use super::doc::MemoryDoc;

/// 解析时间过滤："" / "2026-07-15" / "2026-07-01,2026-07-31" → (start, end)
/// 时间戳定长 "YYYY-MM-DD-HH_MM_SS"，字典序 = 时间序，字符串比较即可。
pub fn parse_time_range(time_range: &str) -> (Option<String>, Option<String>) {
    let tr = time_range.trim();
    if tr.is_empty() {
        return (None, None);
    }
    if let Some((start, end)) = tr.split_once(',') {
        let start = start.trim();
        let end = end.trim();
        let s = (!start.is_empty()).then(|| format!("{start}-00_00_00"));
        let e = (!end.is_empty()).then(|| format!("{end}-23_59_59"));
        (s, e)
    } else {
        (Some(format!("{tr}-00_00_00")), None)
    }
}

/// 时间范围判断（None 表示不限制该侧）
pub fn in_range(ts: &str, start: &Option<String>, end: &Option<String>) -> bool {
    start.as_ref().map_or(true, |s| ts >= s.as_str())
        && end.as_ref().map_or(true, |e| ts <= e.as_str())
}

/// 字符级 bigram 切词（中文检索零依赖方案；不足两字符时按单字符返回）
pub fn bigrams(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < 2 {
        return chars.iter().map(|c| c.to_string()).collect();
    }
    chars
        .windows(2)
        .map(|w| w.iter().collect::<String>())
        .collect()
}

/// 点积（向量均已 L2 归一化，点积 = 余弦相似度）
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// 语义检索：对每个 doc 算余弦相似度，按分数降序返回 (doc_id, score)
pub fn semantic_rank(
    query_vec: &[f32],
    docs: &HashMap<String, MemoryDoc>,
) -> Vec<(String, f32)> {
    let mut scored: Vec<(String, f32)> = docs
        .iter()
        .map(|(id, doc)| (id.clone(), dot(query_vec, &doc.vector)))
        .collect();
    sort_desc(&mut scored);
    scored
}

/// BM25 关键词检索：按分数降序返回 (doc_id, score)
pub fn keyword_rank(
    query: &str,
    inverted: &HashMap<String, HashMap<String, u32>>, // term → (doc_id → tf)
    doc_len: &HashMap<String, usize>,                 // doc_id → bigram 数量
    avgdl: f32,
    total: usize,
) -> Vec<(String, f32)> {
    const K1: f32 = 1.5;
    const B: f32 = 0.75;

    let query_terms = bigrams(query);
    let mut scores: HashMap<String, f32> = HashMap::new();

    for term in &query_terms {
        let Some(posting) = inverted.get(term) else {
            continue;
        };
        let df = posting.len() as f32;
        if df == 0.0 || total == 0 {
            continue;
        }
        let idf = ((total as f32 - df + 0.5) / (df + 0.5) + 1.0).ln();

        for (doc_id, tf) in posting {
            let tf = *tf as f32;
            let dl = *doc_len.get(doc_id).unwrap_or(&1) as f32;
            let norm = tf * (K1 + 1.0) / (tf + K1 * (1.0 - B + B * dl / avgdl.max(1.0)));
            *scores.entry(doc_id.clone()).or_insert(0.0) += idf * norm;
        }
    }

    let mut scored: Vec<(String, f32)> = scores.into_iter().collect();
    sort_desc(&mut scored);
    scored
}

/// RRF 融合两个排序：score = Σ 1/(k + rank)，只看排名、标度无关
pub fn rrf_fuse(a: &[(String, f32)], b: &[(String, f32)], k: f32) -> Vec<(String, f32)> {
    let mut fused: HashMap<String, f32> = HashMap::new();
    for (rank, (doc_id, _)) in a.iter().enumerate() {
        *fused.entry(doc_id.clone()).or_insert(0.0) += 1.0 / (k + rank as f32 + 1.0);
    }
    for (rank, (doc_id, _)) in b.iter().enumerate() {
        *fused.entry(doc_id.clone()).or_insert(0.0) += 1.0 / (k + rank as f32 + 1.0);
    }
    let mut scored: Vec<(String, f32)> = fused.into_iter().collect();
    sort_desc(&mut scored);
    scored
}

/// 按分数降序排序（分数相等用 doc_id 兜底，保证确定性）
fn sort_desc(scored: &mut [(String, f32)]) {
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
}
