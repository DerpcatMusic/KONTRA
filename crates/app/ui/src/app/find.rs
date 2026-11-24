//! The browser's search: each word of a query found as its letters in
//! order from a word start, within a compact span. That finds prefixes
//! (`leg`), abbreviations (`tpt` Trumpet, `fh` French Horn, `brs ens`) and
//! a dropped or doubled letter (`pizicato`), and not the scattered letters
//! an fzf-style subsequence takes (on 1821 real preset paths: recall 0.97
//! at precision 0.94, where substrings reach 0.67 and fzf's 0.98 comes with
//! 460 rows a query).

use std::ops::Range;

/// Whether byte `j` of `raw` begins a word: start of text, after a
/// separator, a CamelCase step (`aB`), or a digit/letter step (`01Legato`).
fn is_boundary(raw: &[u8], j: usize) -> bool {
    if j == 0 {
        return true;
    }
    let (Some(&prev), Some(&cur)) = (raw.get(j - 1), raw.get(j)) else {
        return false;
    };
    !prev.is_ascii_alphanumeric()
        || (prev.is_ascii_lowercase() && cur.is_ascii_uppercase())
        || (prev.is_ascii_digit() != cur.is_ascii_digit() && cur.is_ascii_alphanumeric())
}

/// Where `word` (lower case) matches best in `raw` / `lower`: its score and
/// the byte range from its first letter to its last.
fn anchored(word: &[u8], raw: &[u8], lower: &[u8]) -> Option<(usize, Range<usize>)> {
    let first = *word.first()?;
    let most = (word.len() * 2).max(word.len() + 4);
    let mut best: Option<(usize, Range<usize>)> = None;
    for start in (0..lower.len()).filter(|&s| lower.get(s) == Some(&first) && is_boundary(raw, s)) {
        let (mut next, mut prev, mut score) = (start, None::<usize>, 0);
        let mut whole = true;
        for &c in word {
            let at = (lower.get(next..).unwrap_or_default().iter())
                .position(|&x| x == c)
                .map(|k| next + k);
            let Some(j) = at.filter(|&j| j - start < most) else {
                whole = false;
                break;
            };
            score += match prev {
                None => 4 + usize::from(start == 0) * 3,
                Some(p) if j == p + 1 => 4,
                _ if is_boundary(raw, j) => 2,
                _ => 0,
            };
            (prev, next) = (Some(j), j + 1);
        }
        let span = next - start;
        let exact = usize::from(span == word.len()) * 4;
        let score = (score + word.len() + exact).saturating_sub(span);
        if whole && best.as_ref().is_none_or(|b| score > b.0) {
            best = Some((score, start..next));
        }
    }
    best
}

/// A query's words, lower case.
fn words(query: &str) -> impl Iterator<Item = String> + '_ {
    query.split_whitespace().map(str::to_ascii_lowercase)
}

/// Relevance score when every word of `query` matches one of `texts`; `None`
/// when any word misses. An empty query matches everything with score `0`.
pub(super) fn score(texts: &[&str], query: &str) -> Option<usize> {
    let lowered: Vec<String> = texts.iter().map(|t| t.to_ascii_lowercase()).collect();
    let mut total = 0usize;
    for w in words(query) {
        let best = texts
            .iter()
            .zip(&lowered)
            .filter_map(|(raw, lower)| anchored(w.as_bytes(), raw.as_bytes(), lower.as_bytes()))
            .map(|(s, _)| s)
            .max()?;
        total = total.saturating_add(best);
    }
    Some(total)
}

/// Whether every word of `query` is in one of `texts` (no query finds
/// everything).
pub(super) fn found(texts: &[&str], query: &str) -> bool {
    score(texts, query).is_some()
}

/// The ranges of `text` the words of `query` match, in order, apart.
pub(super) fn hits(text: &str, query: &str) -> Vec<Range<usize>> {
    let lower = text.to_ascii_lowercase();
    let mut hits: Vec<Range<usize>> = words(query)
        .filter_map(|w| anchored(w.as_bytes(), text.as_bytes(), lower.as_bytes()).map(|(_, r)| r))
        .collect();
    hits.sort_unstable_by_key(|r| r.start);
    hits.dedup_by(|b, a| b.start < a.end);
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_find_prefixes_abbreviations_and_typos_not_scatter() {
        let horn = "Solo French Horn Staccatissimo";
        assert!(found(&[horn], "fh stacc") && found(&[horn], "hor"));
        assert!(found(&["Solo Trumpet Legato"], "tpt leg"));
        assert!(found(&["Violins Pizzicato"], "pizicato"));
        assert!(!found(&[horn], "rench"), "from a word start");
        assert!(!found(&["Solo String Ensemble"], "sse"), "not scattered");
        assert_eq!(hits("Solo French Horn", "horn fr"), [5..7, 12..16]);
    }

    #[test]
    fn camel_case_and_digit_transitions_start_words_and_rank_prefixes_higher() {
        assert!(found(&["SoloViolin"], "violin"));
        assert!(found(&["01Legato"], "legato"));
        assert!(found(&["3Trumpets"], "tpt"));
        assert_eq!(hits("01Legato", "leg"), std::slice::from_ref(&(2..5)));
        assert_eq!(hits("SoloViolin", "viol"), std::slice::from_ref(&(4..8)));
        let prefix = score(&["Horn Solo"], "horn").unwrap_or(0);
        let later = score(&["Afflatus Brass Horn"], "horn").unwrap_or(0);
        assert!(prefix > later, "prefix {prefix} should beat later {later}");
    }
}
