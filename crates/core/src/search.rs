//! Tìm file nhanh (Cmd+P) bằng so khớp mờ.

/// Điểm khớp mờ: ký tự của `query` phải xuất hiện theo thứ tự trong `candidate`
/// (không phân biệt hoa thường). Điểm cao hơn khi khớp liền nhau, khớp đầu từ,
/// và khớp trong tên file. `None` = không khớp.
pub fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let cand: Vec<char> = candidate.chars().collect();
    let name_start = candidate.rfind('/').map_or(0, |i| candidate[..=i].chars().count());
    let mut score = 0i32;
    let mut ci = 0usize;
    let mut prev_match: Option<usize> = None;
    for q in query.chars().flat_map(char::to_lowercase) {
        if q == ' ' {
            continue;
        }
        let mut found = None;
        while ci < cand.len() {
            if cand[ci].to_lowercase().any(|c| c == q) {
                found = Some(ci);
                ci += 1;
                break;
            }
            ci += 1;
        }
        let i = found?;
        score += 1;
        if prev_match == Some(i.wrapping_sub(1)) {
            score += 5;
        }
        let prev = if i == 0 { '/' } else { cand[i - 1] };
        if matches!(prev, '/' | '_' | '-' | '.' | ' ') || (prev.is_lowercase() && cand[i].is_uppercase()) {
            score += 8;
        }
        if i >= name_start {
            score += 3;
        }
        prev_match = Some(i);
    }
    // Đường dẫn ngắn hơn xếp trước khi điểm bằng nhau.
    Some(score * 16 - cand.len().min(255) as i32 / 16)
}

/// Lọc và xếp hạng, trả về chỉ số trong `candidates`.
pub fn fuzzy_filter(query: &str, candidates: &[String], limit: usize) -> Vec<usize> {
    let mut scored: Vec<(i32, usize)> = candidates
        .iter()
        .enumerate()
        .filter_map(|(i, c)| fuzzy_score(query, c).map(|s| (s, i)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().take(limit).map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_subsequence() {
        assert!(fuzzy_score("rvw", "src/repo_view.rs").is_some());
        assert!(fuzzy_score("xyz", "src/repo_view.rs").is_none());
    }

    #[test]
    fn prefers_file_name() {
        let files = vec!["main/lib/other.rs".to_string(), "src/main.rs".to_string()];
        assert_eq!(fuzzy_filter("main", &files, 10)[0], 1);
    }
}
