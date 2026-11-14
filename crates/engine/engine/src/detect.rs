//! `detect_*`: the music information a zone's sample was measured for at
//! load ([`crate::Analysis`]), written into the script's variable. In
//! `on init` the answer is at once (-1); elsewhere the command completes
//! through `on async_complete` like every KSP command that runs a while
//! (KSP manual, "Music Information Retrieval").

use kontra_vm::{Action, detect};

use crate::queue::Exit;
use crate::{Analysis, Engine};

impl Engine {
    /// Runs `action` for zone `zone`, storing the result in `cells[cell]`
    /// (the bits of an `f64` when `real`). A zone that is not there or has
    /// no analysis reads as the command's "invalid" and fails its async.
    pub(crate) fn detect(
        &mut self,
        action: Action,
        zone: i32,
        cell: usize,
        real: bool,
        cells: &mut [i64],
    ) -> i32 {
        let found = self
            .plan
            .zone_facts
            .iter()
            .find(|z| i64::from(z.id) == i64::from(zone))
            .and_then(|z| z.analysis.clone());
        let ok = found.is_some();
        let a = found.as_deref();
        let number = |v: Option<f64>, invalid: f64| {
            let v = v.unwrap_or(invalid);
            if real { v.to_bits() as i64 } else { v as i64 }
        };
        let value = match action {
            Action::DetectPeak => number(a.map(|a| a.peak_db), detect::INVALID_PEAK),
            Action::DetectRms => number(a.map(|a| a.rms_db), detect::INVALID_RMS),
            Action::DetectLoudness => number(a.map(|a| a.loudness_lkfs), detect::INVALID_LOUDNESS),
            Action::DetectPitch => number(a.and_then(|a| a.pitch), detect::INVALID_PITCH),
            Action::DetectTempo => number(a.and_then(|a| a.tempo), detect::INVALID_TEMPO),
            _ => i64::from(tag(action, a)),
        };
        if let Some(slot) = cells.get_mut(cell) {
            *slot = value;
        }
        let exit = if ok { Exit::Done } else { Exit::Failed };
        if self.building {
            -1
        } else {
            self.start_async(exit)
        }
    }
}

/// The `$NI_DETECT_*` tag for the integer-valued commands.
fn tag(action: Action, a: Option<&Analysis>) -> i32 {
    match action {
        Action::DetectKey => a.and_then(|a| a.key).map_or(detect::KEY_INVALID, detect::key),
        Action::DetectSampleType => match a.and_then(|a| a.is_drum) {
            Some(true) => detect::SAMPLE_DRUM,
            Some(false) => detect::SAMPLE_INSTRUMENT,
            None => detect::SAMPLE_INVALID,
        },
        Action::DetectDrumType => a
            .and_then(|a| a.drum_type)
            .map_or(detect::DRUM_INVALID, detect::drum),
        _ => a
            .and_then(|a| a.instrument_type)
            .map_or(detect::INSTRUMENT_INVALID, detect::instrument),
    }
}
