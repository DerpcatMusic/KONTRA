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
}

impl EnginePars {
    /// What parameter id `id` changes; `None` when the engine does not answer it.
    #[must_use]
    pub fn get(&self, id: i32) -> Option<EnginePar> {
        let n = usize::try_from(id.checked_sub(self.first)?).ok()?;
        self.by_id.get(n).copied().flatten()
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
        };
        assert_eq!(pars.get(101), Some(EnginePar::Attack));
        assert_eq!(pars.get(100), None);
        assert_eq!(pars.get(99), None);
        assert_eq!(pars.get(i32::MIN), None);
    }
}
