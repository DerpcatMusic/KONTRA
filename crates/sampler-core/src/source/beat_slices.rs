//! Kontakt Beat Machine slice sequencer and crossfade law.
//!
//! Ported from the render-side methods of `BSlice` (vtable `0x144f39f00`, Kontakt 8):
//! slot 12 (cursor reset), slot 13 (cursor settle) and slot 18 (crossfade lengths).
//! The table holds up to 8 entries; entry `i` here is the native element `i + 1`,
//! and missing entries read as kind 0 (skipped), as zeroed native memory does.
//!
//! Slot 18 turns the stored `slice_float_1` and `slice_float_2` (Kontakt's "Slice Atk"
//! and "Slice Rel", milliseconds) into frame counts at the render sample rate, which is
//! the render-context field at `+0x27d00` (set from the host rate, default 44100).
//! Wired as data only: the loader fills `ir::Group::beat_slices`, and no voice runs
//! this sequencer yet, so the Beat Machine still plays as a sampler. Slot 18 also sets a
//! field from a call into the source player, which is not modelled.
//! The native 0.001 and 0.5 constants are the `.rdata` values
//! (`docs/re/kontakt-rdata/constants.tsv`).
#![allow(dead_code)] // the sequencer has no voice caller yet, see the module docs

/// Native entry kinds below this are skipped by the sequencer.
pub const ACTIVE_KIND: i32 = 3;
/// Kind of a loop entry: reaching it sets the loop count to 1.
pub const LOOP_KIND: i32 = 4;
/// Entries the sequencer walks (native indices 0..8).
pub const SLOTS: usize = 8;

/// Native milliseconds-to-seconds factor used by slot 18: `0.001` as `f32` (bits `0x3a83126f`).
const MS_SCALE: f32 = f32::from_bits(0x3a83_126f);
/// Native rounding offset used by slot 18 (bits `0x3f000000`).
const HALF: f32 = f32::from_bits(0x3f00_0000);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Slice {
    /// Native `+0x18`: loop repeat count, 0 means unbounded.
    pub count: i32,
    /// Native `+0x20`: entry kind.
    pub kind: i32,
}

/// Sequencer state of one Beat Machine group (the modelled `BSlice` fields).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sequencer {
    pub slices: [Slice; SLOTS],
    /// Native `+0x88`: current entry index. Written only when the walk advances.
    pub index: u32,
    /// Native `+0x8c`: loop count left; `-1` before a reset.
    pub remaining: i32,
    /// Native `+0x5c`: set on every exit of the reset.
    pub reset_done: bool,
    /// Native `+0x74`: position from the reset argument, used when `flag` is clear.
    pub position: i32,
    /// Native `+0x6c`: cleared by the reset when `flag` is set.
    pub alt_position: i32,
    /// Native `+0x50`: set from the stored `slice_flag_1` by [`Sequencer::crossfade`].
    pub flag: bool,
    /// Native `+0x54`: "Slice Atk" crossfade attack, in frames (`slice_float_1`).
    pub attack: i32,
    /// Native `+0x58`: "Slice Rel" crossfade release, in frames (`slice_float_2`).
    pub release: i32,
}

impl Sequencer {
    pub fn new(slices: [Slice; SLOTS]) -> Self {
        Self {
            slices,
            index: 0,
            remaining: -1,
            reset_done: false,
            position: 0,
            alt_position: 0,
            flag: false,
            attack: 0,
            release: 0,
        }
    }

    fn kind(&self, i: usize) -> i32 {
        self.slices[i].kind
    }

    /// Slot 12: reset the cursor to the first active entry.
    pub fn reset(&mut self, position: f64) {
        if self.flag {
            self.alt_position = 0;
        } else {
            self.position = position as i32;
        }
        self.remaining = -1;

        let mut i = 0usize;
        while self.kind(i) < ACTIVE_KIND {
            i += 1;
            self.index = i as u32;
            if i > SLOTS - 1 {
                self.reset_done = true;
                return;
            }
        }
        self.reset_done = true;
        let count = self.slices[i].count;
        self.remaining = if count == 0 { i32::MAX } else { count };
    }

