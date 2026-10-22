//! Kontakt DirectFromDisk preload window, ported from the Kontakt 8 stream routine
//! `FUN_14082a530` (`source/kontakt8/0140820000.c`, body read 2026-10-22).
//!
//! Field names follow the decompile offsets: `+0x9d0` position, `+0x9d4` reverse position,
//! `+0x9d8` offset, `+0xa8` linked object (`+0x177c` blocked flag, `+0x1794` offset), `+0xa09`
//! blocked flag. Units: positions and offsets in frames; window bytes in bytes.

/// Window granularity in bytes (`0x1800`). Also the step of the UI preload value.
pub const QUANTUM: i64 = 0x1800;
/// Instrument preload default set at init (`0140940000.c`, `0xf000`).
pub const PRELOAD_DEFAULT: i32 = 0xf000;

/// The effective preload for an instrument: the global value when its override flag is set,
/// else the authored value (`DAT_14a5b96cc` selects `DAT_149e55070` at the read sites).
pub fn effective_preload(authored: i32, global_override: Option<i32>) -> i32 {
    global_override.unwrap_or(authored)
}

/// Linked object consulted when the stored offset is negative.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Linked {
    /// `+0x177c`: non-zero means the link is unusable.
    pub blocked: bool,
    /// `+0x1794`: offset used when the link is usable. Negative skips the whole window.
    pub offset: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    /// Bits per sample (the routine's `param_2`, e.g. 16 or 32).
    pub bits: u32,
    /// Channel count (`param_3` before rescaling).
    pub channels: i64,
    /// Authored or global preload, bytes per channel (`0x21b70`).
    pub preload: i32,
    /// Runtime source mode 4 (streamed). Other modes get no window.
    pub streamed: bool,
    pub reverse: bool,
    /// `+0x9d0`.
    pub position: i64,
    /// `+0x9d4`; the start is `-reverse_position` when reversed.
    pub reverse_position: i64,
    /// `+0x9d8`.
    pub offset: i64,
    pub linked: Option<Linked>,
    /// `+0xa09` on the stream object.
    pub blocked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    /// Start in frames (`+8` of the result).
    pub start: i64,
    /// Window bytes, negated when reversed (`+4` of the result). Zero when not streamed.
    pub bytes: i64,
}

/// Port of the routine. Non-positive preloads and skipped links keep the raw (unrounded)
/// preload times channels, exactly as the decompile leaves that variable.
pub fn window(input: Input) -> Window {
    let mut base = if input.streamed {
        i64::from(input.preload) * input.channels
    } else {
        0
    };
    let mut start = if input.reverse {
        -input.reverse_position
    } else {
        input.position
    };
    let mut offset = input.offset;
    let mut skip = false;
    if offset < 0 {
        match input.linked {
            None => offset = 0,
            Some(link) if link.blocked || input.blocked => offset = 0,
            Some(link) if link.offset < 0 => skip = true,
            Some(link) => offset = link.offset,
        }
    }
    let mut bytes = base;
    if !skip && base > 0 {
        let bytes_per_frame = (i64::from(input.bits) + 7) / 8 * input.channels;
        let lag = start - offset;
        if lag < 0 {
            base += bytes_per_frame * lag;
        }
        let total = base + offset * bytes_per_frame * 2;
        // C integer division truncates toward zero, as Rust's `/` does.
        let quotient = total / QUANTUM;
        let rounded = if total == quotient * QUANTUM { quotient } else { quotient + 1 };
        bytes = rounded * QUANTUM;
        start = lag.max(0);
    }
    Window {
        start,
        bytes: if input.reverse { -bytes } else { bytes },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Input {
        Input {
            bits: 32,
            channels: 2,
            preload: 61440,
            streamed: true,
            reverse: false,
            position: 0,
            reverse_position: 0,
            offset: 0,
            linked: None,
            blocked: false,
        }
    }

    #[test]
    fn exact_multiple_keeps_the_preload_and_zero_offset_starts_at_zero() {
        // 61440 * 2 channels = 122880 = 20 quanta exactly.
        assert_eq!(window(base()), Window { start: 0, bytes: 122880 });
    }

    #[test]
    fn offset_before_the_position_shrinks_the_base_then_rounds_up() {
        // lag = 0 - 1000 = -1000 frames; BPF = 8. base = 122880 - 8000 = 114880,
        // total = 114880 + 1000*8*2 = 130880, rounds up to 22 quanta.
        let w = window(Input { offset: 1000, ..base() });
        assert_eq!(w, Window { start: 0, bytes: 22 * QUANTUM });
    }

    #[test]
    fn reverse_negates_the_window_and_uses_the_reverse_position() {
        let w = window(Input { reverse: true, reverse_position: 500, ..base() });
        assert_eq!(w, Window { start: 0, bytes: -122880 });
        let w = window(Input { reverse: true, reverse_position: -500, ..base() });
        assert_eq!(w.start, 500);
    }

    #[test]
    fn not_streamed_has_no_window_but_keeps_the_start() {
        let w = window(Input { streamed: false, position: 77, ..base() });
        assert_eq!(w, Window { start: 77, bytes: 0 });
    }

    #[test]
    fn a_negative_linked_offset_skips_rounding_and_keeps_the_raw_base() {
        let link = Linked { blocked: false, offset: -1 };
        let w = window(Input { offset: -5, linked: Some(link), position: 3, ..base() });
        assert_eq!(w, Window { start: 3, bytes: 122880 });
    }

    #[test]
    fn override_selects_the_global_preload() {
        assert_eq!(effective_preload(1000, Some(PRELOAD_DEFAULT)), PRELOAD_DEFAULT);
        assert_eq!(effective_preload(1000, None), 1000);
    }
}
