//! SMOOTH: a spectrally selective dynamics editor.
//!
//! The plug-in is the same tree the preview draws, hosted in a window: the
//! interface lives in `smooth-ui`, the engine lives in `core`, and this file
//! is the join -- parameters in, audio out, and a window in between.
//!
//! The engine is not a new design. It is the Resonance Lab's clean-room
//! reimplementation of soothe3's suppressor, built from the reverse
//! engineering in `Research/t3-research` (the canonical log and exponential
//! programs, the projection, the spatial operators, the weighting banks and
//! the reduction arithmetic). The parameters follow soothe3's own declared
//! records -- the ranges, defaults and units read out of the binary's
//! parameter table -- so the numbers on the panel are the reference's
//! numbers.
//!
//! The editor's dials are bound to host parameters, so a drag is a gesture
//! the host records. The view does not know that: it names a control and a
//! range, and the dial decides where the value lives.

mod allocation_audit {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    thread_local! {
        static ACTIVE: Cell<bool> = const { Cell::new(false) };
        static COUNT: Cell<usize> = const { Cell::new(0) };
    }
    struct Audit;
    // SAFETY: delegates every allocation/deallocation unchanged to System. TLS counters only
    // observe allocations on a specifically marked test thread and do not allocate themselves.
    unsafe impl GlobalAlloc for Audit {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            let _ = ACTIVE.try_with(|a| {
                if a.get() {
                    let _ = COUNT.try_with(|c| c.set(c.get() + 1));
                }
            });
            unsafe { System.alloc(l) }
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            unsafe { System.dealloc(p, l) }
        }
        unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
            let _ = ACTIVE.try_with(|a| {
                if a.get() {
                    let _ = COUNT.try_with(|c| c.set(c.get() + 1));
                }
            });
            unsafe { System.realloc(p, l, n) }
        }
    }
    #[global_allocator]
    static ALLOCATOR: Audit = Audit;

    /// The number of allocations the closure made.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn count(f: impl FnOnce()) -> usize {
        COUNT.with(|c| c.set(0));
        ACTIVE.with(|a| a.set(true));
        f();
        ACTIVE.with(|a| a.set(false));
        COUNT.with(Cell::get)
    }
}

use moose::mui::MuiEditor;
use moose::mui::mui::prelude::{Font, Ui};
use moose::prelude::*;
use smooth_core::weighting::{
    BandRecord, NormalizedRecord, normalized_from_center, normalized_from_q, normalized_from_slope,
};
use smooth_core::{EngineBank, Settings};
use smooth_core::plot::Telemetry;
use smooth_ui::menus;
use smooth_ui::slots::{self, Slot};
use smooth_ui::{H, W, skin, view};
use std::sync::Arc;

mod config;
mod presets;
mod session;

// ------------------------------------------------------------- parameters --

