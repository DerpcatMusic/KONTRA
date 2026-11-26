//! Host automation of library panel controls: Kontakt's automation slots,
//! a fixed pool of host parameters (#000..#127). A script's control with an
//! automation id (KSP `$CONTROL_PAR_AUTOMATION_ID`) takes that slot when
//! its instrument loads, named by `$CONTROL_PAR_AUTOMATION_NAME`; a taken
//! slot or an id past the pool unwinds to the next free one, as Kontakt's
//! "Unwind automation IDs" does for a second instrument. Any other panel
//! control takes the next free slot the first time the player moves it
//! (learn by touching), unless its script forbids automation
//! (`$CONTROL_PAR_ALLOW_AUTOMATION` 0). The host's automation of a slot
//! moves its control, and its script runs its callback as for the
//! player's move.
//! Assignments save with the plugin's state (`KontraParams::macros`), one
//! per line, tab-separated: macro, rack slot, min, max, menu item values
//! (comma-separated, empty for none), widget name, instrument path (empty
//! until the tick learns it), host name ("Part: Control"). An assignment frees itself when its part is
//! removed or holds another instrument.

use std::path::Path;

use std::sync::atomic::Ordering;

use kontra_host::rack::{Control, MacroTarget, Rack, STEPS, Scale};
use kontra_ui_model::{Kind, Widget};
use moose::prelude::*;

use crate::KontraParams;

/// Host parameters in the pool: `KontraParams::slots`. Kontakt has 2048;
/// the libraries' scripts use ids below 80.
pub const COUNT: usize = kontra_host::rack::MACROS;

/// One `T` per slot.
pub struct PerSlot<T>([T; COUNT]);

impl<T: Default> Default for PerSlot<T> {
    fn default() -> Self {
        Self(std::array::from_fn(|_| T::default()))
    }
}

impl<T> std::ops::Deref for PerSlot<T> {
    type Target = [T; COUNT];
    fn deref(&self) -> &[T; COUNT] {
        &self.0
    }
}

impl<T> std::ops::DerefMut for PerSlot<T> {
    fn deref_mut(&mut self) -> &mut [T; COUNT] {
        &mut self.0
    }
}

fn none() -> [Option<Macro>; COUNT] {
    std::array::from_fn(|_| None)
}

/// A macro's control: a rack part's widget by name, and its range.
#[derive(Clone, Debug, PartialEq)]
pub struct Macro {
    pub part: usize,
    pub widget: String,
    pub min: f64,
    pub max: f64,
    /// A menu's item values: the control takes the nearest. Empty for a
    /// continuous control.
    pub steps: Vec<f64>,
    /// The part's instrument when the control was learnt; empty until the
    /// tick learns it.
    pub path: String,
    /// The macro's host name: "Part: Control".
    pub label: String,
}

impl Macro {
    /// The control's value at host value `normalized` (0..1).
    fn plain(&self, normalized: f64) -> f64 {
        let v = self.min + normalized.clamp(0.0, 1.0) * (self.max - self.min);
        let nearest = |a: &&f64, b: &&f64| (*a - v).abs().total_cmp(&(*b - v).abs());
        self.steps.iter().min_by(nearest).copied().unwrap_or(v)
    }

    /// Host value of the control's `plain` value.
    pub(crate) fn normalized(&self, plain: f64) -> f64 {
        let span = self.max - self.min;
        if span == 0.0 {
            0.0
        } else {
            ((plain - self.min) / span).clamp(0.0, 1.0)
        }
    }
}

/// The range a host macro can drive `w` across: a knob's, slider's or value
/// edit's, a menu's item values, a switch's or latching button's 0..1.
#[must_use]
pub fn range(w: &Widget) -> Option<(f64, f64)> {
    let (min, max, _) = span(w)?;
    Some((min, max))
}

/// [`range`] and a menu's item values.
fn span(w: &Widget) -> Option<(f64, f64, Vec<f64>)> {
    match &w.kind {
        Kind::Knob { range, .. } | Kind::Slider { range, .. } | Kind::ValueEdit { range, .. } => {
            Some((range.min, range.max, Vec::new()))
        }
        Kind::Menu { items } => {
            let values: Vec<f64> = items.iter().map(|i| f64::from(i.value)).collect();
            let (lo, hi) =
                (values.iter()).fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
            (lo < hi).then_some((lo, hi, values))
        }
        Kind::Switch | Kind::Button { momentary: false } => Some((0.0, 1.0, Vec::new())),
        _ => None,
    }
}

