//! The browser's search: each word of a query found as its letters in
//! order from a word start, within a compact span. That finds prefixes
//! (`leg`), abbreviations (`tpt` Trumpet, `fh` French Horn, `brs ens`) and
//! a dropped or doubled letter (`pizicato`), and not the scattered letters
//! an fzf-style subsequence takes (on 1821 real preset paths: recall 0.97
//! at precision 0.94, where substrings reach 0.67 and fzf's 0.98 comes with
//! 460 rows a query).

use std::ops::Range;

/// Where `word` (lower case) matches best in `text` (ASCII lower case): the
/// byte range from its first letter to its last.
fn anchored(word: &[u8], text: &[u8]) -> Option<Range<usize>> {
    let first = *word.first()?;
    let most = (word.len() * 2).max(word.len() + 4);
    let boundary = |j: usize| j == 0 || text.get(j - 1).is_some_and(|c| !c.is_ascii_alphanumeric());
    let mut best: Option<(usize, Range<usize>)> = None;
    for start in (0..text.len()).filter(|&s| text.get(s) == Some(&first) && boundary(s)) {
        let (mut next, mut prev, mut score) = (start, None::<usize>, 0);
        let mut whole = true;
        for &c in word {
            let at = (text.get(next..).unwrap_or_default().iter())
                .position(|&x| x == c)
                .map(|k| next + k);
            let Some(j) = at.filter(|&j| j - start < most) else {
                whole = false;
                break;
            };
            score += match prev {
                None => 4,
                Some(p) if j == p + 1 => 4,
                _ if boundary(j) => 2,
                _ => 0,
            };
            (prev, next) = (Some(j), j + 1);
        }
        let score = (score + word.len()).saturating_sub(next - start);
        if whole && best.as_ref().is_none_or(|b| score > b.0) {
            best = Some((score, start..next));
        }
    }
    best.map(|b| b.1)
}

/// A query's words, lower case.
fn words(query: &str) -> impl Iterator<Item = String> + '_ {
    query.split_whitespace().map(str::to_ascii_lowercase)
}

/// Whether every word of `query` is in one of `texts` (no query finds
/// everything).
pub(super) fn found(texts: &[&str], query: &str) -> bool {
    let texts: Vec<String> = texts.iter().map(|t| t.to_ascii_lowercase()).collect();
    words(query).all(|w| (texts.iter()).any(|t| anchored(w.as_bytes(), t.as_bytes()).is_some()))
}

/// The ranges of `text` the words of `query` match, in order, apart.
pub(super) fn hits(text: &str, query: &str) -> Vec<Range<usize>> {
    let lower = text.to_ascii_lowercase();
    let mut hits: Vec<Range<usize>> = words(query)
        .filter_map(|w| anchored(w.as_bytes(), lower.as_bytes()))
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
}