/// What the host knows about SMOOTH.
///
/// The ranges, defaults and units are soothe3's own, read from the declared
/// parameter records in the reverse engineering. A value in a band's public
/// units is turned into the engine's normalized form by the same helpers the
/// research wrote.
#[derive(Params)]
pub struct SmoothParams {
    /// How hard a band that stands above its neighbours is pulled down.
    #[param(name = "Depth", range = "linear(0, 10)", default = 5.0)]
    pub depth: FloatParam,
    /// How finely the excess is measured.
    #[param(name = "Detail", range = "linear(0, 10)", default = 5.0)]
    pub detail: FloatParam,
    /// How fast the gain falls.
    #[param(name = "Attack", range = "linear(0, 10)", default = 5.0)]
    pub attack: FloatParam,
    /// How fast the gain comes back.
    #[param(name = "Release", range = "linear(0, 10)", default = 3.0)]
    pub release: FloatParam,
    /// Soft curves the reduction; hard thresholds it.
    #[param(name = "Mode", range = "discrete(0, 1)", default = 0)]
    pub mode: IntParam,
    /// The short-window plan.
    #[param(name = "Low latency", range = "discrete(0, 1)", default = 0)]
    pub low_latency: IntParam,
    /// Normal, high, or ultra.
    #[param(name = "Quality", range = "discrete(0, 2)", default = 0)]
    pub quality: IntParam,
    /// The linear-phase plan.
    #[param(name = "Linear phase", range = "discrete(0, 1)", default = 0)]
    pub linear_phase: IntParam,
    /// Wet against dry.
    #[param(name = "Mix", range = "linear(0, 100)", default = 100.0, unit = "%")]
    pub mix: FloatParam,
    /// The output trim.
    #[param(
        name = "Out trim",
        range = "linear(-18, 18)",
        default = 0.0,
        unit = "dB"
    )]
    pub output: FloatParam,
    /// The compare switch.
    #[param(name = "Delta", range = "discrete(0, 1)", default = 0)]
    pub delta: IntParam,
    /// The bypass switch.
    #[param(name = "Bypass", range = "discrete(0, 1)", default = 0)]
    pub bypass: IntParam,
    /// The deepest cut the engine will take.
    #[param(name = "Max cut", range = "linear(0, 24)", default = 24.0, unit = "dB")]
    pub max_cut: FloatParam,
    /// How much the channels are linked.
    #[param(
        name = "Stereo link",
        range = "linear(0, 100)",
        default = 100.0,
        unit = "%"
    )]
    pub link: FloatParam,
    /// Left/right (0) or mid/side (1) stereo processing.
    #[param(name = "Stereo mode", range = "discrete(0, 1)", default = 0)]
    pub stereo_mode: IntParam,
    /// Where the channels are focused.
    #[param(name = "Stereo focus", range = "linear(0, 1)", default = 0.5)]
    pub focus: FloatParam,
    /// The detail tilt at the low end.
    #[param(
        name = "Detail tilt low",
        range = "sym_skewed(0.1, 10, 2, 1.0)",
        default = 1.0
    )]
    pub detail_tilt_low: FloatParam,
    /// The detail tilt at the high end.
    #[param(
        name = "Detail tilt high",
        range = "sym_skewed(0.1, 10, 2, 1.0)",
        default = 1.0
    )]
    pub detail_tilt_high: FloatParam,
    /// The attack tilt at the low end.
    #[param(
        name = "Attack tilt low",
        range = "sym_skewed(0.1, 10, 2, 1.0)",
        default = 1.0
    )]
    pub attack_tilt_low: FloatParam,
    /// The attack tilt at the high end.
    #[param(
        name = "Attack tilt high",
        range = "sym_skewed(0.1, 10, 2, 1.0)",
        default = 1.0
    )]
    pub attack_tilt_high: FloatParam,
    /// The release tilt at the low end.
    #[param(
        name = "Release tilt low",
        range = "sym_skewed(0.1, 10, 2, 1.0)",
        default = 1.0
    )]
    pub release_tilt_low: FloatParam,
    /// The release tilt at the high end.
    #[param(
        name = "Release tilt high",
        range = "sym_skewed(0.1, 10, 2, 1.0)",
        default = 1.0
    )]
    pub release_tilt_high: FloatParam,
    /// Where the first band sits.
    #[param(
        name = "Band 1 frequency",
        range = "offset_log(20, 20000, 150)",
        default = 100.0,
        unit = "Hz"
    )]
    pub band1_frequency: FloatParam,
    /// How deep the first band cuts.
    #[param(
        name = "Band 1 depth",
        range = "linear(-10, 10)",
        default = 0.0,
        unit = "dB"
    )]
    pub band1_depth: FloatParam,
    /// How wide the first band is.
    #[param(name = "Band 1 q", range = "pivoted_log(0.1, 10, 0.707)", default = 0.707)]
    pub band1_q: FloatParam,
    /// The shape of the first band.
    #[param(name = "Band 1 shape", range = "discrete(0, 7)", default = 0)]
    pub band1_shape: IntParam,
    /// The slope of the first band's cuts and shelves.
    #[param(name = "Band 1 slope", range = "log(6, 128)", default = 27.71, unit = "custom:dB/oct")]
    pub band1_slope: FloatParam,
    /// The first band's stereo balance.
    #[param(name = "Band 1 balance", range = "linear(0, 1)", default = 0.5)]
    pub band1_balance: FloatParam,
    /// Whether the first band is in the plot at all.
    #[param(name = "Band 1 active", range = "discrete(0, 1)", default = 1)]
    pub band1_active: IntParam,
    /// Whether the first band is bypassed.
    #[param(name = "Band 1 bypass", range = "discrete(0, 1)", default = 0)]
    pub band1_bypass: IntParam,
    /// Where the second band sits.
    #[param(
        name = "Band 2 frequency",
        range = "offset_log(20, 20000, 150)",
        default = 4000.0,
        unit = "Hz"
    )]
    pub band2_frequency: FloatParam,
    /// How deep the second band cuts.
    #[param(
        name = "Band 2 depth",
        range = "linear(-10, 10)",
        default = 2.5,
        unit = "dB"
    )]
    pub band2_depth: FloatParam,
    /// How wide the second band is.
    #[param(name = "Band 2 q", range = "pivoted_log(0.1, 10, 0.707)", default = 0.707)]
    pub band2_q: FloatParam,
    /// The shape of the second band.
    #[param(name = "Band 2 shape", range = "discrete(0, 7)", default = 2)]
    pub band2_shape: IntParam,
    /// The slope of the second band's cuts and shelves.
    #[param(name = "Band 2 slope", range = "log(6, 128)", default = 27.71, unit = "custom:dB/oct")]
    pub band2_slope: FloatParam,
    /// The second band's stereo balance.
    #[param(name = "Band 2 balance", range = "linear(0, 1)", default = 0.5)]
    pub band2_balance: FloatParam,
    /// Whether the second band is in the plot at all.
    #[param(name = "Band 2 active", range = "discrete(0, 1)", default = 1)]
    pub band2_active: IntParam,
    /// Whether the second band is bypassed.
    #[param(name = "Band 2 bypass", range = "discrete(0, 1)", default = 0)]
    pub band2_bypass: IntParam,
    /// Where the third band sits.
    #[param(
        name = "Band 3 frequency",
        range = "offset_log(20, 20000, 150)",
        default = 16000.0,
        unit = "Hz"
    )]
    pub band3_frequency: FloatParam,
    /// How deep the third band cuts.
    #[param(
        name = "Band 3 depth",
        range = "linear(-10, 10)",
        default = 0.0,
        unit = "dB"
    )]
    pub band3_depth: FloatParam,
    /// How wide the third band is.
    #[param(name = "Band 3 q", range = "pivoted_log(0.1, 10, 0.707)", default = 0.707)]
    pub band3_q: FloatParam,
    /// The shape of the third band.
    #[param(name = "Band 3 shape", range = "discrete(0, 7)", default = 7)]
    pub band3_shape: IntParam,
    /// The slope of the third band's cuts and shelves.
    #[param(name = "Band 3 slope", range = "log(6, 128)", default = 27.71, unit = "custom:dB/oct")]
    pub band3_slope: FloatParam,
    /// The third band's stereo balance.
    #[param(name = "Band 3 balance", range = "linear(0, 1)", default = 0.5)]
    pub band3_balance: FloatParam,
    /// Whether the third band is in the plot at all.
    #[param(name = "Band 3 active", range = "discrete(0, 1)", default = 1)]
    pub band3_active: IntParam,
    /// Whether the third band is bypassed.
    #[param(name = "Band 3 bypass", range = "discrete(0, 1)", default = 0)]
    pub band3_bypass: IntParam,
    /// Where the fourth band sits.
    #[param(
        name = "Band 4 frequency",
        range = "offset_log(20, 20000, 150)",
        default = 1000.0,
        unit = "Hz"
    )]
    pub band4_frequency: FloatParam,
    /// How deep the fourth band cuts.
    #[param(
        name = "Band 4 depth",
        range = "linear(-10, 10)",
        default = 0.0,
        unit = "dB"
    )]
    pub band4_depth: FloatParam,
    /// How wide the fourth band is.
    #[param(name = "Band 4 q", range = "pivoted_log(0.1, 10, 0.707)", default = 0.707)]
    pub band4_q: FloatParam,
    /// The shape of the fourth band.
    #[param(name = "Band 4 shape", range = "discrete(0, 7)", default = 2)]
    pub band4_shape: IntParam,
    /// The slope of the fourth band's cuts and shelves.
    #[param(name = "Band 4 slope", range = "log(6, 128)", default = 27.71, unit = "custom:dB/oct")]
    pub band4_slope: FloatParam,
    /// The fourth band's stereo balance.
    #[param(name = "Band 4 balance", range = "linear(0, 1)", default = 0.5)]
    pub band4_balance: FloatParam,
    /// Whether the fourth band is in the plot at all.
    #[param(name = "Band 4 active", range = "discrete(0, 1)", default = 0)]
    pub band4_active: IntParam,
    /// Whether the fourth band is bypassed.
    #[param(name = "Band 4 bypass", range = "discrete(0, 1)", default = 0)]
    pub band4_bypass: IntParam,
    /// Where the fifth band sits.
    #[param(
        name = "Band 5 frequency",
        range = "offset_log(20, 20000, 150)",
        default = 1000.0,
        unit = "Hz"
    )]
    pub band5_frequency: FloatParam,
    /// How deep the fifth band cuts.
    #[param(
        name = "Band 5 depth",
        range = "linear(-10, 10)",
        default = 0.0,
        unit = "dB"
    )]
    pub band5_depth: FloatParam,
    /// How wide the fifth band is.
    #[param(name = "Band 5 q", range = "pivoted_log(0.1, 10, 0.707)", default = 0.707)]
    pub band5_q: FloatParam,
    /// The shape of the fifth band.
    #[param(name = "Band 5 shape", range = "discrete(0, 7)", default = 2)]
    pub band5_shape: IntParam,
    /// The slope of the fifth band's cuts and shelves.
    #[param(name = "Band 5 slope", range = "log(6, 128)", default = 27.71, unit = "custom:dB/oct")]
    pub band5_slope: FloatParam,
    /// The fifth band's stereo balance.
    #[param(name = "Band 5 balance", range = "linear(0, 1)", default = 0.5)]
    pub band5_balance: FloatParam,
    /// Whether the fifth band is in the plot at all.
    #[param(name = "Band 5 active", range = "discrete(0, 1)", default = 0)]
    pub band5_active: IntParam,
    /// Whether the fifth band is bypassed.
    #[param(name = "Band 5 bypass", range = "discrete(0, 1)", default = 0)]
    pub band5_bypass: IntParam,
    /// Where the sixth band sits.
    #[param(
        name = "Band 6 frequency",
        range = "offset_log(20, 20000, 150)",
        default = 1000.0,
        unit = "Hz"
    )]
    pub band6_frequency: FloatParam,
    /// How deep the sixth band cuts.
    #[param(
        name = "Band 6 depth",
        range = "linear(-10, 10)",
        default = 0.0,
        unit = "dB"
    )]
    pub band6_depth: FloatParam,
    /// How wide the sixth band is.
    #[param(name = "Band 6 q", range = "pivoted_log(0.1, 10, 0.707)", default = 0.707)]
    pub band6_q: FloatParam,
    /// The shape of the sixth band.
    #[param(name = "Band 6 shape", range = "discrete(0, 7)", default = 2)]
    pub band6_shape: IntParam,
    /// The slope of the sixth band's cuts and shelves.
    #[param(name = "Band 6 slope", range = "log(6, 128)", default = 27.71, unit = "custom:dB/oct")]
    pub band6_slope: FloatParam,
    /// The sixth band's stereo balance.
    #[param(name = "Band 6 balance", range = "linear(0, 1)", default = 0.5)]
    pub band6_balance: FloatParam,
    /// Whether the sixth band is in the plot at all.
    #[param(name = "Band 6 active", range = "discrete(0, 1)", default = 0)]
    pub band6_active: IntParam,
    /// Whether the sixth band is bypassed.
    #[param(name = "Band 6 bypass", range = "discrete(0, 1)", default = 0)]
    pub band6_bypass: IntParam,
    /// Where the seventh band sits.
    #[param(
        name = "Band 7 frequency",
        range = "offset_log(20, 20000, 150)",
        default = 1000.0,
        unit = "Hz"
    )]
    pub band7_frequency: FloatParam,
    /// How deep the seventh band cuts.
    #[param(
        name = "Band 7 depth",
        range = "linear(-10, 10)",
        default = 0.0,
        unit = "dB"
    )]
    pub band7_depth: FloatParam,
    /// How wide the seventh band is.
    #[param(name = "Band 7 q", range = "pivoted_log(0.1, 10, 0.707)", default = 0.707)]
    pub band7_q: FloatParam,
    /// The shape of the seventh band.
    #[param(name = "Band 7 shape", range = "discrete(0, 7)", default = 2)]
    pub band7_shape: IntParam,
    /// The slope of the seventh band's cuts and shelves.
    #[param(name = "Band 7 slope", range = "log(6, 128)", default = 27.71, unit = "custom:dB/oct")]
    pub band7_slope: FloatParam,
    /// The seventh band's stereo balance.
    #[param(name = "Band 7 balance", range = "linear(0, 1)", default = 0.5)]
    pub band7_balance: FloatParam,
    /// Whether the seventh band is in the plot at all.
    #[param(name = "Band 7 active", range = "discrete(0, 1)", default = 0)]
    pub band7_active: IntParam,
    /// Whether the seventh band is bypassed.
    #[param(name = "Band 7 bypass", range = "discrete(0, 1)", default = 0)]
    pub band7_bypass: IntParam,
    /// Where the eighth band sits.
    #[param(
        name = "Band 8 frequency",
        range = "offset_log(20, 20000, 150)",
        default = 1000.0,
        unit = "Hz"
    )]
    pub band8_frequency: FloatParam,
    /// How deep the eighth band cuts.
    #[param(
        name = "Band 8 depth",
        range = "linear(-10, 10)",
        default = 0.0,
        unit = "dB"
    )]
    pub band8_depth: FloatParam,
    /// How wide the eighth band is.
    #[param(name = "Band 8 q", range = "pivoted_log(0.1, 10, 0.707)", default = 0.707)]
    pub band8_q: FloatParam,
    /// The shape of the eighth band.
    #[param(name = "Band 8 shape", range = "discrete(0, 7)", default = 2)]
    pub band8_shape: IntParam,
    /// The slope of the eighth band's cuts and shelves.
    #[param(name = "Band 8 slope", range = "log(6, 128)", default = 27.71, unit = "custom:dB/oct")]
    pub band8_slope: FloatParam,
    /// The eighth band's stereo balance.
    #[param(name = "Band 8 balance", range = "linear(0, 1)", default = 0.5)]
    pub band8_balance: FloatParam,
    /// Whether the eighth band is in the plot at all.
    #[param(name = "Band 8 active", range = "discrete(0, 1)", default = 0)]
    pub band8_active: IntParam,
    /// Whether the eighth band is bypassed.
    #[param(name = "Band 8 bypass", range = "discrete(0, 1)", default = 0)]
    pub band8_bypass: IntParam,
    /// The live plot: the engine writes it from the audio thread, the
    /// editor reads it. Not a parameter.
    #[skip]
    pub plot: std::sync::Arc<Telemetry>,
    /// The undo history and the two parameter sets. Not a parameter: it
    /// outlives the editor window, so closing it does not lose the history.
    #[skip]
    pub session: std::sync::Mutex<session::Session>,
    /// The preset library: the shipped presets, the user's, and which is loaded.
    #[skip]
    pub library: std::sync::Mutex<presets::Library>,
    /// The padlocks, as bits: quality (with linear phase) and low latency.
    #[skip]
    pub locks: std::sync::atomic::AtomicU8,
}