    /// Slot 13: skip inactive entries from the current index and mark a loop entry.
    pub fn settle(&mut self) {
        let mut i = self.index as usize;
        if i >= SLOTS {
            return;
        }
        while self.kind(i) < ACTIVE_KIND {
            i += 1;
            self.index = i as u32;
            if i > SLOTS - 1 {
                return;
            }
        }
        if self.kind(i) == LOOP_KIND {
            self.remaining = 1;
        }
    }

    /// Slot 18: crossfade attack and release in frames, from the Slice Atk and Slice Rel
    /// times in milliseconds and the render sample rate (native `+0x27d00`). Each value is
    /// `ms * 0.001 * rate + 0.5`, truncated to an integer.
    pub fn crossfade(&mut self, attack_ms: f32, release_ms: f32, rate: f32, flag: bool) {
        self.release = (release_ms * MS_SCALE * rate + HALF) as i32;
        self.attack = (attack_ms * MS_SCALE * rate + HALF) as i32;
        self.flag = flag;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(kinds: &[(i32, i32)]) -> [Slice; SLOTS] {
        let mut t = [Slice::default(); SLOTS];
        for (dst, &(kind, count)) in t.iter_mut().zip(kinds) {
            *dst = Slice { count, kind };
        }
        t
    }

    #[test]
    fn native_constants_match_rdata() {
        // constants.tsv: 0x1447148a4 = 0.00100000005 (bits 3a83126f), 0x14470bad0 = 0.5.
        assert_eq!(MS_SCALE.to_bits(), 0x3a83_126f);
        assert_eq!(HALF, 0.5);
    }

    #[test]
    fn reset_skips_inactive_entries_and_takes_their_count() {
        // Entries 0 and 1 are below kind 3; entry 2 is active with count 5.
        let mut s = Sequencer::new(table(&[(0, 0), (2, 9), (3, 5), (4, 0)]));
        s.reset(0.0);
        assert_eq!((s.index, s.remaining, s.reset_done), (2, 5, true));
    }

    #[test]
    fn reset_with_zero_count_is_unbounded() {
        let mut s = Sequencer::new(table(&[(3, 0)]));
        s.reset(0.0);
        assert_eq!((s.index, s.remaining), (0, i32::MAX));
    }

    #[test]
    fn reset_with_no_active_entry_walks_past_the_table() {
        let mut s = Sequencer::new(table(&[]));
        s.reset(0.0);
        assert_eq!((s.index, s.remaining, s.reset_done), (8, -1, true));
    }

    #[test]
    fn reset_position_depends_on_the_flag() {
        let mut s = Sequencer::new(table(&[(3, 1)]));
        s.reset(12.9);
        assert_eq!(s.position, 12);
        s.flag = true;
        s.alt_position = 7;
        s.reset(40.0);
        assert_eq!((s.position, s.alt_position), (12, 0));
    }

    #[test]
    fn settle_marks_a_loop_entry() {
        let mut s = Sequencer::new(table(&[(3, 5), (4, 2)]));
        s.index = 1;
        s.remaining = 5;
        s.settle();
        assert_eq!(s.remaining, 1);
    }

    #[test]
    fn settle_skips_inactive_entries_without_marking() {
        let mut s = Sequencer::new(table(&[(1, 0), (2, 0), (3, 4)]));
        s.settle();
        assert_eq!((s.index, s.remaining), (2, -1));
    }

    #[test]
    fn crossfade_is_rounded_milliseconds_at_the_render_rate() {
        // 100 ms at 48 kHz = 4800 frames; 500 ms = 24000 frames; 44.1 kHz: 100 ms = 4410.
        // Atk 100 ms is the `slice_float_1` slot, Rel 500 ms the `slice_float_2` slot.
        let mut s = Sequencer::new(table(&[]));
        s.crossfade(100.0, 500.0, 48000.0, true);
        assert_eq!((s.attack, s.release, s.flag), (4800, 24000, true));
        s.crossfade(100.0, 500.0, 44100.0, false);
        assert_eq!((s.attack, s.release), (4410, 22050));
    }
}