/// The assignments `text` saved, by macro.
#[must_use]
pub fn read(text: &str) -> [Option<Macro>; COUNT] {
    let mut out = none();
    for line in text.lines() {
        let mut f = line.split('\t');
        let mut num = || f.next().and_then(|s| s.parse::<f64>().ok());
        let (Some(n), Some(part), Some(min), Some(max)) = (num(), num(), num(), num()) else {
            continue;
        };
        let steps = f.next().unwrap_or_default();
        let steps = steps.split(',').filter_map(|s| s.parse().ok()).collect();
        let Some(widget) = f.next().filter(|w| !w.is_empty()) else {
            continue;
        };
        let path = f.next().unwrap_or_default().to_owned();
        let label = f.next().unwrap_or_default().to_owned();
        if let Some(slot) = out.get_mut(n as usize) {
            *slot = Some(Macro {
                part: part as usize,
                widget: widget.to_owned(),
                min,
                max,
                steps,
                path,
                label,
            });
        }
    }
    out
}

/// `macros` as saved.
#[must_use]
pub fn write(macros: &[Option<Macro>; COUNT]) -> String {
    let mut out = String::new();
    for (n, m) in macros.iter().enumerate() {
        if let Some(m) = m {
            let steps: Vec<String> = m.steps.iter().map(f64::to_string).collect();
            out.push_str(&format!(
                "{n}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                m.part,
                m.min,
                m.max,
                steps.join(","),
                m.widget,
                m.path,
                m.label
            ));
        }
    }
    out
}

/// The player moved `w` of rack part `part` to `plain`: its macro, taken
/// now if it had none and one is free, and its host value. None for a
/// control no macro can drive, or when every macro is taken. `title` is
/// the part's, for the macro's host name.
pub fn touch(
    p: &KontraParams,
    part: usize,
    title: &str,
    w: &Widget,
    plain: f64,
) -> Option<(u32, f64)> {
    if !w.automation.allowed {
        return None;
    }
    let (min, max, steps) = span(w)?;
    let mut text = p
        .macros
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut macros = read(&text);
    let mine = |m: &Option<Macro>| {
        m.as_ref()
            .is_some_and(|m| m.part == part && m.widget == w.name)
    };
    let n = match macros.iter().position(mine) {
        Some(n) => {
            // The control's bounds can change (a script resets a range):
            // the macro spans them as they are now.
            let m = macros.get_mut(n)?.as_mut()?;
            if (m.min, m.max) != (min, max) || m.steps != steps {
                (m.min, m.max, m.steps) = (min, max, steps);
                *text = write(&macros);
            }
            n
        }
        None => {
            let n = macros.iter().position(Option::is_none)?;
            let taken = Macro {
                part,
                widget: w.name.clone(),
                min,
                max,
                steps,
                path: String::new(),
                label: label(title, &w.name),
            };
            *macros.get_mut(n)? = Some(taken);
            *text = write(&macros);
            renamed(p);
            n
        }
    };
    let m = macros.get(n)?.as_ref()?;
    let value = m.normalized(plain);
    // The store first: a tick between this and the host's echo sees the
    // value the control has, and does not move it back.
    let param = param(p, n)?;
    param.set_value(value);
    if let Some(last) = p.shared.macro_values.get(n) {
        last.store(value.to_bits(), Ordering::Relaxed);
    }
    if let Some(at) = p.shared.macro_touched.get(n) {
        at.store(now(), Ordering::Relaxed);
    }
    Some((id(n), value))
}

/// How long after the editor moves a macro the host's moves of it are
/// taken for echoes of the editor's and dropped, in milliseconds.
const HELD_MS: u64 = 250;

static EPOCH: std::sync::LazyLock<std::time::Instant> =
    std::sync::LazyLock::new(std::time::Instant::now);

