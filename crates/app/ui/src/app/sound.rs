//! A part's sound, as v2 showed it: a group's amplitude envelope and its
//! filter and EQ response, drawn from the values the engine plays, with
//! handles that set them for every group or the shown one alone; under
//! them the group's zones, what modulates it, and its effects.

use mui::prelude::*;

use super::{Action, Part, View};
use crate::keyboard::note_name;
use crate::kit::{self, Icon, icon_button, latch, section};
use crate::theme::{CONTROL, SPACE, TEXT, TIGHT, hairline, lift, part_color, secondary};

/// A group's sound as the engine plays it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sound {
    /// The amplitude envelope; none: notes start and stop at once.
    pub envelope: Option<Envelope>,
    /// Its filters and EQ bands, in series.
    pub filters: Vec<Filter>,
    /// What modulates it: source, target and depth, as words.
    pub mods: Vec<(String, String, String)>,
    /// Its effects, by slot.
    pub effects: Vec<String>,
}

/// Times in seconds, sustain 0..=1; decay and release fall 60 dB.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Envelope {
    pub attack: f64,
    pub hold: f64,
    pub decay: f64,
    pub sustain: f64,
    pub release: f64,
}

/// A filter or an EQ band at `hz`: its Q (a filter's resonance), its gain
/// in dB (a bell's or shelf's; a filter's output), and a filter's poles.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Filter {
    pub pass: Pass,
    pub hz: f64,
    pub q: f64,
    pub gain_db: f64,
    pub poles: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pass {
    #[default]
    Low,
    High,
    Band,
    Notch,
    Peak,
    LowShelf,
    HighShelf,
}

/// A value of a group's sound: an envelope stage, or filter `n`'s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Param {
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
    Hz(usize),
    Q(usize),
    Gain(usize),
}

/// What shows under the graphs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lower {
    #[default]
    Zones,
    Modulation,
    Effects,
}

const ENVELOPE: usize = 0;
const RESPONSE: usize = 1;

impl Param {
    fn slot(self, s: &mut Sound) -> Option<&mut f64> {
        let e = s.envelope.as_mut();
        match self {
            Self::Attack => e.map(|e| &mut e.attack),
            Self::Hold => e.map(|e| &mut e.hold),
            Self::Decay => e.map(|e| &mut e.decay),
            Self::Sustain => e.map(|e| &mut e.sustain),
            Self::Release => e.map(|e| &mut e.release),
            Self::Hz(n) => s.filters.get_mut(n).map(|f| &mut f.hz),
            Self::Q(n) => s.filters.get_mut(n).map(|f| &mut f.q),
            Self::Gain(n) => s.filters.get_mut(n).map(|f| &mut f.gain_db),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Attack => "Attack",
            Self::Hold => "Hold",
            Self::Decay => "Decay",
            Self::Sustain => "Sustain",
            Self::Release => "Release",
            Self::Hz(_) => "Frequency",
            Self::Q(_) => "Q",
            Self::Gain(_) => "Gain",
        }
    }

    fn text(self, v: f64) -> String {
        match self {
            Self::Sustain => format!("{:.0} %", v * 100.),
            Self::Hz(_) if v >= 1000. => format!("{:.2} kHz", v / 1000.),
            Self::Hz(_) => format!("{v:.0} Hz"),
            Self::Q(_) => format!("{v:.2}"),
            Self::Gain(_) => format!("{v:+.1} dB"),
            _ if v >= 1. => format!("{v:.2} s"),
            _ => format!("{:.0} ms", v * 1000.),
        }
    }
}

// Envelope ---------------------------------------------------------------

/// The seconds the envelope graph spans: its stages and a sustain a
/// fifth of the whole.
fn span(e: &Envelope) -> f64 {
    ((e.attack + e.hold + e.decay + e.release) * 1.25).max(0.01)
}

