//! Retriggered, unsynchronized native Glide sources. History belongs to the
//! program container; these clocks belong to each admitted sample voice.

use crate::modulation::{GlideMode, PitchGlide, PitchLfo, VolumeLfo};

pub(super) fn validate_sources(sources: &[PitchGlide], pitch: &[PitchLfo], volume: &[VolumeLfo]) -> anyhow::Result<()> {
    let mut slots = 0_u16;
    for source in sources {
        let slot = usize::from(source.slot);
        anyhow::ensure!(slot < 16, "Glide source slot {slot} is outside the sixteen native source slots");
        let bit = 1_u16 << slot;
        anyhow::ensure!(slots & bit == 0, "Glide source slot {slot} has duplicate rows");
        slots |= bit;
        anyhow::ensure!(!pitch.iter().any(|l| l.slot == source.slot)
            && !volume.iter().any(|l| l.source.slot == source.slot),
            "Glide source slot {slot} is also occupied by an LFO");
        anyhow::ensure!(source.time_ms.is_finite() && source.time_ms > 0.
            && source.depth.is_finite() && source.targets.iter().all(|t| t.1.is_finite()),
            "Glide source slot {slot} has invalid timing or target depth");
        let mut targets = std::collections::HashSet::with_capacity(source.targets.len());
        anyhow::ensure!(source.targets.iter().all(|t| targets.insert(t.0)),
            "Glide source slot {slot} has duplicate target identities");
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default)]
struct Source {
    level: f32,
    step: f32,
    scale: f32,
}

impl Source {
    fn new(source: &PitchGlide, interval: i16, rate: f32) -> Option<Self> {
        let interval = interval.clamp(-12, 12);
        if interval == 0 { return Some(Self::default()); }
        let distance = f32::from(interval.abs());
        let duration = match source.mode {
            GlideMode::Speed => source.time_ms * distance,
            GlideMode::Time => source.time_ms,
        };
        // Preserve native float operation order at the audio-rate/32 clock.
        let step = 1. / ((duration * 0.001) * (rate / 32.));
        if !step.is_finite() || step <= 0. { return None; }
        Some(Self {
            level: if interval < 0 { 1. } else { -1. },
            step: if interval < 0 { -step } else { step },
            scale: distance * (1. / 12.),
            ..Self::default()
        })
    }

    fn point(&mut self, depth: f32) -> f32 {
        let point = (self.level * self.scale) * depth;
        let next = self.level + self.step;
        self.level = if self.step > 0. { next.min(0.) } else { next.max(0.) };
        point
    }
}

/// Copyable planning state: previews never advance the audible source clock.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Clock {
    sources: [Source; 16],
}

impl Clock {
    pub(super) fn new(sources: &[PitchGlide], interval: i16, rate: f32) -> Option<Self> {
        if sources.is_empty() { return Some(Self::default()); }
        if !rate.is_finite() || rate <= 0. { return None; }
        let mut clock = Self::default();
        for source in sources {
            *clock.sources.get_mut(usize::from(source.slot))? = Source::new(source, interval, rate)?;
        }
        Some(clock)
    }

    /// Sample each active source once. The common pitch destination owns the
    /// retained endpoints, so an off-tick bypass cannot erase its old point.
    pub(super) fn points(&mut self, sources: &[PitchGlide], out: &mut [f32; 16]) {
        for source in sources.iter().filter(|source| !source.bypassed) {
            let slot = usize::from(source.slot);
            out[slot] = self.sources[slot].point(source.depth);
        }
    }

}

#[cfg(all(test, feature = "plugin"))]
#[path = "glide_tests.rs"]
mod tests;
