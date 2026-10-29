//! Auto-align as latency compensation: each part's attack delay is
//! measured once, the latest sets the latency the plugin reports, and every
//! other part is held by the difference, so all of them sound on the beat
//! in playback. Attacks are never trimmed.

use std::sync::Arc;

use kontra_host::align::Measured;
use kontra_host::rack::{Plan, Rack};
use kontra_protocol::MAX_PARTS;

/// The note the measurement plays.
// ponytail: one probe key; a patch silent on C4 measures as on time.
const PROBE_KEY: u8 = 60;

/// Measures the parts not measured yet and sends each its hold; with `on`
/// false, nothing is held. The latency to report, in frames.
// ponytail: every hold is sent each tick (the audio side ignores unchanged
// ones), so a part swapped in is never left with a stale hold.
pub fn update(on: bool, rack: &mut Rack) -> u32 {
    let rate = f64::from(rack.rate());
    // None: an empty part.
    let mut latest: [Option<f32>; MAX_PARTS] = [None; MAX_PARTS];
    for (n, ms) in latest.iter_mut().enumerate() {
        let Some(Some(part)) = rack.parts().get(n) else {
            continue;
        };
        *ms = Some(match (part.delay_ms, part.plan.clone()) {
            (Some(ms), _) => ms,
            (None, Some(plan)) if on => {
                let ms = measure(plan, rack);
                rack.set_delay(n, ms);
                ms
            }
            _ => 0.0,
        });
    }
    let reported = kontra_align::reported_ms(latest.iter().flatten().copied());
    let frames = |ms: f32| (f64::from(ms) * rate / 1000.0).round() as u32;
    for (n, ms) in latest.iter().enumerate() {
        if let Some(ms) = ms {
            rack.set_hold(n, if on { frames(reported - ms) } else { 0 });
        }
    }
    if on { frames(reported) } else { 0 }
}

/// How late `plan` sounds at its latest, in ms; it plays a fresh engine of
/// its own, offline, so the part playing is not touched.
fn measure(plan: Arc<Plan>, rack: &mut Rack) -> f32 {
    let switches: Vec<u8> = plan
        .switches
        .iter()
        .flat_map(|r| r.clone())
        .take(16)
        .collect();
    let rows = switches.len();
    let mut player = Measured::new(plan, rack.store_mut().client(), rack.rate(), switches);
    kontra_align::measure(&mut player, rows, PROBE_KEY).latest()
}
