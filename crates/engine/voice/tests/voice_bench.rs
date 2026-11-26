//! Reproducible high-polyphony voice & synthesis benchmarks (48 kHz).
//!
//! Run in release mode:
//! `build-slot cargo test --manifest-path plugins/kontra/Cargo.toml -p kontra-voice --release --test voice_bench -- --ignored --nocapture`

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use kontra_stream::{BlobKey, Encoding, MemSource, SampleFmt, Store, StoreConfig, StreamClient};
use kontra_voice::{
    Ahdsr, Granular, GranularSpec, LEVELS, Loop, ModOsc, OscWave, Oscillator, Quality, Read,
    Region, SIZE, Scratch, Table, Voice, VoiceStart, Warp,
};

const RATE: u32 = 48_000;
const STEREO_F32: SampleFmt = SampleFmt {
    encoding: Encoding::F32,
    channels: 2,
};

fn stereo_sine_region(store: &mut Store, name: &str, frames: u64, looped: bool) -> Region {
    let mut bytes = Vec::with_capacity(frames as usize * 8);
    for n in 0..frames {
        let t = n as f32 / RATE as f32;
        let l = (std::f32::consts::TAU * 220.0 * t).sin() * 0.5;
        let r = (std::f32::consts::TAU * 220.5 * t).sin() * 0.5;
        bytes.extend_from_slice(&l.to_le_bytes());
        bytes.extend_from_slice(&r.to_le_bytes());
    }
    let key = BlobKey {
        file: name.into(),
        offset: 0,
        fmt: STEREO_F32,
    };
    let sample = store
        .load(
            &key,
            Arc::new(MemSource::new(STEREO_F32, bytes)),
            frames,
            std::slice::from_ref(&(0..frames)),
        )
        .unwrap();
    let loops = if looped {
        Some(Arc::from([Loop {
            start: 1024,
            end: frames - 1024,
            count: 0,
            crossfade: 256,
            until_release: false,
            alternating: false,
            tune_ratio: 1.0,
        }]))
    } else {
        None
    };
    Region {
        sample,
        start: 0,
        end: frames,
        loops,
        mix: None,
        table: None,
        reverse: false,
    }
}

fn wavetable_region(store: &mut Store) -> Region {
    let mut region = stereo_sine_region(store, "wt-dummy", 256, false);
    let mut data = Vec::new();
    let mut spans = Vec::new();
    for cycle in 0..4usize {
        for &len in &SIZE {
            let first = data.len() as u32;
            for i in 0..len {
                let ph = f64::from(i) / f64::from(len);
                let s = (std::f64::consts::TAU * ph).sin()
                    + 0.3 * (cycle as f64 / 3.0) * (std::f64::consts::TAU * 3.0 * ph).sin();
                data.push((s * 24_000.0).round() as i16);
            }
            spans.push((first, len));
        }
    }
    assert_eq!(spans.len(), 4 * LEVELS);
    let read = Read {
        position: 0.37,
        phase: 0.0,
        phase_random: 0.0,
        gain: 1.0,
        inharmonic: kontra_voice::Inharmonic::OFF,
        modosc: ModOsc::OFF,
        rate: RATE as f32,
        quality: Quality::High,
        warps: [Warp::OFF; 2],
    };
    region.table = Some(Arc::new(
        Table::new((data.into(), 1.0 / 32_767.0), spans.into(), read).unwrap(),
    ));
    region
}

fn bench_polyphony(
    label: &str,
    client: &StreamClient,
    voices_count: usize,
    block: usize,
    blocks: usize,
    mut make_start: impl FnMut(usize) -> VoiceStart,
    mut per_block: impl FnMut(usize, &mut [Voice]),
) {
    let mut scratch = Scratch::allocate();
    let mut voices: Vec<Voice> = (0..voices_count)
        .map(|i| {
            let mut v = Voice::allocate();
            v.set_sample_rate(RATE);
            v.start(make_start(i));
            v
        })
        .collect();
    let mut out = vec![[0.0f32; 2]; block];

    // Warmup
    for b in 0..8 {
        out.fill([0.0; 2]);
        per_block(b, &mut voices);
        for v in &mut voices {
            v.render(&mut scratch, client, (b * block) as u64, &mut out);
        }
        black_box(&out);
    }

    let t0 = Instant::now();
    for b in 0..blocks {
        out.fill([0.0; 2]);
        per_block(b, &mut voices);
        let now = ((b + 8) * block) as u64;
        for v in &mut voices {
            v.render(&mut scratch, client, now, &mut out);
        }
        black_box(&out);
    }
    let elapsed_ns = t0.elapsed().as_nanos() as f64;
    let total_frames = (blocks * block) as f64;
    let ns_per_frame = elapsed_ns / total_frames;
    let ns_per_voice_frame = ns_per_frame / voices_count as f64;
    let budget_ns_per_frame = 1_000_000_000.0 / f64::from(RATE); // 20833.33 ns
    let cpu_pct = (ns_per_frame / budget_ns_per_frame) * 100.0;
    let voices_at_1pct = (budget_ns_per_frame * 0.01) / ns_per_voice_frame;
    println!(
        "{label:<44} | {voices_count:>3}v blk={block:<4} | {ns_per_frame:>7.1} ns/f | {ns_per_voice_frame:>5.2} ns/vf | {cpu_pct:>5.2}% CPU | ~{voices_at_1pct:>5.0} v@1%"
    );
}