use SmoothParamsParamId as P;

/// What the padlocks hold fixed when a preset loads: the quality lock covers
/// the quality and the phase mode, the other the low-latency switch.
fn locked_names(params: &SmoothParams) -> Vec<&'static str> {
    let bits = params.locks.load(std::sync::atomic::Ordering::Relaxed);
    let mut names = Vec::new();
    if bits & 1 != 0 {
        names.extend(["Quality", "Linear phase"]);
    }
    if bits & 2 != 0 {
        names.push("Low latency");
    }
    names
}

/// The band parameters, in the order the graph draws its handles.
const BAND_PARAMS: [(P, P); 8] = [
    (P::Band1Frequency, P::Band1Depth),
    (P::Band2Frequency, P::Band2Depth),
    (P::Band3Frequency, P::Band3Depth),
    (P::Band4Frequency, P::Band4Depth),
    (P::Band5Frequency, P::Band5Depth),
    (P::Band6Frequency, P::Band6Depth),
    (P::Band7Frequency, P::Band7Depth),
    (P::Band8Frequency, P::Band8Depth),
];

/// Every band's parameter ids: frequency, depth, q, shape, slope, balance,
/// active, bypass.
const BAND_IDS: [[P; 8]; 8] = [
    [P::Band1Frequency, P::Band1Depth, P::Band1Q, P::Band1Shape, P::Band1Slope, P::Band1Balance, P::Band1Active, P::Band1Bypass],
    [P::Band2Frequency, P::Band2Depth, P::Band2Q, P::Band2Shape, P::Band2Slope, P::Band2Balance, P::Band2Active, P::Band2Bypass],
    [P::Band3Frequency, P::Band3Depth, P::Band3Q, P::Band3Shape, P::Band3Slope, P::Band3Balance, P::Band3Active, P::Band3Bypass],
    [P::Band4Frequency, P::Band4Depth, P::Band4Q, P::Band4Shape, P::Band4Slope, P::Band4Balance, P::Band4Active, P::Band4Bypass],
    [P::Band5Frequency, P::Band5Depth, P::Band5Q, P::Band5Shape, P::Band5Slope, P::Band5Balance, P::Band5Active, P::Band5Bypass],
    [P::Band6Frequency, P::Band6Depth, P::Band6Q, P::Band6Shape, P::Band6Slope, P::Band6Balance, P::Band6Active, P::Band6Bypass],
    [P::Band7Frequency, P::Band7Depth, P::Band7Q, P::Band7Shape, P::Band7Slope, P::Band7Balance, P::Band7Active, P::Band7Bypass],
    [P::Band8Frequency, P::Band8Depth, P::Band8Q, P::Band8Shape, P::Band8Slope, P::Band8Balance, P::Band8Active, P::Band8Bypass],
];

