/// Returns `true` when every character of `query` appears in `target` in order,
/// ignoring case (a subsequence match).
pub fn fuzzy_match(query: &str, target: &str) -> bool {
    fuzzy_score(query, target).is_some()
}

/// Scores a fuzzy subsequence match; higher is better. Returns `None` when
/// `query` is not a subsequence of `target`. Consecutive matches and matches at
/// the start of a word are rewarded.
pub fn fuzzy_score(query: &str, target: &str) -> Option<u32> {
    let query: Vec<char> = query.to_lowercase().chars().collect();
    let target: Vec<char> = target.to_lowercase().chars().collect();

    let mut score: u32 = 0;
    let mut ti = 0usize;
    let mut prev: Option<usize> = None;

    for &q in &query {
        let mut found = None;
        while ti < target.len() {
            if target[ti] == q {
                found = Some(ti);
                break;
            }
            ti += 1;
        }
        let mi = found?;

        score += 10;
        if prev == Some(mi.saturating_sub(1)) {
            score += 8; // consecutive match
        }
        if mi == 0 || is_separator(target[mi - 1]) {
            score += 6; // word/start boundary
        }

        prev = Some(mi);
        ti = mi + 1;
    }

    Some(score)
}

fn is_separator(c: char) -> bool {
    matches!(c, ' ' | '.' | '_' | '-' | '/' | '\\' | ':')
}

#[cfg(test)]
#[path = "fuzzy_tests.rs"]
mod tests;
