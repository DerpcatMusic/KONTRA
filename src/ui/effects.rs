//! Standalone descriptor-driven effect editor; preparation and painting run on the UI thread.
use super::{editor, theme::*, viz};
use moose::mui::mui::prelude::*;
use sampler_core::{
    Biquad, ControlId, EngineParameterLaw, FilterKind, ParameterAddress, ParameterDescriptor,
    ParameterLaw, ParameterUnit,
};

/// The common model supplies roles; display names never determine routing.
pub struct FilterModel {
    pub rate: u32,
    pub kernel: FilterKernel,
    pub cutoff: ParameterAddress,
    pub q: ParameterAddress,
    pub frequency_hz: fn(f64, u32) -> f64,
    pub resonance_q: fn(f64) -> f64,
}

#[derive(Clone, Copy)]
pub enum FilterKernel {
    Biquad(FilterKind),
    LadderLP4 { gain: ParameterAddress },
}

#[derive(Clone, Copy)]
enum Response {
    Biquad(Biquad),
    LadderLP4 { knobs: [f32; 3], rate: u32 },
}

impl Response {
    fn magnitude(self, hz: f64) -> f64 {
        match self {
            Self::Biquad(coefficients) => coefficients.magnitude(hz),
            Self::LadderLP4 { knobs, rate } => f64::from(sampler_core::LadderSettings::magnitude(
                knobs, hz as f32, rate,
            )),
        }
    }
}

impl FilterModel {
    fn cutoff_hz(&self, value: f64) -> f64 {
        match self.kernel {
            FilterKernel::Biquad(_) => (self.frequency_hz)(value, self.rate),
            FilterKernel::LadderLP4 { .. } => {
                f64::from(sampler_core::LadderSettings::cutoff_hz(value as f32))
            }
        }
    }

    fn resonance(&self, value: f64) -> f64 {
        match self.kernel {
            FilterKernel::Biquad(_) => (self.resonance_q)(value),
            FilterKernel::LadderLP4 { .. } => value,
        }
    }

    fn preview(&self, controls: &[(&ParameterDescriptor, f64)]) -> Option<(Response, f64, f64)> {
        let read = |address| {
            controls
                .iter()
                .find(|(d, _)| d.address == address)
                .map(|(_, v)| *v)
        };
        if high_hz(self.rate) <= 20. {
            return None;
        }
        let cutoff = read(self.cutoff)?;
        let resonance = self.resonance(read(self.q)?);
        let hz = self.cutoff_hz(cutoff);
        match self.kernel {
            FilterKernel::Biquad(kind) => Some((
                Response::Biquad(Biquad::new(self.rate, kind, hz, resonance).ok()?),
                hz,
                self::kind(self.kernel).2,
            )),
            FilterKernel::LadderLP4 { gain } => {
                let gain = read(gain)?;
                if !(0.0..=1.0).contains(&cutoff)
                    || !(0.0..=1.0).contains(&resonance)
                    || !(-1.0..=1.0).contains(&gain)
                {
                    return None;
                }
                let response = Response::LadderLP4 {
                    knobs: [cutoff as f32, resonance as f32, gain as f32],
                    rate: self.rate,
                };
                response
                    .magnitude(1000.)
                    .is_finite()
                    .then_some((response, hz, gain))
            }
        }
    }
}

#[derive(Default)]
pub struct State {
    grabbed: bool,
    first_drag: bool,
    typing: Option<(ControlId, String)>,
    response: CanvasCache<(u32, u8, u64, u64, u64)>,
    owner: Option<(ParameterAddress, ParameterAddress)>,
}

fn valid(d: &ParameterDescriptor, v: f64) -> bool {
    let [low, high] = d.range;
    low.is_finite()
        && high.is_finite()
        && low <= high
        && (high - low).is_finite()
        && (low..=high).contains(&d.default)
        && (low..=high).contains(&v)
        && match d.law {
            ParameterLaw::Linear => true,
            ParameterLaw::Native(law) => {
                let a = law.decode(if law == EngineParameterLaw::SignedNormalized {
                    -1_000_000
                } else {
                    0
                });
                let b = law.decode(1_000_000);
                if law.normalized_value(a).is_err() {
                    return false;
                }
                let (a, b) = (a.min(b), a.max(b));
                // Native exponential endpoints can differ from declared bounds by roundoff.
                let tolerance = f64::EPSILON * 16. * a.abs().max(b.abs()).max(1.);
                [low, high, d.default, v]
                    .into_iter()
                    .all(|value| value >= a - tolerance && value <= b + tolerance)
            }
        }
}