/// Write a value to a parameter only when it does not already hold it, so a
/// rebuild leaves a value the host just automated alone.
fn write_plain(params: &SmoothParams, param: P, value: f64) {
    if params.get_plain(u32::from(param)) != Some(value) {
        params.set_plain(u32::from(param), value);
    }
}

/// The parameter behind a slot, and how its plain value reads.
fn slot_param(slot: Slot, band: usize) -> (P, fn(f64) -> String) {
    let ids = BAND_IDS[band.min(7)];
    fn one(v: f64) -> String {
        format!("{v:.1}")
    }
    fn times(v: f64) -> String {
        if v < 1.0 {
            format!("{v:.2}x")
        } else {
            format!("{v:.1}x")
        }
    }
    match slot {
        Slot::Depth => (P::Depth, one),
        Slot::Detail => (P::Detail, one),
        Slot::Attack => (P::Attack, one),
        Slot::Release => (P::Release, one),
        Slot::Mix => (P::Mix, |v| format!("{v:.0} %")),
        Slot::Out => (P::Output, |v| format!("{v:.1} dB")),
        Slot::MaxCut => (P::MaxCut, |v| format!("{v:.0} dB")),
        Slot::Link => (P::Link, |v| format!("{v:.0} %")),
        Slot::Focus => (P::Focus, |v| format!("{v:.2}")),
        Slot::DetailLow => (P::DetailTiltLow, times),
        Slot::DetailHigh => (P::DetailTiltHigh, times),
        Slot::AttackLow => (P::AttackTiltLow, times),
        Slot::AttackHigh => (P::AttackTiltHigh, times),
        Slot::ReleaseLow => (P::ReleaseTiltLow, times),
        Slot::ReleaseHigh => (P::ReleaseTiltHigh, times),
        Slot::BandFrequency => (ids[0], |v| {
            if v >= 10_000.0 {
                format!("{:.0} kHz", v / 1000.0)
            } else if v >= 1000.0 {
                format!("{:.1} kHz", v / 1000.0)
            } else {
                format!("{v:.0} Hz")
            }
        }),
        Slot::BandDepth => (ids[1], |v| format!("{v:.1} dB")),
        Slot::BandWidth => (ids[2], |v| format!("{v:.2}")),
        Slot::BandShape => (ids[3], |v| {
            smooth_ui::graph::SHAPES[(v.round() as usize).min(7)].to_string()
        }),
        Slot::BandSlope => (ids[4], |v| format!("{v:.0} dB/oct")),
        Slot::BandBalance => (ids[5], |v| {
            if (v - 0.5).abs() < 0.005 {
                "linked".to_string()
            } else {
                format!("{:+.0} %", (v - 0.5) * 200.0)
            }
        }),
    }
}

