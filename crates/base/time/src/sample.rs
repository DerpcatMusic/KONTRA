/// Absolute position on the sample clock, in frames since playback started.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SampleTime(pub u64);

impl SampleTime {
    /// Returns the position `frames` later, saturating at `u64::MAX`.
    #[must_use]
    pub fn advance(self, frames: Frames) -> Self {
        Self(self.0.saturating_add(u64::from(frames.0)))
    }
}

/// A count of sample frames, such as a block length. A span, not a position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Frames(pub u32);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advance_adds_frames_and_saturates() {
        assert_eq!(SampleTime(10).advance(Frames(256)), SampleTime(266));
        assert_eq!(
            SampleTime(u64::MAX).advance(Frames(1)),
            SampleTime(u64::MAX)
        );
    }
}
