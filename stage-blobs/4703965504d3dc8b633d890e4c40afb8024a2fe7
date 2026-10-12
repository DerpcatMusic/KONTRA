//! The computer keyboard as a piano, laid out as the common DAWs lay it:
//! the home row plays the white keys from A, the row above the black keys,
//! Z and X step the octave, C and V the velocity. It hears keys through the
//! window's raw key hook, so it hears them come up, and it stands aside
//! while a text field has the focus or a modifier is held.

use crate::plugin::SamplerParams;
use moose::mui::mui::Ui;
use moose::mui::mui::host::{KeyEvent, NativeKey};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// Semitones above the octave's C, key by key.
const KEYS: [(char, u8); 18] = [
    ('a', 0), ('w', 1), ('s', 2), ('e', 3), ('d', 4), ('f', 5), ('t', 6), ('g', 7), ('y', 8),
    ('h', 9), ('u', 10), ('j', 11), ('k', 12), ('o', 13), ('l', 14), ('p', 15), (';', 16), ('\'', 17),
];
/// The octave whose C the A key plays: 5 is MIDI 60.
const OCTAVE: u8 = 5;
/// Highest octave that keeps the top key at or below MIDI 127.
const MAX_OCTAVE: u8 = (127 - 17) / 12;
const VELOCITY: u8 = 100;
const VELOCITY_STEP: u8 = 20;

pub struct Computer {
    pub on: AtomicBool,
    pub octave: AtomicU8,
    pub velocity: AtomicU8,
    /// The note each held physical key started.
    held: Mutex<HashMap<u64, u8>>,
}

impl Default for Computer {
    fn default() -> Self {
        Self {
            on: AtomicBool::new(false),
            octave: AtomicU8::new(OCTAVE),
            velocity: AtomicU8::new(VELOCITY),
            held: Mutex::default(),
        }
    }
}

impl Computer {
    /// Take `event` if it plays: true keeps it from MUI and the host.
    pub fn key(&self, ui: &Ui, p: &SamplerParams, event: &KeyEvent) -> bool {
        let mut held = super::lock(&self.held);
        if !event.down {
            return match held.remove(&event.code) {
                Some(note) => {
                    p.shared.release_key(note);
                    true
                }
                None => false,
            };
        }
        let m = event.mods;
        if !self.on.load(Ordering::Relaxed) || ui.focus_is_text() || m.ctrl || m.alt || m.cmd {
            return false;
        }
        // The window repeats a held key: it is already playing.
        if held.contains_key(&event.code) {
            return true;
        }
        let NativeKey::Text(text) = &event.key else {
            return false;
        };
        let Some(c) = text.chars().next().map(|c| c.to_ascii_lowercase()) else {
            return false;
        };
        let step = |a: &AtomicU8, f: &dyn Fn(u8) -> u8| {
            let _ = a.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| Some(f(v)));
        };
        match c {
            'z' => step(&self.octave, &|o| o.saturating_sub(1)),
            'x' => step(&self.octave, &|o| (o + 1).min(MAX_OCTAVE)),
            'c' => step(&self.velocity, &|v| v.saturating_sub(VELOCITY_STEP).max(1)),
            'v' => step(&self.velocity, &|v| v.saturating_add(VELOCITY_STEP).min(127)),
            _ => {
                let Some(&(_, semitone)) = KEYS.iter().find(|(k, _)| *k == c) else {
                    return false;
                };
                let note = self.octave.load(Ordering::Relaxed) * 12 + semitone;
                let slot = p.shared.selected.load(Ordering::Relaxed) as usize;
                p.shared.press_key(slot, note, self.velocity.load(Ordering::Relaxed));
                held.insert(event.code, note);
            }
        }
        true
    }

    /// Forget the held keys, stopping their notes.
    pub fn release(&self, p: &SamplerParams) {
        for (_, note) in super::lock(&self.held).drain() {
            p.shared.release_key(note);
        }
    }

    /// The notes the held keys play.
    pub fn notes(&self) -> Vec<u8> {
        super::lock(&self.held).values().copied().collect()
    }

    /// The octave the A key plays, as a note name's octave number.
    pub fn octave_c(&self) -> u8 {
        self.octave.load(Ordering::Relaxed) * 12
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_span_an_octave_and_a_half() {
        const { assert!(MAX_OCTAVE * 12 + 17 <= 127) };
        assert!(KEYS.windows(2).all(|w| w[1].1 == w[0].1 + 1));
    }
}