/// Where the stages meet, in seconds and level: the attack's peak, the
/// hold's end, the decay's end at the sustain, the key's release, silence.
fn corners(e: &Envelope, span: f64) -> [[f64; 2]; 5] {
    let a = e.attack;
    let h = a + e.hold;
    let d = h + e.decay;
    let off = d + span / 5.;
    [
        [a, 1.],
        [h, 1.],
        [d, e.sustain],
        [off, e.sustain],
        [off + e.release, 0.],
    ]
}

/// The handles: attack, hold, decay (and sustain), release.
fn envelope_handles(e: &Envelope, span: f64) -> Vec<[f64; 2]> {
    let c = corners(e, span);
    [0, 1, 2, 4]
        .iter()
        .filter_map(|&i| c.get(i))
        .map(|&[t, l]| [t / span, l])
        .collect()
}

/// Handle `i` moved to `t` seconds and level `l`.
fn envelope_edit(e: &Envelope, span: f64, i: usize, [t, l]: [f64; 2]) -> Vec<(Param, f64)> {
    let c = corners(e, span);
    let from = |k: usize| c.get(k).map_or(0., |p| p[0]);
    match i {
        0 => vec![(Param::Attack, t.max(0.))],
        1 => vec![(Param::Hold, (t - from(0)).max(0.))],
        2 => vec![
            (Param::Decay, (t - from(1)).max(0.)),
            (Param::Sustain, l.clamp(0., 1.)),
        ],
        _ => vec![(Param::Release, (t - from(3)).max(0.001))],
    }
}

/// The envelope's line in unit coordinates: a linear attack, decay and
/// release falling 60 dB.
// ponytail: linear attack and exponential falls; the IR's stage shapes when the host passes them.
fn envelope_line(e: &Envelope, span: f64) -> Vec<[f64; 2]> {
    let c = corners(e, span);
    let [a, h, d, off, end] = c.map(|p| p[0] / span);
    let s = e.sustain;
    let mut line = vec![[0., 0.], [a, 1.], [h, 1.]];
    let fall = |from: f64, to: f64, x0: f64, x1: f64, line: &mut Vec<[f64; 2]>| {
        for k in 1..=24 {
            let u = f64::from(k) / 24.;
            line.push([x0 + u * (x1 - x0), to + (from - to) * 10f64.powf(-3. * u)]);
        }
    };
    fall(1., s, h, d, &mut line);
    line.push([off, s]);
    fall(s, 0., off, end, &mut line);
    line
}

// Response ---------------------------------------------------------------

/// 20 Hz to 20 kHz across.
fn freq_x(hz: f64) -> f64 {
    (hz.max(1.) / 20.).log10() / 3.
}

fn x_freq(x: f64) -> f64 {
    20. * 10f64.powf(3. * x)
}

/// -36 dB to +18 dB up.
fn db_y(db: f64) -> f64 {
    ((db + 36.) / 54.).clamp(0., 1.)
}

fn y_db(y: f64) -> f64 {
    y * 54. - 36.
}

/// `f`'s linear gain at `hz`, from its analog prototype.
fn gain(f: &Filter, hz: f64) -> f64 {
    let w = hz / f.hz.max(1.);
    let q = f.q.max(0.1);
    // |a·s² + b·s + c| at s = jw.
    let two = |a: f64, b: f64, c: f64| (c - a * w * w).hypot(b * w);
    let resonant = two(1., 1. / q, 1.);
    let gentle = if f.poles >= 4 {
        two(1., std::f64::consts::SQRT_2, 1.)
    } else {
        1.
    };
    let out = 10f64.powf(f.gain_db / 20.);
    let a = 10f64.powf(f.gain_db / 40.);
    let b = a.sqrt() / q;
    match f.pass {
        Pass::Low if f.poles <= 1 => out / 1f64.hypot(w),
        Pass::High if f.poles <= 1 => out * w / 1f64.hypot(w),
        Pass::Low => out / resonant / gentle,
        Pass::High => out * (w * w / resonant) * if f.poles >= 4 { w * w / gentle } else { 1. },
        Pass::Band => out * (w / resonant) * if f.poles >= 4 { w / gentle } else { 1. },
        Pass::Notch => out * (1. - w * w).abs() / resonant,
        Pass::Peak => two(1., a / q, 1.) / two(1., 1. / (a * q), 1.),
        Pass::LowShelf => a * two(1., b, a) / two(a, b, 1.),
        Pass::HighShelf => a * two(a, b, 1.) / two(1., b, a),
    }
}