fn position(d: &ParameterDescriptor, v: f64) -> f64 {
    let [low, high] = d.range;
    if high == low {
        return 0.;
    }
    match d.law {
        ParameterLaw::Linear => (v - low) / (high - low),
        ParameterLaw::Native(law) => {
            let (a, b) = (law.encode(low), law.encode(high));
            if a == b {
                0.
            } else {
                f64::from(law.encode(v) - a) / f64::from(b - a)
            }
        }
    }
    .clamp(0., 1.)
}

fn value_at(d: &ParameterDescriptor, p: f64) -> f64 {
    let p = p.clamp(0., 1.);
    let [low, high] = d.range;
    match d.law {
        ParameterLaw::Linear => low + (high - low) * p,
        ParameterLaw::Native(law) => {
            let (a, b) = (law.encode(low), law.encode(high));
            law.decode((f64::from(a) + f64::from(b - a) * p).round() as i32)
                .clamp(low, high)
        }
    }
}

fn put(
    controls: &mut [(&ParameterDescriptor, f64)],
    index: usize,
    value: f64,
    write: &mut impl FnMut(ControlId, f64),
) {
    let (d, v) = &mut controls[index];
    if valid(d, value) && value != *v {
        *v = value;
        write(d.control, value);
    }
}

fn readout(d: &ParameterDescriptor, v: f64) -> String {
    match d.unit {
        ParameterUnit::Hertz if v >= 1000. => format!("{:.2} kHz", v / 1000.),
        ParameterUnit::Hertz => format!("{v:.1} Hz"),
        ParameterUnit::Seconds if v < 1. => format!("{:.1} ms", v * 1000.),
        ParameterUnit::Seconds => format!("{v:.2} s"),
        ParameterUnit::Decibels => format!("{v:.1} dB"),
        ParameterUnit::Percent => format!("{v:.1}%"),
        ParameterUnit::Semitones => format!("{v:.2} st"),
        ParameterUnit::Octaves => format!("{v:.2} oct"),
        ParameterUnit::Frames => format!("{v:.0} frames"),
        ParameterUnit::Linear | ParameterUnit::Normalized => format!("{v:.3}"),
    }
}

fn kind(kernel: FilterKernel) -> (&'static str, u8, f64) {
    let FilterKernel::Biquad(kind) = kernel else {
        return ("Low-pass · 4 poles", 8, 0.);
    };
    match kind {
        FilterKind::LowPass => ("Low-pass", 0, 0.),
        FilterKind::HighPass => ("High-pass", 1, 0.),
        FilterKind::BandPass => ("Band-pass", 2, 0.),
        FilterKind::Notch => ("Notch", 3, 0.),
        FilterKind::AllPass => ("All-pass", 4, 0.),
        FilterKind::Peak { gain_db } => ("Bell", 5, gain_db),
        FilterKind::LowShelf { gain_db } => ("Low shelf", 6, gain_db),
        FilterKind::HighShelf { gain_db } => ("High shelf", 7, gain_db),
    }
}

fn high_hz(rate: u32) -> f64 {
    20000f64.min(f64::from(rate) * 0.499)
}
fn frequency(x: f64, rate: u32) -> f64 {
    20. * (high_hz(rate) / 20.).powf(x)
}
fn frequency_x(hz: f64, rate: u32) -> f32 {
    ((hz / 20.).ln() / (high_hz(rate) / 20.).ln()).clamp(0., 1.) as f32
}

