//! A voice that cannot be heard skips reading its source; everything it does
//! must match the same voice rendered in full. The reference is
//! `render_through` with no insert: the same code, which never skips.
use std::sync::Arc;

use kontra_stream::{
    BlobKey, Encoding, MemSource, PAGE_FRAMES, SampleFmt, Store, StoreConfig, StreamClient,
};
use kontra_voice::{Ahdsr, Gain, Lag, Loop, Region, Scratch, Voice, VoiceStart};

const FMT: SampleFmt = SampleFmt {
    encoding: Encoding::F32,
    channels: 2,
};
const UNITY: Gain = [[0.8, 0.1], [0.2, 0.7]];
const ZERO: Gain = [[0.0; 2]; 2];

/// A stereo tone with no exact interpolation: every read matters.
fn region(store: &mut Store, frames: u64, pinned: u64, loops: Vec<Loop>) -> Region {
    let bytes = (0..frames)
        .flat_map(|n| {
            let t = n as f32;
            [(t * 0.031).sin() * 0.5, (t * 0.017).cos() * 0.4]
        })
        .flat_map(f32::to_le_bytes)
        .collect();
    let key = BlobKey {
        file: format!("tone{frames}-{pinned}-{}", loops.len()).into(),
        offset: 0,
        fmt: FMT,
    };
    let sample = store
        .load(
            &key,
            Arc::new(MemSource::new(FMT, bytes)),
            frames,
            std::slice::from_ref(&(0..pinned)),
        )
        .unwrap();
    Region {
        mix: None,
        table: None,
        sample,
        start: 0,
        end: frames,
        loops: Region::pack_loops(loops),
        reverse: false,
    }
}

fn voice(region: Region, step: f64, gain: Gain, envelope: Ahdsr) -> Voice {
    let mut voice = Voice::allocate();
    voice.start(VoiceStart {
        speed: 1.0,
        region,
        loop_shift: [0.0; 2],
        mix: None,
        read: None,
        oscillator: None,
        granular: None,
        step,
        gain,
        lag: [Lag::NONE; kontra_voice::LAGS],
        envelope,
        fade: None,
    });
    voice
}

/// What a block of one voice leaves behind: its output bits, whether it
/// still plays, and where its cursor is.
type Trace = Vec<(Vec<u32>, bool, u64)>;

/// Renders `blocks` blocks of 128 frames; before block `b`, `at(b, voice)`
/// changes the voice (gain, release).
fn play(
    mut voice: Voice,
    client: &StreamClient,
    full: bool,
    blocks: usize,
    at: impl Fn(usize, &mut Voice),
) -> Trace {
    let mut scratch = Scratch::allocate();
    (0..blocks)
        .map(|b| {
            at(b, &mut voice);
            let mut out = vec![[0.0f32; 2]; 128];
            let now = (b * 128) as u64;
            if full {
                voice.render_through(&mut scratch, client, now, &mut out, |_| {});
            } else {
                voice.render(&mut scratch, client, now, &mut out);
            }
            let bits = out.as_flattened().iter().map(|v| v.to_bits()).collect();
            (bits, voice.is_active(), voice.position().to_bits())
        })
        .collect()
}

fn same(
    store: &mut Store,
    step: f64,
    loops: Vec<Loop>,
    envelope: Ahdsr,
    blocks: usize,
    at: impl Fn(usize, &mut Voice) + Copy,
) {
    let client = store.client();
    let skip = play(
        voice(
            region(store, 20_000, 20_000, loops.clone()),
            step,
            ZERO,
            envelope,
        ),
        &client,
        false,
        blocks,
        at,
    );
    let full = play(
        voice(region(store, 20_000, 20_000, loops), step, ZERO, envelope),
        &client,
        true,
        blocks,
        at,
    );
    for (b, (s, f)) in skip.iter().zip(&full).enumerate() {
        assert_eq!(s, f, "block {b}");
    }
}

