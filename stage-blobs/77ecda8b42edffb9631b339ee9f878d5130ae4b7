//! KSP's documented block-start host values; the existing lowerer owns slots 8..=18.
use crate::sound::Transport;

pub(super) struct State {
    transport: Transport,
    at: u64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            transport: Transport {
                tempo: 120.,
                signature: (4, 4),
                ..Transport::default()
            },
            at: 0,
        }
    }
}

impl State {
    pub(super) fn update(&mut self, host: Transport, clock: u64, rate: f64) {
        // Missing position follows already-rendered samples at the previous tempo (v1).
        if host.beats.is_finite() {
            self.transport.beats = host.beats;
        } else if self.transport.playing && rate.is_finite() && rate > 0. {
            self.transport.beats +=
                clock.saturating_sub(self.at) as f64 * (self.transport.tempo / 60.) / rate;
        }
        self.at = clock;
        if host.tempo.is_finite() && host.tempo > 0. {
            self.transport.tempo = host.tempo;
        }
        if host.signature.0 > 0 && host.signature.1 > 0 {
            self.transport.signature = host.signature;
        }
        self.transport.playing = host.playing;
    }

    pub(super) fn tempo(&self) -> f64 {
        self.transport.tempo
    }

    pub(super) fn values(&self) -> [i64; 11] {
        let host = self.transport;
        // KSP integer reads are i32; saturate extreme host inputs rather than fault a script.
        let quarter = (60_000_000. / host.tempo).clamp(1., f64::from(i32::MAX)) as i64;
        let bar = if host.playing {
            (quarter * 4 * i64::from(host.signature.0) / i64::from(host.signature.1))
                .min(i64::from(i32::MAX))
        } else {
            0
        };
        [
            quarter,
            quarter / 2,
            quarter / 4,
            quarter * 2 / 3,
            quarter / 3,
            quarter / 6,
            bar,
            i64::from((host.beats * 960.).floor() as i32),
            i64::from(host.signature.0),
            i64::from(host.signature.1),
            i64::from(host.playing),
        ]
    }
}
