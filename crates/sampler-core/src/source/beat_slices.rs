//! Kontakt Beat Machine slice sequencer, crossfade law and voice playback.
//!
//! Ported from the render-side methods of `BSlice` (vtable `0x144f39f00`, Kontakt 8):
//! slot 12 (cursor reset), slot 13 (cursor settle) and slot 18 (crossfade lengths).
//! The table holds up to 8 entries; entry `i` here is the native element `i + 1`,
//! and missing entries read as kind 0 (skipped), as zeroed native memory does.
//!
//! Slot 18 turns the stored `slice_float_1` and `slice_float_2` (Kontakt's "Slice Atk"
//! and "Slice Rel", milliseconds) into frame counts at the render sample rate, which is
//! the render-context field at `+0x27d00` (set from the host rate, default 44100).
//! The voice ([`Player`]) plays the zone's grid slices in order at the zone's stored rate
//! (the source step, carried in `Playback::transpose_semitones`). The fade shape inside a
//! slice is linear over the attack and release lengths: the native shape is not recovered.
//! The native 0.001 and 0.5 constants are the `.rdata` values
//! (`docs/re/kontakt-rdata/constants.tsv`).

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

/// Zone slices of one Beat Machine zone: boundaries in source frames (slice `i` spans
/// `markers[i]..markers[i + 1]`, `count` entries used) and the group's crossfade controls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spec {
    pub markers: [u32; SLOTS + 1],
    pub count: u8,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub flag: bool,
}

/// One Beat Machine voice. The sequencer walks the slices in order; each slice plays for
/// its grid cell (its source length at the output rate) and the next one starts when the
/// cell ends. A slice fades in over `attack` frames and out over `release` frames.
#[derive(Clone, Copy, Debug)]
pub(super) struct Player {
    seq: Sequencer,
    spec: Spec,
    /// Output frames per source frame.
    ratio: f64,
    /// Output frames played in the current slice, and the slice length.
    slot: u32,
    len: u32,
}

impl Player {
    pub(super) fn new(spec: Spec, source_rate: u32, output_rate: u32) -> Self {
        let slices = usize::from(spec.count) - 1;
        let mut table = [Slice::default(); SLOTS];
        for entry in &mut table[..slices] {
            *entry = Slice {
                count: 1,
                kind: ACTIVE_KIND,
            };
        }
        let mut seq = Sequencer::new(table);
        seq.crossfade(
            spec.attack_ms,
            spec.release_ms,
            output_rate as f32,
            spec.flag,
        );
        seq.reset(0.0);
        let mut player = Self {
            seq,
            spec,
            ratio: f64::from(output_rate) / f64::from(source_rate),
            slot: 0,
            len: 0,
        };
        player.len = player.cell();
        player
    }

    /// Source frame where the current slice starts, as an offset from the sample start.
    pub(super) fn origin(&self, start: usize) -> u64 {
        u64::from(self.spec.markers[self.seq.index as usize]).saturating_sub(start as u64)
    }

    /// Output frames of the current slice: its source length at the output rate.
    fn cell(&self) -> u32 {
        let i = self.seq.index as usize;
        let source = self.spec.markers[i + 1] - self.spec.markers[i];
        ((f64::from(source) * self.ratio).round() as u32).max(1)
    }

    pub(super) fn slot_done(&self) -> bool {
        self.slot >= self.len
    }

    /// Advance to the next slice. Returns `false` after the last one.
    pub(super) fn next_slice(&mut self) -> bool {
        self.seq.index += 1;
        self.seq.settle();
        if self.seq.index as usize >= usize::from(self.spec.count) - 1 {
            return false;
        }
        self.slot = 0;
        self.len = self.cell();
        true
    }

    /// Gain of the next output frame: fade-in from the slice start, fade-out to its end.
    pub(super) fn gain(&self) -> f32 {
        let (t, n) = (self.slot as f32, self.len as f32);
        let rise = if self.seq.attack > 0 {
            (t / self.seq.attack as f32).min(1.)
        } else {
            1.
        };
        let fall = if self.seq.release > 0 {
            ((n - t) / self.seq.release as f32).min(1.)
        } else {
            1.
        };
        rise * fall
    }

    pub(super) fn played(&mut self) {
        self.slot += 1;
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

    fn grid_spec() -> Spec {
        Spec {
            markers: [0, 6000, 12000, 18000, 24000, 0, 0, 0, 0],
            count: 5,
            attack_ms: 3.0,
            release_ms: 12.5,
            flag: true,
        }
    }

    #[test]
    fn player_walks_the_grid_slices_in_order_then_ends() {
        let mut p = Player::new(grid_spec(), 48000, 48000);
        assert_eq!((p.origin(0), p.cell()), (0, 6000));
        assert!(p.next_slice());
        assert_eq!(p.origin(0), 6000);
        assert!(p.next_slice());
        assert!(p.next_slice());
        assert_eq!(p.origin(0), 18000);
        assert!(!p.next_slice(), "four slices end after the fourth");
    }

    #[test]
    fn player_cell_follows_the_output_rate() {
        assert_eq!(Player::new(grid_spec(), 48000, 44100).cell(), 5513);
    }

    #[test]
    fn player_fades_in_over_attack_and_out_over_release() {
        let mut p = Player::new(grid_spec(), 48000, 48000);
        assert_eq!(p.gain(), 0.0);
        for _ in 0..144 {
            p.played();
        }
        assert_eq!(p.gain(), 1.0);
        for _ in 144..5999 {
            p.played();
        }
        assert!((p.gain() - 1.0 / 600.0).abs() < 1e-6);
    }
}
