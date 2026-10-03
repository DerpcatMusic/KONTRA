//! Output routing that runs itself: which bus each part (and each of its
//! mic outputs) plays through, what each bus and host port is called, and
//! the one-click actions that replace Kontakt's batch operations
//! (`audits/ROUTING.md`).
//!
//! Assignments live in the saved rack ([`Part::output`], [`Part::mic_buses`])
//! and are kept wherever they still hold: adding, removing or reordering
//! parts moves no other part. A route the player picks by hand
//! ([`Part::output_manual`]) is never moved, and automatic routes keep off
//! its bus and off buses used as sends.

use crate::engine::BUSES;
#[cfg(test)]
use crate::engine::RACK_SLOTS;
use crate::fx::OUTS;
use crate::plugin::{Part, Selection};
use std::time::{Duration, Instant};

/// The mixer's "Outputs" choice ([`Selection::outputs`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Outputs {
    /// Everything on st.1, as Kontakt starts: parts keep the bus they
    /// have, new ones start on st.1 ([`to_stereo`] moves them all back).
    #[default]
    Stereo,
    /// Each part on a bus and host port of its own, named after it.
    Instrument,
    /// As [`Outputs::Instrument`], and each output channel a part's
    /// instrument routes past its own output (a mic mixer's "Out 2") on
    /// one more: "Harp Close".
    Mic,
}

impl Outputs {
    pub const ALL: [Self; 3] = [Self::Stereo, Self::Instrument, Self::Mic];

    pub fn of(n: u8) -> Self {
        match n {
            1 => Self::Instrument,
            2 => Self::Mic,
            _ => Self::Stereo,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Stereo => "Stereo mix",
            Self::Instrument => "One per instrument",
            Self::Mic => "One per mic",
        }
    }
}

/// Per rack slot and output channel, what the instrument routes there past
/// its own output: 0 nothing, `1 + b` its instrument bus `b`, `0x100 + g`
/// its group `g` (published by the audio thread, `plugin::outs_of`).
pub type Mics = [[u16; OUTS]];

/// Loaded slots, rack order first.
fn loaded(sel: &Selection) -> Vec<usize> {
    let used = |s: usize| sel.parts.get(s).is_some_and(|p| !p.is_empty());
    let mut slots = Vec::new();
    for s in sel.order.iter().map(|&s| s as usize).chain(0..sel.parts.len()) {
        if used(s) && !slots.contains(&s) {
            slots.push(s);
        }
    }
    slots
}

/// Route every part not routed by hand as [`Selection::outputs`] says;
/// `mic_name(slot, code)` names a mic output from its [`Mics`] code.
pub fn apply(sel: &mut Selection, mics: &Mics, mic_name: impl Fn(usize, u16) -> String) {
    let mode = Outputs::of(sel.outputs);
    let slots = loaded(sel);
    let mut claimed = [false; BUSES];
    for &s in &slots {
        let p = &sel.parts[s];
        if p.output_manual {
            claimed[usize::from(p.output).min(BUSES - 1)] = true;
        }
        if let Ok(aux) = usize::try_from(p.aux) {
            claimed[aux.min(BUSES - 1)] = true;
        }
    }
    let mut pending = Vec::new();
    for &s in &slots {
        let p = &mut sel.parts[s];
        if mode != Outputs::Mic {
            p.mic_buses.clear();
            p.mic_names.clear();
        }
        if p.output_manual || mode == Outputs::Stereo {
            continue;
        }
        let o = usize::from(p.output).min(BUSES - 1);
        if claimed[o] {
            pending.push((s, None));
        } else {
            claimed[o] = true;
        }
    }
    if mode == Outputs::Mic {
        for &s in &slots {
            let p = &mut sel.parts[s];
            p.mic_buses.resize(OUTS, -1);
            p.mic_names.resize(OUTS, String::new());
            for c in 0..OUTS {
                let code = mics.get(s).map_or(0, |m| m[c]);
                if code == 0 {
                    p.mic_buses[c] = -1;
                    p.mic_names[c].clear();
                    continue;
                }
                p.mic_names[c] = mic_name(s, code);
                match usize::try_from(p.mic_buses[c]) {
                    Ok(b) if b < BUSES && !claimed[b] => claimed[b] = true,
                    _ => pending.push((s, Some(c))),
                }
            }
        }
    }
    // Instruments before mics: they claim in the order pending was built.
    for (s, mic) in pending {
        let free = claimed.iter().position(|c| !c);
        if let Some(b) = free {
            claimed[b] = true;
        }
        let p = &mut sel.parts[s];
        match mic {
            // ponytail: past 16 buses a part shares the one it had.
            None => p.output = free.map_or(p.output, |b| b as u8),
            // Past 16, a mic plays with its instrument.
            Some(c) => p.mic_buses[c] = free.map_or(-1, |b| b as i16),
        }
    }
}