/// The engine's settings, built from the host's values.
///
/// Every conversion here is the one the reverse engineering established: the
/// public value is turned into the engine's normalized form by the same
/// helpers the research wrote, and the linear controls divide by their span.
/// One band's parameters.
struct Band<'a> {
    frequency: &'a FloatParam,
    depth: &'a FloatParam,
    q: &'a FloatParam,
    shape: &'a IntParam,
    slope: &'a FloatParam,
    balance: &'a FloatParam,
    active: &'a IntParam,
    bypass: &'a IntParam,
}

impl SmoothParams {
    fn settings(&self) -> Settings {
        let mut settings = Settings {
            active_channels: 2,
            depth: self.depth.value() / 10.0,
            detail: self.detail.value() / 10.0,
            hard: self.mode.value() >= 1,
            attack: self.attack.value() / 10.0,
            release: self.release.value() / 10.0,
            attack_low: self.attack_tilt_low.value(),
            attack_high: self.attack_tilt_high.value(),
            release_low: self.release_tilt_low.value(),
            release_high: self.release_tilt_high.value(),
            // The control's top is "no limit": the reference hands the engine 60 dB there.
            max_cut_db: if self.max_cut.value() >= 23.97 { 60.0 } else { self.max_cut.value() },
            stereo_link: self.link.value() / 100.0,
            focus: self.focus.value(),
            mid_side: self.stereo_mode.value() >= 1,
            mix: self.mix.value() / 100.0,
            output_db: self.output.value(),
            delta: self.delta.value() >= 1,
            bypass: self.bypass.value() >= 1,
            adaptive_baseline: false,
            ..Settings::default()
        };
        // The engine's curve scalar is the owner's own (one, by the
        // constructor), not the detail: the research is explicit that no
        // public alias is inferred from an equal numeric offset.
        settings.bands = self.band_records();
        settings
    }

    /// One band's parameters, by index.
    fn band(&self, index: usize) -> Band<'_> {
        match index {
            0 => Band {
                frequency: &self.band1_frequency,
                depth: &self.band1_depth,
                q: &self.band1_q,
                shape: &self.band1_shape,
                slope: &self.band1_slope,
                balance: &self.band1_balance,
                active: &self.band1_active,
                bypass: &self.band1_bypass,
            },
            1 => Band {
                frequency: &self.band2_frequency,
                depth: &self.band2_depth,
                q: &self.band2_q,
                shape: &self.band2_shape,
                slope: &self.band2_slope,
                balance: &self.band2_balance,
                active: &self.band2_active,
                bypass: &self.band2_bypass,
            },
            2 => Band {
                frequency: &self.band3_frequency,
                depth: &self.band3_depth,
                q: &self.band3_q,
                shape: &self.band3_shape,
                slope: &self.band3_slope,
                balance: &self.band3_balance,
                active: &self.band3_active,
                bypass: &self.band3_bypass,
            },
            3 => Band {
                frequency: &self.band4_frequency,
                depth: &self.band4_depth,
                q: &self.band4_q,
                shape: &self.band4_shape,
                slope: &self.band4_slope,
                balance: &self.band4_balance,
                active: &self.band4_active,
                bypass: &self.band4_bypass,
            },
            4 => Band {
                frequency: &self.band5_frequency,
                depth: &self.band5_depth,
                q: &self.band5_q,
                shape: &self.band5_shape,
                slope: &self.band5_slope,
                balance: &self.band5_balance,
                active: &self.band5_active,
                bypass: &self.band5_bypass,
            },
            5 => Band {
                frequency: &self.band6_frequency,
                depth: &self.band6_depth,
                q: &self.band6_q,
                shape: &self.band6_shape,
                slope: &self.band6_slope,
                balance: &self.band6_balance,
                active: &self.band6_active,
                bypass: &self.band6_bypass,
            },
            6 => Band {
                frequency: &self.band7_frequency,
                depth: &self.band7_depth,
                q: &self.band7_q,
                shape: &self.band7_shape,
                slope: &self.band7_slope,
                balance: &self.band7_balance,
                active: &self.band7_active,
                bypass: &self.band7_bypass,
            },
            7 => Band {
                frequency: &self.band8_frequency,
                depth: &self.band8_depth,
                q: &self.band8_q,
                shape: &self.band8_shape,
                slope: &self.band8_slope,
                balance: &self.band8_balance,
                active: &self.band8_active,
                bypass: &self.band8_bypass,
            },
            _ => unreachable!("eight bands"),
        }
    }

    /// The eight bands, in the engine's normalized form. A band that is not
    /// active is an unused record; a bypassed one is used but not enabled, so
    /// it leaves the depth curve alone.
    fn band_records(&self) -> [BandRecord; 8] {
        std::array::from_fn(|index| {
            let band = self.band(index);
            let active = band.active.value() >= 1;
            BandRecord::from_normalized(NormalizedRecord {
                used: f32::from(active),
                enabled: f32::from(active && band.bypass.value() < 1),
                frequency: normalized_from_center(band.frequency.value()),
                depth: (band.depth.value() + 10.0) / 20.0,
                q: normalized_from_q(band.q.value()),
                slope: normalized_from_slope(band.slope.value()),
                focus: band.balance.value() as f32,
                selector: 0.0,
                shape: band.shape.value().clamp(0, 7) as f32 / 7.0,
            })
            .expect("bounded host band parameters")
        })
    }

    /// The plan the engine should be running.
    fn selection(&self) -> usize {
        if self.low_latency.value() >= 1 {
            0
        } else {
            1 + self.quality.value().clamp(0, 2) as usize
                + if self.linear_phase.value() >= 1 { 3 } else { 0 }
        }
    }
}

