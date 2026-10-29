//! Auto-align as latency compensation: each part's attack delay is
//! measured once, the latest sets the latency the plugin reports, and every
//! other part is held by the difference, so all of them sound on the beat
//! in playback. Attacks are never trimmed.

use std::sync::{Arc, Weak};

use kontra_host::align::Measured;
use kontra_host::rack::{Plan, Rack};
use kontra_protocol::MAX_PARTS;

/// The note the measurement plays.
// ponytail: one probe key; a patch silent on C4 measures as on time.
const PROBE_KEY: u8 = 60;

/// What the control tick knows of each part's timing.
#[derive(Default)]
pub struct Align {
    /// By part: the plan measured and the latest it sounds, in ms.
    measured: Vec<Option<(Weak<Plan>, f32)>>,
    /// By part: the hold last sent, in frames.
    holds: Vec<u32>,
}

impl Align {
    /// Measures parts loaded since the last call and sends each its hold;
    /// with `on` false, nothing is held. The latency to report, in frames.
    pub fn update(&mut self, on: bool, rack: &mut Rack) -> u32 {
        self.measured.resize_with(MAX_PARTS, || None);
        self.holds.resize(MAX_PARTS, 0);
        let rate = f64::from(rack.rate());
        let plans: Vec<Option<Arc<Plan>>> = (0..MAX_PARTS)
            .map(|n| rack.parts().get(n)?.as_ref()?.plan.clone())
            .collect();
        let mut latest = [0.0f32; MAX_PARTS];
        for (n, plan) in plans.into_iter().enumerate() {
            let (Some(plan), Some(slot), Some(ms)) =
                (plan, self.measured.get_mut(n), latest.get_mut(n))
            else {
                continue;
            };
            if !on {
                continue;
            }
            let fresh = slot
                .as_ref()
                .and_then(|(seen, _)| seen.upgrade())
                .is_some_and(|seen| Arc::ptr_eq(&seen, &plan));
            if !fresh {
                *slot = Some((Arc::downgrade(&plan), measure(plan, rack)));
            }
            *ms = slot.as_ref().map_or(0.0, |(_, ms)| *ms);
        }
        let present: Vec<bool> = (0..MAX_PARTS)
            .map(|n| rack.parts().get(n).is_some_and(Option::is_some))
            .collect();
        let reported = kontra_align::reported_ms(
            latest
                .iter()
                .zip(&present)
                .filter(|(_, p)| **p)
                .map(|(ms, _)| *ms),
        );
        for (n, (ms, sent)) in latest.iter().zip(&mut self.holds).enumerate() {
            let hold = if on {
                (f64::from(reported - ms) * rate / 1000.0).round() as u32
            } else {
                0
            };
            if hold != *sent {
                rack.set_hold(n, hold);
                *sent = hold;
            }
        }
        if on {
            (f64::from(reported) * rate / 1000.0).round() as u32
        } else {
            0
        }
    }
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
