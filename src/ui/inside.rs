//! A part's views beside its interface, from the translated instrument
//! (`PartView.instrument`): its articulations, how its zones map keys and
//! velocities, its envelopes, filters and modulation, and what it is.

use super::{Cx, theme::*};
use moose::mui::mui::geometry::Path as DrawPath;
use moose::mui::mui::prelude::*;
use sampler_ir as ir;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum View {
    #[default]
    Interface,
    Articulations,
    Mapping,
    Sound,
    Info,
}

impl View {
    const ALL: [Self; 5] = [Self::Interface, Self::Articulations, Self::Mapping, Self::Sound, Self::Info];

    fn label(self) -> &'static str {
        match self {
            Self::Interface => "Interface",
            Self::Articulations => "Articulations",
            Self::Mapping => "Mapping",
            Self::Sound => "Sound",
            Self::Info => "Info",
        }
    }
}

/// What selects an articulation. Mirrors `sampler_ir::Driver` on the
/// expression branch; the core applies it once `Part` carries it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Driver {
    #[default]
    Keys,
    Velocity,
    Channel,
    Controller,
    Program,
}

impl Driver {
    const ALL: [Self; 5] = [Self::Keys, Self::Velocity, Self::Channel, Self::Controller, Self::Program];

    fn label(self) -> &'static str {
        match self {
            Self::Keys => "Keyswitch",
            Self::Velocity => "Velocity",
            Self::Channel => "Channel",
            Self::Controller => "CC 32",
            Self::Program => "Program",
        }
    }
}

/// A part's views across frames.
#[derive(Default)]
pub struct State {
    /// `None` until picked: the interface when there is one, else Info.
    pub view: Option<View>,
    pub driver: Driver,
    /// The articulation playing, following switch keys as they are played.
    pub active: Option<usize>,
    /// The group the mapping picks out.
    pub group: Option<usize>,
}

/// Which views `slot` has something to show in.
fn offered(cx: &Cx, slot: usize) -> Vec<View> {
    let v = &cx.view.parts[slot];
    let has_face = v.interfaces.iter().any(|i| !i.widgets.is_empty());
    let inst = v.instrument.as_deref();
    View::ALL
        .into_iter()
        .filter(|view| match view {
            View::Interface => has_face,
            View::Articulations => inst.is_some_and(|i| !i.articulations.is_empty()),
            View::Mapping | View::Sound => inst.is_some_and(|i| !i.zones.is_empty()),
            View::Info => true,
        })
        .collect()
}

/// The view switch above the stage, and the view it picks.
pub fn bar(ui: &mut Ui, cx: &mut Cx, slot: usize) -> (View, Option<El>) {
    let offered = offered(cx, slot);
    let st = cx.state.inside.entry(slot).or_default();
    let view = st.view.filter(|v| offered.contains(v)).unwrap_or(offered[0]);
    if offered.len() < 2 {
        return (view, None);
    }
    let mut picked = view;
    let tabs = offered
        .iter()
        .map(|&v| {
            let (hit, el) = latch(ui, format!("view-{slot}-{}", v.label()), v.label(), v.label(), v == view);
            if hit {
                picked = v;
            }
            el
        })
        .collect();
    cx.state.inside.entry(slot).or_default().view = Some(picked);
    (picked, Some(segmented(tabs)))
}

/// The picked view's body; the interface is [`super::part`]'s.
pub fn view(ui: &mut Ui, cx: &mut Cx, slot: usize, view: View) -> Option<El> {
    let inst = cx.view.parts[slot].instrument.clone();
    let el = match (view, inst) {
        (View::Articulations, Some(i)) => articulations(ui, cx, slot, &i),
        (View::Mapping, Some(i)) => mapping(ui, cx, slot, &i),
        (View::Sound, Some(i)) => sound(&i),
        (View::Info, i) => info(cx, slot, i.as_deref()),
        _ => return None,
    };
    Some(el.pad((SPACE, INSET)).w(Len::Pct(100.)).shrink(0).id(format!("inside-{slot}")))
}

// Articulations ---------------------------------------------------------

/// Row `n`'s place among `arts` when alternatives are handed out: by lowest
/// switch key, keyless ones last in list order (`assign_alternatives`).
fn order(arts: &[ir::Articulation]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..arts.len()).collect();
    order.sort_by_key(|&i| (arts[i].switch_keys.iter().min().copied().unwrap_or(u8::MAX), i));
    let mut place = vec![0; arts.len()];
    for (p, &i) in order.iter().enumerate() {
        place[i] = p;
    }
    place
}