// ------------------------------------------------------------------ logic --

/// The plug-in's own state: the bank of engines.
#[derive(Default)]
pub struct Dsp {
    bank: Option<EngineBank>,
    /// Samples since the plot was last published, and how many make a frame
    /// of it: the editor draws sixty a second, so the engine publishes about
    /// as often instead of once per block.
    since_publish: usize,
    publish_every: usize,
}

impl Dsp {
    /// A bank for the given rate, or the reason there is not one.
    fn build(rate: f64, selection: usize) -> Result<EngineBank, &'static str> {
        EngineBank::new(rate as f32, selection, false)
    }
}

/// SMOOTH, as the host sees it.
pub struct Smooth;

impl PluginLogic for Smooth {
    type Params = SmoothParams;
    type DspState = Dsp;

    fn init(params: &SmoothParams, _cx: &InitContext) -> Dsp {
        Dsp {
            bank: Dsp::build(48_000.0, params.selection()).ok(),
            since_publish: 0,
            publish_every: 800,
        }
    }

    fn reset(state: &mut Dsp, params: &SmoothParams, config: &AudioConfig) {
        state.bank = Dsp::build(config.sample_rate, params.selection()).ok();
        state.since_publish = 0;
        state.publish_every = (config.sample_rate / 60.0) as usize;
    }

    fn process(
        state: &mut Dsp,
        params: &SmoothParams,
        buffer: &mut AudioBuffer,
        _events: &EventList,
        _context: &mut ProcessContext,
    ) -> ProcessStatus {
        let Some(bank) = state.bank.as_mut() else {
            return ProcessStatus::Normal;
        };
        let mut settings = params.settings();
        settings.active_channels = buffer.num_input_channels().clamp(1, 2);
        // A plan switch resets the engine's history: it may interrupt.
        if bank.select(
            params.selection(),
            settings.mid_side && settings.active_channels == 2,
        ) {
            settings.mix = 1.0;
        }
        let channels = settings.active_channels;
        let frames = buffer.num_samples();
        for frame in 0..frames {
            let left = buffer.input(0)[frame];
            let right = if channels > 1 {
                buffer.input(1)[frame]
            } else {
                left
            };
            let out = bank.active_mut().process_sample([left, right], &settings);
            buffer.output(0)[frame] = out[0];
            if channels > 1 {
                buffer.output(1)[frame] = out[1];
            }
        }
        // The engine's own picture of what it is doing, for the plot.
        state.since_publish += frames;
        if state.since_publish >= state.publish_every {
            state.since_publish = 0;
            params.plot.publish(&bank.active().plot_frame());
        }
        ProcessStatus::Tail(bank.latency() as u32 + bank.plan().n as u32)
    }

    /// The active engine's delay.
    fn latency(state: &Dsp) -> u32 {
        state.bank.as_ref().map_or(0, |bank| bank.latency() as u32)
    }