/// Milliseconds since the plugin first asked, for [`touch`] and [`held`].
pub fn now() -> u64 {
    u64::try_from(EPOCH.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Whether the editor moved macro `n` within the last [`HELD_MS`]: the
/// player's hand owns it, not the host's automation.
pub fn held(p: &KontraParams, n: usize) -> bool {
    p.shared
        .macro_touched
        .get(n)
        .map(|at| at.load(Ordering::Relaxed))
        .is_some_and(|at| at != 0 && now().saturating_sub(at) < HELD_MS)
}

/// The host id of macro `n`.
#[must_use]
pub fn id(n: usize) -> u32 {
    FIRST + n as u32
}

/// The id the first macro has (`KontraParams`).
pub const FIRST: u32 = 2;

pub(crate) fn param(p: &KontraParams, n: usize) -> Option<&FloatParam> {
    p.slots.get(n)
}

/// The macro host id `id` is, if one.
#[must_use]
pub fn index(id: u32) -> Option<usize> {
    let n = id.checked_sub(FIRST)? as usize;
    (n < COUNT).then_some(n)
}

/// Tells the audio side what each macro moves, so a host's move inside a
/// block applies at its frame: a KSP control or a Lua widget. A native_ui
/// tap area or a menu past [`STEPS`] items stays on the tick.
pub fn send_targets(p: &KontraParams, rack: &mut Rack) {
    let macros = read(
        &p.macros
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    let mut sent = crate::lock(&p.shared.macro_targets);
    for (n, (m, sent)) in macros.iter().zip(sent.iter_mut()).enumerate() {
        let target = m.as_ref().and_then(|m| target(rack, m));
        if target != *sent {
            *sent = target;
            rack.set_macro(n, target);
        }
    }
}

fn target(rack: &Rack, m: &Macro) -> Option<MacroTarget> {
    let part = rack.parts().get(m.part)?.as_ref()?;
    let generation = part.script.generation();
    let view = part.view.as_ref()?;
    let widget = view
        .tree()
        .widgets
        .iter()
        .position(|w| w.name == m.widget)?;
    let w = view.tree().widgets.get(widget)?;
    // A native_ui tap area's set comes from its page, which lives on the
    // control side: it stays on the tick.
    range(w)?;
    let (slot, control) = match view.widget(widget) {
        Some((slot, n)) => (slot, Control::Widget(n)),
        None => {
            let (slot, id, _) = view.control(widget, m.min)?;
            match w.value {
                Some(kontra_ui_model::Value::Real(_)) => (slot, Control::Real(id)),
                _ => (slot, Control::Integer(id)),
            }
        }
    };
    let scale = if m.steps.is_empty() {
        Scale::Range {
            min: m.min,
            max: m.max,
        }
    } else {
        let len = u8::try_from(m.steps.len())
            .ok()
            .filter(|&l| usize::from(l) <= STEPS)?;
        let mut values = [0; STEPS];
        for (v, s) in values.iter_mut().zip(&m.steps) {
            *v = *s as i32;
        }
        Scale::Steps { values, len }
    };
    Some(MacroTarget {
        part: m.part,
        generation,
        slot,
        control,
        scale,
    })
}

/// Frees the macros whose part no longer holds the instrument they were
/// learnt on (removed or replaced), and records it for the ones learnt
/// since. `instrument` is a rack part's instrument, None for an empty part;
/// call only while no load is pending.
pub fn release(p: &KontraParams, instrument: impl Fn(usize) -> Option<String>) {
    let mut text = p
        .macros
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut macros = read(&text);
    let mut changed = false;
    for slot in &mut macros {
        let Some(m) = slot else { continue };
        match instrument(m.part) {
            Some(now) if now == m.path => {}
            Some(now) if m.path.is_empty() => (m.path, changed) = (now, true),
            _ => (*slot, changed) = (None, true),
        }
    }
    if changed {
        *text = write(&macros);
        renamed(p);
    }
}

/// Gives each script control with an automation id its slot: the one the id
/// names, or the next free one when that is taken or past the pool.
/// `parts` are the rack's loaded parts as (part, title, instrument, its
/// panel's widgets); call after [`release`], while no load is pending.
pub fn declare<'a>(
    p: &KontraParams,
    parts: impl IntoIterator<Item = (usize, &'a str, String, &'a [Widget])>,
) {
    let mut text = p
        .macros
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut macros = read(&text);
    let mut changed = false;
    for (part, title, path, widgets) in parts {
        for w in widgets {
            let (Some(id), true) = (w.automation.id, w.automation.allowed) else {
                continue;
            };
            let Some((min, max, steps)) = span(w) else {
                continue;
            };
            let mine = |m: &Option<Macro>| {
                m.as_ref()
                    .is_some_and(|m| m.part == part && m.widget == w.name)
            };
            if macros.iter().any(mine) {
                continue;
            }
            let wanted = usize::try_from(id).ok().filter(|&n| n < COUNT);
            let free = wanted
                .filter(|&n| macros.get(n).is_some_and(Option::is_none))
                .or_else(|| macros.iter().position(Option::is_none));
            let Some(slot) = free.and_then(|n| macros.get_mut(n)) else {
                continue;
            };
            let name = w.automation.name.as_deref().filter(|n| !n.is_empty());
            *slot = Some(Macro {
                part,
                widget: w.name.clone(),
                min,
                max,
                steps,
                path: path.clone(),
                label: name.map_or_else(|| label(title, &w.name), clean),
            });
            changed = true;
        }
    }
    if changed {
        *text = write(&macros);
        renamed(p);
    }
}

/// A macro's host name changed: the host reads the names again on the
/// next macro move.
pub fn renamed(p: &KontraParams) {
    p.shared.macro_names.fetch_add(1, Ordering::AcqRel);
}

/// "Part: Control", the control's script name without its type sigil.
fn label(title: &str, widget: &str) -> String {
    let control = widget
        .trim_start_matches(['$', '%', '~', '@', '!', '?'])
        .replace('_', " ");
    let label = if title.is_empty() {
        control
    } else {
        format!("{title}: {control}")
    };
    clean(&label)
}

/// `name` fit for a saved line.
fn clean(name: &str) -> String {
    name.replace(['\t', '\n'], " ")
}

/// A loaded part's instrument as [`release`] keeps it: its path, empty for
/// one with none.
#[must_use]
pub fn instrument(path: Option<&Path>) -> String {
    path.map(|p| p.to_string_lossy().replace(['\t', '\n'], " "))
        .unwrap_or_default()
}

/// The controls the host moved since the last call, as (rack part, widget
/// name, the control's value): each macro whose host value is not the one
/// last applied.
pub fn moved(p: &KontraParams) -> Vec<(usize, String, f64)> {
    let macros = read(
        &p.macros
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    let mut out = Vec::new();
    for (n, (m, last)) in macros.iter().zip(p.shared.macro_values.iter()).enumerate() {
        let (Some(m), Some(param)) = (m, param(p, n)) else {
            continue;
        };
        let now = f64::from(param.value());
        if (now - f64::from_bits(last.load(Ordering::Relaxed))).abs() > 1e-6 {
            last.store(now.to_bits(), Ordering::Relaxed);
            out.push((m.part, m.widget.clone(), m.plain(now)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assignments_read_back_as_written() {
        let mut macros = none();
        macros[2] = Some(Macro {
            part: 1,
            widget: "$reverb amount".into(),
            min: -3.0,
            max: 1000.0,
            steps: Vec::new(),
            path: "/a lib/b.nki".into(),
            label: label("Una Corda", "$reverb_amount"),
        });
        assert_eq!(
            macros[2].as_ref().unwrap().label,
            "Una Corda: reverb amount"
        );
        macros[4] = Some(Macro {
            part: 0,
            widget: "mode menu".into(),
            min: 0.0,
            max: 10.0,
            steps: vec![0.0, 1.0, 10.0],
            path: String::new(),
            label: String::new(),
        });
        assert_eq!(read(&write(&macros)), macros);
        let m = macros[2].as_ref().unwrap();
        assert!((m.plain(m.normalized(500.0)) - 500.0).abs() < 1e-9);
        // A menu takes its nearest item value.
        let menu = macros[4].as_ref().unwrap();
        assert_eq!(menu.plain(0.4), 1.0);
        assert_eq!(menu.plain(0.6), 10.0);
    }

    fn knob(name: &str, id: Option<u32>, host: Option<&str>) -> Widget {
        use kontra_ui_model::{Display, PageRef, Range, Rect};
        let range = Range {
            max: 127.0,
            ..Range::default()
        };
        let display = Display::default();
        let mut w = Widget::new(
            name,
            PageRef(0),
            Rect::new(0, 0, 40, 40),
            Kind::Knob { range, display },
        );
        w.automation.id = id;
        w.automation.name = host.map(Into::into);
        w
    }

    #[test]
    fn a_scripts_automation_ids_take_their_slots_and_unwind_when_taken() {
        let p = KontraParams::new();
        let a = [
            knob("$dyn", Some(10), Some("Dynamics")),
            knob("$vib", Some(200), None),
            knob("$free", None, None),
        ];
        let b = [knob("$dyn", Some(10), Some("Dynamics"))];
        let parts = || {
            [
                (0, "Areia", "/a.nki".to_owned(), &a[..]),
                (1, "Areia", "/a.nki".to_owned(), &b[..]),
            ]
        };
        declare(&p, parts());
        let text = p.macros.read().unwrap().clone();
        let m = read(&text);
        let at = |n: usize| {
            m[n].as_ref()
                .map(|m| (m.part, m.widget.as_str(), m.label.as_str()))
        };
        // Kontakt's slot and name; the second instrument's same id and the
        // id past the pool unwind to the free ones.
        assert_eq!(at(10), Some((0, "$dyn", "Dynamics")));
        assert_eq!(at(0), Some((0, "$vib", "Areia: vib")));
        assert_eq!(at(1), Some((1, "$dyn", "Dynamics")));
        assert_eq!(
            m.iter().flatten().count(),
            3,
            "a control without an id waits for a touch"
        );
        // Declared again on the next tick: nothing moves.
        declare(&p, parts());
        assert_eq!(*p.macros.read().unwrap(), text);
        // The host sees the slot's name and drives its control.
        assert_eq!(id(10), 12);
        assert_eq!(
            p.param_infos().iter().find(|i| i.id == 12).map(|i| i.name),
            Some("#010")
        );
        // A control its script keeps from the host takes no slot.
        let mut kept = knob("$kept", None, None);
        kept.automation.allowed = false;
        assert_eq!(touch(&p, 0, "Areia", &kept, 1.0), None);
    }
}
