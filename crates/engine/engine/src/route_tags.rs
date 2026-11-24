//! Indexed lookup of routes tagged with `#key` tokens in [`kontra_ir::Mod::name`],
//! built once at engine construction so script route-amount writes avoid scanning
//! every group's routes.

/// Sorted `(tag_key, group_index, mod_index)` entries for script route addressing.
pub(crate) struct RouteTags(Box<[(i32, usize, usize)]>);

impl RouteTags {
    /// Indexes every canonical `#<i32>` token across all groups' modulation routes.
    pub(crate) fn of_groups(groups: &[crate::Group]) -> Self {
        Self::of_names(
            groups
                .iter()
                .map(|g| g.mods.iter().map(|m| m.name.as_str())),
        )
    }

    fn of_names<'a, G: Iterator<Item = R>, R: Iterator<Item = &'a str>>(groups: G) -> Self {
        let words = |(g, names): (usize, R)| names.enumerate().map(move |(m, n)| (g, m, n));
        let mut keys: Vec<_> = groups
            .enumerate()
            .flat_map(words)
            .flat_map(|(g, m, n)| {
                n.split_whitespace()
                    .filter_map(parse_tag)
                    .map(move |k| (k, g, m))
            })
            .collect();
        keys.sort_unstable();
        keys.dedup();
        Self(keys.into())
    }

    /// Yields `(group_index, mod_index)` for every route tagged with `#key`.
    pub(crate) fn of(&self, key: i32) -> impl Iterator<Item = (usize, usize)> + '_ {
        let from = self.0.partition_point(|e| e.0 < key);
        self.0
            .iter()
            .skip(from)
            .take_while(move |e| e.0 == key)
            .map(|&(_, g, m)| (g, m))
    }
}

/// Parses a canonical `#<i32>` token (`format!("#{key}")`).
fn parse_tag(word: &str) -> Option<i32> {
    let digits = word.strip_prefix('#')?;
    digits
        .parse()
        .ok()
        .filter(|k: &i32| k.to_string() == digits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_is_its_canonical_word_only() {
        assert_eq!(parse_tag("#12"), Some(12));
        assert_eq!(parse_tag("#-3"), Some(-3));
        assert_eq!(parse_tag("#012"), None);
        assert_eq!(parse_tag("#+1"), None);
        assert_eq!(parse_tag("12"), None);
        assert_eq!(parse_tag("#x"), None);
    }

    #[test]
    fn a_tag_finds_every_route_tagged_with_it_and_no_other() {
        let tags = RouteTags::of_names(
            [vec!["Cutoff #5 #7", "#50"], vec!["", "#5", "#5 #5"]]
                .into_iter()
                .map(IntoIterator::into_iter),
        );
        assert_eq!(tags.of(5).collect::<Vec<_>>(), [(0, 0), (1, 1), (1, 2)]);
        assert_eq!(tags.of(7).collect::<Vec<_>>(), [(0, 0)]);
        assert_eq!(tags.of(50).collect::<Vec<_>>(), [(0, 1)]);
        assert_eq!(tags.of(6).count(), 0);
    }
}
