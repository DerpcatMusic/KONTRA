//! Zero-allocation RT-safety proof for all 18 newly integrated Kontakt DSP
//! processors under an isolated counting [`GlobalAlloc`].

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use kontra_dsp::dynamics::{EnvelopeFollower, EnvelopeFollowerSettings};
use kontra_effects::processors::{Dsp, Responses, processor};
use kontra_ir as ir;
use kontra_kontakt_effects::{
    amps, cabinet, cat, core_cell, cry_wah, dyx, kolor, plate_reverb, ringmod, skreamer_deluxe,
    stereo_tune, twin_delay,
};

struct Counting;
thread_local! { static COUNT: Cell<Option<usize>> = const { Cell::new(None) }; }
#[global_allocator]
static ALLOCATOR: Counting = Counting;

// SAFETY: forwards every allocation unchanged to System; thread-local counter never allocates.
#[expect(unsafe_code, reason = "isolated test allocator forwards to System")]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNT.with(|n| n.set(n.get().map(|v| v + 1)));
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        COUNT.with(|n| n.set(n.get().map(|v| v + 1)));
        unsafe { System.realloc(ptr, layout, size) }
    }
}

fn count_allocs(f: impl FnOnce()) -> usize {
    COUNT.with(|n| n.set(Some(0)));
    f();
    COUNT.with(|n| n.take().unwrap_or(0))
}

