use super::*;

#[test]
fn subsequence_match_requires_order() {
    assert!(fuzzy_match("abc", "a_b_c"));
    assert!(fuzzy_match("sea", "seagate sata hdd"));
    assert!(!fuzzy_match("abc", "acb"));
    assert!(!fuzzy_match("zz", "abc"));
}

#[test]
fn matching_is_case_insensitive() {
    assert!(fuzzy_match("SEA", "seagate sata hdd"));
    assert!(fuzzy_match("sea", "SEAGATE SATA HDD"));
}

#[test]
fn empty_query_matches_everything() {
    assert!(fuzzy_match("", "anything"));
    assert_eq!(fuzzy_score("", "x"), Some(0));
}

#[test]
fn scoring_prefers_consecutive_and_boundary_matches() {
    let consecutive = fuzzy_score("sea", "seagate").unwrap();
    let scattered = fuzzy_score("sea", "s e a").unwrap();
    assert!(consecutive > scattered);
}
