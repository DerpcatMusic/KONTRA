//! Host automation of library panel controls: a fixed pool of host
//! parameters (Macro 1..N). The first time the player moves a panel
//! control it takes the next free macro (learn by touching) and the host
//! records its moves; the host's automation of a macro then moves that
//! control, and its script runs its callback as for the player's move.
//! Assignments save with the plugin's state (`KontraParams::macros`), one
//! per line: `macro rack-slot min max widget-name`.

use kontra_ui_model::{Kind, Widget};
use moose::prelude::*;

use crate::KontraParams;

/// Host parameters in the pool.
pub const COUNT: usize = 8;

/// A macro's control: a rack part's widget by name, and its range.
#[derive(Clone, Debug, PartialEq)]
pub struct Macro {
    pub part: usize,
    pub widget: String,
    pub min: f64,
    pub max: f64,
}

impl Macro {
    /// The control's value at host value `normalized` (0..1).
    fn plain(&self, normalized: f64) -> f64 {
        self.min + normalized.clamp(0.0, 1.0) * (self.max - self.min)
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
    match &w.kind {
        Kind::Knob { range, .. } | Kind::Slider { range, .. } | Kind::ValueEdit { range, .. } => {
            Some((range.min, range.max))
        }
        Kind::Menu { items } => {
            let values = items.iter().map(|i| f64::from(i.value));
            let (lo, hi) = values.fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
            (lo < hi).then_some((lo, hi))
        }
        Kind::Switch | Kind::Button { momentary: false } => Some((0.0, 1.0)),
        _ => None,
    }
}

/// The assignments `text` saved, by macro.
#[must_use]
pub fn read(text: &str) -> [Option<Macro>; COUNT] {
    let mut out: [Option<Macro>; COUNT] = Default::default();
    for line in text.lines() {
        let mut f = line.splitn(5, ' ');
        let mut num = || f.next().and_then(|s| s.parse::<f64>().ok());
        let (Some(n), Some(part), Some(min), Some(max)) = (num(), num(), num(), num()) else {
            continue;
        };
        let Some(widget) = f.next().filter(|w| !w.is_empty()) else {
            continue;
        };
        if let Some(slot) = out.get_mut(n as usize) {
            *slot = Some(Macro {
                part: part as usize,
                widget: widget.to_owned(),
                min,
                max,
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
            out.push_str(&format!(
                "{n} {} {} {} {}\n",
                m.part, m.min, m.max, m.widget
            ));
        }
    }
    out
}

/// The player moved `w` of rack part `part` to `plain`: its macro, taken
/// now if it had none and one is free, and its host value. None for a
/// control no macro can drive, or when every macro is taken.
pub fn touch(p: &KontraParams, part: usize, w: &Widget, plain: f64) -> Option<(u32, f64)> {
    let (min, max) = range(w)?;
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
        Some(n) => n,
        None => {
            let n = macros.iter().position(Option::is_none)?;
            let taken = Macro {
                part,
                widget: w.name.clone(),
                min,
                max,
            };
            *macros.get_mut(n)? = Some(taken);
            *text = write(&macros);
            n
        }
    };
    let m = macros.get(n)?.as_ref()?;
    let value = m.normalized(plain);
    // The store first: a tick between this and the host's echo sees the
    // value the control has, and does not move it back.
    let param = param(p, n)?;
    param.set_value(value);
    if let Some(last) = lock_last(p).get_mut(n) {
        *last = value;
    }
    Some((id(n), value))
}

/// The host id of macro `n`.
#[must_use]
pub fn id(n: usize) -> u32 {
    FIRST + n as u32
}

/// The id the first macro has (`KontraParams`).
pub const FIRST: u32 = 2;

pub(crate) fn param(p: &KontraParams, n: usize) -> Option<&FloatParam> {
    Some(match n {
        0 => &p.macro_1,
        1 => &p.macro_2,
        2 => &p.macro_3,
        3 => &p.macro_4,
        4 => &p.macro_5,
        5 => &p.macro_6,
        6 => &p.macro_7,
        7 => &p.macro_8,
        _ => return None,
    })
}

fn lock_last(p: &KontraParams) -> std::sync::MutexGuard<'_, [f64; COUNT]> {
    crate::lock(&p.shared.macro_values)
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
    let mut last = lock_last(p);
    let mut out = Vec::new();
    for (n, (m, last)) in macros.iter().zip(last.iter_mut()).enumerate() {
        let (Some(m), Some(param)) = (m, param(p, n)) else {
            continue;
        };
        let now = f64::from(param.value());
        if (now - *last).abs() > 1e-6 {
            *last = now;
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
        let mut macros: [Option<Macro>; COUNT] = Default::default();
        macros[2] = Some(Macro {
            part: 1,
            widget: "$reverb amount".into(),
            min: -3.0,
            max: 1000.0,
        });
        assert_eq!(read(&write(&macros)), macros);
        let m = macros[2].as_ref().unwrap();
        assert!((m.plain(m.normalized(500.0)) - 500.0).abs() < 1e-9);
    }
}