fn is_band(f: &Filter) -> bool {
    matches!(f.pass, Pass::Peak | Pass::LowShelf | Pass::HighShelf)
}

/// The filters in series, 20 Hz to 20 kHz, in unit coordinates.
fn response(filters: &[Filter]) -> Vec<[f64; 2]> {
    (0..=160)
        .map(|k| {
            let x = f64::from(k) / 160.;
            let g: f64 = filters.iter().map(|f| gain(f, x_freq(x))).product();
            [x, db_y(20. * g.max(1e-6).log10())]
        })
        .collect()
}

/// A band at its frequency and gain; a filter at its cutoff and its Q in dB.
fn response_handles(filters: &[Filter]) -> Vec<[f64; 2]> {
    filters
        .iter()
        .map(|f| {
            let db = if is_band(f) {
                f.gain_db
            } else {
                20. * f.q.max(0.1).log10()
            };
            [freq_x(f.hz), db_y(db)]
        })
        .collect()
}

/// Handle `i` moved to `[x, y]`: frequency across, a band's gain or a
/// filter's Q up.
fn response_edit(filters: &[Filter], i: usize, [x, y]: [f64; 2]) -> Vec<(Param, f64)> {
    let Some(f) = filters.get(i) else {
        return Vec::new();
    };
    let up = if is_band(f) {
        (Param::Gain(i), y_db(y).clamp(-24., 18.))
    } else {
        (Param::Q(i), 10f64.powf(y_db(y) / 20.).clamp(0.3, 30.))
    };
    vec![(Param::Hz(i), x_freq(x.clamp(0., 1.))), up]
}

// View -------------------------------------------------------------------

/// Unit coordinates on a canvas of `s`, inset so handles are not clipped.
fn place(s: Size, [x, y]: [f64; 2]) -> Point {
    Point::new(
        SPACE + x * (s.width - 2. * SPACE).max(1.),
        SPACE + (1. - y) * (s.height - 2. * SPACE).max(1.),
    )
}

fn unplace(s: Size, p: Point) -> [f64; 2] {
    [
        (p.x - SPACE) / (s.width - 2. * SPACE).max(1.),
        1. - (p.y - SPACE) / (s.height - 2. * SPACE).max(1.),
    ]
}