/// How `driver` reaches the articulation at `place` of `count`, or `None`
/// when that family cannot fit them all.
pub fn trigger(driver: Driver, keys: &[u8], place: usize, count: usize) -> Option<String> {
    match driver {
        Driver::Keys => match keys {
            [] => None,
            [k] => Some(note_name(*k)),
            [k, .., l] => Some(format!("{}–{}", note_name(*k), note_name(*l))),
        },
        Driver::Velocity => (count <= 127).then(|| {
            let (lo, hi) = (1 + 127 * place / count, 127 * (place + 1) / count);
            format!("vel {lo}–{hi}")
        }),
        Driver::Channel => (count <= 16).then(|| format!("ch {}", place + 1)),
        Driver::Controller => (count <= 128).then(|| format!("CC32 = {place}")),
        Driver::Program => (count <= 128).then(|| format!("prog {}", place + 1)),
    }
}

/// The articulation whose switch key is down now, if any.
fn played(cx: &Cx, arts: &[ir::Articulation]) -> Option<usize> {
    let shared = &cx.p.shared;
    let down = |k: u8| shared.played[k as usize].load(Ordering::Relaxed) > 0 || shared.heard[k as usize].load(Ordering::Relaxed) > 0;
    arts.iter().position(|a| a.switch_keys.iter().any(|&k| down(k)))
}

/// The articulation `slot` plays: the last switched to, else the default.
pub fn active(cx: &mut Cx, slot: usize) -> Option<usize> {
    let inst = cx.view.parts.get(slot)?.instrument.clone()?;
    let arts = &inst.articulations;
    let now = played(cx, arts);
    let st = cx.state.inside.entry(slot).or_default();
    if now.is_some() {
        st.active = now;
    }
    st.active.or_else(|| arts.iter().position(|a| a.default)).or((!arts.is_empty()).then_some(0))
}

fn articulations(ui: &mut Ui, cx: &mut Cx, slot: usize, inst: &ir::Instrument) -> El {
    let arts = &inst.articulations;
    let active = active(cx, slot);
    let st = cx.state.inside.entry(slot).or_default();
    let mut driver = st.driver;
    let tabs = Driver::ALL
        .into_iter()
        .map(|d| {
            let fits = trigger(d, &[0], arts.len().saturating_sub(1), arts.len()).is_some();
            let (hit, el) = latch(ui, format!("remap-{slot}-{}", d.label()), d.label(), &format!("Remap every articulation to {}", d.label()), d == driver);
            if hit && fits {
                driver = d;
            }
            if fits { el } else { el.opacity(0.4) }
        })
        .collect();
    cx.state.inside.entry(slot).or_default().driver = driver;
    let head = row![section("Articulations"), caption(format!("{}", arts.len())).fill(secondary()), spacer(), caption("Remap all to").fill(secondary()).lines(1), segmented(tabs)]
        .gap(SPACE)
        .align(Align::Center)
        .shrink(0);

    let place = order(arts);
    let mut rows = Vec::new();
    for (n, a) in arts.iter().enumerate() {
        let id = format!("art-{slot}-{n}");
        let on = active == Some(n);
        if ui.get(id.as_str()).activated() {
            if let Some(&k) = a.switch_keys.first() {
                // A tap of its key: the next frame's sweep lets it go.
                cx.p.shared.press_key(slot, k, 1);
            }
            cx.state.inside.entry(slot).or_default().active = Some(n);
        }
        let dot = block(TIGHT * 1.5, TIGHT * 1.5).fill(if on { Fill::from(Role::Ink) } else { hairline() }).shrink(0);
        let how = trigger(driver, &a.switch_keys, place[n], arts.len()).unwrap_or_else(|| "—".into());
        let el = row![
            dot,
            body(a.name.clone()).fill(if on { Fill::from(Role::Ink) } else { secondary() }).lines(1).flex(1).min_w(0),
            caption(how).fill(secondary()).lines(1).shrink(0)
        ]
        .gap(SPACE)
        .align(Align::Center)
        .pad((0, SPACE))
        .h(CONTROL)
        .when(on, |e| e.fill(Role::Raised))
        .focusable()
        .a11y(A11y::Button)
        .named(a.name.clone())
        .id(id);
        rows.push(interactive(el, on));
    }
    let list = col(rows).gap(1).align(Align::Stretch).max_size(Size::new(1e6, CONTROL * 12.)).scroll().shrink(0).id(format!("arts-{slot}"));
    row![col![head, list].gap(SPACE).align(Align::Stretch).w(TEXT * 56.).min_w(0), spacer()].w(Len::Pct(100.))
}

