//! Which routes a script's `#key` names, found once when the engine is made:
//! a `RouteAmount` write looks its routes up instead of reading every
//! route's name (AO programs write thousands in `on init`).

use crate::plan::Group;

/// Every `#key` word in a route's name, as (key, group, route), by key.
pub(crate) struct RouteNames(Box<[(i32, usize, usize)]>);

impl RouteNames {
    /// The `#key` words of `groups`' routes. The engine never adds or
    /// removes a route, so the positions hold for its life.
    pub(crate) fn new(groups: &[Group]) -> Self {
        Self::of_names(
            groups
                .iter()
                .map(|g| g.mods.iter().map(|m| m.name.as_str())),
        )
    }

    /// From each group's route names, in order.
    fn of_names<'a, G, R>(groups: G) -> Self
    where
        G: IntoIterator<Item = R>,
        R: IntoIterator<Item = &'a str>,
    {
        let mut keys: Vec<(i32, usize, usize)> = Vec::new();
        for (g, names) in groups.into_iter().enumerate() {
            for (m, name) in names.into_iter().enumerate() {
                keys.extend(name.split_whitespace().filter_map(key).map(|k| (k, g, m)));
            }
        }
        keys.sort_unstable();
        keys.dedup();
        Self(keys.into())
    }

    /// The (group, route) of every route named with `#key`.
    pub(crate) fn of(&self, key: i32) -> impl Iterator<Item = (usize, usize)> + '_ {
        let from = self.0.partition_point(|e| e.0 < key);
        (self.0.get(from..).unwrap_or_default().iter())
            .take_while(move |e| e.0 == key)
            .map(|&(_, g, m)| (g, m))
    }
}

/// The key of a `#key` word, written as `format!("#{key}")` writes it.
fn key(word: &str) -> Option<i32> {
    let digits = word.strip_prefix('#')?;
    let key: i32 = digits.parse().ok()?;
    (key.to_string() == digits).then_some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_its_canonical_word_only() {
        assert_eq!(key("#12"), Some(12));
        assert_eq!(key("#-3"), Some(-3));
        assert_eq!(key("#012"), None);
        assert_eq!(key("#+1"), None);
        assert_eq!(key("12"), None);
        assert_eq!(key("#x"), None);
    }

    #[test]
    fn a_key_finds_every_route_named_with_it_and_no_other() {
        let names = RouteNames::of_names([vec!["Cutoff #5 #7", "#50"], vec!["", "#5", "#5 #5"]]);
        assert_eq!(names.of(5).collect::<Vec<_>>(), [(0, 0), (1, 1), (1, 2)]);
        assert_eq!(names.of(7).collect::<Vec<_>>(), [(0, 0)]);
        assert_eq!(names.of(50).collect::<Vec<_>>(), [(0, 1)]);
        assert_eq!(names.of(6).count(), 0);
    }
}