pub(super) fn view(ui: &mut Ui, p: &mut Part, n: usize, v: &mut View, out: &mut Vec<Action>) -> El {
    if p.sounds.is_empty() {
        return col![
            section("Sound"),
            body("This instrument has no groups to edit.").fill(secondary())
        ]
        .gap(SPACE)
        .align(Align::Start)
        .pad(SPACE);
    }
    let count = p.sounds.len();
    let g = v.sound_group.min(count - 1);
    let prev = icon_button(
        ui,
        format!("part-{n}-sound-prev"),
        Icon::Left,
        "Previous group",
        false,
    );
    let next = icon_button(
        ui,
        format!("part-{n}-sound-next"),
        Icon::Right,
        "Next group",
        false,
    );
    let g = if prev.changed {
        (g + count - 1) % count
    } else if next.changed {
        (g + 1) % count
    } else {
        g
    };
    v.sound_group = g;
    let all = latch(
        ui,
        format!("part-{n}-sound-all"),
        "All groups",
        "Edits set every group",
        !v.sound_one,
    );
    let one = latch(
        ui,
        format!("part-{n}-sound-one"),
        "This group",
        "Edits set this group alone",
        v.sound_one,
    );
    if all.changed || one.changed {
        v.sound_one = one.changed;
    }
    let name = p
        .groups
        .get(g)
        .cloned()
        .unwrap_or_else(|| format!("Group {}", g + 1));
    let toolbar = kit::strip(vec![
        row![prev.el, body(name).lines(1).min_w(0).w(TEXT * 14.), next.el]
            .gap(TIGHT)
            .align(Align::Center),
        row![all.el, one.el].gap(1),
        spacer(),
        caption(format!("Group {} of {count}", g + 1)).fill(secondary()),
    ]);

    let mut edits = Vec::new();
    let tint = part_color(g);
    let sound = p.sounds.get(g).cloned().unwrap_or_default();
    let envelope = envelope_panel(ui, &sound, n, v, tint, &mut edits);
    let response = response_panel(ui, &sound, n, v, tint, &mut edits);
    for (param, value) in edits {
        let targets: Vec<usize> = if v.sound_one {
            vec![g]
        } else {
            (0..count).collect()
        };
        for t in targets {
            if let Some(slot) = p.sounds.get_mut(t).and_then(|s| param.slot(s)) {
                *slot = value;
            }
        }
        out.push(Action::Sound {
            part: n,
            group: v.sound_one.then_some(g),
            param,
            value,
        });
    }
    col![
        toolbar,
        row![envelope.flex(1).min_w(0), response.flex(1).min_w(0)]
            .gap(SPACE)
            .align(Align::Start),
        lower(ui, p, n, g, v, tint),
    ]
    .gap(SPACE)
    .w(Len::Pct(100.))
}

/// Presses and drags on graph `which`'s canvas `id` among `handles` (unit
/// coordinates): the handle held and where to now.
fn drag(
    ui: &mut Ui,
    id: &str,
    which: usize,
    handles: &[[f64; 2]],
    axis: f64,
    v: &mut View,
) -> Option<(usize, [f64; 2], f64)> {
    let size = ui
        .scene()
        .and_then(|s| s.surface(id))
        .map(|s| s.frame.size)?;
    let at = ui.local(id);
    let r = ui.get(id);
    if r.pressed
        && let Some(at) = at
    {
        let near = handles
            .iter()
            .enumerate()
            .map(|(i, &h)| {
                let p = place(size, h);
                (i, (p.x - at.x).hypot(p.y - at.y))
            })
            .filter(|&(_, d)| d < TEXT)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, _)) = near {
            v.sound_grab = Some((which, i, axis));
            if let Some(f) = v.sound_focus.get_mut(which) {
                *f = i;
            }
        }
    }
    let held = v.sound_grab.filter(|g| g.0 == which);
    if r.released || !r.held {
        v.sound_grab = v.sound_grab.filter(|g| g.0 != which);
    }
    let (_, i, axis) = held?;
    Some((i, unplace(size, at?), axis))
}

fn graph(id: String, title: &str, hint: &str, draw: impl Fn(Size) -> Vec<Draw> + 'static) -> El {
    canvas(draw)
        .w(Len::Pct(100.))
        .h(TEXT * 14.)
        .fill(lift(1))
        .clip()
        .cursor(Cursor::Grab)
        .tracks_pointer()
        .named(format!("{title} graph"))
        .tip(hint.to_owned())
        .id(id)
}

fn squares(s: Size, handles: &[[f64; 2]], focus: usize, tint: Color) -> Vec<Draw> {
    let mut out = Vec::new();
    for (i, &h) in handles.iter().enumerate() {
        let p = place(s, h);
        let r = if i == focus { 4.5 } else { 3.5 };
        let sq = Path::polyline(
            [(-r, -r), (r, -r), (r, r), (-r, r)].map(|(x, y)| Point::new(p.x + x, p.y + y)),
            true,
        );
        out.push(Draw::fill(sq.clone(), Fill::from(tint)));
        out.push(Draw::stroke(sq, Fill::from(Role::Ink), 1.));
    }
    out
}