#[test]
#[ignore = "benchmark"]
fn voice_polyphony_suite() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let region_loop = stereo_sine_region(&mut store, "bench-loop", 65_536, true);
    let region_wt = wavetable_region(&mut store);
    let client = store.client();

    let env_sustain = Ahdsr::GATE;
    let env_moving = Ahdsr {
        attack: 48_000,
        hold: 0,
        decay: 48_000,
        sustain: 0.5,
        release: 48_000,
        attack_shape: 2.0,
        decay_shape: -3.0,
        release_shape: -3.0,
        ..Ahdsr::GATE
    };

    println!("\n=== KONTRA VOICE POLYPHONY BENCHMARK (48 kHz) ===");

    for (voices, blocks) in [(64, 400), (128, 250), (300, 150)] {
        bench_polyphony(
            "unity_pitch_sustain (step=1.0)",
            &client,
            voices,
            256,
            blocks,
            |_| VoiceStart {
                region: region_loop.clone(),
                loop_shift: [0.0; 2],
                mix: None,
                read: None,
                oscillator: None,
                granular: None,
                step: 1.0,
                gain: [[0.5, 0.0], [0.0, 0.5]],
                lag: [kontra_voice::Lag::NONE; kontra_voice::LAGS],
                envelope: env_sustain,
                fade: None,
                speed: 1.0,
            },
            |_, _| {},
        );
    }

    for &block in &[256, 512, 1024] {
        let blocks = (300 * 256 / block).max(40);
        bench_polyphony(
            "unity_pitch_300v_blocksize",
            &client,
            300,
            block,
            blocks,
            |_| VoiceStart {
                region: region_loop.clone(),
                loop_shift: [0.0; 2],
                mix: None,
                read: None,
                oscillator: None,
                granular: None,
                step: 1.0,
                gain: [[0.5, 0.0], [0.0, 0.5]],
                lag: [kontra_voice::Lag::NONE; kontra_voice::LAGS],
                envelope: env_sustain,
                fade: None,
                speed: 1.0,
            },
            |_, _| {},
        );
    }

    bench_polyphony(
        "unity_pitch_moving_env (step=1.0)",
        &client,
        300,
        256,
        150,
        |_| VoiceStart {
            region: region_loop.clone(),
            loop_shift: [0.0; 2],
            mix: None,
            read: None,
            oscillator: None,
            granular: None,
            step: 1.0,
            gain: [[0.5, 0.0], [0.0, 0.5]],
            lag: [kontra_voice::Lag::NONE; kontra_voice::LAGS],
            envelope: env_moving,
            fade: None,
            speed: 1.0,
        },
        |_, _| {},
    );

    bench_polyphony(
        "hermite_down_pitch (step=0.75)",
        &client,
        300,
        256,
        150,
        |_| VoiceStart {
            region: region_loop.clone(),
            loop_shift: [0.0; 2],
            mix: None,
            read: None,
            oscillator: None,
            granular: None,
            step: 0.75,
            gain: [[0.5, 0.0], [0.0, 0.5]],
            lag: [kontra_voice::Lag::NONE; kontra_voice::LAGS],
            envelope: env_sustain,
            fade: None,
            speed: 1.0,
        },
        |_, _| {},
    );

    bench_polyphony(
        "sinc_bank_semitone (step=2^(7/12))",
        &client,
        300,
        256,
        100,
        |_| VoiceStart {
            region: region_loop.clone(),
            loop_shift: [0.0; 2],
            mix: None,
            read: None,
            oscillator: None,
            granular: None,
            step: 2.0f64.powf(7.0 / 12.0),
            gain: [[0.5, 0.0], [0.0, 0.5]],
            lag: [kontra_voice::Lag::NONE; kontra_voice::LAGS],
            envelope: env_sustain,
            fade: None,
            speed: 1.0,
        },
        |_, _| {},
    );

    bench_polyphony(
        "sinc_exact_detuned (step=1.37)",
        &client,
        300,
        256,
        60,
        |_| VoiceStart {
            region: region_loop.clone(),
            loop_shift: [0.0; 2],
            mix: None,
            read: None,
            oscillator: None,
            granular: None,
            step: 1.37,
            gain: [[0.5, 0.0], [0.0, 0.5]],
            lag: [kontra_voice::Lag::NONE; kontra_voice::LAGS],
            envelope: env_sustain,
            fade: None,
            speed: 1.0,
        },
        |_, _| {},
    );

    bench_polyphony(
        "sinc_ramped_vibrato (step 1.2->1.4)",
        &client,
        128,
        256,
        60,
        |_| VoiceStart {
            region: region_loop.clone(),
            loop_shift: [0.0; 2],
            mix: None,
            read: None,
            oscillator: None,
            granular: None,
            step: 1.25,
            gain: [[0.5, 0.0], [0.0, 0.5]],
            lag: [kontra_voice::Lag::NONE; kontra_voice::LAGS],
            envelope: env_moving,
            fade: None,
            speed: 1.0,
        },
        |b, voices| {
            let (a, z) = if b % 2 == 0 {
                (1.20, 1.40)
            } else {
                (1.40, 1.20)
            };
            for v in voices {
                v.set_step_ramp(a, z, 256);
            }
        },
    );

    bench_polyphony(
        "wavetable_high_quality",
        &client,
        128,
        256,
        80,
        |i| VoiceStart {
            region: region_wt.clone(),
            loop_shift: [0.0; 2],
            mix: None,
            read: None,
            oscillator: None,
            granular: None,
            step: kontra_voice::PERIOD * (220.0 + i as f64 * 2.0) / f64::from(RATE),
            gain: [[0.5, 0.0], [0.0, 0.5]],
            lag: [kontra_voice::Lag::NONE; kontra_voice::LAGS],
            envelope: env_sustain,
            fade: None,
            speed: 1.0,
        },
        |_, _| {},
    );

    bench_polyphony(
        "oscillator_polyblep_saw",
        &client,
        300,
        256,
        120,
        |i| VoiceStart {
            region: region_loop.clone(),
            loop_shift: [0.0; 2],
            mix: None,
            read: None,
            oscillator: Some(Oscillator::new(OscWave::Saw, 0.0)),
            granular: None,
            step: kontra_voice::OSC_PERIOD * (110.0 + i as f64) / f64::from(RATE),
            gain: [[0.5, 0.0], [0.0, 0.5]],
            lag: [kontra_voice::Lag::NONE; kontra_voice::LAGS],
            envelope: env_sustain,
            fade: None,
            speed: 1.0,
        },
        |_, _| {},
    );

    bench_polyphony(
        "oscillator_sine",
        &client,
        300,
        256,
        120,
        |i| VoiceStart {
            region: region_loop.clone(),
            loop_shift: [0.0; 2],
            mix: None,
            read: None,
            oscillator: Some(Oscillator::new(OscWave::Sine, 0.0)),
            granular: None,
            step: kontra_voice::OSC_PERIOD * (110.0 + i as f64) / f64::from(RATE),
            gain: [[0.5, 0.0], [0.0, 0.5]],
            lag: [kontra_voice::Lag::NONE; kontra_voice::LAGS],
            envelope: env_sustain,
            fade: None,
            speed: 1.0,
        },
        |_, _| {},
    );

    bench_polyphony(
        "granular_cloud (density=30)",
        &client,
        32,
        256,
        80,
        |_| VoiceStart {
            region: region_loop.clone(),
            loop_shift: [0.0; 2],
            mix: None,
            read: None,
            oscillator: None,
            granular: Some(Granular::new(
                GranularSpec {
                    grain_ms: 40.0,
                    density: 30.0,
                    position: 0.25,
                    pitch_semitones: 0.0,
                    spread: 0.5,
                },
                RATE,
            )),
            step: 1.0,
            gain: [[0.5, 0.0], [0.0, 0.5]],
            lag: [kontra_voice::Lag::NONE; kontra_voice::LAGS],
            envelope: env_sustain,
            fade: None,
            speed: 1.0,
        },
        |_, _| {},
    );
}
