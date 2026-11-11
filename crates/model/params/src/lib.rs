//! Format-neutral parameter identities shared by adapters and the engine.

/// Controls of an algorithmic reverb. These identities do not select an
/// algorithm, network topology, or a format-specific conversion law.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReverbPar {
    /// Decay time.
    Seconds,
    /// Room size.
    Size,
    /// High-frequency damping of the tail.
    Damping,
    /// Echo density.
    Diffusion,
    /// Delay before the tail starts.
    Predelay,
    /// Low-pass on the wet signal.
    HighCut,
    /// Low shelf on the wet signal.
    LowShelf,
    /// Stereo width of the tail.
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
    #[must_use]
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
    /// [`ReverbPar::Seconds`].
    pub seconds: T,
    /// [`ReverbPar::Size`].
    pub size: T,
    /// [`ReverbPar::Damping`].
    pub damping: T,
    /// [`ReverbPar::Diffusion`].
    pub diffusion: T,
    /// [`ReverbPar::Predelay`].
    pub predelay: T,
    /// [`ReverbPar::HighCut`].
    pub high_cut: T,
    /// [`ReverbPar::LowShelf`].
    pub low_shelf: T,
    /// [`ReverbPar::Stereo`].
    pub stereo: T,
}

impl<T> ReverbControls<T> {
    /// Construct all controls from their identities.
    #[must_use]
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
    #[must_use]
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