// Mapping ---------------------------------------------------------------

/// Group `g`'s color in the map and its list.
fn group_color(g: usize) -> Color {
    Color::oklch(0.72, 0.09, golden_hue(200., g))
}

fn mapping(ui: &mut Ui, cx: &mut Cx, slot: usize, inst: &ir::Instrument) -> El {
    let groups = inst.groups.len();
    let mut counts = vec![0usize; groups + 1];
    // ponytail: walks every zone each frame the map shows; cache per load if
    // a 50k-zone instrument makes it slow.
    let mut rects = HashSet::new();
    let (mut low, mut high) = (127u8, 0u8);
    for z in &inst.zones {
        let g = z.group.map_or(groups, |g| g.0);
        counts[g] += 1;
        rects.insert((g, z.keys.low, z.keys.high, z.velocities.low, z.velocities.high));
        (low, high) = (low.min(z.keys.low), high.max(z.keys.high));
    }
    let picked = cx.state.inside.entry(slot).or_default().group;
    let mut pick = picked;
    let mut list = Vec::new();
    let names = inst.groups.iter().map(|g| g.name.as_str()).chain(std::iter::once("No group"));
    for (g, name) in names.enumerate().filter(|&(g, _)| counts[g] > 0) {
        let id = format!("map-group-{slot}-{g}");
        let on = picked == Some(g);
        if ui.get(id.as_str()).activated() {
            pick = if on { None } else { Some(g) };
        }
        let label = if name.is_empty() { format!("Group {}", g + 1) } else { name.to_owned() };
        let el = row![
            block(TIGHT, TIGHT * 3.).fill(group_color(g)).shrink(0),
            body(label.clone()).fill(if on { Fill::from(Role::Ink) } else { secondary() }).lines(1).flex(1).min_w(0),
            caption(counts[g].to_string()).fill(secondary()).shrink(0)
        ]
        .gap(SPACE)
        .align(Align::Center)
        .pad((0, SPACE))
        .h(CONTROL - TIGHT)
        .when(on, |e| e.fill(Role::Raised))
        .focusable()
        .a11y(A11y::Button)
        .named(label)
        .id(id);
        list.push(interactive(el, on));
    }
    cx.state.inside.entry(slot).or_default().group = pick;

    let (first, last) = (low.min(high), high.max(low));
    // Whole octaves, so each C sits at the start of an equal cell.
    let (low, high) = if low > high { (0, 119) } else { (low / 12 * 12, (high / 12 * 12 + 11).min(127)) };
    let rects: Vec<_> = rects.into_iter().collect();
    let keys = f64::from(high - low + 1);
    let map = canvas(move |s| {
        let x = |k: u8| f64::from(k - low) / keys * s.width;
        let y = |v: u8| (1. - f64::from(v) / 127.) * s.height;
        let mut out = vec![Draw::fill(rect(0., 0., s.width, s.height), Role::Field)];
        for c in (low..=high).filter(|k| k % 12 == 0) {
            out.push(Draw::fill(rect(x(c).round(), 0., 1., s.height), hairline()));
        }
        // The picked group last, so it lies on top.
        let mut sorted = rects.clone();
        sorted.sort_by_key(|r| (pick == Some(r.0), r.0));
        for (g, kl, kh, vl, vh) in sorted {
            let r = rect(x(kl), y(vh), x(kh) - x(kl) + s.width / keys, y(vl.saturating_sub(1)) - y(vh));
            let shown = pick.is_none_or(|p| p == g);
            let c = group_color(g);
            out.push(Draw::fill(r.clone(), c.with_alpha(if shown { 0.22 } else { 0.04 })));
            if shown {
                out.push(Draw::stroke(r.clone(), c.with_alpha(0.8), 1.));
            }
        }
        out
    })
    .flex(1)
    .min_w(0)
    .h(TEXT * 16.)
    .clip()
    .named("Zones by key and velocity")
    .id(format!("map-{slot}"));
    let scale = row((low..=high).filter(|k| k % 12 == 0).map(|c| row![caption(note_name(c)).text_size(SMALL).fill(secondary()).lines(1)].flex(1).min_w(0)).collect::<Vec<_>>())
        .gap(0)
        .w(Len::Pct(100.))
        .shrink(0);
    let head = row![section("Mapping"), caption(format!("{} zones · {} – {}", inst.zones.len(), note_name(first), note_name(last))).fill(secondary()).lines(1), spacer(), caption("Keys across, velocity up").fill(secondary())]
        .gap(SPACE)
        .align(Align::Center)
        .shrink(0);
    let groups = col(list).gap(1).align(Align::Stretch).pad(edges(0., SPACE, 0., 0.)).w(TEXT * 16.).h(TEXT * 16.).scroll().shrink(0);
    col![head, row![groups, col![map, scale].gap(TIGHT).flex(1).min_w(0)].gap(SPACE)].gap(SPACE).align(Align::Stretch)
}

