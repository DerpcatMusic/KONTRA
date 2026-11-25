//! A voice that cannot be heard (gain under -140 dB) skips reading its
//! source; everything it does must match the same voice rendered in full,
//! its output within float noise. The reference is `render_through_stages`
//! with no inserts: the same code, which never culls (a post-amp stage rings).
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
fn region(
    store: &mut Store,
    frames: u64,
    pinned: u64,
    loops: Vec<Loop>,
) -> std::io::Result<Region> {
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
    let sample = store.load(
        &key,
        Arc::new(MemSource::new(FMT, bytes)),
        frames,
        std::slice::from_ref(&(0..pinned)),
    )?;
    Ok(Region {
        mix: None,
        table: None,
        sample,
        start: 0,
        end: frames,
        loops: Region::pack_loops(loops),
        reverse: false,
    })
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

/// What a block of one voice leaves behind: its output, whether it still
/// plays, and where its cursor is.
type Trace = Vec<(Vec<f32>, bool, u64)>;

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
                voice.render_through_stages(&mut scratch, client, now, &mut out, |_, _| {});
            } else {
                voice.render(&mut scratch, client, now, &mut out);
            }
            let values = out.as_flattened().to_vec();
            (values, voice.is_active(), voice.position().to_bits())
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
) -> std::io::Result<()> {
    let client = store.client();
    let skip = play(
        voice(
            region(store, 20_000, 20_000, loops.clone())?,
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
        voice(region(store, 20_000, 20_000, loops)?, step, ZERO, envelope),
        &client,
        true,
        blocks,
        at,
    );
    for (b, (s, f)) in skip.iter().zip(&full).enumerate() {
        assert_eq!((s.1, s.2), (f.1, f.2), "block {b}");
        let worst =
            s.0.iter()
                .zip(&f.0)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f32::max);
        assert!(worst <= 1e-6, "block {b}: off by {worst}");
    }
    Ok(())
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
        same(&mut store, step, crossfaded_loop(), Ahdsr::GATE, 60, at).unwrap();
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
    same(&mut store, 0.9187, Vec::new(), envelope, 50, at).unwrap();
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
        same(&mut store, 1.0, crossfaded_loop(), Ahdsr::GATE, 80, at).unwrap();
        same(&mut store, 1.31, crossfaded_loop(), Ahdsr::GATE, 80, at).unwrap();
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
    same(&mut store, 1.0, crossfaded_loop(), envelope, 40, at).unwrap();
    same(&mut store, 1.31, Vec::new(), envelope, 160, |_, _| {}).unwrap();
}

#[test]
fn a_silent_voice_stays_active_and_counts_as_playing() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let trace = play(
        voice(
            region(&mut store, 20_000, 20_000, crossfaded_loop()).unwrap(),
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
            .all(|(values, active, _)| *active && values.iter().all(|&v| v == 0.0))
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
        region(&mut store, frames, PAGE_FRAMES, Vec::new()).unwrap(),
        1.0,
        ZERO,
        Ahdsr::GATE,
    );
    let mut full = voice(
        region(&mut store, frames, PAGE_FRAMES, Vec::new()).unwrap(),
        1.0,
        ZERO,
        Ahdsr::GATE,
    );
    let mut out = vec![[0.0; 2]; 3 * PAGE_FRAMES as usize];
    skipped.render(&mut Scratch::allocate(), &client, 0, &mut out);
    full.render_through_stages(&mut Scratch::allocate(), &client, 0, &mut out, |_, _| {});
    assert!(full.missing_frames() > 0);
    assert_eq!(skipped.missing_frames(), 0);
    assert_eq!(skipped.position(), full.position());
}

/// A one-pole lowpass (about 100 frames) with its state in the closure.
fn lowpass(frames: &std::cell::Cell<usize>) -> impl FnMut(&mut [[f32; 2]]) + '_ {
    let mut z = [0.0f32; 2];
    move |io| {
        frames.set(frames.get() + io.len());
        for f in io {
            for (z, x) in z.iter_mut().zip(f.iter_mut()) {
                *z += (*x - *z) * 0.01;
                *x = *z;
            }
        }
    }
}

/// A filtered voice muted long enough to be culled, then brought back: its
/// filter replays the frames before the cursor first, so it comes back as a
/// voice that never stopped filtering does, without a click.
#[test]
fn a_culled_filtered_voice_comes_back_without_a_click() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let regions = [0, 1].map(|_| region(&mut store, 20_000, 20_000, crossfaded_loop()).unwrap());
    let [mut culled, mut kept] = regions.map(|r| voice(r, 0.9187, UNITY, Ahdsr::GATE));
    let (culled_frames, kept_frames) = (std::cell::Cell::new(0), std::cell::Cell::new(0));
    let (mut a, mut b) = (lowpass(&culled_frames), lowpass(&kept_frames));
    let (mut sa, mut sb) = (Scratch::allocate(), Scratch::allocate());
    let mut worst = 0.0f32;
    for block in 0..100 {
        for v in [&mut culled, &mut kept] {
            match block {
                4 => v.set_gain(ZERO),
                70 => v.set_gain(UNITY),
                _ => {}
            }
        }
        let now = block as u64 * 128;
        let (mut x, mut y) = (vec![[0.0f32; 2]; 128], vec![[0.0f32; 2]; 128]);
        culled.render_through(&mut sa, &client, now, &mut x, &mut a);
        kept.render_through_stages(&mut sb, &client, now, &mut y, |stage, io| {
            if stage == kontra_voice::VoiceStage::PreAmp {
                b(io);
            }
        });
        let mut here = 0.0f32;
        for (p, q) in x.as_flattened().iter().zip(y.as_flattened()) {
            here = here.max((p - q).abs());
        }
        worst = worst.max(here);
    }
    // It was culled (its filter skipped frames, net of the replay) ...
    assert!(culled_frames.get() < kept_frames.get(), "never culled");
    // ... and came back within float noise of the voice that kept filtering.
    assert!(worst <= 1e-6, "off by {worst}");
}