/// Callers provide the selected effect's descriptor slice, admitted lane values and write queue.
pub fn filter(
    ui: &mut Ui,
    state: &mut State,
    model: &FilterModel,
    descriptors: &[ParameterDescriptor],
    read: impl Fn(ControlId) -> Option<f64>,
    mut write: impl FnMut(ControlId, f64),
) -> El {
    if state.owner != Some((model.cutoff, model.q)) {
        state.owner = Some((model.cutoff, model.q));
        state.grabbed = false;
        state.typing = None;
    }
    let mut controls: Vec<_> = descriptors
        .iter()
        .filter_map(|d| read(d.control).filter(|v| valid(d, *v)).map(|v| (d, v)))
        .collect();
    controls.sort_by(|(a, _), (b, _)| {
        (&a.display.group, a.display.order).cmp(&(&b.display.group, b.display.order))
    });
    let roles = controls
        .iter()
        .position(|(d, _)| d.address == model.cutoff)
        .zip(controls.iter().position(|(d, _)| d.address == model.q));
    let Some((cutoff, q)) = roles else {
        state.grabbed = false;
        return caption("Filter parameters are not available.").fill(secondary());
    };
    let (title, k, _) = kind(model.kernel);
    let (reset, reset_el) = action(ui, "effect-filter-reset", "Reset", false);
    if reset {
        for (d, v) in &mut controls {
            if *v != d.default {
                *v = d.default;
                write(d.control, *v);
            }
        }
    }
    let r = ui.get("effect-filter-graph");
    if let Some(size) = ui
        .scene()
        .and_then(|s| s.surface("effect-filter-graph"))
        .map(|s| s.frame.size)
    {
        if r.pressed {
            state.first_drag = true;
            state.grabbed = model
                .preview(&controls)
                .zip(ui.local("effect-filter-graph"))
                .is_some_and(|((coefficients, hz, _), p)| {
                    let hz = hz.clamp(20., high_hz(model.rate));
                    let at = editor::place(
                        size,
                        [
                            frequency_x(hz, model.rate),
                            viz::db_y((20. * coefficients.magnitude(hz).max(1e-6).log10()) as f32)
                                .clamp(0., 1.),
                        ],
                    );
                    (at.x - p.x).hypot(at.y - p.y) <= CONTROL
                });
        }
        if state.grabbed && r.dragged {
            let fine = if r.mods.shift { 0.1 } else { 1. };
            let delta = if state.first_drag {
                r.drag_total
            } else {
                r.drag_delta
            };
            state.first_drag = false;
            let (d, v) = controls[cutoff];
            let p = position(d, v) + delta.x / (size.width - 2. * SPACE).max(1.) * fine;
            put(&mut controls, cutoff, value_at(d, p), &mut write);
            let (d, v) = controls[q];
            let p = position(d, v) - delta.y / (size.height - 2. * SPACE).max(1.) * fine;
            put(&mut controls, q, value_at(d, p), &mut write);
        }
        if r.wheel.y != 0. {
            let (d, v) = controls[q];
            put(
                &mut controls,
                q,
                value_at(
                    d,
                    position(d, v) - r.wheel.y.signum() * if r.mods.shift { 0.002 } else { 0.02 },
                ),
                &mut write,
            );
        }
        if r.double_clicked {
            let (a, b) = (controls[cutoff].0.default, controls[q].0.default);
            put(&mut controls, cutoff, a, &mut write);
            put(&mut controls, q, b, &mut write);
        }
        if r.released {
            state.grabbed = false;
        }
    }
    let mut groups: Vec<(String, Vec<El>)> = Vec::new();
    for (d, v) in &mut controls {
        let id = format!("effect-control-{}", d.control.0);
        let mut p = position(d, *v);
        let previous = p;
        let held = drive(
            ui,
            &id,
            &mut p,
            &(0.0..=1.0),
            TRAVEL,
            true,
            position(d, d.default),
        );
        let to = value_at(d, p);
        if p != previous && valid(d, to) && to != *v {
            *v = to;
            write(d.control, to);
        }
        let lift = ui.state(&id).hover.max(if held { 1. } else { 0. }) as f32;
        let dial = dial_face(p, position(d, d.default), lift, ui.focus_visible(&id))
            .square(KNOB)
            .focusable()
            .tracks_pointer()
            .captures_wheel()
            .cursor(Cursor::ResizeV)
            .a11y(A11y::Slider {
                value: *v,
                min: d.range[0],
                max: d.range[1],
            })
            .named(d.name.clone())
            .tip(format!(
                "{}: drag, wheel or arrows; Shift for fine; double-click to reset",
                d.name
            ))
            .id(id);
        let value_id = format!("effect-value-{}", d.control.0);
        let value_tip = if matches!(model.kernel, FilterKernel::LadderLP4 { .. })
            && d.address == model.cutoff
        {
            format!(
                "{}: {:.1} Hz cutoff. Double-click to type the normalized parameter value.",
                d.name,
                model.cutoff_hz(*v)
            )
        } else {
            format!(
                "{}: double-click to type a value in the parameter's units",
                d.name
            )
        };
        if ui.get(&value_id).double_clicked {
            state.typing = Some((d.control, v.to_string()));
        }
        let value =
            if let Some((_, text)) = state.typing.as_mut().filter(|(id, _)| *id == d.control) {
                let edit_id = format!("{value_id}-edit");
                let existed = ui.scene().and_then(|s| s.surface(&edit_id)).is_some();
                if !existed {
                    ui.focus(&edit_id);
                }
                let field = text_edit(ui, &edit_id, text, TextOpts::default());
                let cancel = ui.keys(&edit_id).iter().any(|k| k.key == Key::Escape);
                let done = field.changed.submitted || (existed && !ui.focused(&edit_id));
                if done || cancel {
                    let (_, text) = state.typing.take().unwrap();
                    if done
                        && !cancel
                        && let Ok(to) = text.parse::<f64>()
                        && valid(d, to)
                        && to != *v
                    {
                        *v = to;
                        write(d.control, to);
                    }
                }
                field.el.h(CONTROL).w(TEXT * 9.).radius(0)
            } else {
                caption(readout(d, *v))
                    .text_size(TEXT)
                    .lines(1)
                    .reserve("20000.00 Hz")
                    .tip(value_tip)
                    .id(value_id)
            };
        if groups
            .last()
            .is_none_or(|(name, _)| name != &d.display.group)
        {
            groups.push((d.display.group.clone(), Vec::new()));
        }
        groups.last_mut().unwrap().1.push(
            col![
                caption(d.name.clone())
                    .lines(1)
                    .min_w(0)
                    .tip(d.name.clone()),
                dial,
                value
            ]
            .gap(TIGHT)
            .align(Align::Center)
            .min_w(0),
        );
    }
    let width = ui
        .scene()
        .and_then(|s| {
            s.surface("effect-filter-graph")
                .map(|s| s.frame.size.width)
                .or_else(|| {
                    s.surface("effect-filter")
                        .map(|s| s.frame.size.width - INSET * 2.)
                })
        })
        .unwrap_or(TEXT * 20.);
    let columns = ((width + INSET) / (TEXT * 9. + INSET)).floor().max(1.) as usize;
    let multiple = groups.len() > 1;
    let control_rows = col(groups
        .into_iter()
        .map(|(name, cells)| {
            let cells = grid(columns, cells).gap(INSET).min_w(0);
            if multiple {
                col![section(&name).lines(1).min_w(0), cells].gap(SPACE)
            } else {
                cells
            }
        })
        .collect::<Vec<_>>())
    .gap(INSET)
    .min_w(0)
    .max_size(Size::new(1e5, CONTROL * 5.))
    .scroll()
    .shrink(0);
    let Some((coefficients, hz, gain)) = model.preview(&controls) else {
        return col![
            section_bar(title, vec![reset_el]),
            caption("Filter preview is not available.").fill(secondary()),
            control_rows
        ]
        .gap(SPACE)
        .min_w(0)
        .id("effect-filter");
    };
    let rate = model.rate;
    let key = (
        rate,
        k,
        gain.to_bits(),
        hz.to_bits(),
        model.resonance(controls[q].1).to_bits(),
    );
    let graph = canvas_keyed(&state.response, key, move |size| {
        let mut draws = Vec::new();
        for hz in [100., 1000., 10000.] {
            if hz < high_hz(rate) {
                let x = editor::place(size, [frequency_x(hz, rate), 0.]).x;
                draws.push(Draw::fill(rect(x, 0., 1., size.height), hairline()));
            }
        }
        for db in [-24., -12., 0., 12.] {
            let y = editor::place(size, [0., viz::db_y(db)]).y;
            draws.push(Draw::fill(
                rect(0., y, size.width, 1.),
                if db == 0. {
                    Role::Ink.alpha(0.18)
                } else {
                    hairline()
                },
            ));
        }
        let points: Vec<_> = (0..=160)
            .map(|n| {
                let x = n as f32 / 160.;
                let db = 20.
                    * coefficients
                        .magnitude(frequency(f64::from(x), rate))
                        .max(1e-6)
                        .log10();
                [x, viz::db_y(db as f32).clamp(0., 1.)]
            })
            .collect();
        draws.push(Draw::fill(
            editor::area(size, &points),
            accent().with_alpha(0.08),
        ));
        draws.push(Draw::stroke(
            editor::line(size, &points),
            value_ink(0.),
            1.5,
        ));
        draws
    })
    .w(Len::Pct(100.))
    .h(Len::Pct(100.));
    let size = ui
        .scene()
        .and_then(|s| s.surface("effect-filter-graph"))
        .map(|s| s.frame.size)
        .unwrap_or(Size::new(1., 1.));
    let node_hz = hz.clamp(20., high_hz(rate));
    let at = editor::place(
        size,
        [
            frequency_x(node_hz, rate),
            viz::db_y((20. * coefficients.magnitude(node_hz).max(1e-6).log10()) as f32)
                .clamp(0., 1.),
        ],
    );
    let handle = block(SPACE, SPACE)
        .fill(value_ink(0.))
        .stroke(Role::Ink)
        .stroke_width(1.)
        .at(at.x - SPACE / 2., at.y - SPACE / 2.)
        .float()
        .id("effect-filter-handle")
        .disabled();
    let (status, status_tip) = match model.kernel {
        FilterKernel::Biquad(_) => (
            "Unverified mapping",
            "Standalone physical Hz/Q fixture uses identity converters. Native filter parameter conversion is awaiting verification.",
        ),
        FilterKernel::LadderLP4 { .. } => (
            "Normalized resonance",
            "LP4 cutoff uses the shared cutoff_hz helper. Resonance stays normalized. The curve is the playback kernel's small-signal response; large signals additionally undergo input soft clipping.",
        ),
    };
    col![
        section_bar(title, vec![
            caption(status).fill(secondary()).lines(1)
                .tip(status_tip)
                .id("effect-filter-mapping-status"),
            reset_el,
        ]),
        col![stack![graph, handle]
            .fill(Role::Field)
            .clip()
            .tracks_pointer()
            .captures_wheel()
            .cursor(Cursor::Grab)
            .named("Filter response")
            .tip(
                "Drag the node: across for cutoff, up for resonance. Wheel changes resonance; double-click resets."
            )
            .id("effect-filter-graph")
            .flex(1)
            .min_h(CONTROL * 4.),
        row![
            caption("20 Hz").fill(secondary()),
            spacer(),
            caption(format!("{:.1} kHz", high_hz(rate) / 1000.)).fill(secondary())
        ],
        control_rows,]
        .gap(SPACE)
        .pad((INSET, 0.))
        .flex(1)
        .min_w(0)
        .min_h(0),
    ]
    .gap(SPACE)
    .min_w(0)
    .min_h(0)
    .id("effect-filter")
}