// Sound -----------------------------------------------------------------

fn ms(t: ir::Time) -> String {
    let s = t.seconds();
    if s >= 1. { format!("{s:.2} s") } else { format!("{:.0} ms", s * 1000.) }
}

fn hz(f: ir::Frequency) -> String {
    match f {
        ir::Frequency::Hertz(h) if h >= 1000. => format!("{:.1} kHz", h / 1000.),
        ir::Frequency::Hertz(h) => format!("{h:.0} Hz"),
        ir::Frequency::Beats(b) => format!("{b} beats"),
    }
}

fn source_name(s: &ir::ModulationSource) -> String {
    match s {
        ir::ModulationSource::Envelope(_) => "Envelope".into(),
        ir::ModulationSource::Lfo(l) => format!("LFO {}", hz(l.rate)),
        ir::ModulationSource::Controller(c) => format!("CC {c}"),
        other => format!("{other:?}").split(['(', ' ', '{']).next().unwrap_or_default().to_owned(),
    }
}

/// An envelope's outline: attack up, decay to sustain, release down.
fn envelope_shape(e: ir::Envelope) -> El {
    canvas(move |s| {
        let (a, d, r) = (e.attack.seconds(), e.decay.seconds(), e.release.seconds());
        let total = (a + d + r).max(1e-3) * 1.25;
        let x = |t: f64| t / total * s.width;
        let top = 2.;
        let sus = top + (1. - e.sustain) * (s.height - top - 1.);
        let pts = [(0., s.height - 1.), (x(a), top), (x(a + d), sus), (x(a + d + total * 0.2), sus), (s.width - 1., s.height - 1.)];
        vec![
            Draw::fill(rect(0., 0., s.width, s.height), Role::Field),
            Draw::stroke(DrawPath::polyline(pts.iter().map(|&(x, y)| Point::new(x, y)), false), Role::Ink.alpha(0.7), 1.5),
        ]
    })
    .w(TEXT * 8.)
    .h(CONTROL * 1.5)
    .shrink(0)
}

fn sound(inst: &ir::Instrument) -> El {
    let line = |t: String| caption(t).fill(secondary()).lines(1).min_w(0);
    // Each distinct amplitude envelope, with how many zones use it.
    let mut envs: Vec<(ir::Envelope, usize)> = Vec::new();
    for m in inst.zones.iter().filter_map(|z| z.amplitude) {
        if let Some(ir::ModulationSource::Envelope(e)) = inst.modulators.get(m.0).map(|m| &m.source) {
            match envs.iter_mut().find(|(x, _)| x == e) {
                Some((_, n)) => *n += 1,
                None => envs.push((*e, 1)),
            }
        }
    }
    envs.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    let mut rows = vec![section("Amplitude")];
    if envs.is_empty() {
        rows.push(line("Gate: on with the key, off with its release".into()));
    }
    for (e, n) in envs.iter().take(4) {
        rows.push(
            row![
                envelope_shape(*e),
                line(format!("A {} · D {} · S {:.0}% · R {} · {n} zones", ms(e.attack), ms(e.decay), e.sustain * 100., ms(e.release)))
            ]
            .gap(SPACE)
            .align(Align::Center)
            .shrink(0),
        );
    }
    let mut filters = Vec::new();
    for c in &inst.chains {
        for p in c.pre_amplitude.iter().chain(&c.post_amplitude) {
            if let ir::Processor::Filter(f) = p {
                let kind = format!("{:?}", f.kind).replace(" { poles: ", " ").replace(" }", "-pole");
                let text = format!("{kind} · {} · {:?}", hz(f.cutoff), f.resonance);
                if !filters.contains(&text) {
                    filters.push(text);
                }
            }
        }
    }
    let mut more = vec![section("Filters")];
    more.extend(filters.iter().take(4).cloned().map(line));
    if filters.is_empty() {
        more.push(line("None".into()));
    }
    let mut routes: Vec<String> = Vec::new();
    for r in &inst.routes {
        let from = inst.modulators.get(r.source.0).map_or("?".into(), |m| source_name(&m.source));
        let to = format!("{:?}", r.target).split(['(', ' ', '{']).next().unwrap_or_default().to_owned();
        let text = format!("{from} → {to}");
        if !routes.contains(&text) {
            routes.push(text);
        }
    }
    more.push(section("Modulation"));
    more.extend(routes.iter().take(6).cloned().map(line));
    if routes.is_empty() {
        more.push(line("None".into()));
    }
    row![
        col(rows).gap(TIGHT).align(Align::Start).flex(1).min_w(0),
        col(more).gap(TIGHT).align(Align::Start).flex(1).min_w(0)
    ]
    .gap(INSET * 2.)
    .align(Align::Start)
}

