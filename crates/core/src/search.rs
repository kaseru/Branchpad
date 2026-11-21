//! Tìm file nhanh (Cmd+P) bằng so khớp mờ.

/// Điểm khớp mờ: ưu tiên hàng đầu là trùng tên file / tên gốc (stem),
/// sau đó đến tiền tố, chuỗi con liền kề, và khớp mờ từng ký tự.
/// `None` = không khớp.
pub fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
    let q = query.trim();
    if q.is_empty() {
        return Some(0);
    }

    let q_lower = q.to_lowercase();
    let cand_lower = candidate.to_lowercase();

    // Tách tên file và thư mục cha
    let file_name = match candidate.rfind('/') {
        Some(i) => &candidate[i + 1..],
        None => candidate,
    };
    let fn_lower = file_name.to_lowercase();
    let stem = file_name
        .rsplit_once('.')
        .map_or(file_name, |(s, _)| s)
        .to_lowercase();

    let path_penalty = (candidate.len().min(300) as i32) * 2;

    // Nếu query có chứa '/', người dùng muốn tìm cả đường dẫn thư mục
    if q.contains('/') {
        if let Some(pos) = cand_lower.find(&q_lower) {
            return Some(40_000 - (pos as i32 * 20) - path_penalty);
        }
        return fuzzy_subseq_score(&q_lower, candidate).map(|s| s - path_penalty);
    }

    // Khi query không có '/', ưu tiên tối đa khớp với TÊN FILE:
    // 1. Trùng chính xác toàn bộ tên file (ví dụ "configs.js" == "configs.js")
    if fn_lower == q_lower {
        return Some(100_000 - path_penalty);
    }

    // 2. Trùng chính xác phần tên gốc (stem) bỏ qua đuôi file (ví dụ "Configs.js" stem "configs" == query "configs")
    if stem == q_lower {
        return Some(80_000 - path_penalty);
    }

    // 3. Tên gốc bắt đầu bằng query (prefix match, ví dụ query "config" khớp "Configs.js")
    if stem.starts_with(&q_lower) {
        let diff = (stem.len() - q_lower.len()).min(50) as i32;
        return Some(50_000 - diff * 100 - path_penalty);
    }

    // 4. Toàn bộ tên file bắt đầu bằng query
    if fn_lower.starts_with(&q_lower) {
        let diff = (fn_lower.len() - q_lower.len()).min(50) as i32;
        return Some(40_000 - diff * 100 - path_penalty);
    }

    // 5. Tên gốc chứa query dạng chuỗi con liền kề (substring match)
    if let Some(pos) = stem.find(&q_lower) {
        return Some(30_000 - (pos as i32 * 100) - path_penalty);
    }

    // 6. Tên file chứa query dạng chuỗi con liền kề
    if let Some(pos) = fn_lower.find(&q_lower) {
        return Some(20_000 - (pos as i32 * 100) - path_penalty);
    }

    // 7. Khớp mờ theo ký tự con trong tên file
    if let Some(score) = fuzzy_subseq_score(&q_lower, file_name) {
        return Some(10_000 + score - path_penalty);
    }

    // 8. Nếu tên file không khớp, tìm trong toàn bộ đường dẫn
    if let Some(pos) = cand_lower.find(&q_lower) {
        return Some(5_000 - (pos as i32 * 20) - path_penalty);
    }

    fuzzy_subseq_score(&q_lower, candidate).map(|score| score - path_penalty)
}

fn fuzzy_subseq_score(query: &str, target: &str) -> Option<i32> {
    let q_chars: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    let t_chars: Vec<char> = target.chars().collect();
    let t_lower: Vec<char> = t_chars.iter().flat_map(|c| c.to_lowercase()).collect();
    if q_chars.is_empty() {
        return Some(0);
    }
    if q_chars.len() > t_chars.len() {
        return None;
    }

    let mut score = 0i32;
    let mut ti = 0usize;
    let mut prev_match: Option<usize> = None;

    for &qc in &q_chars {
        if qc == ' ' {
            continue;
        }
        let mut found = None;
        while ti < t_lower.len() {
            if t_lower[ti] == qc {
                found = Some(ti);
                ti += 1;
                break;
            }
            ti += 1;
        }
        let i = found?;
        score += 10;
        // Điểm thưởng khi khớp liền kề ký tự trước
        if prev_match == Some(i.wrapping_sub(1)) {
            score += 25;
        }
        // Điểm thưởng khi khớp ở đầu từ / ranh giới ký tự
        let is_boundary = if i == 0 {
            true
        } else {
            let prev = t_chars[i - 1];
            matches!(prev, '/' | '_' | '-' | '.' | ' ')
                || (prev.is_lowercase() && t_chars[i].is_uppercase())
        };
        if is_boundary {
            score += 35;
        }
        prev_match = Some(i);
    }
    Some(score)
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

    #[test]
    fn exact_name_ranks_first() {
        let files = vec![
            "babel.config.js".to_string(),
            "jest.config.js".to_string(),
            "tsconfigs.json".to_string(),
            "src/utils/Configs.js".to_string(),
        ];
        let res = fuzzy_filter("configs", &files, 10);
        assert_eq!(files[res[0]], "src/utils/Configs.js");
    }
}
