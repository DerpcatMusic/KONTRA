use crate::SampleTime;

/// Errors from constructing time values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TimeError {
    /// Tempo must be finite and above zero.
    #[error("tempo must be finite and above zero")]
    InvalidTempo,
    /// Sample rate must be nonzero.
    #[error("sample rate must be nonzero")]
    ZeroSampleRate,
}

/// Host tempo in quarter notes per minute. Always finite and above zero.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tempo(f64);

impl Tempo {
    /// Validates `bpm` once, so conversions never divide by zero.
    pub fn new(bpm: f64) -> Result<Self, TimeError> {
        if bpm.is_finite() && bpm > 0.0 {
            Ok(Self(bpm))
        } else {
            Err(TimeError::InvalidTempo)
        }
    }

    /// Quarter notes per minute.
    #[must_use]
    pub fn bpm(self) -> f64 {
        self.0
    }
}

/// Tempo and sample rate, the inputs to beat↔sample conversion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transport {
    tempo: Tempo,
    sample_rate: u32,
}

impl Transport {
    /// Fails when `sample_rate` is zero.
    pub fn new(tempo: Tempo, sample_rate: u32) -> Result<Self, TimeError> {
        if sample_rate == 0 {
            return Err(TimeError::ZeroSampleRate);
        }
        Ok(Self { tempo, sample_rate })
    }

    /// The host tempo.
    #[must_use]
    pub fn tempo(self) -> Tempo {
        self.tempo
    }

    /// Frames per second.
    #[must_use]
    pub fn sample_rate(self) -> u32 {
        self.sample_rate
    }

    /// Converts a beat position to the sample clock, truncated toward zero.
    /// Negative and NaN beats map to `SampleTime(0)`.
    #[must_use]
    pub fn beats_to_samples(self, beats: f64) -> SampleTime {
        SampleTime((beats * 60.0 / self.tempo.0 * f64::from(self.sample_rate)) as u64)
    }

    /// Converts a sample position to beats.
    #[must_use]
    pub fn samples_to_beats(self, time: SampleTime) -> f64 {
        time.0 as f64 * self.tempo.0 / (60.0 * f64::from(self.sample_rate))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tempo_rejects_non_positive_and_non_finite() {
        for bpm in [0.0, -120.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(Tempo::new(bpm), Err(TimeError::InvalidTempo), "bpm {bpm}");
        }
        assert_eq!(Tempo::new(120.0).map(Tempo::bpm), Ok(120.0));
    }

    #[test]
    fn transport_rejects_zero_sample_rate() {
        let tempo = Tempo::new(120.0).expect("valid tempo");
        assert_eq!(Transport::new(tempo, 0), Err(TimeError::ZeroSampleRate));
    }

    #[test]
    fn beats_convert_to_samples_at_120_bpm() {
        let transport = Transport::new(Tempo::new(120.0).expect("valid tempo"), 48_000)
            .expect("valid transport");
        assert_eq!(transport.beats_to_samples(1.0), SampleTime(24_000));
        assert_eq!(transport.beats_to_samples(0.25), SampleTime(6_000));
        assert_eq!(transport.beats_to_samples(-1.0), SampleTime(0));
        assert_eq!(transport.beats_to_samples(f64::NAN), SampleTime(0));
    }

    #[test]
    fn sample_and_beat_round_trip_within_one_frame() {
        for bpm in [60.0, 90.0, 120.0, 140.5] {
            for rate in [44_100, 48_000, 96_000] {
                let transport = Transport::new(Tempo::new(bpm).expect("valid tempo"), rate)
                    .expect("valid transport");
                for frame in (0..10 * u64::from(rate)).step_by(997) {
                    let time = SampleTime(frame);
                    let back = transport.beats_to_samples(transport.samples_to_beats(time));
                    assert!(
                        back.0.abs_diff(frame) <= 1,
                        "bpm {bpm} rate {rate}: {frame} -> {}",
                        back.0
                    );
                }
            }
        }
    }
}