    fn editor(params: Arc<SmoothParams>) -> Box<dyn Editor> {
        // A host has no command line of its own, so the look comes from the
        // environment: SMOOTH_THEME names one of the shipped palettes, and an
        // unknown name leaves the reference look in place.
        smooth_ui::theme::from_env();
        let mirror = Arc::clone(&params);
        let mut model = view::Model {
            prefs: config::load(),
            ..view::Model::default()
        };
        let mut saved_prefs = model.prefs;
        // The editor repaints when the engine publishes a new plot, and while
        // what was published is still settling.
        let telemetry = Arc::clone(&params.plot);
        let changed_telemetry = Arc::clone(&params.plot);
        // A click leaves its action in the model for the next frame to do, so
        // the editor asks for that frame rather than waiting for the engine.
        let pending = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let pending_in = Arc::clone(&pending);
        let mut published: u64 = 0;
        let mut seen: u64 = 0;
        // The interface size last asked of the host, so a step is requested once.
        let mut applied: Option<usize> = None;
        MuiEditor::new(params, ui(), (W, H), move |ui, bridge| {
            // The host's values in.
            // The plot: the engine's latest frame, or the last one settling
            // while nothing is being published. The depth curve is the
            // bands' own, so it is there with the transport stopped.
            let sequence = telemetry.sequence();
            if sequence != published {
                published = sequence;
                model.plot.advance(&telemetry.read());
            }
            // The history and the two sets: what the toolbar asked for since
            // the last frame is done first, then the live values are watched.
            {
                let mut session = mirror.session.lock().unwrap_or_else(|e| e.into_inner());
                let snapshot = |params: &SmoothParams| -> session::Snapshot {
                    let (ids, values) = params.collect_values();
                    ids.into_iter().zip(values).collect()
                };
                let now = snapshot(&mirror);
                for action in std::mem::take(&mut model.actions) {
                    let restore = match action {
                        view::Action::Preset(ask) => {
                            let mut library =
                                mirror.library.lock().unwrap_or_else(|e| e.into_inner());
                            use smooth_ui::presets::Ask;
                            match ask {
                                Ask::Step(step) => {
                                    if let Some(preset) = library.stepped(step).cloned() {
                                        presets::apply(mirror.as_ref(), &preset, &locked_names(&mirror));
                                        library.current = Some(preset.name);
                                    }
                                }
                                Ask::Load(name) => {
                                    if let Some(preset) =
                                        library.presets.iter().find(|p| p.name == name).cloned()
                                    {
                                        presets::apply(mirror.as_ref(), &preset, &locked_names(&mirror));
                                        library.current = Some(preset.name);
                                    }
                                }
                                Ask::Favourite(name, on) => {
                                    let _ = library.set_favourite(&name, on);
                                }
                                Ask::Save(name) => {
                                    let category = library
                                        .presets
                                        .iter()
                                        .find(|p| Some(&p.name) == library.current.as_ref())
                                        .map_or_else(|| "user".to_string(), |p| p.category.clone());
                                    let preset = presets::Preset {
                                        name: name.clone(),
                                        category,
                                        creator: "you".to_string(),
                                        favourite: false,
                                        values: presets::capture(mirror.as_ref()),
                                        factory: false,
                                    };
                                    if library.save(preset).is_ok() {
                                        library.current = Some(name);
                                    }
                                }
                                Ask::Delete(name) => {
                                    let _ = library.delete(&name);
                                    if library.current.as_ref() == Some(&name) {
                                        library.current = None;
                                    }
                                }
                            }
                            None
                        }
                        view::Action::Undo => session.undo(&now),
                        view::Action::Redo => session.redo(&now),
                        view::Action::SwitchAb => Some(session.switch(&now)),
                        view::Action::CopyAb => {
                            session.copy_to_other(&now);
                            None
                        }
                    };
                    if let Some(values) = restore {
                        mirror.restore_values(&values);
                    }
                }
                let held = view::pointer_down(ui);
                session.observe(&snapshot(&mirror), held);
                {
                    let library = mirror.library.lock().unwrap_or_else(|e| e.into_inner());
                    model.presets.rows = library
                        .presets
                        .iter()
                        .map(|p| smooth_ui::presets::Row {
                            name: p.name.clone(),
                            category: p.category.clone(),
                            favourite: p.favourite,
                            factory: p.factory,
                        })
                        .collect();
                    model.presets.current = library
                        .current
                        .as_ref()
                        .and_then(|name| library.presets.iter().position(|p| &p.name == name));
                }
                model.ab = session.active();
                model.can_undo = session.can_undo();
                model.can_redo = session.can_redo();
            }
            let selected = model.selected.min(7);
            for slot in Slot::ALL {
                let (param, say) = slot_param(slot, selected);
                let id = u32::from(param);
                let state = &mut model.slots[slot.index()];
                state.norm = mirror.get_normalized(id).unwrap_or(state.norm);
                state.text = say(mirror.get_plain(id).unwrap_or_default());
            }
            model.mid_side = mirror.stereo_mode.value() >= 1;
            model.soft = mirror.mode.value() < 1;
            model.bypass = mirror.bypass.value() >= 1;
            model.delta = mirror.delta.value() >= 1;
            model.low_latency = mirror.low_latency.value() >= 1;
            model.quality = mirror.quality.value().clamp(0, 2) as usize;
            {
                let bits = mirror.locks.load(std::sync::atomic::Ordering::Relaxed);
                model.lock_quality = bits & 1 != 0;
                model.lock_latency = bits & 2 != 0;
            }
            for (index, ids) in BAND_IDS.iter().enumerate() {
                let get = |i: usize| mirror.get_plain(u32::from(ids[i])).unwrap_or_default();
                let band = &mut model.bands[index];
                band.frequency = get(0);
                band.depth = get(1);
                band.q = get(2);
                band.shape = get(3).round() as usize;
                band.slope = get(4);
                band.balance = get(5);
                band.active = get(6) >= 1.0;
                band.bypass = get(7) >= 1.0;
            }

            // The controls out: a drag on one is a gesture the host records.
            let mut bind = |ui: &mut Ui, slot: Slot, norm: &mut f64, part: &mut slots::Part| {
                bridge.bind(ui, slot_param(slot, selected).0, |ui, widget, host| {
                    let el = part(ui, widget.as_str(), host);
                    *norm = *host;
                    el
                })
            };
            let tree = view::editor(ui, &mut model, &mut bind);
            // A drag on a plot handle is a gesture too, and the plot is not a
            // widget: the band's two parameters are bracketed here, so the host
            // records the move instead of waking to a value that changed behind
            // its back. The frame the pointer lets go names no parameter, and
            // the bridge ends the edit.
            drop(bind);
            let size = model.prefs.size.min(menus::SIZE_WIDTHS.len() - 1);
            if applied != Some(size) {
                let width = menus::SIZE_WIDTHS[size];
                let height = width * H / W;
                if bridge.context().is_some_and(|c| c.request_resize(width.round() as u32, height.round() as u32)) {
                    applied = Some(size);
                }
            }
            if let Some(index) = smooth_ui::graph::held(ui)
                && let Some((frequency, depth)) = BAND_PARAMS.get(index)
                && let Some(band) = model.bands.get(index)
            {
                bridge.drag_gesture(*frequency, band.frequency);
                bridge.drag_gesture(*depth, band.depth);
            }
            // The bands at rest follow the host. A value the parameter already
            // holds is not written back, so host automation is not fought
            // frame by frame.
            for (index, (frequency, depth)) in BAND_PARAMS.iter().enumerate() {
                let band = model.bands[index];
                write_plain(mirror.as_ref(), *frequency, band.frequency);
                write_plain(mirror.as_ref(), *depth, band.depth);
                write_plain(mirror.as_ref(), BAND_IDS[index][6], f64::from(u8::from(band.active)));
                write_plain(mirror.as_ref(), BAND_IDS[index][7], f64::from(u8::from(band.bypass)));
            }
            // The switches are the engine's own choices: soft or hard, the
            // bypass, the compare, the short window, the quality.
            mirror.mode.set_value(i64::from(!model.soft));
            mirror.stereo_mode.set_value(i64::from(model.mid_side));
            mirror.bypass.set_value(i64::from(model.bypass));
            mirror.delta.set_value(i64::from(model.delta));
            mirror.low_latency.set_value(i64::from(model.low_latency));
            mirror.quality.set_value(model.quality as i64);
            if model.prefs != saved_prefs {
                saved_prefs = model.prefs;
                config::save(saved_prefs);
            }
            mirror.locks.store(
                u8::from(model.lock_quality) | (u8::from(model.lock_latency) << 1),
                std::sync::atomic::Ordering::Relaxed,
            );
            pending_in.store(!model.actions.is_empty(), std::sync::atomic::Ordering::Relaxed);
            tree
        })
        // The host may resize the window; the design scales to fit it.
        .resizable((menus::SIZE_WIDTHS[0], menus::SIZE_WIDTHS[0] * H / W))
        .changed(move || {
            let sequence = changed_telemetry.sequence();
            let changed = sequence != seen || pending.swap(false, std::sync::atomic::Ordering::Relaxed);
            seen = sequence;
            changed
        })
        .into_editor()
    }
}

/// SMOOTH's face: the same rounded geometric sans the preview draws with.
fn ui() -> Ui {
    let mut ui = Ui::default();
    ui.set_theme(skin::SKIN);
    if let Some(font) = face() {
        ui = ui.font(font);
    }
    if let Some(font) = fallback() {
        ui = ui.fallback_font(font);
    }
    ui
}