#[test]
fn all_18_missing_dsp_processors_allocate_zero_bytes_on_process_and_set() {
    const RATE: u32 = 48_000;
    let kinds: Vec<(&str, ir::EffectKind, ir::EffectKind)> = vec![
        (
            "Cry Wah",
            ir::EffectKind::Wah(cry_wah::wah(cry_wah::INIT)),
            ir::EffectKind::Wah(cry_wah::wah(cry_wah::CryWah {
                pedal: 0.9,
                mono: true,
            })),
        ),
        (
            "Plate Reverb",
            ir::EffectKind::Reverb(plate_reverb::reverb(plate_reverb::INIT)),
            ir::EffectKind::Reverb(plate_reverb::reverb(plate_reverb::PlateReverb {
                decay: 0.85,
                stereo: 0.8,
                ..plate_reverb::INIT
            })),
        ),
        (
            "Ring Modulator",
            ir::EffectKind::RingMod(ringmod::ringmod(ringmod::INIT)),
            ir::EffectKind::RingMod(ringmod::ringmod(ringmod::RingModulator {
                ring: 0.9,
                fm: 0.4,
                ..ringmod::INIT
            })),
        ),
        (
            "Stereo Tune",
            ir::EffectKind::StereoDetune(stereo_tune::detune(stereo_tune::INIT)),
            ir::EffectKind::StereoDetune(stereo_tune::detune(stereo_tune::StereoTune {
                drift: 0.8,
                mix: 0.9,
                ..stereo_tune::INIT
            })),
        ),
        (
            "Twin Delay",
            ir::EffectKind::DualDelay(twin_delay::delay(twin_delay::INIT)),
            ir::EffectKind::DualDelay(twin_delay::delay(twin_delay::TwinDelay {
                width: 0.25,
                cross_feedback: 0.4,
                ..twin_delay::INIT
            })),
        ),
        (
            "Twang",
            ir::EffectKind::AmpStack(amps::twang::amp(amps::twang::INIT)),
            ir::EffectKind::AmpStack(amps::twang::amp(amps::twang::Twang {
                volume: 0.9,
                bright: true,
                ..amps::twang::INIT
            })),
        ),
        (
            "Cabinet",
            ir::EffectKind::Cabinet(cabinet::cabinet(cabinet::INIT)),
            ir::EffectKind::Cabinet(cabinet::cabinet(cabinet::Cabinet {
                cabinet_type: 9,
                air: 0.8,
                ..cabinet::INIT
            })),
        ),
        (
            "Jump",
            ir::EffectKind::AmpStack(amps::jump::amp(amps::jump::INIT)),
            ir::EffectKind::AmpStack(amps::jump::amp(amps::jump::Jump {
                preamp: 0.85,
                higain: true,
                ..amps::jump::INIT
            })),
        ),
        (
            "Van51",
            ir::EffectKind::AmpStack(amps::van51::amp(amps::van51::INIT)),
            ir::EffectKind::AmpStack(amps::van51::amp(amps::van51::Van51 {
                lead_channel: true,
                crunch: true,
                ..amps::van51::INIT
            })),
        ),
        (
            "AC Box",
            ir::EffectKind::AmpStack(amps::ac_box::amp(amps::ac_box::INIT)),
            ir::EffectKind::AmpStack(amps::ac_box::amp(amps::ac_box::AcBox {
                tremolo_depth: 0.8,
                tone_cut: 0.7,
                ..amps::ac_box::INIT
            })),
        ),
        (
            "Hot Solo",
            ir::EffectKind::AmpStack(amps::hot_solo::amp(amps::hot_solo::INIT)),
            ir::EffectKind::AmpStack(amps::hot_solo::amp(amps::hot_solo::HotSolo {
                overdrive: true,
                depth: 0.9,
                ..amps::hot_solo::INIT
            })),
        ),
        (
            "Cat",
            ir::EffectKind::AmpStack(cat::amp(cat::INIT)),
            ir::EffectKind::AmpStack(cat::amp(cat::Cat {
                distortion: 0.9,
                filter: 0.8,
                ..cat::INIT
            })),
        ),
        (
            "Bass Pro",
            ir::EffectKind::AmpStack(amps::bass_pro::amp(amps::bass_pro::INIT)),
            ir::EffectKind::AmpStack(amps::bass_pro::amp(amps::bass_pro::BassPro {
                geq: true,
                ultra_lo: true,
                ..amps::bass_pro::INIT
            })),
        ),
        (
            "Skreamer Deluxe",
            ir::EffectKind::AmpStack(skreamer_deluxe::amp(skreamer_deluxe::INIT)),
            ir::EffectKind::AmpStack(skreamer_deluxe::amp(skreamer_deluxe::SkreamerDeluxe {
                mode: 2,
                drive: 0.9,
                ..skreamer_deluxe::INIT
            })),
        ),
        (
            "Kolor",
            ir::EffectKind::ColorSat(kolor::color_sat(kolor::INIT)),
            ir::EffectKind::ColorSat(kolor::color_sat(kolor::Kolor {
                mode: 7,
                boost: true,
                bass_saver: 0.6,
                ..kolor::INIT
            })),
        ),
        (
            "DYX",
            ir::EffectKind::SpectralMorph(dyx::spectral_morph(dyx::INIT)),
            ir::EffectKind::SpectralMorph(dyx::spectral_morph(dyx::Dyx {
                morph: 0.9,
                bank: 2,
                tracking: true,
                ..dyx::INIT
            })),
        ),
    ];

    let mut io = [[0.25f32, -0.2f32]; 64];
    for (name, init_kind, moved_kind) in kinds {
        let slot = ir::Effect {
            kind: init_kind,
            bypass: false,
            output: 1.0,
            dry: 0.0,
        };
        let Dsp::Wet(mut unit, _) = processor(Some(&slot), RATE, &Responses(&[], &[])) else {
            panic!("{name} did not instantiate a wet processor");
        };
        let allocs = count_allocs(|| {
            unit.process(&mut io);
            unit.set(&moved_kind, 128.0);
            unit.process(&mut io);
        });
        assert_eq!(allocs, 0, "{name} allocated {allocs} times on audio thread");
        assert!(io.iter().flatten().all(|v| v.is_finite()));
        io = [[0.25, -0.2]; 64];
    }

    // Core Cell (stateless Gain(0.0)) and EnvelopeFollower (BParEnvF) also allocate 0 bytes.
    assert_eq!(
        core_cell::effect(core_cell::INIT),
        ir::EffectKind::Gain(0.0)
    );
    let mut ef = EnvelopeFollower::new(EnvelopeFollowerSettings::default(), RATE);
    let allocs = count_allocs(|| {
        ef.set(EnvelopeFollowerSettings {
            attack_ms: 10.0,
            release_ms: 80.0,
            adaption_ms: 40.0,
            gain_db: 6.0,
        });
        let v = ef.process(&io);
        assert!(v.is_finite() && (0.0..=1.0).contains(&v));
    });
    assert_eq!(allocs, 0, "EnvelopeFollower allocated {allocs} times");
}