fn crossfaded_loop() -> Vec<Loop> {
    vec![Loop {
        start: 3000,
        end: 4100,
        crossfade: 300,
        ..Loop::default()
    }]
}

#[test]
fn gain_coming_back_up_plays_what_full_rendering_plays() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    // Silent, audible, silent again, audible: a crossfade layer moving.
    let at = |b: usize, v: &mut Voice| match b {
        7 | 31 => v.set_gain(UNITY),
        19 => v.set_gain(ZERO),
        _ => {}
    };
    for step in [1.0, 0.9187, 1.31] {
        same(&mut store, step, crossfaded_loop(), Ahdsr::GATE, 60, at);
    }
}

#[test]
fn an_envelope_rising_under_zero_gain_resumes_at_its_level() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let envelope = Ahdsr {
        attack: 3000,
        decay: 2000,
        sustain: 0.6,
        attack_shape: 1.5,
        ..Ahdsr::GATE
    };
    let at = |b: usize, v: &mut Voice| {
        if b == 13 {
            v.set_gain(UNITY);
        }
    };
    same(&mut store, 0.9187, Vec::new(), envelope, 50, at);
}

#[test]
fn a_silent_voice_crosses_loops_and_resumes_mid_crossfade() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    // Silent through several loop passes, audible inside a crossfade.
    for wake in [40, 47, 55] {
        let at = move |b: usize, v: &mut Voice| {
            if b == wake {
                v.set_gain(UNITY);
            }
        };
        same(&mut store, 1.0, crossfaded_loop(), Ahdsr::GATE, 80, at);
        same(&mut store, 1.31, crossfaded_loop(), Ahdsr::GATE, 80, at);
    }
}

#[test]
fn a_silent_voice_releases_and_ends_at_the_same_frame() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let envelope = Ahdsr {
        release: 700,
        ..Ahdsr::GATE
    };
    // Released while silent (looped), and released while silent near the
    // sample's end (unlooped): both end on the same block, same cursor.
    let at = |b: usize, v: &mut Voice| {
        if b == 20 {
            v.release();
        }
    };
    same(&mut store, 1.0, crossfaded_loop(), envelope, 40, at);
    same(&mut store, 1.31, Vec::new(), envelope, 160, |_, _| {});
}

#[test]
fn a_silent_voice_stays_active_and_counts_as_playing() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let trace = play(
        voice(
            region(&mut store, 20_000, 20_000, crossfaded_loop()),
            1.0,
            ZERO,
            Ahdsr::GATE,
        ),
        &client,
        false,
        100,
        |_, _| {},
    );
    assert!(
        trace
            .iter()
            .all(|(bits, active, _)| *active && bits.iter().all(|&b| b == 0))
    );
}

/// The regression guard: a silent voice does not read its source. With its
/// pages out of memory, the full render counts missing frames; the skipped
/// one never asks.
#[test]
fn a_silent_voice_does_not_read_its_source() {
    let mut store = Store::new(StoreConfig {
        request_capacity: 1,
        ..StoreConfig::DEFAULT
    })
    .unwrap();
    let client = store.client();
    let frames = 8 * PAGE_FRAMES;
    let mut skipped = voice(
        region(&mut store, frames, PAGE_FRAMES, Vec::new()),
        1.0,
        ZERO,
        Ahdsr::GATE,
    );
    let mut full = voice(
        region(&mut store, frames, PAGE_FRAMES, Vec::new()),
        1.0,
        ZERO,
        Ahdsr::GATE,
    );
    let mut out = vec![[0.0; 2]; 3 * PAGE_FRAMES as usize];
    skipped.render(&mut Scratch::allocate(), &client, 0, &mut out);
    full.render_through(&mut Scratch::allocate(), &client, 0, &mut out, |_| {});
    assert!(full.missing_frames() > 0);
    assert_eq!(skipped.missing_frames(), 0);
    assert_eq!(skipped.position(), full.position());
}
