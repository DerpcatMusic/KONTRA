//! Editing boundaries. `Ui` selections are scalar indices;
//! convert only at the edge and never split an extended grapheme cluster.
use unicode_segmentation::UnicodeSegmentation;

fn byte(s: &str, scalar: usize) -> usize {
    s.char_indices().nth(scalar).map_or(s.len(), |(i, _)| i)
}
fn scalar(s: &str, byte: usize) -> usize {
    s[..byte].chars().count()
}

pub(crate) fn floor(s: &str, at: usize) -> usize {
    let b = byte(s, at);
    let boundary = s
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .chain(std::iter::once(s.len()))
        .take_while(|&i| i <= b)
        .last()
        .unwrap_or(0);
    scalar(s, boundary)
}
pub(crate) fn previous(s: &str, at: usize) -> usize {
    let b = byte(s, at);
    let boundary = s
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .take_while(|&i| i < b)
        .last()
        .unwrap_or(0);
    scalar(s, boundary)
}
pub(crate) fn next(s: &str, at: usize) -> usize {
    let b = byte(s, at);
    let boundary = s
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .find(|&i| i > b)
        .unwrap_or(s.len());
    scalar(s, boundary)
}
pub(crate) fn word(s: &str, at: usize) -> (usize, usize) {
    let b = byte(s, at);
    for (start, w) in s.unicode_word_indices() {
        if start <= b && (b < start + w.len() || b == s.len() && b == start + w.len()) {
            return (scalar(s, start), scalar(s, start + w.len()));
        }
    }
    let start = floor(s, at);
    (start, next(s, start))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combining_mark_is_one_edit_unit() {
        let s = "Ae\u{301}B";
        assert_eq!(next(s, 1), 3);
        assert_eq!(previous(s, 3), 1);
        assert_eq!(floor(s, 2), 1);
    }

    #[test]
    fn family_emoji_is_one_edit_unit() {
        let cluster = "👨‍👩‍👧‍👦";
        let s = format!("A{cluster}B");
        let end = 1 + cluster.chars().count();
        assert_eq!(next(&s, 1), end);
        assert_eq!(previous(&s, end), 1);
    }

    #[test]
    fn boundaries_round_trip_for_mixed_text() {
        for s in ["", "ASCII", "a\u{301}b", "🇮🇱🇬🇧", "שָׁלוֹם", "👩🏽‍💻!"]
        {
            let n = s.chars().count();
            let mut at = 0;
            while at < n {
                let end = next(s, at);
                assert!(end > at && end <= n);
                assert_eq!(previous(s, end), at);
                assert_eq!(floor(s, at), at);
                at = end;
            }
            assert_eq!(floor(s, usize::MAX), n);
            assert_eq!(next(s, n), n);
        }
    }

    #[test]
    fn word_selection_keeps_marks_and_apostrophes() {
        assert_eq!(word("can't stop", 2), (0, 5));
        assert_eq!(word("cafe\u{301}", 3), (0, 5));
    }
}
