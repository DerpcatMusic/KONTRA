//! The engine parameters scripts read and set, by what they change. A script
//! names them by its own ids; the host maps those to [`EnginePar`] once, so the
//! engine never sees a format's parameter names.

use kontra_ir as ir;

pub use ir::EnginePar;

/// A script's parameter ids: id `first + n` is `by_id[n]`.
#[derive(Clone, Debug, Default)]
pub struct EnginePars {
    pub first: i32,
    pub by_id: Box<[Option<EnginePar>]>,
    /// What id `first + n` shows as text, when the translator gave a law.
    pub readouts: Box<[Option<ir::Readout>]>,
    /// The effect type id `first + n` belongs to, for a parameter only one
    /// effect type has. On a slot holding another type it names nothing.
    pub owners: Box<[Option<i32>]>,
    /// The values `set_engine_par` takes for id `first + n`, when it takes
    /// only some; any other set is refused.
    pub accepts: Box<[Option<(i32, i32)>]>,
}

impl EnginePars {
    /// What parameter id `id` changes; `None` when the engine does not answer it.
    #[must_use]
    pub fn get(&self, id: i32) -> Option<EnginePar> {
        let n = usize::try_from(id.checked_sub(self.first)?).ok()?;
        self.by_id.get(n).copied().flatten()
    }

    /// The only effect type that has parameter id `id`, if one does.
    #[must_use]
    pub fn owner(&self, id: i32) -> Option<i32> {
        let n = usize::try_from(id.checked_sub(self.first)?).ok()?;
        self.owners.get(n).copied().flatten()
    }

    /// Whether `set_engine_par` takes `value` for id `id`.
    #[must_use]
    pub fn takes(&self, id: i32, value: i32) -> bool {
        let n = id
            .checked_sub(self.first)
            .and_then(|n| usize::try_from(n).ok());
        match n.and_then(|n| self.accepts.get(n).copied().flatten()) {
            Some((lo, hi)) => (lo..=hi).contains(&value),
            None => true,
        }
    }

    /// The text law of parameter id `id`.
    #[must_use]
    pub fn readout(&self, id: i32) -> Option<&ir::Readout> {
        let n = usize::try_from(id.checked_sub(self.first)?).ok()?;
        self.readouts.get(n)?.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::{EnginePar, EnginePars};

    #[test]
    fn ids_resolve_from_first() {
        let pars = EnginePars {
            first: 100,
            by_id: vec![None, Some(EnginePar::Attack)].into(),
            readouts: Box::default(),
            owners: Box::default(),
            accepts: vec![None, Some((2, 12))].into(),
        };
        assert!(pars.takes(100, -5) && pars.takes(101, 12) && pars.takes(7, 99));
        assert!(!pars.takes(101, 1) && !pars.takes(101, 13) && !pars.takes(101, -1));
        assert_eq!(pars.get(101), Some(EnginePar::Attack));
        assert_eq!(pars.get(100), None);
        assert_eq!(pars.get(99), None);
        assert_eq!(pars.get(i32::MIN), None);
    }
}