#[cfg(test)]
mod tests {
    use super::*;
    use sampler_core::{EngineParameterLaw, ParameterDisplay, ParameterScope};
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    fn descriptors() -> Vec<ParameterDescriptor> {
        let address = |parameter: u32| ParameterAddress {
            scope: ParameterScope::Group(7),
            node: 19 + parameter,
            parameter: 0,
        };
        // Deliberately reverse registration order and use names that do not identify their roles.
        vec![
            ParameterDescriptor {
                address: address(2),
                control: ControlId(902),
                name: "Width".into(),
                role: sampler_core::ParameterRole::Resonance,
                unit: ParameterUnit::Linear,
                range: [0.1, 12.],
                default: 0.707,
                law: ParameterLaw::Linear,
                display: ParameterDisplay {
                    group: "Filter".into(),
                    order: 2,
                },
            },
            ParameterDescriptor {
                address: address(1),
                control: ControlId(901),
                name: "Frequency".into(),
                role: sampler_core::ParameterRole::Cutoff,
                unit: ParameterUnit::Hertz,
                range: [20., 20000.],
                default: 1000.,
                law: ParameterLaw::Native(EngineParameterLaw::Exponential {
                    low: 20.,
                    high: 20000.,
                }),
                display: ParameterDisplay {
                    group: "Filter".into(),
                    order: 1,
                },
            },
        ]
    }

