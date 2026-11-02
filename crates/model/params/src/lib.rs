//! Format-neutral parameter identities shared by adapters and the engine.

/// Controls of an algorithmic reverb. These identities do not select an
/// algorithm, network topology, or a format-specific conversion law.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReverbPar {
    Seconds,
    Size,
    Damping,
    Diffusion,
    Predelay,
    HighCut,
    LowShelf,
    Stereo,
}

impl ReverbPar {
    /// Canonical presentation order. Identities, rather than positions, select values.
    pub const ALL: [Self; 8] = [
        Self::Seconds,
        Self::Size,
        Self::Damping,
        Self::Diffusion,
        Self::Predelay,
        Self::HighCut,
        Self::LowShelf,
        Self::Stereo,
    ];

    /// Stable format-neutral identity; do not derive this from the enum ordinal.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Seconds => "reverb.seconds",
            Self::Size => "reverb.size",
            Self::Damping => "reverb.damping",
            Self::Diffusion => "reverb.diffusion",
            Self::Predelay => "reverb.predelay",
            Self::HighCut => "reverb.high_cut",
            Self::LowShelf => "reverb.low_shelf",
            Self::Stereo => "reverb.stereo",
        }
    }
}

/// One value for each reverb control. Field names prevent independently
/// prepared laws from accidentally acquiring another control's position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReverbControls<T> {
    pub seconds: T,
    pub size: T,
    pub damping: T,
    pub diffusion: T,
    pub predelay: T,
    pub high_cut: T,
    pub low_shelf: T,
    pub stereo: T,
}

impl<T> ReverbControls<T> {
    /// Construct all controls from their identities.
    pub fn from_fn(mut f: impl FnMut(ReverbPar) -> T) -> Self {
        Self {
            seconds: f(ReverbPar::Seconds),
            size: f(ReverbPar::Size),
            damping: f(ReverbPar::Damping),
            diffusion: f(ReverbPar::Diffusion),
            predelay: f(ReverbPar::Predelay),
            high_cut: f(ReverbPar::HighCut),
            low_shelf: f(ReverbPar::LowShelf),
            stereo: f(ReverbPar::Stereo),
        }
    }

    /// Access a control directly, with no allocation or string lookup.
    pub const fn get(&self, par: ReverbPar) -> &T {
        match par {
            ReverbPar::Seconds => &self.seconds,
            ReverbPar::Size => &self.size,
            ReverbPar::Damping => &self.damping,
            ReverbPar::Diffusion => &self.diffusion,
            ReverbPar::Predelay => &self.predelay,
            ReverbPar::HighCut => &self.high_cut,
            ReverbPar::LowShelf => &self.low_shelf,
            ReverbPar::Stereo => &self.stereo,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identities_are_unique_and_select_their_own_values() {
        let controls = ReverbControls::from_fn(|p| p.id());
        for (i, par) in ReverbPar::ALL.iter().enumerate() {
            assert_eq!(*controls.get(*par), par.id());
            assert!(
                !ReverbPar::ALL[..i]
                    .iter()
                    .any(|other| other.id() == par.id())
            );
        }
    }
}