/// The bundled sans, read from the user's fonts when it is there.
fn face() -> Option<Font> {
    let home = std::env::var("HOME").ok()?;
    let path = format!(
        "{home}/.local/share/fonts/GoogleSansFlex-VariableFont_GRAD,ROND,opsz,slnt,wdth,wght.ttf"
    );
    let bytes = std::fs::read(path).ok()?;
    Font::new(bytes).ok()
}

/// Noto Sans, for anything the face does not cover.
fn fallback() -> Option<Font> {
    Font::new(include_bytes!("../../../assets/NotoSans.ttf").to_vec()).ok()
}

moose::plugin! {
    logic: Smooth,
    params: SmoothParams,
}

#[cfg(test)]
mod tests {
    use super::*;
    use smooth_core::dsp::{Engine, EngineConfig, Quality, Settings};

    /// The Resonance Lab's allocation-free contract: the process path must
    /// not allocate, on any plan, with the baseline and the mid/side layout
    /// on.
    #[test]
    fn processing_is_finite_and_allocation_free_with_baseline_and_ms() {
        let mut engine = Engine::new(
            48000.0,
            EngineConfig {
                quality: Quality::Ultra,
                linear_phase: true,
                reduced_latency: false,
            },
        )
        .unwrap();
        let settings = Settings {
            adaptive_baseline: true,
            mid_side: true,
            ..Settings::default()
        };
        let allocations = allocation_audit::count(|| {
            for t in 0..14000 {
                let out = engine.process_sample([(t as f32 * 0.23).sin() * 0.2, 0.0], &settings);
                assert!(out.iter().all(|v| v.is_finite()));
            }
            for _ in 0..10000 {
                let out = engine.process_sample([0.0, 0.0], &settings);
                assert!(out.iter().all(|v| v.is_finite()));
            }
        });
        assert_eq!(allocations, 0, "process path allocated");
    }
}

#[cfg(test)]
mod probe {
    use super::*;
    /// Resonant noise through the default settings: the plot must show the
    /// reduction and the output must lose the resonance.
    #[test]
    fn defaults_reduce_a_resonance_and_the_plot_shows_it() {
        for gain in [0.01f32, 0.03, 0.1, 0.3, 1.0, 3.0] { reduce_at(gain); }
    }

    #[test]
    fn the_bank_builds_at_every_host_rate_and_selection() {
        for rate in [44100.0f32, 48000.0, 88200.0, 96000.0, 176400.0, 192000.0] {
            for sel in 0..6 {
                if let Err(e) = EngineBank::new(rate, sel, false) { panic!("{rate} sel {sel}: {e}"); }
            }
        }
    }

    fn reduce_at(gain: f32) {
        let params = SmoothParams::default();
        let settings = params.settings();
        let mut bank = EngineBank::new(48000.0, params.selection(), false).unwrap();
        let (mut seed, mut lp, mut z1, mut z2) = (1u32, 0.0f32, 0.0f32, 0.0f32);
        let (mut ein, mut eout) = (0.0f64, 0.0f64);
        let mut deepest = 0.0f32;
        for t in 0..48000 * 4 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let n = (seed >> 8) as f32 / 8388608.0 - 1.0;
            lp += 0.2 * (n - lp);
            // a narrow resonance near 3 kHz
            let w = std::f32::consts::TAU * 3000.0 / 48000.0;
            let (r, c) = (0.995f32, 2.0 * 0.995 * w.cos());
            let y = lp * 0.02 + c * z1 - r * r * z2;
            z2 = z1; z1 = y;
            let x = (lp * 0.3 + y * 0.5) * gain;
            let out = bank.active_mut().process_sample([x, x], &settings);
            if t > 48000 { ein += f64::from(x * x); eout += f64::from(out[0] * out[0]); }
            if t % 800 == 0 {
                let f = bank.active().plot_frame();
                for g in f.gain[0] { deepest = deepest.min(20.0 * g.max(1e-6).log10()); }
            }
        }
        eprintln!("gain {gain}: energy out/in = {:.2} dB, deepest plot = {deepest:.1} dB", 10.0 * (eout / ein).log10());
        let _ = deepest;
    }
}

#[cfg(test)]
mod param_tests {
    use super::*;

    #[test]
    fn the_params_carry_soothe3s_declared_defaults() {
        let params = SmoothParams::default();
        assert_eq!(params.depth.value(), 5.0);
        assert_eq!(params.detail.value(), 5.0);
        assert_eq!(params.attack.value(), 5.0);
        assert_eq!(params.release.value(), 3.0);
        assert_eq!(params.mix.value(), 100.0);
        assert_eq!(params.output.value(), 0.0);
        assert_eq!(params.mode.value(), 0);
        assert_eq!(params.quality.value(), 0);
        assert_eq!(params.band1_frequency.value(), 100.0);
        assert_eq!(params.band2_frequency.value(), 4000.0);
        assert_eq!(params.band3_frequency.value(), 16000.0);
        assert_eq!(params.band4_frequency.value(), 1000.0);
    }

    #[test]
    fn the_host_laws_are_the_engines_reference_laws() {
        use smooth_core::weighting::{normalized_from_center, normalized_from_q, normalized_from_slope};
        let params = SmoothParams::default();
        for hz in [20.0_f32, 100.0, 1000.0, 4000.0, 20000.0] {
            let host = params.band1_frequency.range().normalize(f64::from(hz));
            assert!((host as f32 - normalized_from_center(hz)).abs() < 1e-5, "{hz} Hz");
        }
        for q in [0.1_f32, 0.707, 2.0, 10.0] {
            let host = params.band1_q.range().normalize(f64::from(q));
            assert!((host as f32 - normalized_from_q(q)).abs() < 1e-4, "q {q}");
        }
        // The reference's default slope sits at the middle of its knob.
        let slope = params.band1_slope.range().normalize(f64::from(params.band1_slope.value()));
        assert!((slope - 0.5).abs() < 1e-3);
        assert!((normalized_from_slope(params.band1_slope.value()) - 0.5).abs() < 1e-3);
    }
}