    struct Fixture {
        ui: Ui,
        state: State,
        descriptors: Vec<ParameterDescriptor>,
        values: RefCell<BTreeMap<ControlId, f64>>,
        writes: RefCell<Vec<(ControlId, f64)>>,
        size: Size,
        frequency_hz: fn(f64, u32) -> f64,
        kernel: FilterKernel,
    }
    impl Fixture {
        fn new(width: f64, height: f64) -> Self {
            let mut registry = sampler_core::ParameterRegistry::default();
            for descriptor in descriptors() {
                registry.register(descriptor).unwrap();
            }
            let registry = registry.prepare().unwrap();
            let descriptors: Vec<_> = registry.descriptors().cloned().collect();
            let values = RefCell::new(descriptors.iter().map(|d| (d.control, d.default)).collect());
            let mut f = Self {
                ui: ui(),
                state: State::default(),
                descriptors,
                values,
                writes: RefCell::default(),
                size: Size::new(width, height),
                frequency_hz: |v, _| v,
                kernel: FilterKernel::Biquad(FilterKind::LowPass),
            };
            f.idle(4);
            f
        }
        fn tick(&mut self, input: Input) {
            let model = FilterModel {
                rate: 48000,
                kernel: self.kernel,
                cutoff: self.descriptors[1].address,
                q: self.descriptors[0].address,
                frequency_hz: self.frequency_hz,
                resonance_q: |v| v,
            };
            let panel = filter(
                &mut self.ui,
                &mut self.state,
                &model,
                &self.descriptors,
                |id| self.values.borrow().get(&id).copied(),
                |id, value| {
                    self.values.borrow_mut().insert(id, value);
                    self.writes.borrow_mut().push((id, value));
                },
            );
            let root = col![
                section_bar("Sound · Effects", vec![]),
                rule(),
                panel.flex(1).min_h(0)
            ]
            .gap(SPACE)
            .pad(INSET)
            .fill(Role::Surface)
            .id("effect-fixture");
            self.ui
                .frame(root, Some(self.size), input, 1. / 60.)
                .unwrap();
        }
        fn idle(&mut self, count: usize) {
            for _ in 0..count {
                self.tick(Input::default());
            }
        }
        fn pointer(&mut self, at: Point, down: bool) {
            self.tick(Input {
                pointer: PointerInput {
                    pos: Some(at),
                    buttons: if down {
                        Buttons::PRIMARY
                    } else {
                        Buttons::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            });
        }

        fn lp4(width: f64, height: f64) -> Self {
            let mut f = Self::new(width, height);
            for (d, default) in f.descriptors.iter_mut().zip([0.6, 0.5]) {
                d.unit = ParameterUnit::Normalized;
                d.range = [0., 1.];
                d.default = default;
                d.law = ParameterLaw::Linear;
            }
            let mut gain = f.descriptors[0].clone();
            gain.control = ControlId(903);
            gain.address.node = 22;
            gain.name = "Drive".into();
            gain.unit = ParameterUnit::Linear;
            gain.range = [-1., 1.];
            gain.default = 0.;
            gain.display.order = 3;
            f.kernel = FilterKernel::LadderLP4 { gain: gain.address };
            f.descriptors.push(gain);
            let mut registry = sampler_core::ParameterRegistry::default();
            for d in &f.descriptors {
                registry.register(d.clone()).unwrap();
            }
            f.descriptors = registry.prepare().unwrap().descriptors().cloned().collect();
            *f.values.borrow_mut() = f
                .descriptors
                .iter()
                .map(|d| (d.control, d.default))
                .collect();
            // LP4 must use its shared helper rather than a Biquad converter supplied by the caller.
            f.frequency_hz = |_, _| f64::NAN;
            f.state = State::default();
            f.idle(4);
            f
        }
    }

    #[test]
    fn filter_graph_and_controls_share_the_supplied_lanes_at_both_sizes() {
        for (w, h) in [(900., 600.), (1180., 900.)] {
            let mut f = Fixture::new(w, h);
            let scene = f.ui.scene().unwrap();
            let graph = scene
                .surface("effect-filter-graph")
                .expect("dedicated filter graph")
                .frame;
            let cutoff = scene
                .surface("effect-control-901")
                .expect("descriptor cutoff")
                .frame;
            let q = scene
                .surface("effect-control-902")
                .expect("descriptor Q")
                .frame;
            let status = scene.surface("effect-filter-mapping-status").unwrap();
            let reset = scene.surface("effect-filter-reset").unwrap().frame;
            assert_eq!(status.text_value.as_deref(), Some("Unverified mapping"));
            assert!(status.frame.x + status.frame.size.width + TIGHT <= reset.x);
            assert_eq!(graph.x, INSET * 2., "body follows the section-bar inset");
            assert!(cutoff.x < q.x, "display.order overrides registration order");
            for r in [graph, cutoff, q] {
                assert!(
                    r.x >= INSET
                        && r.y >= INSET
                        && r.x + r.size.width <= w - INSET + 0.5
                        && r.y + r.size.height <= h - INSET + 0.5,
                    "{r:?} stays in its panel"
                );
            }
            let at = super::super::tests::center(&f.ui, "effect-filter-handle");
            f.pointer(at, true);
            f.pointer(Point::new(at.x + 80., at.y - 30.), true);
            f.pointer(Point::new(at.x + 80., at.y - 30.), false);
            f.idle(2);
            assert!(
                f.values.borrow()[&ControlId(901)] > 1000.,
                "rightward handle movement raises frequency"
            );
            assert!(
                f.values.borrow()[&ControlId(902)] > 0.707,
                "upward handle movement raises Q"
            );
            let at = super::super::tests::center(&f.ui, "effect-control-901");
            let before = f.values.borrow()[&ControlId(901)];
            f.tick(Input {
                pointer: PointerInput {
                    pos: Some(at),
                    ..Default::default()
                },
                wheel: Vec2::new(0., -60.),
                ..Default::default()
            });
            f.idle(2);
            assert!(
                f.values.borrow()[&ControlId(901)] > before,
                "the shared knob driver reaches the same cutoff lane"
            );
            assert!(
                f.writes
                    .borrow()
                    .iter()
                    .all(|(id, value)| [ControlId(901), ControlId(902)].contains(id)
                        && value.is_finite())
            );
        }
    }

    #[test]
    fn filter_idle_typing_reset_and_invalid_values_preserve_the_lane_domain() {
        let mut f = Fixture::new(900., 600.);
        f.idle(20);
        assert!(
            f.writes.borrow().is_empty(),
            "painting must not quantize native-law readback"
        );
        let at = super::super::tests::center(&f.ui, "effect-value-901");
        for _ in 0..2 {
            f.pointer(at, true);
            f.pointer(at, false);
        }
        f.idle(3);
        assert!(
            f.ui.scene()
                .unwrap()
                .surface("effect-value-901-edit")
                .is_some(),
            "double-click opens numeric entry"
        );
        f.ui.focus("effect-value-901-edit");
        f.tick(Input {
            keys: vec![KeyPress {
                key: Key::Char('a'),
                mods: Mods {
                    ctrl: true,
                    ..Default::default()
                },
            }],
            ..Default::default()
        });
        f.tick(Input {
            text: "2500".into(),
            ..Default::default()
        });
        f.tick(Input {
            keys: vec![KeyPress {
                key: Key::Enter,
                mods: Mods::default(),
            }],
            ..Default::default()
        });
        f.idle(3);
        assert_eq!(
            f.values.borrow()[&ControlId(901)],
            2500.,
            "typing writes physical Hz, without a second normalization"
        );
        f.ui.focus("effect-filter-reset");
        f.tick(Input {
            keys: vec![KeyPress {
                key: Key::Enter,
                mods: Mods::default(),
            }],
            ..Default::default()
        });
        f.idle(3);
        assert_eq!(f.values.borrow()[&ControlId(901)], 1000.);
        assert_eq!(f.values.borrow()[&ControlId(902)], 0.707);
        f.values.borrow_mut().insert(ControlId(902), f64::NAN);
        f.writes.borrow_mut().clear();
        f.idle(3);
        assert!(
            f.ui.scene()
                .unwrap()
                .surface("effect-filter-graph")
                .is_none(),
            "invalid lane readback must not become a false preview"
        );
        assert!(f.writes.borrow().is_empty());
    }

    #[test]
    fn normalized_lanes_use_the_shared_preview_adapter_without_hertz_labels() {
        let mut f = Fixture::new(900., 600.);
        let cutoff = &mut f.descriptors[1];
        cutoff.unit = ParameterUnit::Normalized;
        cutoff.range = [0., 1.];
        cutoff.default = 0.5;
        cutoff.law = ParameterLaw::Linear;
        f.values.borrow_mut().insert(ControlId(901), 0.5);
        f.frequency_hz = |lane, _| 20. * 1000f64.powf(lane);
        f.idle(4);
        let scene = f.ui.scene().unwrap();
        assert_eq!(
            scene
                .surface("effect-value-901")
                .unwrap()
                .text_value
                .as_deref(),
            Some("0.500")
        );
        let graph = scene.surface("effect-filter-graph").unwrap().frame;
        let handle = scene.surface("effect-filter-handle").unwrap().frame;
        let x = handle.x + handle.size.width / 2. - graph.x;
        assert!(
            (x - graph.size.width / 2.).abs() < 0.5,
            "the adapter's geometric midpoint is the physical graph midpoint"
        );
        let expected = SPACE
            + (1. - f64::from(super::super::viz::db_y((20. * 0.707f64.log10()) as f32)))
                * (graph.size.height - 2. * SPACE);
        let y = handle.y + handle.size.height / 2. - graph.y;
        assert!(
            (y - expected).abs() < 0.5,
            "the node lies on the playback low-pass response: magnitude at cutoff is Q"
        );
        assert!(f.writes.borrow().is_empty());
    }

    #[test]
    fn lp4_uses_shared_cutoff_and_response_without_claiming_physical_resonance_units() {
        for (w, h) in [(900., 600.), (1180., 900.)] {
            let mut f = Fixture::lp4(w, h);
            let scene = f.ui.scene().unwrap();
            let graph = scene
                .surface("effect-filter-graph")
                .expect("dedicated LP4 preview")
                .frame;
            for (id, value) in [(901, "0.500"), (902, "0.600")] {
                assert_eq!(
                    scene
                        .surface(&format!("effect-value-{id}"))
                        .unwrap()
                        .text_value
                        .as_deref(),
                    Some(value)
                );
            }
            let at = scene.surface("effect-filter-handle").unwrap().frame;
            let hz = f64::from(sampler_core::LadderSettings::cutoff_hz(0.5));
            let magnitude =
                sampler_core::LadderSettings::magnitude([0.5, 0.6, 0.], hz as f32, 48000);
            let expected = editor::place(
                graph.size,
                [
                    frequency_x(hz, 48000),
                    viz::db_y(20. * magnitude.max(1e-6).log10()).clamp(0., 1.),
                ],
            );
            assert!((at.x + SPACE / 2. - graph.x - expected.x).abs() < 0.5);
            assert!(
                (at.y + SPACE / 2. - graph.y - expected.y).abs() < 0.5,
                "node uses the playback LP4 kernel"
            );
            assert!(
                f.writes.borrow().is_empty(),
                "painting preserves normalized lanes"
            );
            let at = super::super::tests::center(&f.ui, "effect-filter-handle");
            f.pointer(at, true);
            f.pointer(Point::new(at.x + 50., at.y - 30.), true);
            f.pointer(Point::new(at.x + 50., at.y - 30.), false);
            assert!(f.values.borrow()[&ControlId(901)] > 0.5);
            assert!(f.values.borrow()[&ControlId(902)] > 0.6);
            assert_eq!(
                f.values.borrow()[&ControlId(903)],
                0.,
                "cutoff/resonance gesture leaves drive alone"
            );
            assert!(
                f.writes
                    .borrow()
                    .iter()
                    .all(|(id, value)| [ControlId(901), ControlId(902)].contains(id)
                        && (0.0..=1.0).contains(value))
            );
        }
    }

    #[test]
    #[cfg(feature = "shots")]
    fn lp4_panel_shots() {
        let Some(out) = std::env::var_os("KONTRA_LP4_SHOTS").map(std::path::PathBuf::from) else {
            return;
        };
        std::fs::create_dir_all(&out).unwrap();
        for (w, h) in [(900, 600), (1180, 900)] {
            let mut f = Fixture::lp4(w as f64, h as f64);
            f.idle(20);
            moose::core::screenshot::save_png(
                &out.join(format!("lp4-{w}.png")),
                &super::super::tests::pixels(&f.ui, w, h),
                w.into(),
                h.into(),
            );
        }
    }

    #[test]
    #[cfg(feature = "shots")]
    fn filter_panel_shots() {
        let Some(out) = std::env::var_os("KONTRA_FILTER_SHOTS").map(std::path::PathBuf::from)
        else {
            return;
        };
        std::fs::create_dir_all(&out).unwrap();
        for (w, h) in [(900, 600), (1180, 900)] {
            let mut f = Fixture::new(w as f64, h as f64);
            f.idle(20);
            moose::core::screenshot::save_png(
                &out.join(format!("filter-{w}.png")),
                &super::super::tests::pixels(&f.ui, w, h),
                w.into(),
                h.into(),
            );
        }
    }
}
