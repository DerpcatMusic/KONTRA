//! Auto-align as latency compensation: each part's attack delay is
//! measured once, the latest sets the latency the plugin reports, and every
//! other part is held by the difference, so all of them sound on the beat
//! in playback. Attacks are never trimmed.

use std::sync::Arc;

use kontra_align::{Delay, Step, Timing};
use kontra_host::align::{Measured, hold};
use kontra_host::rack::{Delays, Hold, Plan, Rack};
use kontra_protocol::MAX_PARTS;

/// The note the measurement plays.
// ponytail: one probe key; a patch silent on C4 measures as on time.
const PROBE_KEY: u8 = 60;

/// Measures the parts not measured yet and sends each its holds; with `on`
/// false, nothing is held. The latency to report, in frames.
// ponytail: every hold is sent each tick (the audio side ignores unchanged
// ones), so a part swapped in is never left with a stale hold.
pub fn update(on: bool, rack: &mut Rack) -> u32 {
    let rate = f64::from(rack.rate());
    // None: an empty part.
    let mut timings: [Option<Timing>; MAX_PARTS] = Default::default();
    for (n, timing) in timings.iter_mut().enumerate() {
        let Some(Some(part)) = rack.parts().get(n) else {
            continue;
        };
        let delays = match (part.delays.clone(), part.plan.clone()) {
            (Some(delays), _) => Some(delays),
            (None, Some(plan)) if on => {
                let delays = measure(plan, rack);
                rack.set_delays(n, delays.clone());
                Some(delays)
            }
            _ => None,
        };
        let steps = (delays.unwrap_or_default().into_iter())
            .map(|(top, first, legato)| Step { top, first, legato })
            .collect();
        *timing = Some(Timing {
            loaded: Delay { steps },
            ..Timing::default()
        });
    }
    let reported = kontra_align::reported_ms(timings.iter().flatten().map(Timing::latest));
    for (n, timing) in timings.iter().enumerate() {
        if let Some(timing) = timing {
            let hold = if on {
                hold(timing, reported, rate)
            } else {
                Hold::flat(0)
            };
            rack.set_hold(n, hold);
        }
    }
    if on {
        (f64::from(reported) * rate / 1000.0).round() as u32
    } else {
        0
    }
}

/// How late `plan`'s notes sound as it loads; it plays a fresh engine of
/// its own, offline, so the part playing is not touched.
// ponytail: the patch as loaded; a keyswitched articulation slower than it
// plays late by the difference (never trimmed). Measure per switch row and
// follow the engine's keyswitch if a library needs it.
fn measure(plan: Arc<Plan>, rack: &mut Rack) -> Delays {
    let mut player = Measured::new(plan, rack.store_mut().client(), rack.rate(), Vec::new());
    let d = kontra_align::measure(&mut player, 0, PROBE_KEY).loaded;
    d.steps.iter().map(|s| (s.top, s.first, s.legato)).collect()
}
