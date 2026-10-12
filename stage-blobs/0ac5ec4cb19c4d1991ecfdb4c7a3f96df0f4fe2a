//! Cheap, non-realtime check that an instrument answers MPE per-note
//! expression: one note per run on a member channel, then a +2 semitone bend
//! or full channel pressure sent after it has started.
use crate::{Mpe, Packets, Zone};
use sampler_core::{Error, Runtime};

/// What the rendered second window showed against an untouched note.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MpeResponse {
    /// Zero crossings with the bend over without it; 1.12 for a clean +2 st.
    pub pitch_ratio: f64,
    /// RMS with pressure at full over RMS without, in dB.
    pub pressure_db: f64,
}
impl MpeResponse {
    /// Pitch moved up by about 1 to 3 semitones' worth of crossings.
    pub fn pitch_responds(&self) -> bool {
        (1.06..=1.19).contains(&self.pitch_ratio)
    }
    /// Level moved by at least 1 dB.
    pub fn pressure_responds(&self) -> bool {
        self.pressure_db.abs() >= 1.0
    }
}

const WINDOW: usize = 8192;

/// `fresh` builds a new runtime for each of the three runs. The note plays on
/// lower-zone member channel 1 (member bend range 48, so +2 st is 8533).
pub fn mpe_response(mut fresh: impl FnMut() -> Runtime, key: u8) -> Result<MpeResponse, Error> {
    let mut run = |send: Option<u32>| -> Result<Vec<[f32; 2]>, Error> {
        let mut rt = fresh();
        let mut mpe = Mpe::new(&rt, 0, 0, Zone::Lower, 2, 8)?;
        let mut apply = |rt: &mut Runtime, word: u32| {
            let words = [word];
            let packet = Packets::new(&words).next().ok_or(Error::InvalidInput)?;
            let packet = packet.map_err(|_| Error::InvalidInput)?;
            mpe.apply(rt, packet).map_err(|_| Error::InvalidInput)
        };
        apply(&mut rt, 0x2091_0000 | u32::from(key) << 8 | 100)?;
        let mut out = vec![[0.0; 2]; 2 * WINDOW];
        rt.render(&mut out[..WINDOW])?;
        if let Some(word) = send {
            apply(&mut rt, word)?;
        }
        rt.render(&mut out[WINDOW..])?;
        out.drain(..WINDOW);
        Ok(out)
    };
    let plain = run(None)?;
    let bent = run(Some(0x20E1_0000 | (8533 & 127) << 8 | 8533 >> 7))?;
    let pressed = run(Some(0x20D1_0000 | 127 << 8))?;
    let crossings = |x: &[[f32; 2]]| {
        x.windows(2)
            .filter(|w| (w[0][0] < 0.0) != (w[1][0] < 0.0))
            .count() as f64
    };
    let rms = |x: &[[f32; 2]]| {
        let s: f64 = x
            .iter()
            .map(|f| f64::from(f[0]).powi(2) + f64::from(f[1]).powi(2))
            .sum();
        (s / (2 * x.len()) as f64).sqrt()
    };
    Ok(MpeResponse {
        pitch_ratio: crossings(&bent) / crossings(&plain).max(1.0),
        pressure_db: 20.0 * ((rms(&pressed) + 1e-12) / (rms(&plain) + 1e-12)).log10(),
    })
}
