//! The routes each `#key` word names, found once: a script's route writes
//! look them up instead of reading every route's name (AO writes thousands
//! in `on init`).

/// (key, group, route) for every `#key` word of a route's name, sorted.
pub struct RouteNames(Box<[(i32, usize, usize)]>);

impl RouteNames {
    /// From each group's route names ([`crate::Mod::name`]), in order; the
    /// positions hold while no route is added or removed.
    pub fn of_names<'a, G: Iterator<Item = R>, R: Iterator<Item = &'a str>>(groups: G) -> Self {
        let words = |(g, names): (usize, R)| names.enumerate().map(move |(m, n)| (g, m, n));
        let mut keys: Vec<_> = (groups.enumerate().flat_map(words))
            .flat_map(|(g, m, n)| n.split_whitespace().filter_map(key).map(move |k| (k, g, m)))
            .collect();
        keys.sort_unstable();
        keys.dedup();
        Self(keys.into())
    }

    /// The (group, route) of every route named with `#key`.
    pub fn of(&self, key: i32) -> impl Iterator<Item = (usize, usize)> + '_ {
        let from = self.0.partition_point(|e| e.0 < key);
        (self.0.iter().skip(from))
            .take_while(move |e| e.0 == key)
            .map(|&(_, g, m)| (g, m))
    }
}

/// A `#key` word's key, written as `format!("#{key}")` writes it.
fn key(word: &str) -> Option<i32> {
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
        let names = RouteNames::of_names(
            [vec!["Cutoff #5 #7", "#50"], vec!["", "#5", "#5 #5"]]
                .into_iter()
                .map(|g| g.into_iter()),
        );
        assert_eq!(names.of(5).collect::<Vec<_>>(), [(0, 0), (1, 1), (1, 2)]);
        assert_eq!(names.of(7).collect::<Vec<_>>(), [(0, 0)]);
        assert_eq!(names.of(50).collect::<Vec<_>>(), [(0, 1)]);
        assert_eq!(names.of(6).count(), 0);
    }
}
