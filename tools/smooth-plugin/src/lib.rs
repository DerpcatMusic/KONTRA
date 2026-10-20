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
use smooth_ui::graph::{PLOT_BINS, Telemetry};
use smooth_ui::slots::{self, Slot};
use smooth_ui::{H, W, skin, view};
use std::sync::Arc;

// ------------------------------------------------------------- parameters --

/// The bands the editor draws: one handle each.
const BANDS: usize = 4;

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
        range = "log(20, 20000)",
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
    #[param(name = "Band 1 q", range = "log(0.1, 10)", default = 0.707)]
    pub band1_q: FloatParam,
    /// Where the second band sits.
    #[param(
        name = "Band 2 frequency",
        range = "log(20, 20000)",
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
    #[param(name = "Band 2 q", range = "log(0.1, 10)", default = 0.707)]
    pub band2_q: FloatParam,
    /// Where the third band sits.
    #[param(
        name = "Band 3 frequency",
        range = "log(20, 20000)",
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
    #[param(name = "Band 3 q", range = "log(0.1, 10)", default = 0.707)]
    pub band3_q: FloatParam,
    /// Where the fourth band sits.
    #[param(
        name = "Band 4 frequency",
        range = "log(20, 20000)",
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
    #[param(name = "Band 4 q", range = "log(0.1, 10)", default = 0.707)]
    pub band4_q: FloatParam,
    /// The live plot: the engine writes it from the audio thread, the
    /// editor reads it. Not a parameter.
    #[skip]
    pub plot: std::sync::Arc<Telemetry>,
    /// The shape of the first band.
    #[param(name = "Band 1 shape", range = "discrete(0, 7)", default = 0)]
    pub band1_shape: IntParam,
    /// The shape of the second band.
    #[param(name = "Band 2 shape", range = "discrete(0, 7)", default = 2)]
    pub band2_shape: IntParam,
    /// The shape of the third band.
    #[param(name = "Band 3 shape", range = "discrete(0, 7)", default = 7)]
    pub band3_shape: IntParam,
    /// The shape of the fourth band.
    #[param(name = "Band 4 shape", range = "discrete(0, 7)", default = 2)]
    pub band4_shape: IntParam,
}

use SmoothParamsParamId as P;

/// The band parameters, in the order the graph draws its handles.
const BAND_PARAMS: [(P, P); 4] = [
    (P::Band1Frequency, P::Band1Depth),
    (P::Band2Frequency, P::Band2Depth),
    (P::Band3Frequency, P::Band3Depth),
    (P::Band4Frequency, P::Band4Depth),
];

/// Write a value to a parameter only when it does not already hold it, so a
/// rebuild leaves a value the host just automated alone.
fn write_plain(params: &SmoothParams, param: P, value: f64) {
    if params.get_plain(u32::from(param)) != Some(value) {
        params.set_plain(u32::from(param), value);
    }
}

/// The parameter behind a slot, and how its plain value reads.
fn slot_param(slot: Slot) -> (P, fn(f64) -> String) {
    fn one(v: f64) -> String {
        format!("{v:.1}")
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
        Slot::DetailLow => (P::DetailTiltLow, one),
        Slot::DetailHigh => (P::DetailTiltHigh, one),
        Slot::AttackLow => (P::AttackTiltLow, one),
        Slot::AttackHigh => (P::AttackTiltHigh, one),
        Slot::ReleaseLow => (P::ReleaseTiltLow, one),
        Slot::ReleaseHigh => (P::ReleaseTiltHigh, one),
    }
}

/// The engine's settings, built from the host's values.
///
/// Every conversion here is the one the reverse engineering established: the
/// public value is turned into the engine's normalized form by the same
/// helpers the research wrote, and the linear controls divide by their span.
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
            max_cut_db: self.max_cut.value(),
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

    /// The four bands the editor draws, in the engine's normalized form.
    fn band_records(&self) -> [BandRecord; 8] {
        let frequencies = [
            self.band1_frequency.value(),
            self.band2_frequency.value(),
            self.band3_frequency.value(),
            self.band4_frequency.value(),
        ];
        let depths = [
            self.band1_depth.value(),
            self.band2_depth.value(),
            self.band3_depth.value(),
            self.band4_depth.value(),
        ];
        let widths = [
            self.band1_q.value(),
            self.band2_q.value(),
            self.band3_q.value(),
            self.band4_q.value(),
        ];
        let shapes = [
            self.band1_shape.value(),
            self.band2_shape.value(),
            self.band3_shape.value(),
            self.band4_shape.value(),
        ];
        std::array::from_fn(|index| {
            // soothe3's declared defaults: a low cut at 100 Hz, a bell at
            // 4 kHz, a high cut at 16 kHz, and a bell at 1 kHz. The bands
            // past the fourth are declared but unused.
            let (frequency, depth, width, shape) = if index < BANDS {
                (
                    frequencies[index],
                    depths[index],
                    widths[index],
                    shapes[index].clamp(0, 7) as f32,
                )
            } else {
                (1000.0, 0.0, 0.707, 2.0)
            };
            BandRecord::from_normalized(NormalizedRecord {
                used: 1.0,
                enabled: f32::from(index < BANDS),
                frequency: normalized_from_center(frequency),
                depth: (depth + 10.0) / 20.0,
                q: normalized_from_q(width),
                slope: normalized_from_slope(5.0),
                focus: 0.5,
                selector: 0.0,
                shape: shape / 7.0,
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
        }
    }

    fn reset(state: &mut Dsp, params: &SmoothParams, config: &AudioConfig) {
        state.bank = Dsp::build(config.sample_rate, params.selection()).ok();
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
        params
            .plot
            .publish(bank.active().plot_snapshot::<PLOT_BINS>());
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
            telemetry: Arc::clone(&params.plot),
            ..view::Model::default()
        };
        // The editor repaints when the engine publishes a new plot.
        let plot = Arc::clone(&params.plot);
        let mut seen: u64 = 0;
        MuiEditor::new(params, ui(), (W, H), move |ui, bridge| {
            // The host's values in.
            for slot in Slot::ALL {
                let (param, say) = slot_param(slot);
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
            for (index, (frequency, depth)) in BAND_PARAMS.iter().enumerate() {
                model.bands[index].frequency =
                    mirror.get_plain(u32::from(*frequency)).unwrap_or_default();
                model.bands[index].depth = mirror.get_plain(u32::from(*depth)).unwrap_or_default();
            }

            // The controls out: a drag on one is a gesture the host records.
            let mut bind = |ui: &mut Ui, slot: Slot, norm: &mut f64, part: &mut slots::Part| {
                bridge.bind(ui, slot_param(slot).0, |ui, widget, host| {
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
                write_plain(mirror.as_ref(), *frequency, model.bands[index].frequency);
                write_plain(mirror.as_ref(), *depth, model.bands[index].depth);
            }
            // The switches are the engine's own choices: soft or hard, the
            // bypass, the compare, the short window, the quality.
            mirror.mode.set_value(i64::from(!model.soft));
            mirror.stereo_mode.set_value(i64::from(model.mid_side));
            mirror.bypass.set_value(i64::from(model.bypass));
            mirror.delta.set_value(i64::from(model.delta));
            mirror.low_latency.set_value(i64::from(model.low_latency));
            mirror.quality.set_value(model.quality as i64);
            tree
        })
        .changed(move || {
            let sequence = plot.sequence();
            let changed = sequence != seen;
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
}