fn readouts(cells: Vec<(Param, f64)>) -> El {
    row(cells
        .into_iter()
        .map(|(p, v)| {
            col![section(p.label()), body(p.text(v)).reserve("00.00 kHz")]
                .gap(1)
                .align(Align::Start)
        })
        .collect::<Vec<_>>())
    .gap(SPACE)
}

fn envelope_panel(
    ui: &mut Ui,
    s: &Sound,
    n: usize,
    v: &mut View,
    tint: Color,
    edits: &mut Vec<(Param, f64)>,
) -> El {
    let Some(e) = s.envelope else {
        return col![
            section("Envelope"),
            caption("No volume envelope: notes start and stop at once.").fill(secondary())
        ]
        .gap(SPACE)
        .align(Align::Start);
    };
    let id = format!("part-{n}-envelope");
    let grabbed = v.sound_grab.filter(|g| g.0 == ENVELOPE).map(|g| g.2);
    // A held handle keeps the axis it was pressed on.
    let span = grabbed.unwrap_or_else(|| span(&e));
    let handles = envelope_handles(&e, span);
    if let Some((i, at, span)) = drag(ui, &id, ENVELOPE, &handles, span, v) {
        edits.extend(envelope_edit(&e, span, i, [at[0] * span, at[1]]));
    }
    let line = envelope_line(&e, span);
    let focus = v.sound_focus.get(ENVELOPE).copied().unwrap_or(0);
    let draw = move |s: Size| {
        let mut out = vec![Draw::fill(
            Path::polyline(line.iter().map(|&p| place(s, p)), true),
            Fill::from(tint.with_alpha(0.14)),
        )];
        out.push(Draw::stroke(
            Path::polyline(line.iter().map(|&p| place(s, p)), false),
            Fill::from(tint),
            1.5,
        ));
        out.extend(squares(s, &handles, focus, tint));
        out
    };
    col![
        row![section("Envelope"), spacer()],
        graph(id, "Envelope", "Drag a handle to shape the envelope", draw),
        readouts(vec![
            (Param::Attack, e.attack),
            (Param::Hold, e.hold),
            (Param::Decay, e.decay),
            (Param::Sustain, e.sustain),
            (Param::Release, e.release),
        ]),
    ]
    .gap(TIGHT)
    .align(Align::Stretch)
}

fn response_panel(
    ui: &mut Ui,
    s: &Sound,
    n: usize,
    v: &mut View,
    tint: Color,
    edits: &mut Vec<(Param, f64)>,
) -> El {
    let id = format!("part-{n}-response");
    let handles = response_handles(&s.filters);
    if let Some((i, at, _)) = drag(ui, &id, RESPONSE, &handles, 0., v) {
        edits.extend(response_edit(&s.filters, i, at));
    }
    let line = response(&s.filters);
    let flat = s.filters.is_empty();
    let focus = v.sound_focus.get(RESPONSE).copied().unwrap_or(0);
    let draw = move |s: Size| {
        let rule = |a: Point, b: Point| Path::polyline([a, b], false);
        let mut out = Vec::new();
        for hz in [100., 1000., 10_000.] {
            let x = place(s, [freq_x(hz), 0.]).x;
            out.push(Draw::stroke(
                rule(Point::new(x, 0.), Point::new(x, s.height)),
                hairline(),
                1.,
            ));
        }
        for db in [-24., -12., 0., 12.] {
            let y = place(s, [0., db_y(db)]).y;
            out.push(Draw::stroke(
                rule(Point::new(0., y), Point::new(s.width, y)),
                hairline(),
                1.,
            ));
        }
        let path = |close| Path::polyline(line.iter().map(|&p| place(s, p)), close);
        if flat {
            out.push(Draw::stroke(path(false), Role::Ink.alpha(0.45), 1.5));
            return out;
        }
        let mut floor = line.clone();
        floor.extend([[1., 0.], [0., 0.]]);
        out.push(Draw::fill(
            Path::polyline(floor.iter().map(|&p| place(s, p)), true),
            Fill::from(tint.with_alpha(0.14)),
        ));
        out.push(Draw::stroke(path(false), Fill::from(tint), 1.5));
        out.extend(squares(s, &handles, focus, tint));
        out
    };
    let shown = match s.filters.get(focus) {
        Some(f) if is_band(f) => vec![
            (Param::Hz(focus), f.hz),
            (Param::Gain(focus), f.gain_db),
            (Param::Q(focus), f.q),
        ],
        Some(f) => vec![(Param::Hz(focus), f.hz), (Param::Q(focus), f.q)],
        None => Vec::new(),
    };
    let what = s.filters.get(focus).map_or_else(
        || "No filter or EQ · flat".to_owned(),
        |f| format!("{} {} of {}", kind(f), focus + 1, s.filters.len()),
    );
    col![
        row![
            section("Filter · EQ"),
            spacer(),
            caption(what).fill(secondary())
        ],
        graph(
            id,
            "Filter · EQ",
            "Drag a handle: across for frequency, up for gain or resonance",
            draw
        ),
        row![
            caption("20 Hz").fill(secondary()),
            spacer(),
            caption("1 kHz").fill(secondary()),
            spacer(),
            caption("20 kHz").fill(secondary())
        ],
        readouts(shown),
    ]
    .gap(TIGHT)
    .align(Align::Stretch)
}