/// Choose "Stereo mix": every part not routed by hand back on st.1.
pub fn to_stereo(sel: &mut Selection) {
    sel.outputs = Outputs::Stereo as u8;
    for p in sel.parts.iter_mut().filter(|p| !p.output_manual) {
        p.output = 0;
    }
}

/// A part's name: the player's, else its preset file's.
pub fn part_name(p: &Part) -> String {
    if !p.name.is_empty() {
        return p.name.clone();
    }
    std::path::Path::new(&p.path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// "Harp Close", or just "Harp Close" when the mic is already named so.
fn mic_label(part: &str, mic: &str) -> String {
    if mic.to_lowercase().starts_with(&part.to_lowercase()) {
        mic.to_owned()
    } else {
        format!("{part} {mic}")
    }
}

/// One name for several: the words they all start with ("Violins 1",
/// "Violins 2": "Violins"), else the first and how many more.
fn common(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [first, rest @ ..] => {
            let words: Vec<&str> = first.split_whitespace().collect();
            let shared = (0..words.len())
                .take_while(|&i| rest.iter().all(|n| n.split_whitespace().nth(i) == Some(words[i])))
                .count();
            if shared > 0 {
                words[..shared].join(" ")
            } else {
                format!("{first} +{}", rest.len())
            }
        }
    }
}

/// What each bus plays, by name: empty when nothing is routed to it.
pub fn auto_names(sel: &Selection) -> [String; BUSES] {
    let mut sources: [Vec<String>; BUSES] = Default::default();
    for s in loaded(sel) {
        let p = &sel.parts[s];
        let name = part_name(p);
        sources[usize::from(p.output).min(BUSES - 1)].push(name.clone());
        for (c, &b) in p.mic_buses.iter().enumerate() {
            if let Ok(b) = usize::try_from(b)
                && b < BUSES
            {
                let mic = p.mic_names.get(c).filter(|m| !m.is_empty()).cloned().unwrap_or(format!("Out {}", c + 1));
                sources[b].push(mic_label(&name, &mic));
            }
        }
    }
    sources.map(|names| common(&names))
}

/// Bus `n`'s name: the player's, else what plays through it, else "st.N".
pub fn label(sel: &Selection, n: usize) -> String {
    label_with(sel, &auto_names(sel), n)
}

fn label_with(sel: &Selection, auto: &[String; BUSES], n: usize) -> String {
    let bus = sel.bus(n);
    match auto.get(n) {
        _ if !bus.name.is_empty() => bus.name,
        Some(name) if !name.is_empty() => name.clone(),
        _ => bus.label(n),
    }
}

/// What the host lists each stereo output port as: the buses playing
/// through it, named; "st.N" when none does.
pub fn port_names(sel: &Selection) -> [String; BUSES] {
    let auto = auto_names(sel);
    std::array::from_fn(|port| {
        let names: Vec<String> = (0..BUSES)
            .filter(|&n| {
                let b = sel.bus(n);
                let to = usize::try_from(b.port).ok().filter(|&p| p < BUSES).unwrap_or(n);
                to == port && (!b.name.is_empty() || !auto[n].is_empty())
            })
            .map(|n| label_with(sel, &auto, n))
            .collect();
        match &names[..] {
            [] => format!("st.{}", port + 1),
            names => common(names),
        }
    })
}

/// Parts listen on A1, A2, … in rack order (B1… past 16).
pub fn own_channels(sel: &mut Selection) {
    for (n, s) in loaded(sel).into_iter().enumerate() {
        let p = &mut sel.parts[s];
        (p.port, p.channel) = ((n / 16).min(3) as u8, (n % 16) as i16);
    }
}

/// Every part hears everything on port A.
pub fn all_omni(sel: &mut Selection) {
    for s in loaded(sel) {
        let p = &mut sel.parts[s];
        (p.port, p.channel) = (0, -1);
    }
}

/// Keep what plays through each bus as its name.
pub fn name_outputs(sel: &mut Selection) {
    let auto = auto_names(sel);
    for (n, name) in auto.into_iter().enumerate() {
        if !name.is_empty() && sel.bus(n).name.is_empty() {
            sel.bus_mut(n).name = name;
        }
    }
}

/// Forget the hand-picked routes: every part and bus routed automatically
/// again (on st.1 in "Stereo mix"), every bus on its own port. Levels,
/// names and sends stay.
pub fn reset(sel: &mut Selection) {
    for p in &mut sel.parts {
        p.output_manual = false;
    }
    if Outputs::of(sel.outputs) == Outputs::Stereo {
        to_stereo(sel);
    }
    for n in 0..sel.buses.len() {
        sel.bus_mut(n).port = -1;
    }
}

/// How long new port names must hold before the host hears of them: a
/// host rescans for each change, and parts load one at a time.
pub const SETTLE: Duration = Duration::from_millis(500);

/// Host port names as last published, and a change waiting to settle.
pub struct PortNames {
    pub published: [String; BUSES],
    pending: Option<([String; BUSES], Instant)>,
}

impl Default for PortNames {
    fn default() -> Self {
        Self {
            published: std::array::from_fn(|n| format!("st.{}", n + 1)),
            pending: None,
        }
    }
}

impl PortNames {
    /// Offer the names as of `now`; true when they are published: they
    /// differ from the last published and have held for [`SETTLE`].
    pub fn offer(&mut self, names: [String; BUSES], now: Instant) -> bool {
        if names == self.published {
            self.pending = None;
            return false;
        }
        match &self.pending {
            Some((pending, since)) if *pending == names => {
                if now.duration_since(*since) < SETTLE {
                    return false;
                }
            }
            _ => {
                self.pending = Some((names, now));
                return false;
            }
        }
        self.published = names;
        self.pending = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rack(names: &[&str]) -> Selection {
        let mut sel = Selection::default();
        for n in names {
            sel.parts.push(Part {
                path: format!("/lib/{n}.nki"),
                ..Default::default()
            });
        }
        sel.order = (0..names.len() as u32).collect();
        sel
    }

    const NO_MICS: [[u16; OUTS]; RACK_SLOTS] = [[0; OUTS]; RACK_SLOTS];

    fn outputs(sel: &Selection) -> Vec<u8> {
        sel.parts.iter().map(|p| p.output).collect()
    }

    fn apply_now(sel: &mut Selection) {
        apply(sel, &NO_MICS, |_, _| String::new());
    }

    #[test]
    fn one_per_instrument_is_stable() {
        let mut sel = rack(&["Harp", "Celli", "Horns"]);
        sel.parts[1].output = 4;
        apply_now(&mut sel);
        assert_eq!(outputs(&sel), [0, 4, 0], "stereo mix: an old session keeps its routes");
        to_stereo(&mut sel);
        assert_eq!(outputs(&sel), [0, 0, 0], "choosing it: everything on st.1");
        sel.outputs = 1;
        apply_now(&mut sel);
        assert_eq!(outputs(&sel), [0, 1, 2]);
        // Remove the middle one: the others keep their buses.
        sel.parts[1] = Part::default();
        sel.order.retain(|&s| s != 1);
        apply_now(&mut sel);
        assert_eq!((sel.parts[0].output, sel.parts[2].output), (0, 2));
        // Reorder: nothing moves.
        sel.order = vec![2, 0];
        apply_now(&mut sel);
        assert_eq!((sel.parts[0].output, sel.parts[2].output), (0, 2));
        // A new part takes the free bus, a duplicate in rack order gets its own.
        sel.parts[1] = Part { path: "/lib/Flute.nki".into(), ..Default::default() };
        sel.order.push(1);
        let dup = Part { output: 2, ..sel.parts[2].clone() };
        sel.parts.push(dup);
        sel.order.push(3);
        apply_now(&mut sel);
        assert_eq!(outputs(&sel), [0, 1, 2, 3]);
        // Saved and reloaded: the same routes.
        let again = sel.clone();
        apply_now(&mut sel);
        assert!(sel == again);
        let names = port_names(&sel);
        assert_eq!(&names[..5], ["Harp", "Flute", "Horns", "Horns", "st.5"]);
    }

    #[test]
    fn manual_routes_stick() {
        let mut sel = rack(&["Harp", "Celli", "Horns"]);
        sel.outputs = 1;
        sel.parts[2].output = 1;
        sel.parts[2].output_manual = true;
        sel.parts[0].aux = 0;
        apply_now(&mut sel);
        // Horns keep st.2; Harp leaves st.1, a send; Celli take the next free.
        assert_eq!(outputs(&sel), [2, 3, 1]);
        to_stereo(&mut sel);
        apply_now(&mut sel);
        assert_eq!(outputs(&sel), [0, 0, 1], "stereo mix keeps a hand-picked route");
        reset(&mut sel);
        apply_now(&mut sel);
        assert_eq!(outputs(&sel), [0, 0, 0]);
    }

    #[test]
    fn mics_get_buses_and_names() {
        let mut sel = rack(&["Harp", "Celli"]);
        sel.outputs = 2;
        let mut mics = NO_MICS;
        mics[0][1] = 1 + 3; // instrument bus 3 on "Out 2"
        mics[0][2] = 0x100 + 7; // group 7 on "Out 3"
        let name = |_: usize, code: u16| match code {
            4 => "Close".to_owned(),
            _ => "Tree".to_owned(),
        };
        apply(&mut sel, &mics, name);
        assert_eq!(outputs(&sel), [0, 1]);
        assert_eq!(sel.parts[0].mic_buses[..3], [-1, 2, 3]);
        let names = port_names(&sel);
        assert_eq!(&names[..5], ["Harp", "Celli", "Harp Close", "Harp Tree", "st.5"]);
        // The library moves "Close" back to its own output: its bus frees.
        mics[0][1] = 0;
        apply(&mut sel, &mics, name);
        assert_eq!(sel.parts[0].mic_buses[..3], [-1, -1, 3], "the other mic stays put");
        sel.outputs = 1;
        apply(&mut sel, &mics, name);
        assert!(sel.parts[0].mic_buses.is_empty());
    }

    #[test]
    fn names_and_channels() {
        assert_eq!(common(&["Violins 1".into(), "Violins 2".into()]), "Violins");
        assert_eq!(common(&["Harp".into(), "Celli".into(), "Flute".into()]), "Harp +2");
        let mut sel = rack(&["Violins 1", "Violins 2", "Harp"]);
        assert_eq!(port_names(&sel)[0], "Violins 1 +2");
        sel.parts[2].output = 1;
        sel.parts[2].output_manual = true;
        apply_now(&mut sel);
        assert_eq!(port_names(&sel)[..3], ["Violins", "Harp", "st.3"]);
        // Two buses on one port share it; a player's name wins.
        sel.bus_mut(1).port = 0;
        assert_eq!(port_names(&sel)[..2], ["Violins +1", "st.2"]);
        sel.bus_mut(0).name = "Strings".into();
        assert_eq!(label(&sel, 0), "Strings");
        name_outputs(&mut sel);
        assert_eq!(sel.bus(1).name, "Harp");
        own_channels(&mut sel);
        assert_eq!(sel.parts.iter().map(|p| p.channel).collect::<Vec<_>>(), [0, 1, 2]);
        all_omni(&mut sel);
        assert!(sel.parts.iter().all(|p| p.channel == -1 && p.port == 0));
    }

    #[test]
    fn port_names_settle_before_publishing() {
        let mut names = PortNames::default();
        let t = Instant::now();
        let mut harp = names.published.clone();
        harp[0] = "Harp".into();
        assert!(!names.offer(harp.clone(), t), "not at once");
        assert!(!names.offer(harp.clone(), t + SETTLE / 2));
        let mut celli = harp.clone();
        celli[1] = "Celli".into();
        assert!(!names.offer(celli.clone(), t + SETTLE), "a change starts the wait again");
        assert!(!names.offer(celli.clone(), t + SETTLE + SETTLE / 2));
        assert!(names.offer(celli.clone(), t + SETTLE * 2));
        assert_eq!(names.published[1], "Celli");
        assert!(!names.offer(celli, t + SETTLE * 3), "unchanged: nothing to tell");
    }
}