// Info ------------------------------------------------------------------

fn info(cx: &Cx, slot: usize, inst: Option<&ir::Instrument>) -> El {
    let v = &cx.view.parts[slot];
    let pair = |k: &str, val: String| {
        row![caption(k.to_owned()).fill(secondary()).w(TEXT * 8.).shrink(0), body(val.clone()).lines(1).min_w(0).tip(val)].gap(SPACE).align(Align::Center).shrink(0)
    };
    let mut rows = Vec::new();
    rows.push(pair("Instrument", super::rack::name(cx, slot)));
    rows.push(pair("File", cx.selection.parts[slot].path.clone()));
    if let Some(r) = &v.report {
        let d = &r.decoded;
        rows.push(pair("Format", d.format.clone()));
        rows.push(pair("Contents", format!("{} zones · {} groups · {} samples · {} buses", d.zones, d.groups, d.samples, d.buses)));
        rows.push(pair("Scripts", format!("{} scripts · {} controls · {} articulations", d.scripts, d.controls, d.articulations)));
        let keys: Vec<u8> = (0..128).filter(|&k| d.maps(k)).collect();
        if let (Some(&lo), Some(&hi)) = (keys.first(), keys.last()) {
            rows.push(pair("Keys", format!("{} – {} · {} keys", note_name(lo), note_name(hi), keys.len())));
        }
        rows.push(pair("Not translated", r.missing.len().to_string()));
    }
    if let Some(i) = inst {
        let switches: usize = i.articulations.iter().map(|a| a.switch_keys.len()).sum();
        if switches > 0 {
            rows.push(pair("Keyswitches", switches.to_string()));
        }
    }
    if let Some(f) = cx.state.faces.get(&slot) {
        rows.push(pair("Interface", format!("{:.1} MB of pictures decoded", f.bytes() as f64 / (1024. * 1024.))));
    }
    row![col(rows).gap(TIGHT).align(Align::Stretch).w(TEXT * 60.).min_w(0), spacer()].w(Len::Pct(100.))
}

/// The keys `slot`'s articulations switch on, for the keyboard's marks.
pub fn switch_keys(cx: &Cx, slot: usize) -> Option<Arc<ir::Instrument>> {
    cx.view.parts.get(slot)?.instrument.clone().filter(|i| i.articulations.iter().any(|a| !a.switch_keys.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triggers_follow_the_driver() {
        assert_eq!(trigger(Driver::Keys, &[24], 0, 3).as_deref(), Some("C0"));
        assert_eq!(trigger(Driver::Keys, &[], 0, 3), None);
        assert_eq!(trigger(Driver::Velocity, &[], 0, 2).as_deref(), Some("vel 1–63"));
        assert_eq!(trigger(Driver::Velocity, &[], 1, 2).as_deref(), Some("vel 64–127"));
        assert_eq!(trigger(Driver::Channel, &[], 15, 16).as_deref(), Some("ch 16"));
        assert_eq!(trigger(Driver::Channel, &[], 0, 17), None, "more than 16 do not fit on channels");
        let arts = |keys: &[&[u8]]| keys.iter().map(|k| ir::Articulation { name: String::new(), switch_keys: k.to_vec(), default: false }).collect::<Vec<_>>();
        assert_eq!(order(&arts(&[&[26], &[], &[24]])), vec![1, 2, 0], "by lowest key, keyless last");
    }
}