fn kind(f: &Filter) -> String {
    let pass = match f.pass {
        Pass::Low => "Low-pass",
        Pass::High => "High-pass",
        Pass::Band => "Band-pass",
        Pass::Notch => "Notch",
        Pass::Peak => return "Bell".into(),
        Pass::LowShelf => return "Low shelf".into(),
        Pass::HighShelf => return "High shelf".into(),
    };
    format!("{pass} {}-pole", f.poles.max(1))
}

/// Under the graphs: the group's zones, what modulates it, or its effects.
fn lower(ui: &mut Ui, p: &Part, n: usize, g: usize, v: &mut View, tint: Color) -> El {
    let mut latches = Vec::new();
    for (which, label, name) in [
        (Lower::Zones, "Zones", "The group's zones"),
        (Lower::Modulation, "Modulation", "What modulates the group"),
        (Lower::Effects, "Effects", "The group's effects"),
    ] {
        let r = latch(
            ui,
            format!("part-{n}-sound-{which:?}"),
            label,
            name,
            v.sound_lower == which,
        );
        if r.changed {
            v.sound_lower = which;
        }
        latches.push(r.el);
    }
    let sound = p.sounds.get(g);
    let lines = |rows: Vec<String>, empty: &str| {
        if rows.is_empty() {
            return caption(empty.to_owned()).fill(secondary());
        }
        col(rows
            .into_iter()
            .map(|r| body(r).lines(1).min_w(0))
            .collect::<Vec<_>>())
        .gap(TIGHT)
        .align(Align::Start)
    };
    let body = match v.sound_lower {
        Lower::Zones => zones(p, g, tint),
        Lower::Modulation => lines(
            sound
                .map(|s| {
                    s.mods
                        .iter()
                        .map(|(from, to, by)| format!("{from} to {to} · {by}"))
                        .collect()
                })
                .unwrap_or_default(),
            "Nothing modulates this group.",
        ),
        Lower::Effects => lines(
            sound
                .map(|s| {
                    s.effects
                        .iter()
                        .enumerate()
                        .map(|(i, e)| format!("{} · {e}", i + 1))
                        .collect()
                })
                .unwrap_or_default(),
            "This group has no effects.",
        ),
    };
    col![row(latches).gap(1), body]
        .gap(SPACE)
        .align(Align::Stretch)
}

/// The group's zones on a key × velocity strip.
fn zones(p: &Part, g: usize, tint: Color) -> El {
    let cells: Vec<(u8, u8, u8, u8)> = (p.zones.iter())
        .filter(|z| z.group == g)
        .map(|z| (z.keys.0, z.keys.1, z.velocities.0, z.velocities.1))
        .collect();
    let map = canvas(move |s| {
        let at = |key: u8, vel: u8| {
            Point::new(
                f64::from(key) / 128. * s.width,
                f64::from(127 - vel.min(127)) / 128. * s.height,
            )
        };
        let mut out = Vec::new();
        for k in (12..128).step_by(12) {
            out.push(Draw::stroke(
                Path::polyline([at(k, 127), at(k, 0)], false),
                hairline(),
                1.,
            ));
        }
        for &(lo, hi, lv, hv) in &cells {
            let (a, b) = (at(lo, hv), at(hi.saturating_add(1), lv.saturating_sub(1)));
            let r = Path::polyline(
                [
                    (a.x, a.y),
                    (b.x - 1., a.y),
                    (b.x - 1., b.y - 1.),
                    (a.x, b.y - 1.),
                ]
                .map(|(x, y)| Point::new(x, y)),
                true,
            );
            out.push(Draw::fill(r, Fill::from(tint.with_alpha(0.3))));
        }
        out
    })
    .w(Len::Pct(100.))
    .h(CONTROL * 2.)
    .fill(lift(1))
    .clip()
    .named("The group's zones by key and velocity");
    col![
        map,
        row![
            caption(note_name(0)).fill(secondary()),
            spacer(),
            caption(note_name(127)).fill(secondary())
        ]
    ]
    .gap(TIGHT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_land_on_the_curves_and_a_drag_reads_back_its_value() {
        let e = Envelope {
            attack: 0.1,
            hold: 0.,
            decay: 0.5,
            sustain: 0.5,
            release: 0.4,
        };
        let span = span(&e);
        let hs = envelope_handles(&e, span);
        // The decay handle sits at the sustain; dragging it to where it is
        // sets what it shows.
        let decay = hs.get(2).copied().unwrap_or_default();
        assert_eq!(decay[1], 0.5);
        let back = envelope_edit(&e, span, 2, [decay[0] * span, decay[1]]);
        assert!(
            back.iter()
                .any(|&(p, v)| p == Param::Decay && (v - 0.5).abs() < 1e-9)
        );
        let release = hs.get(3).copied().unwrap_or_default();
        let back = envelope_edit(&e, span, 3, [release[0] * span, 0.]);
        assert!(
            back.iter()
                .any(|&(p, v)| p == Param::Release && (v - 0.4).abs() < 1e-9)
        );

        // A 4-pole low-pass: flat below, -3 dB-ish at Q 0.707 cutoff, 24 dB
        // an octave down past it; a bell lifts its gain at its centre.
        let lp = Filter {
            pass: Pass::Low,
            hz: 1000.,
            q: std::f64::consts::FRAC_1_SQRT_2,
            gain_db: 0.,
            poles: 4,
        };
        let db = |f: &Filter, hz| 20. * gain(f, hz).log10();
        assert!(db(&lp, 50.).abs() < 0.1);
        assert!((db(&lp, 1000.) + 6.).abs() < 0.2);
        assert!((db(&lp, 8000.) - db(&lp, 4000.) + 24.).abs() < 1.);
        let bell = Filter {
            pass: Pass::Peak,
            hz: 2000.,
            q: 1.,
            gain_db: 6.,
            poles: 0,
        };
        assert!((db(&bell, 2000.) - 6.).abs() < 1e-9);
        let h = response_handles(&[lp, bell]);
        let back = response_edit(&[lp, bell], 1, h.get(1).copied().unwrap_or_default());
        assert!(
            back.iter()
                .any(|&(p, v)| p == Param::Hz(1) && (v - 2000.).abs() < 1e-6)
        );
        assert!(
            back.iter()
                .any(|&(p, v)| p == Param::Gain(1) && (v - 6.).abs() < 1e-9)
        );
    }
}
